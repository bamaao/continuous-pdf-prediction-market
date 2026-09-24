//! Settlement waterfall (FR-SET-01–06). Session is not authority.
//! $L=E(x^*)$, $C_{\max}=R_{\mathrm{net}}+C_M+C_R^{\mathrm{final}}$, one global $\rho$.

use crate::views::{
    Grid, Layer, Market, Position, Quote, Resolution, RiskBook, PHASE_FAILED, PHASE_FINALIZED,
    PHASE_VOIDED,
};
use crate::{accounting, VaultError};
use anchor_lang::prelude::*;
use math::football;
use math::outcome::outcome_cell;
use math::settle::{c_max, layer_loss, payout_floor, r_net, recovery_rate, surplus, surplus_parts, usdc};
use math::Q64;

pub const BOARD_IDLE: u8 = 0;
pub const BOARD_SETTLED: u8 = 1;
pub const BOARD_REFUND: u8 = 2;

#[account]
pub struct Board {
    pub market: Pubkey,
    pub c_m_locked: u64,
    pub trading_revenue: u64,
    pub fees_accrued: u64,
    pub premium_payable: u64,
    pub c_r_final: u64,
    pub liability: u64,
    pub c_max: u64,
    pub rho_raw: i128,
    pub surplus: u64,
    pub surplus_r: u64,
    pub surplus_p: u64,
    pub paid_users: u64,
    pub paid_premium: u64,
    pub paid_surplus: u64,
    pub drawn_h: u64,
    pub dust: u64,
    pub cell: u16,
    pub phase: u8,
    pub bump: u8,
}

impl Board {
    pub const SIZE: usize = 8 + 32 + 8 * 16 + 16 + 2 + 1 + 1;
}

#[account]
pub struct Claim {
    pub position: Pubkey,
    pub paid: u64,
    pub bump: u8,
}

impl Claim {
    pub const SIZE: usize = 8 + 32 + 8 + 1;
}

/// Lock C_M: debit creator available into the board pot (tokens stay in the vault ATA).
pub fn fund_cm_inner(
    board: &mut Board,
    market_key: Pubkey,
    listed_c_m: u64,
    user: &mut crate::UserVault,
    amount: u64,
) -> Result<()> {
    require!(amount == listed_c_m, VaultError::BadSettle);
    require!(board.c_m_locked == 0, VaultError::AlreadySettled);
    if board.market == Pubkey::default() {
        board.market = market_key;
    }
    require!(board.market == market_key, VaultError::WrongBoard);
    if amount > 0 {
        user.available = accounting::debit_available(user.available, user.reserved, amount)?;
    }
    board.c_m_locked = amount;
    Ok(())
}

pub fn credit_trade_inner(board: &mut Board, user: &mut crate::UserVault, cost: u64, fee: u64) -> Result<()> {
    let pay = cost.checked_add(fee).ok_or(VaultError::Overflow)?;
    require!(pay > 0, VaultError::ZeroAmount);
    user.available = accounting::debit_available(user.available, user.reserved, pay)?;
    board.trading_revenue = board.trading_revenue.saturating_add(cost);
    board.fees_accrued = board.fees_accrued.saturating_add(fee);
    Ok(())
}

pub fn compute_begin(
    board: &mut Board,
    market: &Market,
    grid: &Grid,
    record: &Resolution,
    book: Option<&RiskBook>,
) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    require!(record.phase == PHASE_FINALIZED, VaultError::NotFinalized);
    require!(!record.refunds_due, VaultError::RefundsDue);
    require!(record.market == board.market, VaultError::WrongBoard);
    require!(grid.market == board.market, VaultError::WrongBoard);

    let o = &record.final_outcome;
    let cell = outcome_cell(
        record.family,
        grid.n as usize,
        record.k_max,
        market.extra.a,
        market.extra.b,
        o.kind,
        o.a,
        o.b,
    )
    .ok_or(VaultError::BadOutcome)?;
    require!(cell < grid.exposure.len(), VaultError::BadOutcome);

    let liability = usdc(Q64::from_raw(grid.exposure[cell]));
    let premium = book.map(|b| b.premium_payable).unwrap_or(0);
    let c_r = book.map(|b| b.c_r).unwrap_or(0);
    if let Some(b) = book {
        require!(b.market == board.market, VaultError::WrongBoard);
    }
    let r = r_net(board.trading_revenue, premium);
    let cmax = usdc(c_max(
        Q64::from_int(r as i64),
        Q64::from_int(board.c_m_locked as i64),
        Q64::from_int(c_r as i64),
    ));
    let rho = recovery_rate(Q64::from_int(cmax as i64), Q64::from_int(liability as i64));
    let s = usdc(surplus(
        Q64::from_int(r as i64),
        Q64::from_int(board.c_m_locked as i64),
        Q64::from_int(liability as i64),
        rho,
    ));
    // No filled risk capital → residual stays with the platform (fees are a separate pot).
    let (sr, sp) = if c_r == 0 {
        (0, s)
    } else {
        surplus_parts(s, market.alpha_r_bps)
    };

    board.premium_payable = premium;
    board.c_r_final = c_r;
    board.liability = liability;
    board.c_max = cmax;
    board.rho_raw = rho.raw();
    board.surplus = s;
    board.surplus_r = sr;
    board.surplus_p = sp;
    board.cell = cell as u16;
    board.phase = BOARD_SETTLED;
    Ok(())
}

pub fn compute_refund(board: &mut Board, record: &Resolution) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    require!(
        record.phase == PHASE_FAILED || record.phase == PHASE_VOIDED,
        VaultError::NotFinalized
    );
    require!(record.refunds_due, VaultError::RefundsDue);
    require!(record.market == board.market, VaultError::WrongBoard);
    board.phase = BOARD_REFUND;
    board.rho_raw = 0;
    board.surplus = 0;
    Ok(())
}

fn rho_dust(rho_raw: i128, q: u64, _pay: u64) -> u64 {
    let exact_num = if rho_raw <= 0 {
        0
    } else {
        (rho_raw as u128).saturating_mul(q as u128)
    };
    let floor = exact_num >> 64;
    exact_num.saturating_sub(floor << 64) as u64
}

pub fn pay_winner_clean(
    board: &mut Board,
    pos: &Position,
    hits: bool,
    user: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(pos.market == board.market, VaultError::WrongBoard);
    if !hits || pos.q <= 0 {
        return Ok(0);
    }
    let q = usdc(Q64::from_raw(pos.q));
    let pay = payout_floor(Q64::from_raw(board.rho_raw), q);
    board.dust = board.dust.saturating_add(rho_dust(board.rho_raw, q, pay));
    if pay > 0 {
        user.available = accounting::credit(user.available, pay)?;
        board.paid_users = board.paid_users.saturating_add(pay);
    }
    Ok(pay)
}

pub fn draw_quote(
    board: &mut Board,
    layer: &Layer,
    quote: &Quote,
    lp: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(layer.market == board.market && quote.market == board.market, VaultError::WrongBoard);
    require!(quote.layer_id == layer.layer_id, VaultError::WrongBoard);
    // Own funds first: R_net + C_M. Draw C_R only for the leftover shortfall.
    let own = r_net(board.trading_revenue, board.premium_payable).saturating_add(board.c_m_locked);
    let need = board.liability.saturating_sub(own).saturating_sub(board.drawn_h);
    let h_raw = usdc(layer_loss(
        Q64::from_int(board.liability as i64),
        Q64::from_int(layer.attachment as i64),
        Q64::from_int(quote.filled as i64),
    ));
    let h = h_raw.min(need);
    require!(h <= quote.capacity, VaultError::Overflow);
    if quote.capacity > 0 {
        lp.reserved = accounting::release(lp.reserved, quote.capacity)?;
    }
    if h > 0 {
        lp.available = accounting::debit_available(lp.available, lp.reserved, h)?;
        board.drawn_h = board.drawn_h.saturating_add(h);
    }
    Ok(h)
}

pub fn release_quote(board: &Board, quote: &Quote, lp: &mut crate::UserVault) -> Result<()> {
    require!(board.phase == BOARD_REFUND, VaultError::NotFinalized);
    require!(quote.market == board.market, VaultError::WrongBoard);
    if quote.capacity > 0 {
        lp.reserved = accounting::release(lp.reserved, quote.capacity)?;
    }
    Ok(())
}

pub fn pay_premium_inner(board: &mut Board, quote: &Quote, lp: &mut crate::UserVault) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(quote.market == board.market, VaultError::WrongBoard);
    let due = quote.premium_owed;
    if due == 0 {
        return Ok(0);
    }
    lp.available = accounting::credit(lp.available, due)?;
    board.paid_premium = board.paid_premium.saturating_add(due);
    Ok(due)
}

pub fn pay_surplus_lp_inner(
    board: &mut Board,
    quote: &Quote,
    lp: &mut crate::UserVault,
    weight_sum: u64,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(board.rho_raw >= Q64::ONE.raw(), VaultError::NoSurplus);
    require!(quote.market == board.market, VaultError::WrongBoard);
    if board.surplus_r == 0 || weight_sum == 0 || quote.filled == 0 {
        return Ok(0);
    }
    let w = (quote.profit_share_bps as u64).saturating_mul(quote.filled);
    let raw = (board.surplus_r as u128 * w as u128 / weight_sum as u128) as u64;
    let left = board.surplus_r.saturating_sub(board.paid_surplus);
    let share = raw.min(left);
    if share > 0 {
        lp.available = accounting::credit(lp.available, share)?;
        board.paid_surplus = board.paid_surplus.saturating_add(share);
    }
    Ok(share)
}

pub fn pay_surplus_platform_inner(
    board: &mut Board,
    platform: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(board.rho_raw >= Q64::ONE.raw(), VaultError::NoSurplus);
    let share = board.surplus_p;
    if share == 0 {
        return Ok(0);
    }
    require!(board.paid_surplus.saturating_add(share) <= board.surplus, VaultError::Overflow);
    platform.available = accounting::credit(platform.available, share)?;
    board.paid_surplus = board.paid_surplus.saturating_add(share);
    board.surplus_p = 0;
    Ok(share)
}

pub fn refund_position_inner(
    board: &mut Board,
    pos: &Position,
    user: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_REFUND, VaultError::NotFinalized);
    require!(pos.market == board.market, VaultError::WrongBoard);
    let back = pos.cost_paid;
    if back > 0 {
        user.available = accounting::credit(user.available, back)?;
        board.paid_users = board.paid_users.saturating_add(back);
    }
    Ok(back)
}

/// Same digest as `market::ids::set_hash`.
pub fn set_hash(mask: &[u8]) -> [u8; 32] {
    digest(&[b"set", mask])
}

pub fn skellam_ticket(kind: u8, a: i16, b: i16) -> [u8; 32] {
    digest(&[b"skset", &[kind], &a.to_le_bytes(), &b.to_le_bytes()])
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut st = [
        0x736f6d6570736575u64,
        0x646f72616e646f6du64,
        0x6c7967656e657261u64,
        0x7465646279746573u64,
    ];
    for (n, part) in parts.iter().enumerate() {
        st[0] ^= (part.len() as u64).wrapping_add((n as u64) << 32);
        for chunk in part.chunks(8) {
            let mut x = 0u64;
            for (i, b) in chunk.iter().enumerate() {
                x |= (*b as u64) << (8 * i);
            }
            st[0] = st[0].wrapping_add(x).rotate_left(13);
            st[1] ^= st[0];
            st[2] = st[2].wrapping_add(st[1]).rotate_left(17);
            st[3] ^= st[2];
            st[0] = st[0].wrapping_mul(0x9E3779B97F4A7C15);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in st.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&word.to_le_bytes());
    }
    out
}

pub fn mask_hits(mask: &[u8], n: usize, cell: usize) -> Result<bool> {
    require!(n > 0 && cell < n, VaultError::BadOutcome);
    let need = n.div_ceil(8);
    require!(mask.len() == need, VaultError::BadMask);
    Ok(mask[cell / 8] & (1 << (cell % 8)) != 0)
}

pub fn skellam_hits(kind: u8, a: i16, b: i16, k_max: u8, cell: usize) -> Result<bool> {
    let k = if k_max == 0 { 10 } else { k_max as u32 };
    let mask = match kind {
        0 => football::mask_home(k),
        1 => football::mask_draw(k),
        2 => football::mask_away(k),
        3 => football::mask_over(k, a as i32),
        4 => football::mask_under(k, a as i32),
        5 => football::mask_btts_yes(k),
        6 => football::mask_btts_no(k),
        7 => football::mask_exact(k, a as u32, b as u32),
        8 => football::mask_home_handicap(k, a as i32),
        9 => football::mask_away_handicap(k, a as i32),
        10 => {
            let (x, y) = football::quarter_to_halves(a as i32);
            let m1 = football::mask_home_handicap(k, x);
            let m2 = football::mask_home_handicap(k, y);
            m1.into_iter().zip(m2).map(|(p, q)| p || q).collect()
        }
        11 => {
            let (x, y) = football::quarter_to_halves(a as i32);
            let m1 = football::mask_away_handicap(k, x);
            let m2 = football::mask_away_handicap(k, y);
            m1.into_iter().zip(m2).map(|(p, q)| p || q).collect()
        }
        _ => return err!(VaultError::BadMask),
    };
    require!(cell < mask.len(), VaultError::BadOutcome);
    Ok(mask[cell])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UserVault;

    fn user(available: u64, reserved: u64) -> UserVault {
        UserVault {
            owner: Pubkey::default(),
            available,
            reserved,
            bump: 0,
        }
    }

    fn board() -> Board {
        Board {
            market: Pubkey::new_from_array([7; 32]),
            c_m_locked: 20,
            trading_revenue: 30,
            fees_accrued: 5,
            premium_payable: 0,
            c_r_final: 0,
            liability: 0,
            c_max: 0,
            rho_raw: 0,
            surplus: 0,
            surplus_r: 0,
            surplus_p: 0,
            paid_users: 0,
            paid_premium: 0,
            paid_surplus: 0,
            drawn_h: 0,
            dust: 0,
            cell: 0,
            phase: BOARD_IDLE,
            bump: 0,
        }
    }

    #[test]
    fn trade_only_pool_when_no_cm_no_cr() {
        let mut b = board();
        b.c_m_locked = 0;
        b.trading_revenue = 40;
        b.fees_accrued = 7;
        let r = r_net(b.trading_revenue, 0);
        assert_eq!(r, 40);
        let c = r + b.c_m_locked;
        let rho = recovery_rate(Q64::from_int(c as i64), Q64::from_int(100));
        assert!(rho.approx_eq(Q64::from_ratio(2, 5), 1 << 40));
        assert_eq!(
            usdc(surplus(Q64::from_int(r as i64), Q64::ZERO, Q64::from_int(30), Q64::ONE)),
            10
        );
        assert_eq!(b.fees_accrued, 7);
    }

    #[test]
    fn fund_cm_zero_opens_board_without_debit() {
        let mut b = board();
        b.c_m_locked = 0;
        b.market = Pubkey::default();
        let key = Pubkey::new_from_array([7; 32]);
        let mut u = user(50, 0);
        fund_cm_inner(&mut b, key, 0, &mut u, 0).unwrap();
        assert_eq!(b.c_m_locked, 0);
        assert_eq!(b.market, key);
        assert_eq!(u.available, 50);
    }

    #[test]
    fn fees_out_of_c_max() {
        let mut b = board();
        // R_net = 30 − 0, C_M = 20, C_R = 0 → 50. L = 100 → ρ = 1/2. S = 0.
        b.phase = BOARD_IDLE;
        let r = r_net(b.trading_revenue, 0);
        let c = r + b.c_m_locked;
        let rho = recovery_rate(Q64::from_int(c as i64), Q64::from_int(100));
        assert!(rho.approx_eq(Q64::from_ratio(1, 2), 1 << 40));
        assert_eq!(usdc(surplus(Q64::from_int(r as i64), Q64::from_int(20), Q64::from_int(100), rho)), 0);
        assert_eq!(b.fees_accrued, 5);
    }

    #[test]
    fn two_winners_same_rho_not_fifo() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.rho_raw = recovery_rate(Q64::from_int(90), Q64::from_int(100)).raw();
        let mut pa = Position {
            market: b.market,
            owner: Pubkey::default(),
            set_hash: [0; 32],
            q: Q64::from_int(60).raw(),
            cost_paid: 10,
            claimed: false,
            bump: 0,
        };
        let mut pb = pa.clone();
        pb.q = Q64::from_int(40).raw();
        let mut ua = user(0, 0);
        let mut ub = user(0, 0);
        let pa_pay = pay_winner_clean(&mut b, &pa, true, &mut ua).unwrap();
        let pb_pay = pay_winner_clean(&mut b, &pb, true, &mut ub).unwrap();
        assert_eq!(ua.available + ub.available, pa_pay + pb_pay);
        assert!(pa_pay + pb_pay <= 90);
        assert!(pa_pay > pb_pay);
    }

    #[test]
    fn void_refunds_cost_not_rho() {
        let mut b = board();
        b.phase = BOARD_REFUND;
        let mut pos = Position {
            market: b.market,
            owner: Pubkey::default(),
            set_hash: [0; 32],
            q: Q64::from_int(50).raw(),
            cost_paid: 12,
            claimed: false,
            bump: 0,
        };
        let mut u = user(0, 0);
        assert_eq!(refund_position_inner(&mut b, &pos, &mut u).unwrap(), 12);
        assert_eq!(u.available, 12);
    }

    #[test]
    fn layer_draw_uses_h() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.liability = 80;
        let layer = Layer {
            market: b.market,
            attachment: 50,
            thickness: 40,
            filled: 40,
            layer_id: 1,
            quote_count: 1,
            bump: 0,
        };
        let quote = Quote {
            market: b.market,
            lp: Pubkey::default(),
            capacity: 40,
            filled: 40,
            premium: 4,
            premium_owed: 4,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let mut lp = user(100, 40);
        assert_eq!(draw_quote(&mut b, &layer, &quote, &mut lp).unwrap(), 30);
        assert_eq!(lp.reserved, 0);
        assert_eq!(lp.available, 70);
        assert_eq!(b.drawn_h, 30);
    }

    #[test]
    fn no_draw_when_own_funds_cover() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        // R_net=30 + C_M=20 = 50 ≥ L=40 → risk capital stands by, H=0, then surplus.
        b.liability = 40;
        b.rho_raw = Q64::ONE.raw();
        let s = usdc(surplus(
            Q64::from_int(30),
            Q64::from_int(20),
            Q64::from_int(40),
            Q64::ONE,
        ));
        assert_eq!(s, 10);
        let (sr, sp) = surplus_parts(s, 7_000);
        assert_eq!((sr, sp), (7, 3));
        let layer = Layer {
            market: b.market,
            attachment: 20,
            thickness: 40,
            filled: 40,
            layer_id: 1,
            quote_count: 1,
            bump: 0,
        };
        let quote = Quote {
            market: b.market,
            lp: Pubkey::default(),
            capacity: 40,
            filled: 40,
            premium: 4,
            premium_owed: 4,
            ts: 1,
            profit_share_bps: 7_000,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let mut lp = user(100, 40);
        assert_eq!(draw_quote(&mut b, &layer, &quote, &mut lp).unwrap(), 0);
        assert_eq!(lp.reserved, 0);
        assert_eq!(lp.available, 100);
        assert_eq!(b.drawn_h, 0);
    }

    #[test]
    fn draw_only_own_funds_shortfall() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        // own=50, L=60, A=20 → raw H=40 but need only 10.
        b.liability = 60;
        let layer = Layer {
            market: b.market,
            attachment: 20,
            thickness: 40,
            filled: 40,
            layer_id: 1,
            quote_count: 1,
            bump: 0,
        };
        let quote = Quote {
            market: b.market,
            lp: Pubkey::default(),
            capacity: 40,
            filled: 40,
            premium: 4,
            premium_owed: 4,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let mut lp = user(100, 40);
        assert_eq!(draw_quote(&mut b, &layer, &quote, &mut lp).unwrap(), 10);
        assert_eq!(lp.available, 90);
        assert_eq!(b.drawn_h, 10);
    }
}
