//! Continuous LMSR on a discrete grid / atom list.

use crate::q64::Q64;

/// LMSR book state. `p0` is the genesis mass (sums to 1). `theta` starts at 0.
#[derive(Clone, Debug)]
pub struct LmsrState {
    pub beta: Q64,
    pub p0: Vec<Q64>,
    pub theta: Vec<Q64>,
    pub exposure: Vec<Q64>,
}

impl LmsrState {
    pub fn new(beta: Q64, p0: Vec<Q64>) -> Self {
        let n = p0.len();
        assert!(n > 0, "empty grid");
        Self {
            beta,
            p0,
            theta: vec![Q64::ZERO; n],
            exposure: vec![Q64::ZERO; n],
        }
    }

    pub fn n(&self) -> usize {
        self.p0.len()
    }

    pub fn l_max(&self) -> Q64 {
        self.exposure.iter().copied().max().unwrap_or(Q64::ZERO)
    }
}

fn weight(p0: Q64, theta: Q64, beta: Q64) -> Q64 {
    let t_over_b = theta.checked_div(beta).unwrap_or(Q64::ZERO);
    p0.saturating_mul(t_over_b.exp())
}

fn partition(state: &LmsrState) -> Q64 {
    let mut z = Q64::ZERO;
    for i in 0..state.n() {
        z = z.saturating_add(weight(state.p0[i], state.theta[i], state.beta));
    }
    z
}

/// Implied atom masses $p_k$ after trading. This is the PDF, not $E(x)$.
pub fn implied_probs(state: &LmsrState) -> Vec<Q64> {
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
    let z = partition(state);
    assert!(z.raw() > 0, "Z=0");
    let mut num = Q64::ZERO;
    for i in 0..state.n() {
        if in_set[i] {
            num = num.saturating_add(weight(state.p0[i], state.theta[i], state.beta));
        }
    }
    num.checked_div(z).unwrap_or(Q64::ZERO)
}

/// $C_S(q) = \beta \ln((1-p)+p e^{q/\beta})$.
pub fn buy_cost(state: &LmsrState, in_set: &[bool], q: Q64) -> Q64 {
    let p = interval_prob(state, in_set);
    let e = q.checked_div(state.beta).unwrap_or(Q64::ZERO).exp();
    let inner = Q64::ONE
        .saturating_sub(p)
        .saturating_add(p.saturating_mul(e));
    state.beta.saturating_mul(inner.ln())
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
    for i in 0..state.n() {
        if in_set[i] {
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
}
