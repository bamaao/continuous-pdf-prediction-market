//! Genesis masses $p_0$. Written once at create; trading never rewrites them.

use crate::lmsr::uniform_prior;
use crate::q64::Q64;

pub fn normalize(weights: &[Q64]) -> Vec<Q64> {
    assert!(!weights.is_empty(), "empty weights");
    let mut sum = Q64::ZERO;
    for w in weights {
        sum = sum.saturating_add(*w);
    }
    assert!(sum.raw() > 0, "zero weight sum");
    let mut v: Vec<Q64> = weights
        .iter()
        .map(|w| w.checked_div(sum).unwrap_or(Q64::ZERO))
        .collect();
    let mut acc = Q64::ZERO;
    for p in &v {
        acc = acc.saturating_add(*p);
    }
    if acc != Q64::ONE {
        v[0] = v[0].saturating_add(Q64::ONE.saturating_sub(acc));
    }
    v
}

pub fn dirichlet(alpha: &[Q64]) -> Vec<Q64> {
    assert!(alpha.len() >= 2, "dirichlet K>=2");
    for a in alpha {
        assert!(a.raw() > 0, "alpha must be > 0");
    }
    normalize(alpha)
}

pub fn binary(alpha_yes: Q64, alpha_no: Q64) -> Vec<Q64> {
    dirichlet(&[alpha_yes, alpha_no])
}

fn poisson_pmf_trunc(k_max: u32, lambda: Q64) -> Vec<Q64> {
    assert!(lambda.raw() > 0, "lambda must be > 0");
    let mut p = Q64::from_raw(-lambda.raw()).exp();
    let mut v = Vec::with_capacity(k_max as usize + 1);
    v.push(p);
    for k in 1..=k_max {
        p = p
            .saturating_mul(lambda)
            .checked_div(Q64::from_int(k as i64))
            .unwrap_or(Q64::ZERO);
        v.push(p);
    }
    v
}

/// Independent Poisson on a $(k_{\max}+1)^2$ score grid, then renormalized.
pub fn independent_poisson_2d(k_max: u32, lambda_home: Q64, lambda_away: Q64) -> Vec<Q64> {
    let n = k_max as usize + 1;
    let ph = poisson_pmf_trunc(k_max, lambda_home);
    let pa = poisson_pmf_trunc(k_max, lambda_away);
    let mut w = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            w.push(ph[i].saturating_mul(pa[j]));
        }
    }
    normalize(&w)
}

/// Dixon–Coles low-score correction on the independent Poisson table.
pub fn dixon_coles_2d(k_max: u32, lambda_home: Q64, lambda_away: Q64, rho: Q64) -> Vec<Q64> {
    let n = k_max as usize + 1;
    let mut w = independent_poisson_2d(k_max, lambda_home, lambda_away);
    let lh_la = lambda_home.saturating_mul(lambda_away);
    let tau = |i: usize, j: usize| -> Q64 {
        match (i, j) {
            (0, 0) => Q64::ONE.saturating_sub(lh_la.saturating_mul(rho)),
            (0, 1) => Q64::ONE.saturating_add(lambda_home.saturating_mul(rho)),
            (1, 0) => Q64::ONE.saturating_add(lambda_away.saturating_mul(rho)),
            (1, 1) => Q64::ONE.saturating_sub(rho),
            _ => Q64::ONE,
        }
    };
    for i in 0..n {
        for j in 0..n {
            w[i * n + j] = w[i * n + j].saturating_mul(tau(i, j));
        }
    }
    normalize(&w)
}

pub fn truncated_normal(n: usize, x_min: Q64, x_max: Q64, mu: Q64, sigma: Q64) -> Vec<Q64> {
    assert!(n >= 2 && x_max > x_min && sigma.raw() > 0);
    let span = x_max.saturating_sub(x_min);
    let den = Q64::from_int((n as i64) - 1);
    let mut w = Vec::with_capacity(n);
    for i in 0..n {
        let x = x_min.saturating_add(
            span.saturating_mul(Q64::from_int(i as i64))
                .checked_div(den)
                .unwrap_or(Q64::ZERO),
        );
        let z = x.saturating_sub(mu).checked_div(sigma).unwrap_or(Q64::ZERO);
        let half = z.saturating_mul(z).checked_div(Q64::from_int(2)).unwrap_or(Q64::ZERO);
        // Short Taylor exp() is only trustworthy near 0; drop >~3σ tails then renormalize.
        if half > Q64::from_int(4) {
            w.push(Q64::ZERO);
        } else {
            w.push(Q64::from_raw(-half.raw()).exp());
        }
    }
    normalize(&w)
}

/// Lognormal prior: $\log X\sim\mathcal{N}(\mu,\sigma^2)$ on a log-spaced grid of $X$.
pub fn truncated_lognormal(n: usize, x_min: Q64, x_max: Q64, mu: Q64, sigma: Q64) -> Vec<Q64> {
    assert!(n >= 2 && x_min.raw() > 0 && x_max > x_min && sigma.raw() > 0);
    truncated_normal(n, x_min.ln(), x_max.ln(), mu, sigma)
}

pub fn binom(n: u32, k: u32) -> Option<u32> {
    if k > n {
        return Some(0);
    }
    let k = k.min(n - k);
    let mut acc = 1u64;
    for i in 0..k {
        acc = acc
            .saturating_mul((n - i) as u64)
            / (i as u64 + 1);
        if acc > u32::MAX as u64 {
            return None;
        }
    }
    Some(acc as u32)
}

pub fn simplex_cell_count(k: u32, bins: u32) -> Option<u32> {
    if k < 2 || bins == 0 {
        return None;
    }
    binom(bins + k - 1, k - 1)
}

fn compositions(k: usize, sum: usize, acc: &mut Vec<u16>, out: &mut Vec<Vec<u16>>) {
    if k == 1 {
        acc.push(sum as u16);
        out.push(acc.clone());
        acc.pop();
        return;
    }
    for x in 0..=sum {
        acc.push(x as u16);
        compositions(k - 1, sum - x, acc, out);
        acc.pop();
    }
}

/// Discrete Dirichlet on the simplex $\\{s:\sum s_i=1\\}$ with $s_i = c_i / \\mathrm{bins}$.
pub fn vote_share_simplex(k: usize, bins: usize, alpha: &[Q64]) -> Vec<Q64> {
    assert!(k >= 2 && bins >= 1 && alpha.len() == k);
    let mut cells = Vec::new();
    compositions(k, bins, &mut Vec::new(), &mut cells);
    let m = Q64::from_int(bins as i64);
    let mut w = Vec::with_capacity(cells.len());
    for cell in &cells {
        let mut weight = Q64::ONE;
        for (i, &c) in cell.iter().enumerate() {
            if c == 0 {
                if alpha[i] > Q64::ONE {
                    weight = Q64::ZERO;
                    break;
                }
                continue;
            }
            let s = Q64::from_int(c as i64).checked_div(m).unwrap_or(Q64::ZERO);
            let exp = alpha[i].saturating_sub(Q64::ONE);
            if exp.raw() == 0 {
                continue;
            }
            weight = weight.saturating_mul(exp.saturating_mul(s.ln()).exp());
        }
        w.push(weight);
    }
    normalize(&w)
}

pub fn football_uniform(k_max: u32) -> Vec<Q64> {
    let n = (k_max as usize + 1) * (k_max as usize + 1);
    uniform_prior(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(v: &[Q64]) -> Q64 {
        v.iter().copied().fold(Q64::ZERO, Q64::saturating_add)
    }

    #[test]
    fn dirichlet_uninformative() {
        let p = dirichlet(&[Q64::ONE, Q64::ONE, Q64::ONE]);
        assert_eq!(p.len(), 3);
        assert!(sum(&p).approx_eq(Q64::ONE, 1 << 40));
        assert!(p[0].approx_eq(Q64::from_ratio(1, 3), 1 << 48));
    }

    #[test]
    fn binary_fifty_fifty() {
        let p = binary(Q64::ONE, Q64::ONE);
        assert!(p[0].approx_eq(Q64::from_ratio(1, 2), 1 << 48));
    }

    #[test]
    fn football_121_cells() {
        let p = independent_poisson_2d(10, Q64::from_ratio(3, 2), Q64::from_ratio(11, 10));
        assert_eq!(p.len(), 121);
        assert!(sum(&p).approx_eq(Q64::ONE, 1 << 40));
        assert!(p[0].raw() > 0);
    }

    #[test]
    fn gauss_peaks_near_mu() {
        let p = truncated_normal(
            32,
            Q64::from_int(-2),
            Q64::from_int(12),
            Q64::from_int(4),
            Q64::ONE,
        );
        let mid = p.iter().enumerate().max_by_key(|(_, x)| x.raw()).unwrap().0;
        assert!((10..=16).contains(&mid), "peak at {mid}");
        assert!(sum(&p).approx_eq(Q64::ONE, 1 << 40));
    }

    #[test]
    fn lognormal_positive_support() {
        let p = truncated_lognormal(
            16,
            Q64::from_int(10),
            Q64::from_int(250),
            Q64::from_int(4),
            Q64::from_ratio(1, 4),
        );
        assert_eq!(p.len(), 16);
        assert!(sum(&p).approx_eq(Q64::ONE, 1 << 40));
    }

    #[test]
    fn simplex_k2() {
        let p = vote_share_simplex(2, 4, &[Q64::ONE, Q64::ONE]);
        assert_eq!(p.len(), 5);
        assert!(sum(&p).approx_eq(Q64::ONE, 1 << 40));
    }

    #[test]
    fn binom_values() {
        assert_eq!(binom(4, 2), Some(6));
        assert_eq!(simplex_cell_count(2, 4), Some(5));
    }
}
