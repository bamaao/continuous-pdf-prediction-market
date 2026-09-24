//! Map a finalized $x^*$ onto the grid cell whose $E$ is $L$.

use crate::football;
use crate::q64::Q64;

pub const FAMILY_SKELLAM: u8 = 0;
pub const FAMILY_GAUSSIAN: u8 = 1;
pub const FAMILY_LOGNORMAL: u8 = 2;
pub const FAMILY_DIRICHLET: u8 = 3;
pub const FAMILY_BERNOULLI: u8 = 4;

/// Nearest node on the listing grid $x_{\min}+i\cdot\mathrm{span}/(n-1)$.
pub fn interval_index(x: Q64, x_min: Q64, x_max: Q64, n: usize) -> Option<usize> {
    if n < 2 || x_max <= x_min {
        return None;
    }
    let span = x_max.saturating_sub(x_min);
    let t = x.saturating_sub(x_min);
    let num = t.saturating_mul(Q64::from_int((n as i64) - 1));
    let q = num.checked_div(span)?;
    let rounded = if q.raw() < 0 {
        0
    } else {
        (q.raw() as u128 + (1u128 << 63)) >> 64
    };
    Some((rounded as usize).min(n - 1))
}

/// Cell of $E(x^*)$. Dirichlet share-vectors are not a cell (kind 3 → None).
pub fn outcome_cell(
    family: u8,
    n: usize,
    k_max: u8,
    extra_a: i128,
    extra_b: i128,
    kind: u8,
    a: i128,
    b: i128,
) -> Option<usize> {
    if n == 0 {
        return None;
    }
    match family {
        FAMILY_SKELLAM => {
            if kind != 0 || a < 0 || b < 0 {
                return None;
            }
            Some(football::cell(a as u32, b as u32, k_max as u32))
        }
        FAMILY_GAUSSIAN => {
            if kind != 1 {
                return None;
            }
            interval_index(
                Q64::from_raw(a),
                Q64::from_raw(extra_a),
                Q64::from_raw(extra_b),
                n,
            )
        }
        FAMILY_LOGNORMAL => {
            if kind != 1 || a <= 0 || extra_a <= 0 {
                return None;
            }
            interval_index(
                Q64::from_raw(a).ln(),
                Q64::from_raw(extra_a).ln(),
                Q64::from_raw(extra_b).ln(),
                n,
            )
        }
        FAMILY_DIRICHLET => {
            if kind != 2 || a < 0 {
                return None;
            }
            let i = a as usize;
            (i < n).then_some(i)
        }
        FAMILY_BERNOULLI => {
            if kind != 4 || (a != 0 && a != 1) || n < 2 {
                return None;
            }
            Some(a as usize)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skellam_overflow_uses_k_max_bucket() {
        assert_eq!(outcome_cell(0, 121, 10, 0, 0, 0, 12, 1), Some(football::cell(10, 1, 10)));
    }

    #[test]
    fn bernoulli_yes_is_cell_one() {
        assert_eq!(outcome_cell(4, 2, 0, 0, 0, 4, 1, 0), Some(1));
    }

    #[test]
    fn interval_picks_nearest_node() {
        let lo = Q64::from_int(0);
        let hi = Q64::from_int(10);
        assert_eq!(interval_index(Q64::from_int(0), lo, hi, 11), Some(0));
        assert_eq!(interval_index(Q64::from_int(10), lo, hi, 11), Some(10));
        assert_eq!(interval_index(Q64::from_int(5), lo, hi, 11), Some(5));
    }

    #[test]
    fn l_is_cell_not_l_max() {
        // Settlement reads one cell; L_max is a different index.
        let e = [3u64, 9, 4];
        let cell = outcome_cell(4, 3, 0, 0, 0, 4, 0, 0).unwrap();
        assert_eq!(e[cell], 3);
        assert_eq!(*e.iter().max().unwrap(), 9);
    }
}
