//! Settlement waterfall (FR-SET-01–06). Session is not authority.
//! $L=E(x^*)$, $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$, one global $\rho$.

use crate::views::{
    Layer, Market, Position, Quote, Resolution, RiskBook, PHASE_FAILED, PHASE_FINALIZED, PHASE_VOIDED,
};
use crate::{accounting, VaultError};
use anchor_lang::prelude::*;
use math::football;
use math::outcome::outcome_cell;
use math::settle::{
    c_max, c_p_alloc, layer_loss, payout_floor, r_net, recovery_rate, surplus, surplus_parts, ticket_face,
    usdc,
};
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
    pub fee_bps: u16,
    pub fee_timing: u8,
}

impl Board {
    pub const SIZE: usize = 8 + 32 + 8 * 16 + 16 + 2 + 1 + 1 + 2 + 1;
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

/// Protocol-wide $C_P$ pool. Tokens stay in the vault ATA; this is the ledger.
#[account]
pub struct AdjustPool {
    pub available: u64,
    pub allocated: u64,
    pub bump: u8,
}

impl AdjustPool {
    pub const SIZE: usize = 8 + 8 + 8 + 1;
}

/// Per-board $C_P$ cap. `allocated` is written at `begin_settle`.
#[account]
pub struct BoardTap {
    pub market: Pubkey,
    pub cap: u64,
    pub allocated: u64,
    pub bump: u8,
}

impl BoardTap {
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 1;
}

/// Open the Board account. Amount MUST be 0; leftover `c_m_locked` stays unused.
pub fn fund_cm_inner(
    board: &mut Board,
    market_key: Pubkey,
    listed_c_m: u64,
    user: &mut crate::UserVault,
    amount: u64,
) -> Result<()> {
    let _ = listed_c_m;
    require!(amount == 0, VaultError::BadSettle);
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

/// Return LMSR sell proceeds from the pot to unused margin. Fees stay accrued.
pub fn refund_trade_inner(board: &mut Board, user: &mut crate::UserVault, cost: u64) -> Result<()> {
    require!(cost > 0, VaultError::ZeroAmount);
    require!(board.trading_revenue >= cost, VaultError::InsufficientAvailable);
    board.trading_revenue = board.trading_revenue.checked_sub(cost).ok_or(VaultError::Overflow)?;
    user.available = crate::accounting::credit(user.available, cost)?;
    Ok(())
}

const GRID_HDR: usize = 8 + 32 + 2 + 2 + 1 + 16;

fn grid_i128(data: &[u8], off: usize) -> Result<i128> {
    require!(data.len() >= off + 16, VaultError::BadSettle);
    let bytes: [u8; 16] = data[off..off + 16]
        .try_into()
        .map_err(|_| error!(VaultError::BadSettle))?;
    Ok(i128::from_le_bytes(bytes))
}

/// Header only: market + local n + start. Does not copy the four n-vecs.
pub fn parse_grid_meta(data: &[u8]) -> Result<(Pubkey, u16, u16)> {
    require!(data.len() >= GRID_HDR + 4, VaultError::BadSettle);
    let market_bytes: [u8; 32] = data[8..40]
        .try_into()
        .map_err(|_| error!(VaultError::BadSettle))?;
    let n_bytes: [u8; 2] = data[40..42]
        .try_into()
        .map_err(|_| error!(VaultError::BadSettle))?;
    let start_bytes: [u8; 2] = data[42..44]
        .try_into()
        .map_err(|_| error!(VaultError::BadSettle))?;
    Ok((
        Pubkey::from(market_bytes),
        u16::from_le_bytes(n_bytes),
        u16::from_le_bytes(start_bytes),
    ))
}

/// Sealed `exposure[cell]` on the shard that contains `cell` (`cell` is global).
pub fn parse_grid_exposure(data: &[u8], cell: usize) -> Result<i128> {
    let (_, n, start) = parse_grid_meta(data)?;
    let nu = n as usize;
    require!(cell >= start as usize && cell < start as usize + nu, VaultError::BadOutcome);
    let cell = cell - start as usize;
    require!(data.len() >= 77 + 64 * nu, VaultError::BadSettle);
    let p0_len = u32::from_le_bytes(
        data[GRID_HDR..GRID_HDR + 4]
            .try_into()
            .map_err(|_| error!(VaultError::BadSettle))?,
    ) as usize;
    require!(p0_len == nu, VaultError::BadSettle);
    let theta_len_off = GRID_HDR + 4 + nu * 16;
    let theta_len = u32::from_le_bytes(
        data[theta_len_off..theta_len_off + 4]
            .try_into()
            .map_err(|_| error!(VaultError::BadSettle))?,
    ) as usize;
    require!(theta_len == nu, VaultError::BadSettle);
    let exp_len_off = theta_len_off + 4 + nu * 16;
    let exp_len = u32::from_le_bytes(
        data[exp_len_off..exp_len_off + 4]
            .try_into()
            .map_err(|_| error!(VaultError::BadSettle))?,
    ) as usize;
    require!(exp_len == nu, VaultError::BadSettle);
    grid_i128(data, exp_len_off + 4 + cell * 16)
}

pub fn compute_begin(
    board: &mut Board,
    market: &Market,
    grid_data: &[u8],
    record: &Resolution,
    book: Option<&RiskBook>,
    pool: Option<&mut AdjustPool>,
    tap: Option<&mut BoardTap>,
) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    require!(record.phase == PHASE_FINALIZED, VaultError::NotFinalized);
    require!(!record.refunds_due, VaultError::RefundsDue);
    require!(record.market == board.market, VaultError::WrongBoard);
    let (grid_market, _, _) = parse_grid_meta(grid_data)?;
    require!(grid_market == board.market, VaultError::WrongBoard);

    let o = &record.final_outcome;
    let cell = outcome_cell(
        record.family,
        market.n as usize,
        record.k_max,
        market.extra.a,
        market.extra.b,
        o.kind,
        o.a,
        o.b,
    )
    .ok_or(VaultError::BadOutcome)?;
    require!(cell < market.n as usize, VaultError::BadOutcome);
    let exposure_cell = parse_grid_exposure(grid_data, cell)?;

    let liability = usdc(Q64::from_raw(exposure_cell));
    let premium = book.map(|b| b.premium_payable).unwrap_or(0);
    let c_r = book.map(|b| b.c_r).unwrap_or(0);
    if let Some(b) = book {
        require!(b.market == board.market, VaultError::WrongBoard);
    }
    let r = r_net(board.trading_revenue, premium);
    let alloc = draw_c_p(liability, r, 0, c_r, pool, tap, board.market)?;
    let cmax = usdc(c_max(
        Q64::from_int(r as i64),
        Q64::ZERO,
        Q64::from_int(c_r as i64),
        Q64::from_int(alloc as i64),
    ));
    let rho = recovery_rate(Q64::from_int(cmax as i64), Q64::from_int(liability as i64));
    let s = usdc(surplus(
        Q64::from_int(r as i64),
        Q64::ZERO,
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
    board.fee_bps = market.fee_bps;
    board.fee_timing = market.fee_timing;
    Ok(())
}

/// $C_P^{\mathrm{alloc}}$ only when $L>R_{\mathrm{net}}$. Missing pool/tap → 0.
pub fn draw_c_p(
    liability: u64,
    r_net: u64,
    c_m: u64,
    c_r: u64,
    pool: Option<&mut AdjustPool>,
    tap: Option<&mut BoardTap>,
    market: Pubkey,
) -> Result<u64> {
    let (Some(pool), Some(tap)) = (pool, tap) else {
        return Ok(0);
    };
    require!(tap.market == market, VaultError::WrongBoard);
    let alloc = c_p_alloc(liability, r_net, c_m, c_r, tap.cap, pool.available);
    if alloc > 0 {
        pool.available = pool.available.saturating_sub(alloc);
        pool.allocated = pool.allocated.saturating_add(alloc);
        tap.allocated = alloc;
    }
    Ok(alloc)
}

pub fn fund_pool_inner(pool: &mut AdjustPool, user: &mut crate::UserVault, amount: u64) -> Result<()> {
    require!(amount > 0, VaultError::ZeroAmount);
    user.available = accounting::debit_available(user.available, user.reserved, amount)?;
    pool.available = accounting::credit(pool.available, amount)?;
    Ok(())
}

/// Move this board's accrued fees into the platform UserVault. Never credits $C_P$.
pub fn claim_fees_inner(board: &mut Board, platform: &mut crate::UserVault) -> Result<u64> {
    let fees = board.fees_accrued;
    if fees > 0 {
        platform.available = accounting::credit(platform.available, fees)?;
        board.fees_accrued = 0;
    }
    Ok(fees)
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
    face_usdc: u64,
    user: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(pos.market == board.market, VaultError::WrongBoard);
    if face_usdc == 0 {
        return Ok(0);
    }
    let pay = payout_floor(Q64::from_raw(board.rho_raw), face_usdc);
    board.dust = board.dust.saturating_add(rho_dust(board.rho_raw, face_usdc, pay));
    let fee = if board.fee_timing == 1 && board.fee_bps > 0 && pay > 0 {
        (pay as u128 * board.fee_bps as u128 / 10_000) as u64
    } else {
        0
    };
    let net = pay.saturating_sub(fee);
    if fee > 0 {
        board.fees_accrued = board.fees_accrued.saturating_add(fee);
    }
    if net > 0 {
        user.available = accounting::credit(user.available, net)?;
        board.paid_users = board.paid_users.saturating_add(net);
    }
    Ok(net)
}

fn reserved_for_quote(quote: &Quote) -> u64 {
    if quote.cancelled {
        quote.filled
    } else {
        quote.capacity
    }
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
    // Own funds first: R_net. Draw C_R only for the leftover shortfall.
    let own = r_net(board.trading_revenue, board.premium_payable);
    let need = board.liability.saturating_sub(own).saturating_sub(board.drawn_h);
    let h_raw = usdc(layer_loss(
        Q64::from_int(board.liability as i64),
        Q64::from_int(layer.attachment as i64),
        Q64::from_int(quote.filled as i64),
    ));
    let h = h_raw.min(need);
    require!(h <= quote.capacity, VaultError::Overflow);
    let locked = reserved_for_quote(quote);
    if locked > 0 {
        lp.reserved = accounting::release(lp.reserved, locked)?;
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
    let locked = reserved_for_quote(quote);
    if locked > 0 {
        lp.reserved = accounting::release(lp.reserved, locked)?;
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

pub fn mask_face(q_raw: i128, mask: &[u8], n: usize, cell: usize) -> Result<u64> {
    let hits = mask_hits(mask, n, cell)?;
    Ok(ticket_face(usdc(Q64::from_raw(q_raw)), u32::from(hits), 1))
}

pub fn skellam_face(q_raw: i128, kind: u8, a: i16, b: i16, k_max: u8, cell: usize) -> Result<u64> {
    let k = if k_max == 0 { 10 } else { k_max as u32 };
    let (hit, tot) = football::skellam_hit_parts(kind, a, b, k, cell).ok_or(VaultError::BadMask)?;
    Ok(ticket_face(usdc(Q64::from_raw(q_raw)), hit, tot))
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
            c_m_locked: 0,
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
            fee_bps: 0,
            fee_timing: 0,
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
        let c = r;
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
        // R_net = 30, C_R = 0 → C_max = 30. L = 100 → ρ = 3/10. S = 0.
        b.phase = BOARD_IDLE;
        let r = r_net(b.trading_revenue, 0);
        let c = r;
        let rho = recovery_rate(Q64::from_int(c as i64), Q64::from_int(100));
        assert!(rho.approx_eq(Q64::from_ratio(3, 10), 1 << 40));
        assert_eq!(usdc(surplus(Q64::from_int(r as i64), Q64::ZERO, Q64::from_int(100), rho)), 0);
        assert_eq!(b.fees_accrued, 5);
    }

    #[test]
    fn claim_fees_go_to_platform_not_pool() {
        let mut b = board();
        b.fees_accrued = 7;
        let pool = AdjustPool {
            available: 10,
            allocated: 0,
            bump: 0,
        };
        let mut plat = user(1, 0);
        let n = claim_fees_inner(&mut b, &mut plat).unwrap();
        assert_eq!(n, 7);
        assert_eq!(b.fees_accrued, 0);
        assert_eq!(plat.available, 8);
        assert_eq!(pool.available, 10);
    }

    #[test]
    fn claim_timing_fee_taken_from_payout() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.rho_raw = Q64::ONE.raw();
        b.fee_bps = 1_000;
        b.fee_timing = 1;
        b.fees_accrued = 0;
        let pos = Position {
            market: b.market,
            owner: Pubkey::default(),
            set_hash: [0; 32],
            q: Q64::from_int(100).raw(),
            cost_paid: 20,
            claimed: false,
            bump: 0,
            vault_owed_cost: 0,
            vault_owed_fee: 0,
            vault_owed_credit: 0,
        };
        let mut u = user(0, 0);
        let net = pay_winner_clean(&mut b, &pos, 100, &mut u).unwrap();
        assert_eq!(net, 90);
        assert_eq!(u.available, 90);
        assert_eq!(b.fees_accrued, 10);
        assert_eq!(b.paid_users, 90);
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
            vault_owed_cost: 0,
            vault_owed_fee: 0,
            vault_owed_credit: 0,
        };
        let mut pb = pa.clone();
        pb.q = Q64::from_int(40).raw();
        let mut ua = user(0, 0);
        let mut ub = user(0, 0);
        let pa_pay = pay_winner_clean(&mut b, &pa, 60, &mut ua).unwrap();
        let pb_pay = pay_winner_clean(&mut b, &pb, 40, &mut ub).unwrap();
        assert_eq!(ua.available + ub.available, pa_pay + pb_pay);
        assert!(pa_pay + pb_pay <= 90);
        assert!(pa_pay > pb_pay);
    }

    #[test]
    fn quarter_ticket_face_matches_exposure_write() {
        let q = Q64::from_int(100).raw();
        let k = 10;
        // 2-1: only the -0.5 half of home -0.75 hits → q/2.
        let half = skellam_face(q, 10, -3, 0, k, math::football::cell(2, 1, k as u32)).unwrap();
        assert_eq!(half, 50);
        // 2-0: both halves hit → q.
        let full = skellam_face(q, 10, -3, 0, k, math::football::cell(2, 0, k as u32)).unwrap();
        assert_eq!(full, 100);
        // 0-0: miss.
        let miss = skellam_face(q, 10, -3, 0, k, math::football::cell(0, 0, k as u32)).unwrap();
        assert_eq!(miss, 0);
    }

    #[test]
    fn sell_returns_cost_from_pot() {
        let mut b = board();
        let mut u = user(10, 0);
        refund_trade_inner(&mut b, &mut u, 12).unwrap();
        assert_eq!(b.trading_revenue, 18);
        assert_eq!(u.available, 22);
        assert!(refund_trade_inner(&mut b, &mut u, 19).is_err());
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
            vault_owed_cost: 0,
            vault_owed_fee: 0,
            vault_owed_credit: 0,
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
        // R_net=50 ≥ L=40 → risk capital stands by, H=0, then surplus.
        b.trading_revenue = 50;
        b.liability = 40;
        b.rho_raw = Q64::ONE.raw();
        let s = usdc(surplus(
            Q64::from_int(50),
            Q64::ZERO,
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
        b.trading_revenue = 50;
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

    #[test]
    fn draw_after_cancel_releases_only_filled() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.liability = 80;
        let layer = Layer {
            market: b.market,
            attachment: 50,
            thickness: 40,
            filled: 10,
            layer_id: 1,
            quote_count: 1,
            bump: 0,
        };
        let quote = Quote {
            market: b.market,
            lp: Pubkey::default(),
            capacity: 100,
            filled: 10,
            premium: 10,
            premium_owed: 1,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: true,
            bump: 0,
        };
        let mut lp = user(200, 10);
        assert_eq!(draw_quote(&mut b, &layer, &quote, &mut lp).unwrap(), 10);
        assert_eq!(lp.reserved, 0);
        assert_eq!(lp.available, 190);
    }

    #[test]
    fn c_p_draws_only_on_shortfall() {
        let market = Pubkey::new_from_array([3; 32]);
        let mut pool = AdjustPool {
            available: 50,
            allocated: 0,
            bump: 1,
        };
        let mut tap = BoardTap {
            market,
            cap: 20,
            allocated: 0,
            bump: 1,
        };
        assert_eq!(
            draw_c_p(100, 40, 0, 0, Some(&mut pool), Some(&mut tap), market).unwrap(),
            20
        );
        assert_eq!(pool.available, 30);
        assert_eq!(pool.allocated, 20);
        assert_eq!(tap.allocated, 20);
        assert_eq!(draw_c_p(30, 40, 0, 0, Some(&mut pool), Some(&mut tap), market).unwrap(), 0);
        assert_eq!(draw_c_p(100, 40, 0, 0, None, None, market).unwrap(), 0);
    }
}
