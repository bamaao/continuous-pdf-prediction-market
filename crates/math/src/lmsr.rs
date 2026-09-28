//! Continuous LMSR on a discrete grid / atom list.

use crate::q64::Q64;

/// Trading-period peak of $E(x)$. Not the settlement $L=E(c)$.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeakExposure {
    pub cell: usize,
    pub value: Q64,
    pub run_lo: usize,
    pub run_hi: usize,
    pub ties: usize,
}

/// LMSR book state. `p0` is the genesis mass (sums to 1). `theta` starts at 0.
/// `weights`/`z` cache $w_i=p0_i e^{\theta_i/\beta}$ so a fill is one `exp(q/β)`, not `n` exps.
#[derive(Clone, Debug)]
pub struct LmsrState {
    pub beta: Q64,
    pub p0: Vec<Q64>,
    pub theta: Vec<Q64>,
    pub exposure: Vec<Q64>,
    pub weights: Vec<Q64>,
    pub z: Q64,
}

impl LmsrState {
    pub fn new(beta: Q64, p0: Vec<Q64>) -> Self {
        let n = p0.len();
        assert!(n > 0, "empty grid");
        let z = p0.iter().copied().fold(Q64::ZERO, |a, p| a.saturating_add(p));
        Self {
            beta,
            weights: p0.clone(),
            z,
            p0,
            theta: vec![Q64::ZERO; n],
            exposure: vec![Q64::ZERO; n],
        }
    }

    pub fn from_vecs(beta: Q64, p0: Vec<Q64>, theta: Vec<Q64>, exposure: Vec<Q64>) -> Self {
        assert_eq!(p0.len(), theta.len());
        assert_eq!(p0.len(), exposure.len());
        let weights: Vec<Q64> = (0..p0.len()).map(|i| weight(p0[i], theta[i], beta)).collect();
        let z = weights.iter().copied().fold(Q64::ZERO, |a, w| a.saturating_add(w));
        Self {
            beta,
            p0,
            theta,
            exposure,
            weights,
            z,
        }
    }

    pub fn n(&self) -> usize {
        self.p0.len()
    }

    pub fn l_max(&self) -> Q64 {
        self.exposure.iter().copied().max().unwrap_or(Q64::ZERO)
    }

    /// Hottest atom and the contiguous plateau that shares the same $E$.
    /// Settlement still pays only one cell; this is the trading-period monitor.
    pub fn peak_exposure(&self) -> PeakExposure {
        let mut cell = 0usize;
        let mut value = Q64::ZERO;
        for (i, e) in self.exposure.iter().copied().enumerate() {
            if e > value {
                value = e;
                cell = i;
            }
        }
        let mut run_lo = cell;
        while run_lo > 0 && self.exposure[run_lo - 1] == value {
            run_lo -= 1;
        }
        let mut run_hi = cell;
        while run_hi + 1 < self.exposure.len() && self.exposure[run_hi + 1] == value {
            run_hi += 1;
        }
        let ties = self.exposure.iter().filter(|e| **e == value).count();
        PeakExposure {
            cell,
            value,
            run_lo,
            run_hi,
            ties,
        }
    }

    fn cache_live(&self) -> bool {
        self.weights.len() == self.n() && self.z.raw() > 0
    }
}

fn weight(p0: Q64, theta: Q64, beta: Q64) -> Q64 {
    if theta.raw() == 0 {
        return p0;
    }
    let t_over_b = theta.checked_div(beta).unwrap_or(Q64::ZERO);
    p0.saturating_mul(t_over_b.exp())
}

fn partition(state: &LmsrState) -> Q64 {
    if state.cache_live() {
        return state.z;
    }
    let mut z = Q64::ZERO;
    for i in 0..state.n() {
        z = z.saturating_add(weight(state.p0[i], state.theta[i], state.beta));
    }
    z
}

/// Implied atom masses $p_k$ after trading. This is the PDF, not $E(x)$.
pub fn implied_probs(state: &LmsrState) -> Vec<Q64> {
    if state.cache_live() {
        return state
            .weights
            .iter()
            .map(|w| w.checked_div(state.z).unwrap_or(Q64::ZERO))
            .collect();
    }
    let z = partition(state);
    (0..state.n())
        .map(|i| {
            weight(state.p0[i], state.theta[i], state.beta)
                .checked_div(z)
                .unwrap_or(Q64::ZERO)
        })
        .collect()
}

/// Probability mass of a set `S` (true = in the set).
pub fn interval_prob(state: &LmsrState, in_set: &[bool]) -> Q64 {
    assert_eq!(in_set.len(), state.n());
    if state.cache_live() {
        let mut num = Q64::ZERO;
        for i in 0..state.n() {
            if in_set[i] {
                num = num.saturating_add(state.weights[i]);
            }
        }
        return num.checked_div(state.z).unwrap_or(Q64::ZERO);
    }
    let mut z = Q64::ZERO;
    let mut num = Q64::ZERO;
    for i in 0..state.n() {
        let w = weight(state.p0[i], state.theta[i], state.beta);
        z = z.saturating_add(w);
        if in_set[i] {
            num = num.saturating_add(w);
        }
    }
    assert!(z.raw() > 0, "Z=0");
    num.checked_div(z).unwrap_or(Q64::ZERO)
}

/// $C_S(q) = \beta \ln((1-p)+p e^{q/\beta})$.
pub fn lmsr_cost(beta: Q64, p: Q64, q: Q64) -> Q64 {
    let e = q.checked_div(beta).unwrap_or(Q64::ZERO).exp();
    let inner = Q64::ONE
        .saturating_sub(p)
        .saturating_add(p.saturating_mul(e));
    beta.saturating_mul(inner.ln())
}

pub fn buy_cost(state: &LmsrState, in_set: &[bool], q: Q64) -> Q64 {
    lmsr_cost(state.beta, interval_prob(state, in_set), q)
}

/// In-place fill on the on-chain cache (`weights` / `z`). No extra $n$-vectors.
pub fn lmsr_apply_cached(
    beta: Q64,
    weights: &mut [i128],
    theta: &mut [i128],
    exposure: &mut [i128],
    z: &mut i128,
    in_set: &[bool],
    q: Q64,
) -> Q64 {
    let n = weights.len();
    assert_eq!(theta.len(), n);
    assert_eq!(exposure.len(), n);
    assert_eq!(in_set.len(), n);
    assert!(*z != 0, "z=0");
    let z0 = Q64::from_raw(*z);
    let mut num = Q64::ZERO;
    for i in 0..n {
        if in_set[i] {
            num = num.saturating_add(Q64::from_raw(weights[i]));
        }
    }
    let p = num.checked_div(z0).unwrap_or(Q64::ZERO);
    let cost = lmsr_cost(beta, p, q);
    let e = q.checked_div(beta).unwrap_or(Q64::ZERO).exp();
    let mut z_now = z0;
    for i in 0..n {
        if in_set[i] {
            let w_old = Q64::from_raw(weights[i]);
            let w_new = w_old.saturating_mul(e);
            z_now = z_now.saturating_sub(w_old).saturating_add(w_new);
            weights[i] = w_new.raw();
            theta[i] = Q64::from_raw(theta[i]).saturating_add(q).raw();
            exposure[i] = Q64::from_raw(exposure[i]).saturating_add(q).raw();
        }
    }
    *z = z_now.raw();
    cost
}

/// Instantaneous marginal price after a buy of size `q` (end of the trade).
pub fn marginal_price(state: &LmsrState, in_set: &[bool], q: Q64) -> Q64 {
    let p = interval_prob(state, in_set);
    let e = q.checked_div(state.beta).unwrap_or(Q64::ZERO).exp();
    let num = p.saturating_mul(e);
    let den = Q64::ONE.saturating_sub(p).saturating_add(num);
    num.checked_div(den).unwrap_or(Q64::ZERO)
}

/// Apply a buy of `q` on set `S`. Returns cost $C_S(q)$ (fee is applied by the caller).
pub fn lmsr_update(state: &mut LmsrState, in_set: &[bool], q: Q64) -> Q64 {
    let cost = buy_cost(state, in_set, q);
    let e = q.checked_div(state.beta).unwrap_or(Q64::ZERO).exp();
    let live = state.cache_live();
    for i in 0..state.n() {
        if in_set[i] {
            if live {
                let w_new = state.weights[i].saturating_mul(e);
                state.z = state.z.saturating_sub(state.weights[i]).saturating_add(w_new);
                state.weights[i] = w_new;
            }
            state.theta[i] = state.theta[i].saturating_add(q);
            state.exposure[i] = state.exposure[i].saturating_add(q);
        }
    }
    cost
}

/// Uniform prior on `n` atoms.
pub fn uniform_prior(n: usize) -> Vec<Q64> {
    assert!(n > 0);
    let p = Q64::from_ratio(1, n as i64);
    // Fix rounding so sum is exactly 1.
    let mut v = vec![p; n];
    let sum = p.saturating_mul(Q64::from_int(n as i64));
    if sum != Q64::ONE {
        v[0] = v[0].saturating_add(Q64::ONE.saturating_sub(sum));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(n: usize, start: usize, end: usize) -> Vec<bool> {
        (0..n).map(|i| i >= start && i < end).collect()
    }

    #[test]
    fn prior_is_uniform() {
        let p0 = uniform_prior(4);
        let s = LmsrState::new(Q64::from_int(10), p0);
        let p = interval_prob(&s, &mask(4, 0, 2));
        assert!(p.approx_eq(Q64::from_ratio(1, 2), 1 << 48), "p={}", p.raw());
    }

    #[test]
    fn cached_apply_matches_update() {
        let mut s = LmsrState::new(Q64::from_int(10), uniform_prior(32));
        let mut weights: Vec<i128> = s.weights.iter().map(|q| q.raw()).collect();
        let mut theta: Vec<i128> = s.theta.iter().map(|q| q.raw()).collect();
        let mut exposure: Vec<i128> = s.exposure.iter().map(|q| q.raw()).collect();
        let mut z = s.z.raw();
        let m = mask(32, 4, 12);
        let c1 = lmsr_update(&mut s, &m, Q64::ONE);
        let c2 = lmsr_apply_cached(
            Q64::from_int(10),
            &mut weights,
            &mut theta,
            &mut exposure,
            &mut z,
            &m,
            Q64::ONE,
        );
        assert!(c1.approx_eq(c2, 1 << 32));
        for i in 0..32 {
            assert_eq!(weights[i], s.weights[i].raw());
            assert_eq!(theta[i], s.theta[i].raw());
            assert_eq!(exposure[i], s.exposure[i].raw());
        }
        assert_eq!(z, s.z.raw());
    }

    #[test]
    fn buy_raises_p() {
        let mut s = LmsrState::new(Q64::from_int(10), uniform_prior(4));
        let m = mask(4, 0, 2);
        let p0 = interval_prob(&s, &m);
        let _c = lmsr_update(&mut s, &m, Q64::ONE);
        let p1 = interval_prob(&s, &m);
        assert!(p1 > p0, "p should rise: {} vs {}", p1.raw(), p0.raw());
    }

    #[test]
    fn masses_sum_to_one() {
        let mut s = LmsrState::new(Q64::from_int(8), uniform_prior(5));
        lmsr_update(&mut s, &mask(5, 1, 3), Q64::from_ratio(1, 2));
        let mut acc = Q64::ZERO;
        for i in 0..5 {
            let mut m = vec![false; 5];
            m[i] = true;
            acc = acc.saturating_add(interval_prob(&s, &m));
        }
        assert!(acc.approx_eq(Q64::ONE, 1 << 46), "sum p={}", acc.raw());
    }

    #[test]
    fn cost_positive() {
        let s = LmsrState::new(Q64::from_int(10), uniform_prior(4));
        let c = buy_cost(&s, &mask(4, 0, 1), Q64::ONE);
        assert!(c.raw() > 0);
    }

    #[test]
    fn incremental_matches_recompute() {
        let mut s = LmsrState::new(Q64::from_int(10), uniform_prior(8));
        for k in 0..80 {
            let start = k % 8;
            lmsr_update(&mut s, &mask(8, start, start + 1), Q64::from_ratio(1, 5));
        }
        let rebuilt = LmsrState::from_vecs(s.beta, s.p0.clone(), s.theta.clone(), s.exposure.clone());
        assert!(s.z.approx_eq(rebuilt.z, 1 << 40), "z {} vs {}", s.z.raw(), rebuilt.z.raw());
        for i in 0..8 {
            assert!(
                s.weights[i].approx_eq(rebuilt.weights[i], 1 << 40),
                "w[{}] {} vs {}",
                i,
                s.weights[i].raw(),
                rebuilt.weights[i].raw()
            );
        }
    }

    #[test]
    fn peak_is_hottest_atom_not_interval_sum() {
        let mut s = LmsrState::new(Q64::from_int(10), uniform_prior(5));
        lmsr_update(&mut s, &mask(5, 1, 4), Q64::from_int(2));
        lmsr_update(&mut s, &mask(5, 2, 3), Q64::from_int(3));
        let peak = s.peak_exposure();
        assert_eq!(peak.cell, 2);
        assert_eq!(peak.value, Q64::from_int(5));
        assert_eq!(peak.run_lo, 2);
        assert_eq!(peak.run_hi, 2);
        assert_eq!(peak.ties, 1);
        assert_eq!(s.l_max(), peak.value);
    }
}
