//! Pure auction matching (FR-RSK-01–04). No Session. No layers, no γ.

use math::{sort_bids, unit_premium, Bid};

/// Compatibility PDA: quotes still seed with layer_id = 1.
pub const MAX_LAYERS: u8 = 1;
/// Standing quotes in the single pool (FR-RSK-02). `Layer` is `zero_copy` so this
/// fits the 4KiB BPF stack (Borsh-loading 64 quotes does not).
pub const MAX_QUOTES: usize = 64;
pub const POOL_LAYER: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuoteView {
    pub capacity: u64,
    pub filled: u64,
    pub premium: u64,
    pub ts: i64,
    pub lp_filled_on_board: u64,
}

pub fn layer_attachment(_d_unit: u64, layer_id: u8) -> Option<u64> {
    if layer_id != POOL_LAYER {
        return None;
    }
    Some(0)
}

pub fn d_required(l_max: u64) -> u64 {
    l_max
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

/// Every accepted quote joins the pool for its leftover \(D\). Rank is for rewards, not a fill cap.
pub fn next_fillable(remain: u64, quotes: &[QuoteView]) -> Option<(usize, u64)> {
    for i in rank(quotes) {
        match take_from_head(remain, &quotes[i]) {
            Ok(0) => continue,
            Ok(take) => return Some((i, take)),
            Err(_) => continue,
        }
    }
    None
}

/// Fill `take` from this quote. \(D_i\) does not change after the quote exists.
pub fn take_from_head(remain: u64, quote: &QuoteView) -> Result<u64, &'static str> {
    if remain == 0 {
        return Ok(0);
    }
    let leftover = quote.capacity.saturating_sub(quote.filled);
    if leftover == 0 {
        return Ok(0);
    }
    Ok(remain.min(leftover))
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
    fn pool_layer_only() {
        assert!(layer_attachment(10, 0).is_none());
        assert_eq!(layer_attachment(10, 1).unwrap(), 0);
        assert!(layer_attachment(10, 2).is_none());
    }

    #[test]
    fn lowest_unit_premium_ranks_first() {
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
    fn same_lp_may_lock_full_capacity() {
        let quote = QuoteView {
            capacity: 100,
            filled: 0,
            premium: 1,
            ts: 1,
            lp_filled_on_board: 50,
        };
        assert_eq!(take_from_head(100, &quote), Ok(100));
    }

    #[test]
    fn take_does_not_raise_capacity() {
        let quote = q(1, 1);
        let t = take_from_head(40, &quote).unwrap();
        assert_eq!(t, 40);
        assert_eq!(quote.capacity, 100);
    }

    #[test]
    fn premium_scales_with_fill() {
        assert_eq!(premium_due(80, 100, 50), 40);
        assert_eq!(premium_due(80, 100, 100), 80);
    }

    #[test]
    fn cheaper_head_does_not_block_later_quotes() {
        let first = q(1, 1);
        let next = q(9, 2);
        let (idx, take) = next_fillable(40, &[first, next]).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(take, 40);
    }
}
