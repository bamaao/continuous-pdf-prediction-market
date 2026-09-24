//! One Skellam / 2-D score board. 1X2, handicap, totals, and exact score are
//! set projections of the same $\theta[i][j]$. LMSR never opens a second book.

use crate::lmsr::{buy_cost, interval_prob, lmsr_update, LmsrState};
use crate::q64::Q64;

pub const K_MAX: u32 = 10;
pub const N_CELLS: usize = 121;

pub fn cell(home: u32, away: u32, k_max: u32) -> usize {
    let i = home.min(k_max) as usize;
    let j = away.min(k_max) as usize;
    i * (k_max as usize + 1) + j
}

fn cells(k_max: u32) -> usize {
    (k_max as usize + 1) * (k_max as usize + 1)
}

fn map_cells(k_max: u32, pred: impl Fn(u32, u32) -> bool) -> Vec<bool> {
    let n = k_max + 1;
    let mut m = vec![false; cells(k_max)];
    for i in 0..n {
        for j in 0..n {
            m[cell(i, j, k_max)] = pred(i, j);
        }
    }
    m
}

pub fn mask_home(k_max: u32) -> Vec<bool> {
    map_cells(k_max, |i, j| i > j)
}

pub fn mask_draw(k_max: u32) -> Vec<bool> {
    map_cells(k_max, |i, j| i == j)
}

pub fn mask_away(k_max: u32) -> Vec<bool> {
    map_cells(k_max, |i, j| i < j)
}

/// Totals line in half-goals: 5 = 2.5. Over: $2(i+j) > \mathrm{halves}$.
pub fn mask_over(k_max: u32, halves: i32) -> Vec<bool> {
    map_cells(k_max, |i, j| 2 * (i + j) as i32 > halves)
}

pub fn mask_under(k_max: u32, halves: i32) -> Vec<bool> {
    map_cells(k_max, |i, j| 2 * (i + j) as i32 <= halves)
}

pub fn mask_btts_yes(k_max: u32) -> Vec<bool> {
    map_cells(k_max, |i, j| i >= 1 && j >= 1)
}

pub fn mask_btts_no(k_max: u32) -> Vec<bool> {
    map_cells(k_max, |i, j| i == 0 || j == 0)
}

pub fn mask_exact(k_max: u32, home: u32, away: u32) -> Vec<bool> {
    let mut m = vec![false; cells(k_max)];
    m[cell(home, away, k_max)] = true;
    m
}

/// Asian line on home, in half-goals: -1 = -0.5, -2 = -1, -3 = -1.5.
/// Wins iff $2(i-j)+\mathrm{halves}>0$. Integer push ($=0$) is not in $S$.
pub fn mask_home_handicap(k_max: u32, halves: i32) -> Vec<bool> {
    map_cells(k_max, |i, j| 2 * (i as i32 - j as i32) + halves > 0)
}

pub fn mask_away_handicap(k_max: u32, halves: i32) -> Vec<bool> {
    map_cells(k_max, |i, j| 2 * (j as i32 - i as i32) + halves > 0)
}

/// Expand a typed Skellam ticket to the LMSR set(s) written at fill.
/// Ordinary lines are one set. Quarter lines are two half-stake sets.
pub fn skellam_masks(kind: u8, a: i16, b: i16, k_max: u32) -> Option<Vec<Vec<bool>>> {
    Some(match kind {
        0 => vec![mask_home(k_max)],
        1 => vec![mask_draw(k_max)],
        2 => vec![mask_away(k_max)],
        3 => vec![mask_over(k_max, a as i32)],
        4 => vec![mask_under(k_max, a as i32)],
        5 => vec![mask_btts_yes(k_max)],
        6 => vec![mask_btts_no(k_max)],
        7 => vec![mask_exact(k_max, a as u32, b as u32)],
        8 => vec![mask_home_handicap(k_max, a as i32)],
        9 => vec![mask_away_handicap(k_max, a as i32)],
        10 => {
            if a % 2 == 0 {
                return None;
            }
            let (x, y) = quarter_to_halves(a as i32);
            vec![mask_home_handicap(k_max, x), mask_home_handicap(k_max, y)]
        }
        11 => {
            if a % 2 == 0 {
                return None;
            }
            let (x, y) = quarter_to_halves(a as i32);
            vec![mask_away_handicap(k_max, x), mask_away_handicap(k_max, y)]
        }
        _ => return None,
    })
}

/// `(parts_hit, parts_total)` of a typed ticket at `cell`.
/// Fill writes $q/\mathrm{total}$ onto each part; settle must credit the same face.
pub fn skellam_hit_parts(kind: u8, a: i16, b: i16, k_max: u32, cell: usize) -> Option<(u32, u32)> {
    let masks = skellam_masks(kind, a, b, k_max)?;
    let n = masks.first()?.len();
    if cell >= n {
        return None;
    }
    let total = masks.len() as u32;
    let hit = masks.iter().filter(|m| m[cell]).count() as u32;
    Some((hit, total))
}

/// Quarter line in quarter-goals (odd): -3 = -0.75 → (-1.0, -0.5) half-lines.
pub fn quarter_to_halves(quarters: i32) -> (i32, i32) {
    assert!(quarters % 2 != 0, "quarter line must be odd quarters");
    let lo = quarters.div_euclid(2);
    (lo, lo + 1)
}

pub fn encode_mask(mask: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; mask.len().div_ceil(8)];
    for (i, on) in mask.iter().enumerate() {
        if *on {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

pub fn set_prob(state: &LmsrState, mask: &[bool]) -> Q64 {
    interval_prob(state, mask)
}

/// One notional quarter ticket: two LMSR fills of $q/2$ on adjacent half-lines.
pub fn lmsr_update_quarter(
    state: &mut LmsrState,
    first: &[bool],
    second: &[bool],
    q: Q64,
) -> (Q64, Q64) {
    let half = q.checked_div(Q64::from_int(2)).unwrap_or(Q64::ZERO);
    let c0 = lmsr_update(state, first, half);
    let c1 = lmsr_update(state, second, half);
    (c0, c1)
}

pub fn cost_quarter(state: &LmsrState, first: &[bool], second: &[bool], q: Q64) -> Q64 {
    let half = q.checked_div(Q64::from_int(2)).unwrap_or(Q64::ZERO);
    let c0 = buy_cost(state, first, half);
    let mut mid = state.clone();
    lmsr_update(&mut mid, first, half);
    let c1 = buy_cost(&mid, second, half);
    c0.saturating_add(c1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lmsr::implied_probs;
    use crate::prior::independent_poisson_2d;
    use crate::settle::{ticket_face, usdc};

    fn book() -> LmsrState {
        LmsrState::new(
            Q64::from_int(20),
            independent_poisson_2d(K_MAX, Q64::from_ratio(3, 2), Q64::from_ratio(11, 10)),
        )
    }

    fn ones(mask: &[bool]) -> usize {
        mask.iter().filter(|x| **x).count()
    }

    #[test]
    fn grid_is_121() {
        assert_eq!(cells(K_MAX), N_CELLS);
        assert_eq!(mask_home(K_MAX).len(), N_CELLS);
    }

    #[test]
    fn one_x_two_partitions_the_table() {
        let h = mask_home(K_MAX);
        let d = mask_draw(K_MAX);
        let a = mask_away(K_MAX);
        assert_eq!(ones(&h) + ones(&d) + ones(&a), N_CELLS);
        for i in 0..N_CELLS {
            assert_eq!(u8::from(h[i]) + u8::from(d[i]) + u8::from(a[i]), 1);
        }
        let s = book();
        let p = set_prob(&s, &h)
            .saturating_add(set_prob(&s, &d))
            .saturating_add(set_prob(&s, &a));
        assert!(p.approx_eq(Q64::ONE, 1 << 40), "1x2 sum {}", p.raw());
    }

    #[test]
    fn handicap_minus_15_is_win_by_two() {
        let m = mask_home_handicap(K_MAX, -3);
        for i in 0..=K_MAX {
            for j in 0..=K_MAX {
                assert_eq!(m[cell(i, j, K_MAX)], i as i32 - j as i32 >= 2);
            }
        }
        let home = mask_home(K_MAX);
        for (a, b) in m.iter().zip(home.iter()) {
            if *a {
                assert!(*b, "AH -1.5 must be a subset of home");
            }
        }
    }

    #[test]
    fn totals_25_over_is_three_plus() {
        let m = mask_over(K_MAX, 5);
        assert!(m[cell(3, 0, K_MAX)]);
        assert!(!m[cell(2, 0, K_MAX)]);
        assert!(m[cell(1, 2, K_MAX)]);
    }

    #[test]
    fn exact_21_is_one_cell() {
        let m = mask_exact(K_MAX, 2, 1);
        assert_eq!(ones(&m), 1);
        assert!(m[cell(2, 1, K_MAX)]);
        assert!(mask_home(K_MAX)[cell(2, 1, K_MAX)]);
    }

    #[test]
    fn buy_home_raises_home_and_moves_shared_theta() {
        let mut s = book();
        let home = mask_home(K_MAX);
        let away = mask_away(K_MAX);
        let exact = mask_exact(K_MAX, 2, 1);
        let p_h0 = set_prob(&s, &home);
        let p_a0 = set_prob(&s, &away);
        let p_e0 = set_prob(&s, &exact);
        let cost = lmsr_update(&mut s, &home, Q64::ONE);
        assert!(cost.raw() > 0);
        assert!(set_prob(&s, &home) > p_h0);
        assert!(set_prob(&s, &away) < p_a0);
        assert!(set_prob(&s, &exact) > p_e0, "2-1 is inside home");
        assert_eq!(s.l_max(), Q64::ONE);
    }

    #[test]
    fn buy_score_then_1x2_share_one_book() {
        let mut s = book();
        let exact = mask_exact(K_MAX, 2, 1);
        let home = mask_home(K_MAX);
        lmsr_update(&mut s, &exact, Q64::from_int(2));
        assert_eq!(s.exposure[cell(2, 1, K_MAX)], Q64::from_int(2));
        assert_eq!(s.exposure[cell(1, 0, K_MAX)], Q64::ZERO);
        let p_home = set_prob(&s, &home);
        lmsr_update(&mut s, &home, Q64::ONE);
        assert_eq!(s.exposure[cell(2, 1, K_MAX)], Q64::from_int(3));
        assert_eq!(s.exposure[cell(1, 0, K_MAX)], Q64::ONE);
        assert!(set_prob(&s, &home) > p_home);
        assert_eq!(s.l_max(), Q64::from_int(3));
    }

    #[test]
    fn buy_totals_and_handicap_stack_on_intersection() {
        let mut s = book();
        let over = mask_over(K_MAX, 5);
        let ah = mask_home_handicap(K_MAX, -3);
        lmsr_update(&mut s, &over, Q64::ONE);
        lmsr_update(&mut s, &ah, Q64::ONE);
        // (3,0): 3-0 is over 2.5 and home -1.5
        assert_eq!(s.exposure[cell(3, 0, K_MAX)], Q64::from_int(2));
        // (2,0): 2-0 is not over 2.5, is home -1.5
        assert_eq!(s.exposure[cell(2, 0, K_MAX)], Q64::ONE);
        // (2,1): 2-1 is over 2.5, not home -1.5
        assert_eq!(s.exposure[cell(2, 1, K_MAX)], Q64::ONE);
        // (0,0): neither
        assert_eq!(s.exposure[cell(0, 0, K_MAX)], Q64::ZERO);
    }

    #[test]
    fn quarter_minus_75_is_two_half_fills() {
        let (a, b) = quarter_to_halves(-3);
        assert_eq!((a, b), (-2, -1));
        let mut s = book();
        let m0 = mask_home_handicap(K_MAX, a);
        let m1 = mask_home_handicap(K_MAX, b);
        let (c0, c1) = lmsr_update_quarter(&mut s, &m0, &m1, Q64::from_int(2));
        assert!(c0.raw() > 0 && c1.raw() > 0);
        // -1.0 wins on i-j>=2; -0.5 wins on i-j>=1
        assert_eq!(s.exposure[cell(2, 0, K_MAX)], Q64::from_int(2));
        assert_eq!(s.exposure[cell(1, 0, K_MAX)], Q64::ONE);
        assert_eq!(s.exposure[cell(0, 0, K_MAX)], Q64::ZERO);
    }

    #[test]
    fn after_any_mix_1x2_still_sums_to_one() {
        let mut s = book();
        lmsr_update(&mut s, &mask_home(K_MAX), Q64::from_ratio(3, 2));
        lmsr_update(&mut s, &mask_over(K_MAX, 5), Q64::ONE);
        lmsr_update(&mut s, &mask_exact(K_MAX, 0, 0), Q64::from_ratio(1, 2));
        let p = set_prob(&s, &mask_home(K_MAX))
            .saturating_add(set_prob(&s, &mask_draw(K_MAX)))
            .saturating_add(set_prob(&s, &mask_away(K_MAX)));
        assert!(p.approx_eq(Q64::ONE, 1 << 38), "sum {}", p.raw());
    }

    #[test]
    fn e_at_outcome_equals_sum_of_ticket_faces() {
        let mut s = book();
        let k = K_MAX;
        // Home q=2, over 2.5 q=1, exact 2-1 q=1, home -0.75 q=2.
        lmsr_update(&mut s, &mask_home(k), Q64::from_int(2));
        lmsr_update(&mut s, &mask_over(k, 5), Q64::ONE);
        lmsr_update(&mut s, &mask_exact(k, 2, 1), Q64::ONE);
        let (h0, h1) = quarter_to_halves(-3);
        lmsr_update_quarter(
            &mut s,
            &mask_home_handicap(k, h0),
            &mask_home_handicap(k, h1),
            Q64::from_int(2),
        );
        let cell = cell(2, 1, k);
        let mut face = 0u64;
        for (kind, a, b, q) in [(0, 0, 0, 2u64), (3, 5, 0, 1), (7, 2, 1, 1), (10, -3, 0, 2)] {
            let (hit, tot) = skellam_hit_parts(kind, a, b, k, cell).unwrap();
            face += ticket_face(q, hit, tot);
        }
        assert_eq!(usdc(s.exposure[cell]), face);
        // 2-1: home + over + exact + half of -0.75 = 2+1+1+1.
        assert_eq!(face, 5);
        let p = implied_probs(&s);
        let sum = p.iter().copied().fold(Q64::ZERO, Q64::saturating_add);
        assert!(sum.approx_eq(Q64::ONE, 1 << 38), "pdf {}", sum.raw());
        assert_ne!(usdc(s.exposure[cell]), usdc(p[cell]), "E is not the PDF");
    }

    #[test]
    fn encode_roundtrip_home() {
        let m = mask_home(K_MAX);
        let bytes = encode_mask(&m);
        assert_eq!(bytes.len(), N_CELLS.div_ceil(8));
        assert_ne!(bytes, vec![0u8; bytes.len()]);
    }
}
