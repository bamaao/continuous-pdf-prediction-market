//! Settlement: $C_{\max}$, $\rho$, surplus, layer loss $H$.

use crate::q64::Q64;

/// $C_{\max} = R_{\mathrm{net}} + C_R^{\mathrm{final}} + C_P^{\mathrm{alloc}}$.
/// The unused `c_m` argument is kept so call sites compile; it is never added.
pub fn c_max(r_net: Q64, _c_m: Q64, c_r_final: Q64, c_p_alloc: Q64) -> Q64 {
    r_net.saturating_add(c_r_final).saturating_add(c_p_alloc)
}

/// $C_P^{\mathrm{alloc}}=\min(\mathrm{shortfall}, C_P^{\mathrm{board}}, C_P^{\mathrm{pool}})$.
/// Shortfall is $(L-R_{\mathrm{net}}-C_R)^+$. Own-funds path: shortfall 0.
pub fn c_p_alloc(liability: u64, r_net: u64, _c_m: u64, c_r: u64, cap: u64, pool: u64) -> u64 {
    let own = r_net;
    if liability <= own {
        return 0;
    }
    let after_r = liability.saturating_sub(own).saturating_sub(c_r);
    after_r.min(cap).min(pool)
}

/// $\rho = \min(1, C_{\max}/L)$. If $L=0$, $\rho=1$.
pub fn recovery_rate(c_max: Q64, liability: Q64) -> Q64 {
    if liability.raw() <= 0 {
        return Q64::ONE;
    }
    let ratio = c_max.checked_div(liability).unwrap_or(Q64::ZERO);
    if ratio > Q64::ONE {
        Q64::ONE
    } else {
        ratio
    }
}

/// $S = \max(R_{\mathrm{net}}-L, 0)$ only meaningful when $\rho=1$.
pub fn surplus(r_net: Q64, _c_m: Q64, liability: Q64, rho: Q64) -> Q64 {
    if rho < Q64::ONE {
        return Q64::ZERO;
    }
    let s = r_net.saturating_sub(liability);
    if s.raw() < 0 {
        Q64::ZERO
    } else {
        s
    }
}

/// Integer USDC of a non-negative Q64 amount. Negative is 0.
pub fn usdc(q: Q64) -> u64 {
    if q.raw() <= 0 {
        0
    } else {
        (q.raw() as u128 >> 64) as u64
    }
}

/// Buy debit: smallest integer USDC that covers a positive Q64 `C_S`.
/// Floor would mint face-$1 shares for free and force $\rho=0$ when $C_{\max}=0$.
pub fn usdc_charge(q: Q64) -> u64 {
    if q.raw() <= 0 {
        return 0;
    }
    let floor = usdc(q);
    let frac = (q.raw() as u128) & ((1u128 << 64) - 1);
    if frac == 0 {
        floor
    } else {
        floor.saturating_add(1)
    }
}

/// Face USDC of one ticket at $x^*$.
/// Ordinary set: `parts_total=1`, hit → $q$, miss → $0$.
/// Quarter line: `parts_total=2`, both legs → $q$, one leg → $q/2$.
pub fn ticket_face(q_usdc: u64, parts_hit: u32, parts_total: u32) -> u64 {
    if q_usdc == 0 || parts_hit == 0 || parts_total == 0 {
        return 0;
    }
    if parts_hit >= parts_total {
        return q_usdc;
    }
    ((q_usdc as u128) * (parts_hit as u128) / (parts_total as u128)) as u64
}

/// $R_{\mathrm{net}}=$ trading revenue minus premium payable. Fees never enter.
pub fn r_net(trading_revenue: u64, premium_payable: u64) -> u64 {
    trading_revenue.saturating_sub(premium_payable)
}

/// $\lfloor\rho\cdot q\rfloor$ in USDC. Dust stays in reserves (FR-SET-05).
pub fn payout_floor(rho: Q64, q_usdc: u64) -> u64 {
    if rho.raw() <= 0 || q_usdc == 0 {
        return 0;
    }
    let prod = (rho.raw() as u128).saturating_mul(q_usdc as u128);
    (prod >> 64) as u64
}

/// $S_R=\alpha_R S$, $S_P=S-S_R$. $\alpha_R$ in bps.
pub fn surplus_parts(s: u64, alpha_r_bps: u16) -> (u64, u64) {
    let sr = (s as u128)
        .saturating_mul(alpha_r_bps.min(10_000) as u128)
        / 10_000;
    let sr = sr as u64;
    (sr, s.saturating_sub(sr))
}

/// Slice of surplus that funds the protocol loss-cover pool (20%).
pub const COVER_BPS: u16 = 2_000;

pub fn surplus_cover(s: u64) -> u64 {
    ((s as u128) * (COVER_BPS as u128) / 10_000) as u64
}

/// $(S_C, S_R, S_P)$. $S_C=20\%S$ always when $S>0$. Remainder: all $S_P$ if no $C_R$, else $\alpha_R$ split.
pub fn surplus_split(s: u64, alpha_r_bps: u16, has_cr: bool) -> (u64, u64, u64) {
    let sc = surplus_cover(s);
    let rest = s.saturating_sub(sc);
    if !has_cr {
        (sc, 0, rest)
    } else {
        let (sr, sp) = surplus_parts(rest, alpha_r_bps);
        (sc, sr, sp)
    }
}

/// $H_{A,D}(L)=\min((L-A)^+, D)$.
pub fn layer_loss(liability: Q64, attachment: Q64, thickness: Q64) -> Q64 {
    let over = liability.saturating_sub(attachment);
    let pos = if over.raw() < 0 { Q64::ZERO } else { over };
    if pos > thickness {
        thickness
    } else {
        pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_charge_ceils_fractional_cs() {
        assert_eq!(usdc_charge(Q64::ZERO), 0);
        assert_eq!(usdc_charge(Q64::from_int(2)), 2);
        assert_eq!(usdc_charge(Q64::from_ratio(2, 5)), 1);
    }

    #[test]
    fn rho_one_when_funded() {
        let c = c_max(Q64::from_int(30), Q64::ZERO, Q64::from_int(20), Q64::ZERO);
        assert_eq!(c, Q64::from_int(50));
        assert_eq!(recovery_rate(c, Q64::from_int(40)), Q64::ONE);
    }

    #[test]
    fn rho_pro_rata() {
        let c = Q64::from_int(50);
        let l = Q64::from_int(100);
        let rho = recovery_rate(c, l);
        assert!(rho.approx_eq(Q64::from_ratio(1, 2), 1 << 40));
    }

    #[test]
    fn surplus_zero_if_haircut() {
        let rho = Q64::from_ratio(1, 2);
        let s = surplus(Q64::from_int(10), Q64::from_int(10), Q64::from_int(40), rho);
        assert_eq!(s, Q64::ZERO);
    }

    #[test]
    fn surplus_when_full_pay() {
        let s = surplus(Q64::from_int(60), Q64::ZERO, Q64::from_int(50), Q64::ONE);
        assert_eq!(s, Q64::from_int(10));
    }

    #[test]
    fn layer_hits_middle() {
        // L=80, A=50, D=40 → H=30
        let h = layer_loss(Q64::from_int(80), Q64::from_int(50), Q64::from_int(40));
        assert_eq!(h, Q64::from_int(30));
    }

    #[test]
    fn layer_caps_at_d() {
        let h = layer_loss(Q64::from_int(200), Q64::from_int(50), Q64::from_int(40));
        assert_eq!(h, Q64::from_int(40));
    }

    #[test]
    fn same_rho_for_two_winners() {
        let rho = recovery_rate(Q64::from_int(90), Q64::from_int(100));
        let pay_a = rho.saturating_mul(Q64::from_int(60));
        let pay_b = rho.saturating_mul(Q64::from_int(40));
        let sum = pay_a.saturating_add(pay_b);
        assert!(sum.approx_eq(Q64::from_int(90), 1 << 32));
    }

    #[test]
    fn same_rho_floor_two_winners_no_fifo() {
        let rho = recovery_rate(Q64::from_int(90), Q64::from_int(100));
        let a = payout_floor(rho, 60);
        let b = payout_floor(rho, 40);
        assert!(a + b <= 90, "{a}+{b}");
        assert!((a as i64) * 2 - (b as i64) * 3 <= 2);
        assert!((b as i64) * 3 - (a as i64) * 2 <= 2);
        assert!(a > b);
    }

    #[test]
    fn fees_never_enter_r_net() {
        // 100 cost + 5 fee recorded separately; premium 20
        assert_eq!(r_net(100, 20), 80);
    }

    #[test]
    fn surplus_split_and_zero_when_haircut() {
        assert_eq!(surplus_parts(100, 7_000), (70, 30));
        assert_eq!(surplus_split(100, 7_000, true), (20, 56, 24));
        assert_eq!(surplus_split(100, 7_000, false), (20, 0, 80));
        assert_eq!(surplus_split(0, 7_000, true), (0, 0, 0));
        let s = usdc(surplus(
            Q64::from_int(10),
            Q64::from_int(10),
            Q64::from_int(40),
            Q64::from_ratio(1, 2),
        ));
        assert_eq!(s, 0);
    }

    #[test]
    fn ticket_face_quarter_is_half_or_full() {
        assert_eq!(ticket_face(100, 0, 2), 0);
        assert_eq!(ticket_face(100, 1, 2), 50);
        assert_eq!(ticket_face(100, 2, 2), 100);
        assert_eq!(ticket_face(60, 1, 1), 60);
        assert_eq!(ticket_face(60, 0, 1), 0);
    }

    #[test]
    fn trade_only_pool_when_no_cm_no_cr() {
        let c = c_max(Q64::from_int(40), Q64::ZERO, Q64::ZERO, Q64::ZERO);
        assert_eq!(c_p_alloc(100, 40, 0, 0, 20, 50), 20);
        assert_eq!(c_p_alloc(30, 40, 0, 0, 20, 50), 0);
        assert_eq!(c, Q64::from_int(40));
        assert_eq!(recovery_rate(c, Q64::from_int(100)), Q64::from_ratio(2, 5));
        assert_eq!(
            usdc(surplus(Q64::from_int(40), Q64::ZERO, Q64::from_int(30), Q64::ONE)),
            10
        );
    }
}
