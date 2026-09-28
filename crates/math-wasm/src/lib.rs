//! WASM / native preview of the same LMSR as chain and Quote (FR-TRD-10, NFR-07).
//! TypeScript SHALL call this crate. It SHALL NOT reimplement $p_S$ / $C_S$.

use math::football;
use math::Q64;
use quote::{decode_mask, q_bps, Book};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BookSnap {
    pub beta_raw: String,
    pub p0_raw: Vec<String>,
    pub theta_raw: Vec<String>,
    pub exposure_raw: Vec<String>,
    pub trading_revenue: u64,
    pub premium_payable: u64,
    pub c_m: u64,
    pub c_r: u64,
    pub slot: u64,
    #[serde(default)]
    pub fee_bps: u16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Preview {
    pub p_s_raw: String,
    pub p_s_bps: u64,
    pub c_s_raw: String,
    pub c_s_usdc: u64,
    pub coverage_bps: u64,
    pub rho_hat_bps: u64,
    pub l_max_usdc: u64,
    pub c_max_usdc: u64,
    pub r_net: u64,
    pub slot: u64,
    pub n: u16,
    pub pdf_bps: Vec<u64>,
    pub fee_bps: u16,
    pub fee_usdc: u64,
    pub pay_usdc: u64,
    pub face_usdc: u64,
    pub payout_if_hit_usdc: u64,
    pub payout_if_miss_usdc: u64,
    pub net_if_hit: i64,
    pub ev_if_p_s: i64,
}

fn parse_i128(s: &str) -> Result<i128, String> {
    s.parse::<i128>().map_err(|e| e.to_string())
}

fn book_from(snap: &BookSnap) -> Result<Book, String> {
    let beta = parse_i128(&snap.beta_raw)?;
    let p0: Result<Vec<i128>, _> = snap.p0_raw.iter().map(|s| parse_i128(s)).collect();
    let theta: Result<Vec<i128>, _> = snap.theta_raw.iter().map(|s| parse_i128(s)).collect();
    let exposure: Result<Vec<i128>, _> = snap.exposure_raw.iter().map(|s| parse_i128(s)).collect();
    Ok(Book::from_grid(
        beta,
        &p0?,
        &theta?,
        &exposure?,
        snap.trading_revenue,
        snap.premium_payable,
        snap.c_m,
        snap.c_r,
        snap.slot,
    ))
}

pub fn preview_mask(snap: &BookSnap, mask_hex: &str, shares: i64) -> Result<Preview, String> {
    let book = book_from(snap)?;
    let h = mask_hex.trim().trim_start_matches("0x");
    if h.len() % 2 != 0 {
        return Err("bad mask".into());
    }
    let bytes: Result<Vec<u8>, _> = (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| "bad mask".to_string()))
        .collect();
    let in_set = decode_mask(&bytes?, book.n()).map_err(|e| e.to_string())?;
    let q = Q64::from_int(if shares > 0 { shares } else { 1 });
    let v = book.view(&in_set, q);
    let t = quote::ticket_from_view(&v, shares, snap.fee_bps);
    Ok(Preview {
        p_s_raw: v.p_s.raw().to_string(),
        p_s_bps: q_bps(v.p_s),
        c_s_raw: v.c_s.raw().to_string(),
        c_s_usdc: math::usdc(v.c_s),
        coverage_bps: q_bps(v.coverage),
        rho_hat_bps: q_bps(v.rho_hat),
        l_max_usdc: v.l_max_usdc,
        c_max_usdc: v.c_max_usdc,
        r_net: v.r_net,
        slot: v.slot,
        n: v.n,
        pdf_bps: book.pdf().into_iter().map(q_bps).collect(),
        fee_bps: t.fee_bps,
        fee_usdc: t.fee_usdc,
        pay_usdc: t.pay_usdc,
        face_usdc: t.face_usdc,
        payout_if_hit_usdc: t.payout_if_hit_usdc,
        payout_if_miss_usdc: t.payout_if_miss_usdc,
        net_if_hit: t.net_if_hit,
        ev_if_p_s: t.ev_if_p_s,
    })
}

pub fn encode_mask(in_set: &[bool]) -> String {
    let need = in_set.len().div_ceil(8);
    let mut bytes = vec![0u8; need];
    for (i, on) in in_set.iter().enumerate() {
        if *on {
            bytes[i / 8] |= 1 << (i % 8);
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn skellam_mask_hex(kind: u8, a: i16, b: i16, k_max: u32) -> Result<Vec<String>, String> {
    let sets = football::skellam_masks(kind, a, b, k_max).ok_or_else(|| "bad skellam line".to_string())?;
    Ok(sets.into_iter().map(|s| encode_mask(&s)).collect())
}

#[cfg(target_arch = "wasm32")]
mod wasm_api {
    use super::*;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn preview_json(snap_json: &str, mask_hex: &str, shares: i64) -> Result<String, JsValue> {
        let snap: BookSnap = serde_json::from_str(snap_json).map_err(|e| JsValue::from_str(&e.to_string()))?;
        let v = preview_mask(&snap, mask_hex, shares).map_err(|e| JsValue::from_str(&e))?;
        serde_json::to_string(&v).map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[wasm_bindgen]
    pub fn skellam_masks_json(kind: u8, a: i16, b: i16, k_max: u32) -> Result<String, JsValue> {
        let v = skellam_mask_hex(kind, a, b, k_max).map_err(|e| JsValue::from_str(&e))?;
        serde_json::to_string(&v).map_err(|e| JsValue::from_str(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use math::uniform_prior;

    fn uniform_snap(n: usize) -> BookSnap {
        let p0: Vec<String> = uniform_prior(n).into_iter().map(|q| q.raw().to_string()).collect();
        let z = vec!["0".into(); n];
        BookSnap {
            beta_raw: Q64::from_int(100).raw().to_string(),
            p0_raw: p0,
            theta_raw: z.clone(),
            exposure_raw: z,
            trading_revenue: 4,
            premium_payable: 0,
            c_m: 0,
            c_r: 0,
            slot: 3,
            fee_bps: 0,
        }
    }

    #[test]
    fn preview_matches_quote_kernel() {
        let snap = uniform_snap(8);
        let got = preview_mask(&snap, "01", 1).unwrap();
        let book = book_from(&snap).unwrap();
        let mut cell0 = vec![false; 8];
        cell0[0] = true;
        let expect = book.view(&cell0, Q64::from_int(1));
        assert_eq!(got.p_s_raw, expect.p_s.raw().to_string());
        assert_eq!(got.c_s_raw, expect.c_s.raw().to_string());
        assert_eq!(got.coverage_bps, q_bps(expect.coverage));
        assert_eq!(got.n, 8);
        assert_eq!(got.pdf_bps.len(), 8);
    }

    #[test]
    fn coverage_is_not_folded_into_price() {
        let snap = uniform_snap(8);
        let a = preview_mask(&snap, "01", 1).unwrap();
        let mut poor = snap.clone();
        poor.trading_revenue = 0;
        poor.c_m = 0;
        let b = preview_mask(&poor, "01", 1).unwrap();
        assert_eq!(a.p_s_raw, b.p_s_raw);
        assert!(b.coverage_bps < a.coverage_bps || b.c_max_usdc < a.c_max_usdc);
    }

    #[test]
    fn skellam_1x2_masks_are_projections() {
        let masks = skellam_mask_hex(0, 0, 0, 10).unwrap();
        assert_eq!(masks.len(), 1);
        assert!(!masks[0].is_empty());
    }

    #[test]
    fn prebet_ticket_fee_zero_and_miss_is_zero() {
        let snap = uniform_snap(8);
        let got = preview_mask(&snap, "01", 1).unwrap();
        assert_eq!(got.fee_bps, 0);
        assert_eq!(got.fee_usdc, 0);
        assert_eq!(got.pay_usdc, got.c_s_usdc);
        assert_eq!(got.payout_if_miss_usdc, 0);
        assert_eq!(got.face_usdc, 1);
        assert_eq!(got.net_if_hit, got.payout_if_hit_usdc as i64 - got.pay_usdc as i64);
    }

    #[test]
    fn prebet_fee_does_not_change_p_s() {
        let a = preview_mask(&uniform_snap(8), "01", 2).unwrap();
        let mut taxed = uniform_snap(8);
        taxed.fee_bps = 1_000;
        let b = preview_mask(&taxed, "01", 2).unwrap();
        assert_eq!(a.p_s_raw, b.p_s_raw);
        assert_eq!(a.c_s_raw, b.c_s_raw);
        assert_eq!(b.pay_usdc, a.pay_usdc + b.fee_usdc);
        assert_eq!(b.payout_if_miss_usdc, 0);
    }
}
