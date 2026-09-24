//! Settlement: $C_{\max}$, $\rho$, surplus, layer loss $H$.

use crate::q64::Q64;

/// $C_{\max} = R_{\mathrm{net}} + C_M + C_R^{\mathrm{final}}$.
pub fn c_max(r_net: Q64, c_m: Q64, c_r_final: Q64) -> Q64 {
    r_net.saturating_add(c_m).saturating_add(c_r_final)
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

/// $S = \max(R_{\mathrm{net}}+C_M-L, 0)$ only meaningful when $\rho=1$.
pub fn surplus(r_net: Q64, c_m: Q64, liability: Q64, rho: Q64) -> Q64 {
    if rho < Q64::ONE {
        return Q64::ZERO;
    }
    let s = r_net.saturating_add(c_m).saturating_sub(liability);
    if s.raw() < 0 {
        Q64::ZERO
    } else {
        s
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
    fn rho_one_when_funded() {
        let c = c_max(Q64::from_int(30), Q64::from_int(50), Q64::from_int(20));
        assert_eq!(c, Q64::from_int(100));
        assert_eq!(recovery_rate(c, Q64::from_int(80)), Q64::ONE);
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
        let s = surplus(Q64::from_int(40), Q64::from_int(20), Q64::from_int(50), Q64::ONE);
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
}
