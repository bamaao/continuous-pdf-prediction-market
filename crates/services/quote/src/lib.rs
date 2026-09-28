//! Quote Engine kernel (FR-TRD-02, FR-TRD-10, NFR-07).
//! Coverage / $\hat\rho$ are displayed only — never folded into $p_S$ or $C_S$.

use math::lmsr::{buy_cost, interval_prob, LmsrState};
use math::settle::{c_max, r_net, recovery_rate, usdc};
use math::Q64;

/// Live book reconstructed from on-chain $\theta$ / $p_0$ / capital ledgers.
#[derive(Clone, Debug)]
pub struct Book {
    pub state: LmsrState,
    pub trading_revenue: u64,
    pub premium_payable: u64,
    pub c_m: u64,
    pub c_r: u64,
    pub c_p: u64,
    pub slot: u64,
}

impl Book {
    pub fn from_grid(
        beta: i128,
        p0: &[i128],
        theta: &[i128],
        exposure: &[i128],
        trading_revenue: u64,
        premium_payable: u64,
        c_m: u64,
        c_r: u64,
        slot: u64,
    ) -> Self {
        assert_eq!(p0.len(), theta.len());
        assert_eq!(p0.len(), exposure.len());
        Self {
            state: LmsrState::from_vecs(
                Q64::from_raw(beta),
                p0.iter().copied().map(Q64::from_raw).collect(),
                theta.iter().copied().map(Q64::from_raw).collect(),
                exposure.iter().copied().map(Q64::from_raw).collect(),
            ),
            trading_revenue,
            premium_payable,
            c_m,
            c_r,
            c_p: 0,
            slot,
        }
    }

    pub fn n(&self) -> usize {
        self.state.n()
    }

    /// Pure LMSR probability. Not haircut by coverage.
    pub fn p_s(&self, in_set: &[bool]) -> Q64 {
        interval_prob(&self.state, in_set)
    }

    /// Pure LMSR cost $C_S(q)$. Fees are applied by the chain, not here.
    pub fn c_s(&self, in_set: &[bool], q: Q64) -> Q64 {
        buy_cost(&self.state, in_set, q)
    }

    pub fn r_net(&self) -> u64 {
        r_net(self.trading_revenue, self.premium_payable)
    }

    pub fn c_max_usdc(&self) -> u64 {
        usdc(c_max(
            Q64::from_int(self.r_net() as i64),
            Q64::from_int(self.c_m as i64),
            Q64::from_int(self.c_r as i64),
            Q64::from_int(self.c_p as i64),
        ))
    }

    pub fn l_max_usdc(&self) -> u64 {
        usdc(self.state.l_max())
    }

    pub fn peak_exposure(&self) -> math::PeakExposure {
        self.state.peak_exposure()
    }

    /// Worst-case coverage $C_{\max}/L_{\max}$. Display only.
    pub fn coverage(&self) -> Q64 {
        recovery_rate(
            Q64::from_int(self.c_max_usdc() as i64),
            Q64::from_int(self.l_max_usdc() as i64),
        )
    }

    /// $\hat\rho=\min(1,C_{\max}/L_{\max})$ while the board is live.
    pub fn rho_hat(&self) -> Q64 {
        self.coverage()
    }

    /// Trading-implied PDF $p_k$. Not $E(x)$.
    pub fn pdf(&self) -> Vec<Q64> {
        math::implied_probs(&self.state)
    }

    pub fn view(&self, in_set: &[bool], q: Q64) -> QuoteView {
        QuoteView {
            p_s: self.p_s(in_set),
            c_s: self.c_s(in_set, q),
            coverage: self.coverage(),
            rho_hat: self.rho_hat(),
            l_max_usdc: self.l_max_usdc(),
            c_max_usdc: self.c_max_usdc(),
            r_net: self.r_net(),
            slot: self.slot,
            n: self.n() as u16,
        }
    }
}

/// Read-path numbers. $p_S$ / $C_S$ are pure LMSR; coverage is display-only.
#[derive(Clone, Copy, Debug)]
pub struct QuoteView {
    pub p_s: Q64,
    pub c_s: Q64,
    pub coverage: Q64,
    pub rho_hat: Q64,
    pub l_max_usdc: u64,
    pub c_max_usdc: u64,
    pub r_net: u64,
    pub slot: u64,
    pub n: u16,
}

/// Pre-bet cashflows. Display only. Does not change $p_S$ / $C_S$ (XX-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TicketPreview {
    pub shares: i64,
    pub fee_bps: u16,
    pub fee_usdc: u64,
    pub pay_usdc: u64,
    pub face_usdc: u64,
    pub payout_if_hit_usdc: u64,
    pub payout_if_miss_usdc: u64,
    pub net_if_hit: i64,
    pub ev_if_p_s: i64,
}

/// Same fee as chain: $\mathrm{usdc}(C_S\cdot\mathrm{fee\_bps}/10000)$.
/// `fee_timing` 0: Pay $=C_S+\phi C_S$ at fill. 1: Pay $=C_S$ now; $\phi$ of hit payout at claim.
pub fn ticket_from_view(v: &QuoteView, shares: i64, fee_bps: u16) -> TicketPreview {
    ticket_from_view_ex(v, shares, fee_bps, 0)
}

pub fn ticket_from_view_ex(v: &QuoteView, shares: i64, fee_bps: u16, fee_timing: u8) -> TicketPreview {
    let q = if shares > 0 { shares } else { 1 };
    let face = q as u64;
    let c = usdc(v.c_s);
    let payout_hit = usdc(v.rho_hat.saturating_mul(Q64::from_int(q)));
    let (fee, pay, net_base) = if fee_timing == 1 {
        let claim_fee = if fee_bps == 0 {
            0
        } else {
            payout_hit.saturating_mul(fee_bps as u64) / 10_000
        };
        (claim_fee, c, payout_hit.saturating_sub(claim_fee) as i64 - c as i64)
    } else {
        let fee = if fee_bps == 0 {
            0
        } else {
            usdc(
                v.c_s
                    .saturating_mul(Q64::from_int(fee_bps as i64))
                    .checked_div(Q64::from_int(10_000))
                    .unwrap_or(Q64::ZERO),
            )
        };
        (fee, c.saturating_add(fee), payout_hit as i64 - c.saturating_add(fee) as i64)
    };
    let ev_gross = usdc(v.p_s.saturating_mul(Q64::from_int(payout_hit as i64)));
    TicketPreview {
        shares: q,
        fee_bps,
        fee_usdc: fee,
        pay_usdc: pay,
        face_usdc: face,
        payout_if_hit_usdc: if fee_timing == 1 { payout_hit.saturating_sub(fee) } else { payout_hit },
        payout_if_miss_usdc: 0,
        net_if_hit: net_base,
        ev_if_p_s: ev_gross as i64 - pay as i64,
    }
}

pub fn q_bps(q: Q64) -> u64 {
    ((q.raw().max(0) as u128).saturating_mul(10_000) >> 64) as u64
}

/// Truncate to bps then Hamilton-assign leftover so a PDF vector sums to 10_000.
pub fn q_bps_renorm(qs: &[Q64]) -> Vec<u64> {
    if qs.is_empty() {
        return Vec::new();
    }
    let mut bps: Vec<u64> = qs.iter().copied().map(q_bps).collect();
    let sum: u64 = bps.iter().copied().sum();
    if sum >= 10_000 {
        return bps;
    }
    let mask = (1u128 << 64) - 1;
    let mut order: Vec<usize> = (0..qs.len()).collect();
    order.sort_by(|&i, &j| {
        let fi = (qs[i].raw().max(0) as u128).saturating_mul(10_000) & mask;
        let fj = (qs[j].raw().max(0) as u128).saturating_mul(10_000) & mask;
        fj.cmp(&fi).then(i.cmp(&j))
    });
    let mut rem = 10_000 - sum;
    for i in order {
        if rem == 0 {
            break;
        }
        bps[i] = bps[i].saturating_add(1);
        rem -= 1;
    }
    bps
}

/// Bitmask → membership, same layout as `market::mask`.
pub fn decode_mask(bytes: &[u8], n: usize) -> Result<Vec<bool>, &'static str> {
    if n == 0 {
        return Err("empty grid");
    }
    let need = n.div_ceil(8);
    if bytes.len() != need {
        return Err("bad mask length");
    }
    let mut out = vec![false; n];
    let mut any = false;
    for i in 0..n {
        if bytes[i / 8] & (1 << (i % 8)) != 0 {
            out[i] = true;
            any = true;
        }
    }
    let rem = n % 8;
    if rem != 0 && bytes[need - 1] >> rem != 0 {
        return Err("trailing mask bits");
    }
    if !any {
        return Err("empty set");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use math::uniform_prior;

    fn uniform_book(n: usize, revenue: u64, c_m: u64) -> Book {
        let p0: Vec<i128> = uniform_prior(n).into_iter().map(|q| q.raw()).collect();
        let z = vec![0i128; n];
        Book::from_grid(Q64::from_int(100).raw(), &p0, &z, &z, revenue, 0, c_m, 0, 1)
    }

    #[test]
    fn price_is_prior_when_theta_is_zero() {
        let book = uniform_book(8, 0, 0);
        let mut cell0 = vec![false; 8];
        cell0[0] = true;
        let p = book.p_s(&cell0);
        assert!((p.raw() - Q64::from_ratio(1, 8).raw()).abs() < Q64::from_ratio(1, 1000).raw());
    }

    #[test]
    fn buying_the_set_raises_p_s() {
        let mut book = uniform_book(8, 0, 0);
        let mut cell0 = vec![false; 8];
        cell0[0] = true;
        let before = book.p_s(&cell0);
        math::lmsr_update(&mut book.state, &cell0, Q64::from_int(10));
        let after = book.p_s(&cell0);
        assert!(after > before);
    }

    #[test]
    fn fees_never_enter_c_max() {
        let with_fees_excluded = uniform_book(4, 40, 5);
        assert_eq!(with_fees_excluded.c_max_usdc(), 40);
        // fees_accrued is not an argument — quoting from trading_revenue only.
        let same = Book::from_grid(
            Q64::from_int(100).raw(),
            &vec![Q64::from_ratio(1, 4).raw(); 4],
            &[0, 0, 0, 0],
            &[0, 0, 0, 0],
            40,
            0,
            5,
            0,
            1,
        );
        assert_eq!(same.c_max_usdc(), 40);
    }

    #[test]
    fn coverage_and_rho_hat_are_display_only() {
        let p0: Vec<i128> = uniform_prior(4).into_iter().map(|q| q.raw()).collect();
        let exposure = vec![Q64::from_int(100).raw(); 4];
        let book = Book::from_grid(
            Q64::from_int(100).raw(),
            &p0,
            &[0, 0, 0, 0],
            &exposure,
            10,
            0,
            5,
            0,
            1,
        );
        assert_eq!(book.l_max_usdc(), 100);
        assert_eq!(book.c_max_usdc(), 10);
        assert!(book.coverage() < Q64::ONE);
        assert_eq!(book.rho_hat(), book.coverage());
        let mut cell0 = vec![false; 4];
        cell0[0] = true;
        let p = book.p_s(&cell0);
        assert!((p.raw() - Q64::from_ratio(1, 4).raw()).abs() < Q64::from_ratio(1, 1000).raw());
    }

    #[test]
    fn mask_cell0() {
        let bits = decode_mask(&[0b0000_0001], 8).unwrap();
        assert!(bits[0] && bits[1..].iter().all(|b| !*b));
    }

    #[test]
    fn view_matches_kernel_fields() {
        let book = uniform_book(8, 4, 5);
        let mut cell0 = vec![false; 8];
        cell0[0] = true;
        let v = book.view(&cell0, Q64::from_int(1));
        assert_eq!(v.p_s, book.p_s(&cell0));
        assert_eq!(v.c_s, book.c_s(&cell0, Q64::from_int(1)));
        assert_eq!(v.coverage, book.coverage());
        assert_eq!(v.rho_hat, book.rho_hat());
        assert_eq!(v.slot, 1);
        assert_eq!(v.n, 8);
    }

    #[test]
    fn ticket_fee_zero_pay_equals_c_s() {
        let book = uniform_book(8, 4, 5);
        let mut cell0 = vec![false; 8];
        cell0[0] = true;
        let v = book.view(&cell0, Q64::from_int(1));
        let t = ticket_from_view(&v, 1, 0);
        assert_eq!(t.fee_usdc, 0);
        assert_eq!(t.pay_usdc, usdc(v.c_s));
        assert_eq!(t.payout_if_miss_usdc, 0);
        assert_eq!(t.face_usdc, 1);
        assert_eq!(t.payout_if_hit_usdc, usdc(v.rho_hat.saturating_mul(Q64::from_int(1))));
        assert_eq!(t.net_if_hit, t.payout_if_hit_usdc as i64 - t.pay_usdc as i64);
    }

    #[test]
    fn ticket_fee_matches_chain_and_does_not_change_price() {
        let book = uniform_book(8, 4, 5);
        let mut cell0 = vec![false; 8];
        cell0[0] = true;
        let v = book.view(&cell0, Q64::from_int(10));
        let zero = ticket_from_view(&v, 10, 0);
        let taxed = ticket_from_view(&v, 10, 1_000);
        let expect_fee = usdc(
            v.c_s
                .saturating_mul(Q64::from_int(1_000))
                .checked_div(Q64::from_int(10_000))
                .unwrap_or(Q64::ZERO),
        );
        assert_eq!(taxed.fee_usdc, expect_fee);
        assert_eq!(taxed.pay_usdc, zero.pay_usdc + expect_fee);
        assert_eq!(taxed.payout_if_hit_usdc, zero.payout_if_hit_usdc);
        assert_eq!(taxed.face_usdc, 10);
        assert_eq!(v.p_s, book.p_s(&cell0));
    }

    #[test]
    fn bps_renorm_sums_to_10000() {
        let p = uniform_prior(159);
        let bps = q_bps_renorm(&p);
        assert_eq!(bps.iter().copied().sum::<u64>(), 10_000);
        assert!(bps.iter().all(|&x| (62..=63).contains(&x)));
    }
}
