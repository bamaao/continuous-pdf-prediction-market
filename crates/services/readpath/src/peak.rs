//! Trading-period peak of $E(x)$. Do not enumerate user intervals.

use crate::store::MarketProj;
use math::settle::usdc;
use math::{PeakExposure, Q64};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct PeakRisk {
    pub cell: u16,
    pub payout_usdc: u64,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lo: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hi: Option<f64>,
    pub run_lo: u16,
    pub run_hi: u16,
    pub ties: u16,
}

fn q_as_f64(q: Q64) -> f64 {
    q.raw() as f64 / ((1u128 << 64) as f64)
}

fn fmt_x(x: f64) -> String {
    if !x.is_finite() {
        return "—".into();
    }
    let a = x.abs();
    if a >= 1000.0 {
        format!("{x:.0}")
    } else if a >= 10.0 {
        format!("{x:.2}")
    } else {
        format!("{x:.3}")
    }
}

fn node(i: usize, n: usize, xmin: f64, xmax: f64, log: bool) -> f64 {
    if n < 2 {
        return xmin;
    }
    let t = i as f64 / ((n - 1) as f64);
    if log {
        (xmin.ln() + t * (xmax.ln() - xmin.ln())).exp()
    } else {
        xmin + t * (xmax - xmin)
    }
}

fn band(i: usize, n: usize, xmin: f64, xmax: f64, log: bool) -> (f64, f64) {
    let lo = if i == 0 {
        xmin
    } else {
        0.5 * (node(i - 1, n, xmin, xmax, log) + node(i, n, xmin, xmax, log))
    };
    let hi = if i + 1 >= n {
        xmax
    } else {
        0.5 * (node(i, n, xmin, xmax, log) + node(i + 1, n, xmin, xmax, log))
    };
    (lo, hi)
}

fn score_label(cell: usize) -> String {
    let h = cell / 11;
    let a = cell % 11;
    format!(
        "{}–{}",
        if h >= 10 { "10+".into() } else { h.to_string() },
        if a >= 10 { "10+".into() } else { a.to_string() }
    )
}

pub fn peak_risk(row: &MarketProj) -> PeakRisk {
    let book = row.book();
    let peak: PeakExposure = book.peak_exposure();
    let n = row.n.max(1) as usize;
    let cell = peak.cell.min(n.saturating_sub(1));
    let run_lo = peak.run_lo.min(n.saturating_sub(1));
    let run_hi = peak.run_hi.min(n.saturating_sub(1));
    let (label, lo, hi) = match row.family {
        0 => (format!("FT {}", score_label(cell)), None, None),
        1 | 2 => {
            let xmin = q_as_f64(Q64::from_raw(row.extra_a));
            let xmax = q_as_f64(Q64::from_raw(row.extra_b));
            if xmax > xmin && xmin.is_finite() && xmax.is_finite() {
                let log = row.family == 2;
                let (a, _) = band(run_lo, n, xmin, xmax, log);
                let (_, b) = band(run_hi, n, xmin, xmax, log);
                let extra = if peak.ties > run_hi.saturating_sub(run_lo) + 1 {
                    format!(" · {} other peaks", peak.ties - (run_hi - run_lo + 1))
                } else {
                    String::new()
                };
                (
                    format!("overlap thickest on [{}, {}]{extra}", fmt_x(a), fmt_x(b)),
                    Some(a),
                    Some(b),
                )
            } else {
                ("thickest overlap on Ω".into(), None, None)
            }
        }
        3 => (format!("outcome {cell}"), None, None),
        4 => (
            if cell == 0 { "NO".into() } else { "YES".into() },
            None,
            None,
        ),
        _ => (format!("outcome {cell}"), None, None),
    };
    PeakRisk {
        cell: cell as u16,
        payout_usdc: usdc(peak.value),
        label,
        lo,
        hi,
        run_lo: peak.run_lo as u16,
        run_hi: peak.run_hi as u16,
        ties: peak.ties as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_peak_is_the_thickest_overlap_band() {
        let n = 5u16;
        let row = MarketProj {
            market: "m".into(),
            family: 1,
            status: 1,
            n,
            beta: Q64::from_int(10).raw(),
            p0: vec![Q64::from_ratio(1, 5).raw(); 5],
            theta: vec![0; 5],
            exposure: vec![
                Q64::ZERO.raw(),
                Q64::from_int(2).raw(),
                Q64::from_int(5).raw(),
                Q64::from_int(2).raw(),
                Q64::ZERO.raw(),
            ],
            trading_revenue: 0,
            premium_payable: 0,
            c_m: 0,
            c_r: 0,
            fee_bps: 0,
            fee_timing: 0,
            slot: 1,
            traders: 0,
            tickets: 0,
            stake_usdc: 0,
            board_phase: 0,
            rho_raw: 0,
            settle_cell: 0,
            liability: 0,
            c_p_board: 0,
            c_p_alloc: 0,
            close_ts: 0,
            risk_lock_ts: 0,
            report_window_secs: 0,
            report_open_ts: 0,
            extra_a: Q64::from_int(-2).raw(),
            extra_b: Q64::from_int(12).raw(),
            extra_u2: 0,
            delegated: false,
            platform: String::new(),
        };
        let peak = peak_risk(&row);
        assert_eq!(peak.cell, 2);
        assert_eq!(peak.payout_usdc, 5);
        assert!(peak.label.contains('['), "{}", peak.label);
        assert!(peak.lo.is_some() && peak.hi.is_some());
        assert!(peak.lo.unwrap() < peak.hi.unwrap());
    }

    #[test]
    fn gaussian_plateau_is_the_full_overlap_interval() {
        let n = 5u16;
        let row = MarketProj {
            market: "m".into(),
            family: 1,
            status: 1,
            n,
            beta: Q64::from_int(10).raw(),
            p0: vec![Q64::from_ratio(1, 5).raw(); 5],
            theta: vec![0; 5],
            exposure: vec![
                Q64::ZERO.raw(),
                Q64::from_int(5).raw(),
                Q64::from_int(5).raw(),
                Q64::from_int(5).raw(),
                Q64::ZERO.raw(),
            ],
            trading_revenue: 0,
            premium_payable: 0,
            c_m: 0,
            c_r: 0,
            fee_bps: 0,
            fee_timing: 0,
            slot: 1,
            traders: 0,
            tickets: 0,
            stake_usdc: 0,
            board_phase: 0,
            rho_raw: 0,
            settle_cell: 0,
            liability: 0,
            c_p_board: 0,
            c_p_alloc: 0,
            close_ts: 0,
            risk_lock_ts: 0,
            report_window_secs: 0,
            report_open_ts: 0,
            extra_a: Q64::from_int(-2).raw(),
            extra_b: Q64::from_int(12).raw(),
            extra_u2: 0,
            delegated: false,
            platform: String::new(),
        };
        let peak = peak_risk(&row);
        assert_eq!(peak.run_lo, 1);
        assert_eq!(peak.run_hi, 3);
        assert_eq!(peak.payout_usdc, 5);
        let (a, _) = band(1, 5, -2.0, 12.0, false);
        let (_, b) = band(3, 5, -2.0, 12.0, false);
        assert!((peak.lo.unwrap() - a).abs() < 1e-9);
        assert!((peak.hi.unwrap() - b).abs() < 1e-9);
        assert!(peak.hi.unwrap() - peak.lo.unwrap() > 3.0);
    }
}
