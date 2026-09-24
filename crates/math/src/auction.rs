//! Risk-auction ranking: lowest unit premium, then earlier time (FR-RSK-02).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bid {
    pub unit_premium: u128,
    pub ts: i64,
    pub idx: usize,
}

/// Stable order: cheaper first, then older, then input index.
pub fn sort_bids(mut bids: Vec<Bid>) -> Vec<Bid> {
    bids.sort_by(|a, b| {
        a.unit_premium
            .cmp(&b.unit_premium)
            .then(a.ts.cmp(&b.ts))
            .then(a.idx.cmp(&b.idx))
    });
    bids
}

/// `premium * SCALE / capacity` so integer USDC amounts stay ordered.
pub fn unit_premium(premium: u64, capacity: u64) -> Option<u128> {
    if capacity == 0 {
        return None;
    }
    Some((premium as u128).saturating_mul(1_000_000) / capacity as u128)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cheaper_beats_richer() {
        let ordered = sort_bids(vec![
            Bid {
                unit_premium: 20,
                ts: 1,
                idx: 0,
            },
            Bid {
                unit_premium: 10,
                ts: 9,
                idx: 1,
            },
        ]);
        assert_eq!(ordered[0].idx, 1);
    }

    #[test]
    fn same_rate_earlier_time_wins() {
        let ordered = sort_bids(vec![
            Bid {
                unit_premium: 10,
                ts: 5,
                idx: 0,
            },
            Bid {
                unit_premium: 10,
                ts: 3,
                idx: 1,
            },
        ]);
        assert_eq!(ordered[0].idx, 1);
    }
}
