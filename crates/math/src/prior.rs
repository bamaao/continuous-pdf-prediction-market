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

/// In-place $\ell_1$ normalize of raw Q64 words. Used by `seal_grid` so it does not allocate $n$ $Q64$s.
pub fn normalize_i128(xs: &mut [i128]) {
    assert!(!xs.is_empty(), "empty weights");
    let mut sum = Q64::ZERO;
    for x in xs.iter() {
        sum = sum.saturating_add(Q64::from_raw(*x));
    }
    assert!(sum.raw() > 0, "zero weight sum");
    let inv = Q64::ONE.checked_div(sum).unwrap_or(Q64::ZERO);
    let mut acc = Q64::ZERO;
    for x in xs.iter_mut() {
        let p = Q64::from_raw(*x).saturating_mul(inv);
        *x = p.raw();
        acc = acc.saturating_add(p);
    }
    if acc != Q64::ONE {
        xs[0] = Q64::from_raw(xs[0])
            .saturating_add(Q64::ONE.saturating_sub(acc))
            .raw();
    }
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

/// $2\sqrt{2}$. Unnormalized mass is 0 when $|x-\mu|>\sigma\sqrt{8}$ (`z^2>8`, same cut as `half>4`).
const SQRT8: Q64 = Q64(0x16A09E667F3BCC908i128 << 1);

fn q_index_floor(dist: Q64, dx: Q64) -> usize {
    if dist.raw() <= 0 || dx.raw() <= 0 {
        return 0;
    }
    let q = dist.checked_div(dx).unwrap_or(Q64::ZERO);
    if q.raw() <= 0 {
        0
    } else {
        (q.raw() >> 64) as usize
    }
}

fn q_index_ceil(dist: Q64, dx: Q64) -> usize {
    if dist.raw() <= 0 || dx.raw() <= 0 {
        return 0;
    }
    let q = dist.checked_div(dx).unwrap_or(Q64::ZERO);
    if q.raw() <= 0 {
        return 0;
    }
    let floor = (q.raw() >> 64) as usize;
    if q.raw() & ((1i128 << 64) - 1) == 0 {
        floor
    } else {
        floor.saturating_add(1)
    }
}

/// Uniform-grid truncated $\mathcal{N}$. Setup once; each live node is one cheap `exp`.
pub struct TruncatedNormal {
    n: usize,
    x_min: Q64,
    dx: Q64,
    mu: Q64,
    inv_sigma: Q64,
    pub i_lo: usize,
    pub i_hi: usize,
}

impl TruncatedNormal {
    pub fn new(n: usize, x_min: Q64, x_max: Q64, mu: Q64, sigma: Q64) -> Self {
        assert!(n >= 2 && x_max > x_min && sigma.raw() > 0);
        let span = x_max.saturating_sub(x_min);
        let den = Q64::from_int((n as i64) - 1);
        let dx = span.checked_div(den).unwrap_or(Q64::ZERO);
        let width = sigma.saturating_mul(SQRT8);
        let i_lo = q_index_ceil(mu.saturating_sub(width).saturating_sub(x_min), dx).min(n);
        let i_hi = q_index_floor(mu.saturating_add(width).saturating_sub(x_min), dx).min(n.saturating_sub(1));
        let inv_sigma = Q64::ONE.checked_div(sigma).unwrap_or(Q64::ZERO);
        Self {
            n,
            x_min,
            dx,
            mu,
            inv_sigma,
            i_lo,
            i_hi,
        }
    }

    fn mass_at_x(&self, x: Q64) -> Q64 {
        let z = x.saturating_sub(self.mu).saturating_mul(self.inv_sigma);
        let zz = z.saturating_mul(z);
        if zz > Q64::from_int(8) {
            Q64::ZERO
        } else {
            let half = zz.checked_div(Q64::from_int(2)).unwrap_or(Q64::ZERO);
            Q64::from_raw(-half.raw()).exp()
        }
    }

    pub fn weight_at(&self, i: usize) -> Q64 {
        if i >= self.n || i < self.i_lo || i > self.i_hi {
            return Q64::ZERO;
        }
        let x = self.x_min.saturating_add(self.dx.saturating_mul(Q64::from_int(i as i64)));
        self.mass_at_x(x)
    }

    /// Write unnormalized masses on `[start, end)` into `p0`, which must already have `len==start`.
    /// Tails are zeros (one `resize`); only the $3\sigma$ window calls `exp`.
    pub fn fill_raw(&self, p0: &mut Vec<i128>, start: usize, end: usize) {
        assert!(p0.len() == start && end <= self.n && start < end);
        p0.resize(end, 0);
        let lo = self.i_lo.max(start);
        let hi = self.i_hi.min(end.saturating_sub(1));
        if lo > hi {
            return;
        }
        let mut x = self
            .x_min
            .saturating_add(self.dx.saturating_mul(Q64::from_int(lo as i64)));
        for i in lo..=hi {
            p0[i] = self.mass_at_x(x).raw();
            x = x.saturating_add(self.dx);
        }
    }
}

/// Fill `out[j] =` unnormalized $\mathcal{N}$ at grid node `start+j`.
pub fn fill_truncated_normal_weights(
    out: &mut [Q64],
    start: usize,
    n: usize,
    x_min: Q64,
    x_max: Q64,
    mu: Q64,
    sigma: Q64,
) {
    let end = start.saturating_add(out.len());
    assert!(end <= n);
    let kn = TruncatedNormal::new(n, x_min, x_max, mu, sigma);
    for (slot, w) in out.iter_mut().enumerate() {
        *w = kn.weight_at(start + slot);
    }
}

/// Unnormalized $\mathcal{N}$ mass at node $i$. Same kernel as `truncated_normal` before `normalize`.
pub fn truncated_normal_weight_at(i: usize, n: usize, x_min: Q64, x_max: Q64, mu: Q64, sigma: Q64) -> Q64 {
    TruncatedNormal::new(n, x_min, x_max, mu, sigma).weight_at(i)
}

pub fn truncated_normal(n: usize, x_min: Q64, x_max: Q64, mu: Q64, sigma: Q64) -> Vec<Q64> {
    let kn = TruncatedNormal::new(n, x_min, x_max, mu, sigma);
    let w: Vec<Q64> = (0..n).map(|i| kn.weight_at(i)).collect();
    normalize(&w)
}

/// Lognormal prior: $\log X\sim\mathcal{N}(\mu,\sigma^2)$ on a log-spaced grid of $X$.
pub fn truncated_lognormal_weight_at(i: usize, n: usize, x_min: Q64, x_max: Q64, mu: Q64, sigma: Q64) -> Q64 {
    assert!(n >= 2 && x_min.raw() > 0 && x_max > x_min && sigma.raw() > 0);
    truncated_normal_weight_at(i, n, x_min.ln(), x_max.ln(), mu, sigma)
}

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

fn simplex_cell_weight(cell: &[u16], alpha: &[Q64], bins: usize) -> Q64 {
    let m = Q64::from_int(bins as i64);
    let mut weight = Q64::ONE;
    for (i, &c) in cell.iter().enumerate() {
        if c == 0 {
            if alpha[i] > Q64::ONE {
                return Q64::ZERO;
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
    weight
}

/// Unnormalized simplex masses on `[start, end)` without an $n$-vector of compositions.
/// `seal_grid` $\ell_1$-normalizes. $k\le 4$ so the walk frame is tiny.
pub fn simplex_chunk_raw(
    k: usize,
    bins: usize,
    alpha: &[Q64],
    start: usize,
    end: usize,
) -> Vec<i128> {
    assert!(k >= 2 && bins >= 1 && alpha.len() == k && start < end);
    let n = simplex_cell_count(k as u32, bins as u32).expect("simplex n") as usize;
    assert!(end <= n);
    let mut out = Vec::with_capacity(end - start);
    let mut idx = 0usize;
    let mut acc = Vec::with_capacity(k);
    simplex_walk(k, bins, &mut acc, &mut idx, start, end, alpha, bins, &mut out);
    assert_eq!(out.len(), end - start);
    out
}

fn simplex_walk(
    k: usize,
    remaining: usize,
    acc: &mut Vec<u16>,
    idx: &mut usize,
    start: usize,
    end: usize,
    alpha: &[Q64],
    bins: usize,
    out: &mut Vec<i128>,
) {
    if *idx >= end {
        return;
    }
    if k == 1 {
        acc.push(remaining as u16);
        if *idx >= start {
            out.push(simplex_cell_weight(acc, alpha, bins).raw());
        }
        *idx += 1;
        acc.pop();
        return;
    }
    for x in 0..=remaining {
        if *idx >= end {
            return;
        }
        acc.push(x as u16);
        simplex_walk(k - 1, remaining - x, acc, idx, start, end, alpha, bins, out);
        acc.pop();
    }
}

/// Discrete Dirichlet on the simplex $\\{s:\sum s_i=1\\}$ with $s_i = c_i / \\mathrm{bins}$.
pub fn vote_share_simplex(k: usize, bins: usize, alpha: &[Q64]) -> Vec<Q64> {
    assert!(k >= 2 && bins >= 1 && alpha.len() == k);
    let n = simplex_cell_count(k as u32, bins as u32).expect("simplex n") as usize;
    let raw = simplex_chunk_raw(k, bins, alpha, 0, n);
    let w: Vec<Q64> = raw.into_iter().map(Q64::from_raw).collect();
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
    fn chunk_weights_normalize_like_truncated_normal() {
        let n = 32;
        let xmin = Q64::from_int(-2);
        let xmax = Q64::from_int(12);
        let mu = Q64::from_int(4);
        let sigma = Q64::ONE;
        let full = truncated_normal(n, xmin, xmax, mu, sigma);
        let w: Vec<Q64> = (0..n)
            .map(|i| truncated_normal_weight_at(i, n, xmin, xmax, mu, sigma))
            .collect();
        let got = normalize(&w);
        for (a, b) in full.iter().zip(got.iter()) {
            assert!(a.approx_eq(*b, 1 << 32));
        }
    }

    #[test]
    fn cpi_n256_support_is_narrow() {
        let n = 256;
        let kn = TruncatedNormal::new(
            n,
            Q64::from_int(-2),
            Q64::from_int(12),
            Q64::from_ratio(24, 10),
            Q64::from_ratio(35, 100),
        );
        let live = (0..n).filter(|i| kn.weight_at(*i).raw() > 0).count();
        assert!(live < 80, "live={live}");
        let p = truncated_normal(
            n,
            Q64::from_int(-2),
            Q64::from_int(12),
            Q64::from_ratio(24, 10),
            Q64::from_ratio(35, 100),
        );
        assert!(sum(&p).approx_eq(Q64::ONE, 1 << 40));
        let mut raw = Vec::new();
        kn.fill_raw(&mut raw, 0, n);
        assert_eq!(raw.len(), n);
        for i in 0..n {
            assert_eq!(raw[i], kn.weight_at(i).raw());
        }
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
    fn simplex_chunk_matches_full_k4() {
        let alpha = [Q64::ONE, Q64::from_int(2), Q64::ONE, Q64::from_int(3)];
        let n = simplex_cell_count(4, 8).unwrap() as usize;
        assert_eq!(n, 165);
        let full = vote_share_simplex(4, 8, &alpha);
        let mut raw = simplex_chunk_raw(4, 8, &alpha, 0, 80);
        raw.extend(simplex_chunk_raw(4, 8, &alpha, 80, n));
        normalize_i128(&mut raw);
        assert_eq!(raw.len(), full.len());
        for i in 0..n {
            assert!(
                Q64::from_raw(raw[i]).approx_eq(full[i], 1 << 40),
                "cell {i}"
            );
        }
    }

    #[test]
    fn binom_values() {
        assert_eq!(binom(4, 2), Some(6));
        assert_eq!(simplex_cell_count(2, 4), Some(5));
        assert_eq!(simplex_cell_count(2, 158), Some(159));
    }
}
