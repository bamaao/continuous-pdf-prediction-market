//! Pure auction matching (FR-RSK-01–04). No Session.

use math::{sort_bids, unit_premium, Bid};

pub const MAX_LAYERS: u8 = 8;
pub const MAX_QUOTES: usize = 16;
pub const DEFAULT_GAMMA_BPS: u16 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteView {
    pub capacity: u64,
    pub filled: u64,
    pub premium: u64,
    pub ts: i64,
    pub lp_filled_on_board: u64,
}

/// Layer \(k\) attaches at \((k-1)\,D_{\mathrm{unit}}\). There is no retained seed layer.
pub fn layer_attachment(d_unit: u64, layer_id: u8) -> Option<u64> {
    if layer_id == 0 || layer_id > MAX_LAYERS {
        return None;
    }
    d_unit.checked_mul((layer_id as u64) - 1)
}

pub fn d_required(l_max: u64) -> u64 {
    l_max
}

/// Same LP may not take more than γ of the working size on this board.
pub fn concentration_cap(gamma_bps: u16, c_r: u64, d_required: u64, d_unit: u64) -> u64 {
    let base = c_r.max(d_required).max(d_unit);
    let cap = (base as u128)
        .saturating_mul(gamma_bps as u128)
        / 10_000;
    if cap == 0 {
        d_unit
    } else {
        cap as u64
    }
}

pub fn rank(quotes: &[QuoteView]) -> Vec<usize> {
    let bids: Vec<Bid> = quotes
        .iter()
        .enumerate()
        .filter_map(|(i, q)| {
            if q.filled >= q.capacity || q.capacity == 0 {
                return None;
            }
            Some(Bid {
                unit_premium: unit_premium(q.premium, q.capacity)?,
                ts: q.ts,
                idx: i,
            })
        })
        .collect();
    sort_bids(bids).into_iter().map(|b| b.idx).collect()
}

/// Cheapest quote that can take size. A γ-blocked head is skipped, not a stall.
pub fn next_fillable(
    remain: u64,
    quotes: &[QuoteView],
    gamma_bps: u16,
    c_r: u64,
    d_required: u64,
    d_unit: u64,
) -> Option<(usize, u64)> {
    for i in rank(quotes) {
        match take_from_head(remain, &quotes[i], gamma_bps, c_r, d_required, d_unit) {
            Ok(0) => continue,
            Ok(take) => return Some((i, take)),
            Err(_) => continue,
        }
    }
    None
}

/// Fill `take` from the cheapest quote. `D_i` does not change after the quote exists.
pub fn take_from_head(
    remain: u64,
    quote: &QuoteView,
    gamma_bps: u16,
    c_r: u64,
    d_required: u64,
    d_unit: u64,
) -> Result<u64, &'static str> {
    if remain == 0 {
        return Ok(0);
    }
    let leftover = quote.capacity.saturating_sub(quote.filled);
    if leftover == 0 {
        return Ok(0);
    }
    let cap = concentration_cap(gamma_bps, c_r, d_required, d_unit);
    let room = cap.saturating_sub(quote.lp_filled_on_board);
    if room == 0 {
        return Err("concentration");
    }
    Ok(remain.min(leftover).min(room))
}

/// Premium becomes a protocol debt in proportion to filled capacity.
pub fn premium_due(premium: u64, capacity: u64, take: u64) -> u64 {
    if capacity == 0 {
        return 0;
    }
    ((premium as u128) * (take as u128) / (capacity as u128)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(premium: u64, ts: i64) -> QuoteView {
        QuoteView {
            capacity: 100,
            filled: 0,
            premium,
            ts,
            lp_filled_on_board: 0,
        }
    }

    #[test]
    fn published_layer_ids_only() {
        assert!(layer_attachment(10, 0).is_none());
        assert_eq!(layer_attachment(10, 1).unwrap(), 0);
        assert_eq!(layer_attachment(10, 2).unwrap(), 10);
    }

    #[test]
    fn lowest_unit_premium_fills_first() {
        let quotes = [q(30, 1), q(10, 5), q(20, 2)];
        assert_eq!(rank(&quotes)[0], 1);
    }

    #[test]
    fn filled_quote_drops_out() {
        let mut rich = q(1, 1);
        rich.filled = 100;
        assert!(rank(&[rich, q(9, 2)]).first().copied() == Some(1));
    }

    #[test]
    fn gamma_blocks_same_lp() {
        let quote = QuoteView {
            capacity: 100,
            filled: 0,
            premium: 1,
            ts: 1,
            lp_filled_on_board: 50,
        };
        // γ=10%, working size 100 → cap 10, already 50
        assert_eq!(
            take_from_head(100, &quote, 1_000, 100, 0, 100),
            Err("concentration")
        );
    }

    #[test]
    fn take_does_not_raise_capacity() {
        let quote = q(1, 1);
        let t = take_from_head(40, &quote, 10_000, 0, 0, 100).unwrap();
        assert_eq!(t, 40);
        assert_eq!(quote.capacity, 100);
    }

    #[test]
    fn premium_scales_with_fill() {
        assert_eq!(premium_due(80, 100, 50), 40);
        assert_eq!(premium_due(80, 100, 100), 80);
    }

    #[test]
    fn gamma_blocked_head_does_not_stall_layer() {
        let blocked = QuoteView {
            capacity: 100,
            filled: 0,
            premium: 1,
            ts: 1,
            lp_filled_on_board: 50,
        };
        let next = q(9, 2);
        let (idx, take) = next_fillable(40, &[blocked, next], 1_000, 100, 0, 100).unwrap();
        assert_eq!(idx, 1);
        // next LP is still under γ (10% of working size 100)
        assert_eq!(take, 10);
    }
}
