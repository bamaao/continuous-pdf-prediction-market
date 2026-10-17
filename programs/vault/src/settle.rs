//! Settlement waterfall (FR-SET-01–06). Session is not authority.
//! $L=E(x^*)$, $C_{\max}=R_{\mathrm{net}}+C_R^{\mathrm{final}}+C_P^{\mathrm{alloc}}$, one global $\rho$.

use crate::views::{
    Layer, Market, Position, Quote, Resolution, RiskBook, PHASE_FAILED, PHASE_FINALIZED, PHASE_VOIDED,
    STATUS_SETTLED, STATUS_VOID,
};
use crate::{accounting, VaultError};
use anchor_lang::prelude::*;
use math::football;
use math::outcome::outcome_cell;
use math::settle::{
    c_max, c_p_alloc, payout_floor, r_net, recovery_rate, surplus, surplus_parts, surplus_split, ticket_face,
    usdc,
};
use math::{sort_bids, waterfall_pay, Bid, Q64};

pub const BOARD_IDLE: u8 = 0;
pub const BOARD_SETTLED: u8 = 1;
pub const BOARD_REFUND: u8 = 2;

#[account]
pub struct Board {
    pub market: Pubkey,
    pub c_m_locked: u64,
    pub trading_revenue: u64,
    /// Mirrors `Market.cost_basis` (Σ `cost_paid`). VOID uses ρ = min(1, R / basis).
    pub cost_basis: u64,
    pub fees_accrued: u64,
    pub premium_payable: u64,
    pub c_r_final: u64,
    pub liability: u64,
    pub c_max: u64,
    pub rho_raw: i128,
    pub surplus: u64,
    pub surplus_r: u64,
    pub surplus_p: u64,
    pub surplus_c: u64,
    pub paid_users: u64,
    pub paid_premium: u64,
    pub paid_surplus: u64,
    pub paid_cover: u64,
    pub drawn_h: u64,
    pub dust: u64,
    pub cell: u16,
    pub phase: u8,
    pub bump: u8,
    pub fee_bps: u16,
    pub fee_timing: u8,
    pub champion: Pubkey,
    /// Per-layer quote slot bits: no-draw / draw surplus already paid for that quote.
    pub reward_bits: u64,
    /// Per-layer quote slot bits: `draw_lp` / `release_lp` already released reserved for that quote.
    pub draw_bits: u64,
}

impl Board {
    /// disc + market + 18×u64 + rho + cell/phase/bump/fee + champion + reward_bits + draw_bits
    pub const SIZE: usize = 8 + 32 + 8 * 18 + 16 + 2 + 1 + 1 + 2 + 1 + 32 + 8 + 8;
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

/// Protocol-wide cover for Risk LPs with cumulative $\Pi<0$. Ledger only; USDC stays in the vault ATA.
/// Outflows are platform-signed (`CoverLp.authority == Market.platform`). No calendar gate.

#[account]
pub struct LossPool {
    pub available: u64,
    pub bump: u8,
}

impl LossPool {
    pub const SIZE: usize = 8 + 8 + 1;
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
        let locked = accounting::locked(user.reserved, user.trade_pending)?;
        user.available = accounting::debit_available(user.available, locked, amount)?;
    }
    board.c_m_locked = amount;
    Ok(())
}

pub fn credit_trade_inner(board: &mut Board, user: &mut crate::UserVault, cost: u64, fee: u64) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    let pay = cost.checked_add(fee).ok_or(VaultError::Overflow)?;
    require!(pay > 0, VaultError::ZeroAmount);
    let locked = accounting::locked(user.reserved, user.trade_pending)?;
    user.available = accounting::debit_available(user.available, locked, pay)?;
    board.trading_revenue = board.trading_revenue.saturating_add(cost);
    board.cost_basis = board.cost_basis.saturating_add(cost);
    board.fees_accrued = board.fees_accrued.saturating_add(fee);
    Ok(())
}

/// ER sync path: unlock pre-locked trade_pending for the fill, then debit available into the Board.
/// Requires trade_pending >= pay always (no free-margin heal).
pub fn credit_er_trade_inner(
    board: &mut Board,
    user: &mut crate::UserVault,
    cost: u64,
    fee: u64,
) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    let pay = cost.checked_add(fee).ok_or(VaultError::Overflow)?;
    require!(pay > 0, VaultError::ZeroAmount);
    require!(user.trade_pending >= pay, VaultError::InsufficientTradePending);
    user.trade_pending = accounting::release(user.trade_pending, pay)?;
    let locked = accounting::locked(user.reserved, user.trade_pending)?;
    user.available = accounting::debit_available(user.available, locked, pay)?;
    board.trading_revenue = board.trading_revenue.saturating_add(cost);
    board.cost_basis = board.cost_basis.saturating_add(cost);
    board.fees_accrued = board.fees_accrued.saturating_add(fee);
    Ok(())
}

/// Return LMSR sell proceeds from the pot to unused margin. Fees stay accrued.
/// `basis_cut` is min(position.cost_paid, proceeds) — profit above basis does not reduce VOID liability.
pub fn refund_trade_inner(
    board: &mut Board,
    user: &mut crate::UserVault,
    cost: u64,
    basis_cut: u64,
) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    require!(cost > 0, VaultError::ZeroAmount);
    require!(basis_cut <= cost, VaultError::BadSettle);
    require!(board.trading_revenue >= cost, VaultError::InsufficientAvailable);
    require!(board.cost_basis >= basis_cut, VaultError::InsufficientAvailable);
    board.trading_revenue = board.trading_revenue.checked_sub(cost).ok_or(VaultError::Overflow)?;
    board.cost_basis = board.cost_basis.checked_sub(basis_cut).ok_or(VaultError::Overflow)?;
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
pub const GRID_SEED: &[u8] = b"grid";
/// Must match `market::state::Grid::SHARD_CELLS`.
pub const GRID_SHARD_CELLS: u16 = 16;

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

/// Live trading grid PDA only — rejects `gdump` and any other market-owned account.
pub fn require_grid_pda(grid_key: &Pubkey, market: &Pubkey, start: u16) -> Result<()> {
    require!(start % GRID_SHARD_CELLS == 0, VaultError::BadSettle);
    let ix = start / GRID_SHARD_CELLS;
    let expected = if ix == 0 {
        Pubkey::find_program_address(&[GRID_SEED, market.as_ref()], &crate::views::MARKET_ID).0
    } else {
        let ix_u8 = u8::try_from(ix).map_err(|_| error!(VaultError::BadSettle))?;
        Pubkey::find_program_address(&[GRID_SEED, market.as_ref(), &[ix_u8]], &crate::views::MARKET_ID)
            .0
    };
    require_keys_eq!(*grid_key, expected, VaultError::WrongBoard);
    Ok(())
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
    book: &RiskBook,
    pool: Option<&mut AdjustPool>,
    tap: Option<&mut BoardTap>,
) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    require!(record.phase == PHASE_FINALIZED, VaultError::NotFinalized);
    require!(!record.refunds_due, VaultError::RefundsDue);
    require!(record.market == board.market, VaultError::WrongBoard);
    require!(book.market == board.market, VaultError::WrongBoard);
    require!(market.status == STATUS_SETTLED, VaultError::NotFinalized);
    // Partial wide apply without finish would inflate E(x*) / L without matching revenue.
    require!(
        market.wide_write == 0 && market.wide_flags & 1 == 0,
        VaultError::DirtyWide
    );
    // ER fills update Market first; Board catches up only after sync_vault.
    require!(
        board.trading_revenue == market.trading_revenue
            && board.fees_accrued == market.fees_accrued
            && board.cost_basis == market.cost_basis,
        VaultError::BoardNotSynced
    );
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
    let premium = book.premium_payable;
    let c_r = book.c_r;
    board.champion = book.head_quote;
    // Winners first: \(\rho\) uses full trading revenue (premium is not prepaid from \(R\)).
    // Draw-path `pay_premium` is disabled — LP premium only via no-draw \(S_R\) waterfall.
    let r = r_net(board.trading_revenue, 0);
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
    // No filled risk capital → remainder after $S_C$ stays with the platform.
    let (sc, sr, sp) = surplus_split(s, market.alpha_r_bps, c_r > 0);

    board.premium_payable = premium;
    board.c_r_final = c_r;
    board.liability = liability;
    board.c_max = cmax;
    board.rho_raw = rho.raw();
    board.surplus = s;
    board.surplus_r = sr;
    board.surplus_p = sp;
    board.surplus_c = sc;
    board.cell = cell as u16;
    board.phase = BOARD_SETTLED;
    board.fee_bps = market.fee_bps;
    board.fee_timing = market.fee_timing;
    board.reward_bits = 0;
    board.draw_bits = 0;
    Ok(())
}

/// $C_P^{\mathrm{alloc}}$ only when $L>R_{\mathrm{net}}$.
/// When $L>R$, both pool and tap are required (omitting them locked ρ too low forever).
pub fn draw_c_p(
    liability: u64,
    r_net: u64,
    c_m: u64,
    c_r: u64,
    pool: Option<&mut AdjustPool>,
    tap: Option<&mut BoardTap>,
    market: Pubkey,
) -> Result<u64> {
    let (pool, tap) = match (pool, tap) {
        (Some(pool), Some(tap)) => (pool, tap),
        (None, None) if liability <= r_net => return Ok(0),
        _ => return err!(VaultError::MissingCpAccounts),
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
    let locked = accounting::locked(user.reserved, user.trade_pending)?;
    user.available = accounting::debit_available(user.available, locked, amount)?;
    pool.available = accounting::credit(pool.available, amount)?;
    Ok(())
}

/// Move this board's accrued fees into the platform UserVault. Never credits $C_P$.
/// Only after settle/refund — claiming while IDLE would desync `Board.fees` vs `Market.fees`
/// and permanently brick `BoardNotSynced`.
pub fn claim_fees_inner(board: &mut Board, platform: &mut crate::UserVault) -> Result<u64> {
    require!(
        board.phase == BOARD_SETTLED || board.phase == BOARD_REFUND,
        VaultError::NotFinalized
    );
    let fees = board.fees_accrued;
    if fees > 0 {
        platform.available = accounting::credit(platform.available, fees)?;
        board.fees_accrued = 0;
    }
    Ok(fees)
}

pub fn compute_refund(board: &mut Board, market: &Market, record: &Resolution) -> Result<()> {
    require!(board.phase == BOARD_IDLE, VaultError::AlreadySettled);
    require!(
        record.phase == PHASE_FAILED || record.phase == PHASE_VOIDED,
        VaultError::NotFinalized
    );
    require!(record.refunds_due, VaultError::RefundsDue);
    require!(record.market == board.market, VaultError::WrongBoard);
    require!(market.status == STATUS_VOID, VaultError::NotFinalized);
    require!(
        market.wide_write == 0 && market.wide_flags & 1 == 0,
        VaultError::DirtyWide
    );
    // Same gate as settle: ER fills hit Market first; refunding cost_paid before sync
    // would credit available twice (buy never debited available until sync_vault).
    require!(
        board.trading_revenue == market.trading_revenue
            && board.fees_accrued == market.fees_accrued
            && board.cost_basis == market.cost_basis,
        VaultError::BoardNotSynced
    );
    let r = board.trading_revenue;
    let basis = board.cost_basis;
    // Pro-rata VOID: ρ = min(1, R / Σcost_paid). Avoids first-come drain after profitable sells.
    board.rho_raw = recovery_rate(
        Q64::from_int(r as i64),
        Q64::from_int(basis as i64),
    )
    .raw();
    board.liability = basis;
    board.c_max = r;
    board.phase = BOARD_REFUND;
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
    require!(
        pos.vault_owed_cost == 0 && pos.vault_owed_fee == 0 && pos.vault_owed_credit == 0,
        VaultError::BoardNotSynced
    );
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
        let next = board
            .paid_users
            .checked_add(net)
            .ok_or(VaultError::Overflow)?;
        require!(next <= board.c_max, VaultError::InsufficientAvailable);
        user.available = accounting::credit(user.available, net)?;
        board.paid_users = next;
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
    quote_key: Pubkey,
    quote: &Quote,
    lp: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(layer.market == board.market && quote.market == board.market, VaultError::WrongBoard);
    require!(quote.layer_id == layer.layer_id, VaultError::WrongBoard);
    let slot = quote_slot(layer, quote_key)?;
    require!(slot < 64, VaultError::Overflow);
    let bit = 1u64 << slot;
    if board.draw_bits & bit != 0 {
        return Ok(0);
    }
    // Fixed rank order (cheap unit, then earlier ts) so callers cannot pick who eats H.
    require_draw_rank_ready(board, layer, slot)?;
    // Own funds first: trading revenue. Every filled quote may cover the leftover shortfall.
    let own = board.trading_revenue;
    let need = board.liability.saturating_sub(own).saturating_sub(board.drawn_h);
    let h = quote.filled.min(need);
    require!(h <= quote.capacity, VaultError::Overflow);
    let locked = reserved_for_quote(quote);
    if locked > 0 {
        lp.reserved = accounting::release(lp.reserved, locked)?;
    }
    if h > 0 {
        let locked_now = accounting::locked(lp.reserved, lp.trade_pending)?;
        lp.available = accounting::debit_available(lp.available, locked_now, h)?;
        board.drawn_h = board.drawn_h.saturating_add(h);
        pnl_add(lp, -(h as i64));
    }
    board.draw_bits |= bit;
    Ok(h)
}

/// All strictly better-ranked **filled** quotes must already be drawn.
/// `live==0` (cancelled leftover) does not drop filled exposure from the H order.
fn require_draw_rank_ready(board: &Board, layer: &Layer, my_slot: usize) -> Result<()> {
    let my_unit = layer.unit_at(my_slot);
    let my_ts = layer.quote_ts[my_slot];
    for i in 0..layer.quote_count as usize {
        if i == my_slot || layer.quote_filled[i] == 0 {
            continue;
        }
        let u = layer.unit_at(i);
        let ts = layer.quote_ts[i];
        let better = u < my_unit || (u == my_unit && (ts < my_ts || (ts == my_ts && i < my_slot)));
        if better {
            let bit = 1u64 << i;
            require!(board.draw_bits & bit != 0, VaultError::WrongBoard);
        }
    }
    Ok(())
}

pub fn release_quote(
    board: &mut Board,
    layer: &Layer,
    quote_key: Pubkey,
    quote: &Quote,
    lp: &mut crate::UserVault,
) -> Result<()> {
    require!(board.phase == BOARD_REFUND, VaultError::NotFinalized);
    require!(quote.market == board.market, VaultError::WrongBoard);
    require!(layer.market == board.market, VaultError::WrongBoard);
    require!(quote.layer_id == layer.layer_id, VaultError::WrongBoard);
    let slot = quote_slot(layer, quote_key)?;
    require!(slot < 64, VaultError::Overflow);
    let bit = 1u64 << slot;
    if board.draw_bits & bit != 0 {
        return Ok(());
    }
    let locked = reserved_for_quote(quote);
    if locked > 0 {
        lp.reserved = accounting::release(lp.reserved, locked)?;
    }
    board.draw_bits |= bit;
    Ok(())
}

pub fn pay_premium_inner(
    board: &mut Board,
    _quote_key: Pubkey,
    quote: &Quote,
    layer: &Layer,
    _lp: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(quote.market == board.market, VaultError::WrongBoard);
    require!(layer.market == board.market, VaultError::WrongBoard);
    // No-draw: LP rewards come from \(S_R\) waterfall (`pay_surplus_lp`).
    // Draw path: do not mint `premium_owed` into `available` — there is no escrowed
    // premium slice after winners take full \(R\) (would inflate the shared vault ATA).
    Ok(0)
}

pub fn pay_surplus_lp_inner(
    board: &mut Board,
    quote_key: Pubkey,
    quote: &Quote,
    layer: Option<&Layer>,
    lp: &mut crate::UserVault,
    _weight_sum: u64,
) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(board.rho_raw >= Q64::ONE.raw(), VaultError::NoSurplus);
    require!(quote.market == board.market, VaultError::WrongBoard);
    if board.surplus_r == 0 || quote.filled == 0 {
        return Ok(0);
    }
    let left = board.surplus_r.saturating_sub(board.paid_surplus);
    if left == 0 {
        return Ok(0);
    }
    let layer = layer.ok_or(error!(VaultError::WrongBoard))?;
    let (share, mark_slot) = if board.liability <= board.trading_revenue {
        (no_draw_waterfall_share(board, layer, quote_key, quote)?, None)
    } else {
        draw_surplus_share(board, layer, quote_key, quote, left)?
    };
    let share = share.min(left);
    if share > 0 {
        lp.available = accounting::credit(lp.available, share)?;
        board.paid_surplus = board.paid_surplus.saturating_add(share);
        pnl_add(lp, share as i64);
    }
    if let Some(slot) = mark_slot {
        board.reward_bits |= 1u64 << slot;
    }
    Ok(share)
}

fn quote_slot(layer: &Layer, quote_key: Pubkey) -> Result<usize> {
    for i in 0..layer.quote_count as usize {
        if layer.quote_keys[i] == quote_key {
            return Ok(i);
        }
    }
    err!(VaultError::WrongBoard)
}

/// Draw-path \(S_R\): pro-rata by `filled` (on-chain `weight_sum`; caller arg ignored).
/// Layer does not store per-quote `profit_share_bps`, so weights cannot be forged off-chain.
fn draw_surplus_share(
    board: &Board,
    layer: &Layer,
    quote_key: Pubkey,
    quote: &Quote,
    left: u64,
) -> Result<(u64, Option<usize>)> {
    require!(layer.market == board.market, VaultError::WrongBoard);
    require!(quote.layer_id == layer.layer_id, VaultError::WrongBoard);
    require!(!quote.cancelled, VaultError::WrongBoard);
    let slot = quote_slot(layer, quote_key)?;
    require!(slot < 64, VaultError::Overflow);
    require!(layer.quote_live[slot] != 0, VaultError::WrongBoard);
    let bit = 1u64 << slot;
    if board.reward_bits & bit != 0 {
        return Ok((0, None));
    }
    let mut weight_sum: u128 = 0;
    for i in 0..layer.quote_count as usize {
        if layer.quote_live[i] == 0 || layer.quote_filled[i] == 0 {
            continue;
        }
        weight_sum = weight_sum.saturating_add(layer.quote_filled[i] as u128);
    }
    if weight_sum == 0 || quote.filled == 0 {
        // Consume the slot so a zero-weight quote cannot retry after others pay.
        return Ok((0, Some(slot)));
    }
    let share = ((board.surplus_r as u128).saturating_mul(quote.filled as u128) / weight_sum) as u64;
    Ok((share.min(left), Some(slot)))
}

/// Ceil(`unit * filled / 1e6`) so truncated unit storage never underestimates others' caps.
fn reconstructed_owed(unit: u128, filled: u64) -> u64 {
    if filled == 0 || unit == 0 {
        return 0;
    }
    let num = unit.saturating_mul(filled as u128);
    ((num + 999_999) / 1_000_000) as u64
}

fn no_draw_waterfall_share(
    board: &mut Board,
    layer: &Layer,
    quote_key: Pubkey,
    quote: &Quote,
) -> Result<u64> {
    require!(layer.market == board.market, VaultError::WrongBoard);
    require!(quote.layer_id == layer.layer_id, VaultError::WrongBoard);
    let mut slot = None;
    let mut mine = None;
    let mut bids = Vec::new();
    let mut caps = Vec::new();
    for i in 0..layer.quote_count as usize {
        if layer.quote_live[i] == 0 || layer.quote_filled[i] == 0 {
            continue;
        }
        let live = caps.len();
        if layer.quote_keys[i] == quote_key {
            slot = Some(i);
            mine = Some(live);
        }
        bids.push(Bid {
            unit_premium: layer.unit_at(i),
            ts: layer.quote_ts[i],
            idx: live,
        });
        // Same ceil formula for every quote — never underestimate peers via truncated unit.
        caps.push(reconstructed_owed(layer.unit_at(i), layer.quote_filled[i]));
    }
    let slot = slot.ok_or(error!(VaultError::WrongBoard))?;
    let mine = mine.ok_or(error!(VaultError::WrongBoard))?;
    require!(slot < 64, VaultError::Overflow);
    let bit = 1u64 << slot;
    if board.reward_bits & bit != 0 {
        return Ok(0);
    }
    let order = sort_bids(bids);
    let ranked: Vec<u64> = order.iter().map(|b| caps[b.idx]).collect();
    let pays = waterfall_pay(board.surplus_r, &ranked);
    let mut due = 0u64;
    for (rank_pos, b) in order.iter().enumerate() {
        if b.idx == mine {
            due = pays[rank_pos];
            break;
        }
    }
    board.reward_bits |= bit;
    Ok(due)
}

/// Credit this board's \(S_P\) into the platform UserVault. Any payer MAY submit.
/// USDC stays in the vault ATA; only the platform owner can `withdraw`.
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
    // `paid_surplus` tracks only \(S_R\); \(S_P\) is idempotent via zeroing `surplus_p`.
    platform.available = accounting::credit(platform.available, share)?;
    board.surplus_p = 0;
    Ok(share)
}

pub fn pay_surplus_cover_inner(board: &mut Board, pool: &mut LossPool) -> Result<u64> {
    require!(board.phase == BOARD_SETTLED, VaultError::NotFinalized);
    require!(board.rho_raw >= Q64::ONE.raw(), VaultError::NoSurplus);
    let due = board.surplus_c.saturating_sub(board.paid_cover);
    if due == 0 {
        return Ok(0);
    }
    pool.available = accounting::credit(pool.available, due)?;
    board.paid_cover = board.paid_cover.saturating_add(due);
    Ok(due)
}

/// Uncovered operating loss: $(-\Pi)^+ - \texttt{cover\_paid}$.
pub fn uncovered_loss(lp: &crate::UserVault) -> u64 {
    if lp.risk_pnl >= 0 {
        return 0;
    }
    lp.risk_pnl.unsigned_abs().saturating_sub(lp.cover_paid)
}

/// Credit a losing LP from the protocol cover pool up to uncovered $(-\Pi)^+$.
/// Does not rewrite `risk_pnl`. Inflows (`pay_surplus_cover`) are not gated.
pub fn cover_lp_loss_inner(pool: &mut LossPool, lp: &mut crate::UserVault) -> Result<u64> {
    let need = uncovered_loss(lp);
    if need == 0 || pool.available == 0 {
        return Ok(0);
    }
    let take = need.min(pool.available);
    pool.available = pool.available.saturating_sub(take);
    lp.available = accounting::credit(lp.available, take)?;
    lp.cover_paid = lp.cover_paid.saturating_add(take);
    Ok(take)
}

fn pnl_add(user: &mut crate::UserVault, delta: i64) {
    user.risk_pnl = user.risk_pnl.saturating_add(delta);
}

pub fn refund_position_inner(
    board: &mut Board,
    pos: &Position,
    user: &mut crate::UserVault,
) -> Result<u64> {
    require!(board.phase == BOARD_REFUND, VaultError::NotFinalized);
    require!(pos.market == board.market, VaultError::WrongBoard);
    // Unsynced ER seats still hold cost in trade_pending / vault_owed — must sync_vault first.
    require!(
        pos.vault_owed_cost == 0 && pos.vault_owed_fee == 0 && pos.vault_owed_credit == 0,
        VaultError::BoardNotSynced
    );
    if pos.cost_paid == 0 {
        return Ok(0);
    }
    let back = payout_floor(Q64::from_raw(board.rho_raw), pos.cost_paid);
    board.dust = board
        .dust
        .saturating_add(rho_dust(board.rho_raw, pos.cost_paid, back));
    if back > 0 {
        let next = board
            .paid_users
            .checked_add(back)
            .ok_or(VaultError::Overflow)?;
        require!(next <= board.c_max, VaultError::InsufficientAvailable);
        user.available = accounting::credit(user.available, back)?;
        board.paid_users = next;
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
    use crate::views::{FamilyExtra, Outcome};
    use crate::UserVault;

    fn user(available: u64, reserved: u64) -> UserVault {
        UserVault {
            owner: Pubkey::default(),
            available,
            reserved,
            bump: 0,
            risk_pnl: 0,
            bond: 0,
            cover_paid: 0,
            trade_pending: 0,
        }
    }

    fn board() -> Board {
        Board {
            market: Pubkey::new_from_array([7; 32]),
            c_m_locked: 0,
            trading_revenue: 30,
            cost_basis: 30,
            fees_accrued: 5,
            premium_payable: 0,
            c_r_final: 0,
            liability: 0,
            c_max: 0,
            rho_raw: 0,
            surplus: 0,
            surplus_r: 0,
            surplus_p: 0,
            surplus_c: 0,
            paid_users: 0,
            paid_premium: 0,
            paid_surplus: 0,
            paid_cover: 0,
            drawn_h: 0,
            dust: 0,
            cell: 0,
            phase: BOARD_IDLE,
            bump: 0,
            fee_bps: 0,
            fee_timing: 0,
            champion: Pubkey::default(),
            reward_bits: 0,
            draw_bits: 0,
        }
    }


    fn qkey(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn layer1(market: Pubkey, quote_key: Pubkey, attachment: u64, thickness: u64, filled: u64) -> Layer {
        let mut layer = Layer {
            market,
            attachment,
            thickness,
            filled,
            layer_id: 1,
            quote_count: 1,
            bump: 0,
            ..Layer::default()
        };
        layer.quote_keys[0] = quote_key;
        layer
    }

    #[test]
    fn pay_premium_draw_path_does_not_mint() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.trading_revenue = 10;
        b.liability = 100;
        b.rho_raw = Q64::ONE.raw();
        b.premium_payable = 4;
        let qk = Pubkey::new_from_array([9; 32]);
        let mut layer = Layer {
            market: b.market,
            quote_count: 1,
            ..Layer::default()
        };
        layer.quote_keys[0] = qk;
        layer.quote_live[0] = 1;
        layer.quote_filled[0] = 40;
        let quote = Quote {
            market: b.market,
            lp: Pubkey::default(),
            capacity: 40,
            filled: 40,
            premium: 4,
            premium_owed: 4,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 0,
            cancelled: false,
            bump: 0,
        };
        let mut lp = user(0, 0);
        assert_eq!(pay_premium_inner(&mut b, qk, &quote, &layer, &mut lp).unwrap(), 0);
        assert_eq!(lp.available, 0);
        assert_eq!(b.paid_premium, 0);
    }

    #[test]
    fn platform_surplus_does_not_starve_lp_surplus() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.rho_raw = Q64::ONE.raw();
        b.surplus = 100;
        b.surplus_c = 20;
        b.surplus_r = 56;
        b.surplus_p = 24;
        let mut plat = user(0, 0);
        assert_eq!(pay_surplus_platform_inner(&mut b, &mut plat).unwrap(), 24);
        assert_eq!(plat.available, 24);
        assert_eq!(b.surplus_p, 0);
        assert_eq!(b.paid_surplus, 0);
        // LP still sees full S_R budget.
        assert_eq!(b.surplus_r.saturating_sub(b.paid_surplus), 56);
    }

    #[test]
    fn draw_surplus_ignores_forged_weight_sum_and_is_idempotent() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.trading_revenue = 10;
        b.liability = 100;
        b.rho_raw = Q64::ONE.raw();
        b.surplus_r = 100;
        let a = Pubkey::new_from_array([1; 32]);
        let c = Pubkey::new_from_array([2; 32]);
        let mut layer = Layer {
            market: b.market,
            quote_count: 2,
            ..Layer::default()
        };
        layer.quote_keys[0] = a;
        layer.quote_keys[1] = c;
        layer.quote_live[0] = 1;
        layer.quote_live[1] = 1;
        layer.quote_filled[0] = 50;
        layer.quote_filled[1] = 50;
        let qa = Quote {
            market: b.market,
            lp: a,
            capacity: 50,
            filled: 50,
            premium: 0,
            premium_owed: 0,
            ts: 1,
            profit_share_bps: 10_000,
            layer_id: 0,
            cancelled: false,
            bump: 0,
        };
        let mut lp = user(0, 0);
        // Forged weight_sum=1 must not grant the whole surplus_r.
        assert_eq!(
            pay_surplus_lp_inner(&mut b, a, &qa, Some(&layer), &mut lp, 1).unwrap(),
            50
        );
        assert_eq!(lp.available, 50);
        assert_eq!(
            pay_surplus_lp_inner(&mut b, a, &qa, Some(&layer), &mut lp, 1).unwrap(),
            0
        );
        assert_eq!(lp.available, 50);
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
        b.phase = BOARD_SETTLED;
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
    fn claim_fees_rejected_while_idle() {
        let mut b = board();
        b.phase = BOARD_IDLE;
        b.fees_accrued = 7;
        let mut plat = user(0, 0);
        assert!(claim_fees_inner(&mut b, &mut plat).is_err());
        assert_eq!(b.fees_accrued, 7);
    }

    #[test]
    fn claim_timing_fee_taken_from_payout() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.c_max = 100;
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
        b.c_max = 90;
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
        refund_trade_inner(&mut b, &mut u, 12, 12).unwrap();
        assert_eq!(b.trading_revenue, 18);
        assert_eq!(b.cost_basis, 18);
        assert_eq!(u.available, 22);
        assert!(refund_trade_inner(&mut b, &mut u, 19, 19).is_err());
    }

    fn market_view(board: &Board) -> Market {
        Market {
            family: 0,
            status: STATUS_VOID,
            bump: 0,
            grid_bump: 0,
            n: 1,
            fee_bps: 0,
            creator: Pubkey::default(),
            committee: Pubkey::default(),
            authorized_reporter: Pubkey::default(),
            close_ts: 0,
            risk_lock_ts: 0,
            report_window_secs: 0,
            challenge_secs: 0,
            n_layers: 0,
            gamma_bps: 0,
            d_unit: 0,
            beta: 0,
            c_m: 0,
            fees_accrued: board.fees_accrued,
            trading_revenue: board.trading_revenue,
            alpha_r_bps: 0,
            platform: Pubkey::default(),
            l_max: 0,
            id_hash: [0; 32],
            extra: FamilyExtra {
                a: 0,
                b: 0,
                c: 0,
                d: 0,
                e: 0,
                f: 0,
                u0: 0,
                u1: 0,
                u2: 0,
                u3: 0,
            },
            fee_timing: 0,
            delegated: false,
            trades_root: [0; 32],
            commit_ts: 0,
            p0_sum: 0,
            seal_bits: 0,
            wide_z0: 0,
            wide_z: 0,
            wide_q: 0,
            wide_nonce: 0,
            wide_read: 0,
            wide_write: 0,
            wide_tag: 0,
            wide_flags: 0,
            report_open_ts: 0,
            committee_bond: 0,
            cost_basis: board.cost_basis,
        }
    }

    fn void_record(market: Pubkey) -> Resolution {
        Resolution {
            market,
            members: [Pubkey::default(); 16],
            authorized_reporter: Pubkey::default(),
            family: 0,
            phase: PHASE_VOIDED,
            m: 0,
            n: 0,
            extensions: 0,
            votes_proposal: 0,
            votes_challenge: 0,
            bump: 0,
            refunds_due: true,
            early_resolve: false,
            k_max: 0,
            layout: 0,
            n_atoms: 0,
            close_ts: 0,
            report_window_secs: 0,
            report_deadline: 0,
            challenge_secs: 0,
            challenge_end: 0,
            vote_end: 0,
            proposer: Pubkey::default(),
            challenger: Pubkey::default(),
            proposed: Outcome::default(),
            challenged: Outcome::default(),
            final_outcome: Outcome::default(),
            evidence_hash: [0; 32],
            report_open_ts: 0,
            slash_due: false,
            void_votes: 0,
        }
    }

    #[test]
    fn void_refunds_cost_not_rho() {
        let mut b = board();
        b.phase = BOARD_REFUND;
        b.rho_raw = Q64::ONE.raw();
        b.c_max = b.trading_revenue;
        b.liability = b.cost_basis;
        let pos = Position {
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
    fn void_pro_rata_when_r_below_basis() {
        // Profitable sell drained R below Σ cost_paid — holders share R pro-rata.
        let mut b = board();
        b.trading_revenue = 50;
        b.cost_basis = 100;
        b.fees_accrued = 0;
        let m = market_view(&b);
        let r = void_record(b.market);
        compute_refund(&mut b, &m, &r).unwrap();
        assert_eq!(b.phase, BOARD_REFUND);
        assert_eq!(b.c_max, 50);
        let pos = Position {
            market: b.market,
            owner: Pubkey::default(),
            set_hash: [0; 32],
            q: Q64::from_int(50).raw(),
            cost_paid: 100,
            claimed: false,
            bump: 0,
            vault_owed_cost: 0,
            vault_owed_fee: 0,
            vault_owed_credit: 0,
        };
        let mut u = user(0, 0);
        assert_eq!(refund_position_inner(&mut b, &pos, &mut u).unwrap(), 50);
        assert_eq!(u.available, 50);
    }

    #[test]
    fn void_begin_requires_board_synced() {
        let mut b = board();
        let mut m = market_view(&b);
        m.trading_revenue = b.trading_revenue + 1;
        let r = void_record(b.market);
        assert!(compute_refund(&mut b, &m, &r).is_err());
        m.trading_revenue = b.trading_revenue;
        compute_refund(&mut b, &m, &r).unwrap();
        assert_eq!(b.phase, BOARD_REFUND);
        assert_eq!(b.rho_raw, Q64::ONE.raw());
    }

    #[test]
    fn void_refund_rejects_unsynced_seat() {
        let mut b = board();
        b.phase = BOARD_REFUND;
        let pos = Position {
            market: b.market,
            owner: Pubkey::default(),
            set_hash: [0; 32],
            q: Q64::from_int(50).raw(),
            cost_paid: 12,
            claimed: false,
            bump: 0,
            vault_owed_cost: 12,
            vault_owed_fee: 0,
            vault_owed_credit: 0,
        };
        let mut u = user(0, 0);
        assert!(refund_position_inner(&mut b, &pos, &mut u).is_err());
        assert_eq!(u.available, 0);
    }

    #[test]
    fn layer_draw_uses_h() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.liability = 80;
        let qk = qkey(9);
        let layer = layer1(b.market, qk, 50, 40, 40);
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
        assert_eq!(draw_quote(&mut b, &layer, qk, &quote, &mut lp).unwrap(), 40);
        assert_eq!(lp.reserved, 0);
        assert_eq!(lp.available, 60);
        assert_eq!(b.drawn_h, 40);
        // Idempotent: second call must not release again.
        assert_eq!(draw_quote(&mut b, &layer, qk, &quote, &mut lp).unwrap(), 0);
        assert_eq!(lp.reserved, 0);
        assert_eq!(lp.available, 60);
        assert_eq!(b.drawn_h, 40);
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
        let qk = qkey(9);
        let layer = layer1(b.market, qk, 20, 40, 40);
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
        assert_eq!(draw_quote(&mut b, &layer, qk, &quote, &mut lp).unwrap(), 0);
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
        let qk = qkey(9);
        let layer = layer1(b.market, qk, 20, 40, 40);
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
        assert_eq!(draw_quote(&mut b, &layer, qk, &quote, &mut lp).unwrap(), 10);
        assert_eq!(lp.available, 90);
        assert_eq!(b.drawn_h, 10);
    }

    #[test]
    fn draw_after_cancel_releases_only_filled() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.liability = 80;
        let qk = qkey(9);
        let layer = layer1(b.market, qk, 50, 40, 10);
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
        assert_eq!(draw_quote(&mut b, &layer, qk, &quote, &mut lp).unwrap(), 10);
        assert_eq!(lp.reserved, 0);
        assert_eq!(lp.available, 190);
    }

    #[test]
    fn release_quote_is_idempotent() {
        let mut b = board();
        b.phase = BOARD_REFUND;
        let qk = qkey(3);
        let layer = layer1(b.market, qk, 0, 40, 20);
        let quote = Quote {
            market: b.market,
            lp: Pubkey::default(),
            capacity: 40,
            filled: 20,
            premium: 0,
            premium_owed: 0,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let mut lp = user(100, 40);
        release_quote(&mut b, &layer, qk, &quote, &mut lp).unwrap();
        assert_eq!(lp.reserved, 0);
        release_quote(&mut b, &layer, qk, &quote, &mut lp).unwrap();
        assert_eq!(lp.reserved, 0);
        assert_eq!(b.draw_bits & 1, 1);
    }

    #[test]
    fn no_draw_surplus_waterfall_by_rank() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.trading_revenue = 100;
        b.liability = 40;
        b.rho_raw = Q64::ONE.raw();
        b.surplus_r = 100;
        let cheap = Pubkey::new_from_array([1; 32]);
        let rich = Pubkey::new_from_array([2; 32]);
        let mut layer = Layer {
            market: b.market,
            attachment: 0,
            thickness: 0,
            filled: 200,
            layer_id: 1,
            quote_count: 2,
            bump: 0,
            ..Layer::default()
        };
        layer.quote_live[0] = 1;
        layer.quote_live[1] = 1;
        layer.quote_keys[0] = cheap;
        layer.quote_keys[1] = rich;
        layer.quote_filled[0] = 80;
        layer.quote_filled[1] = 80;
        layer.set_unit(0, 80 * 1_000_000 / 80);
        layer.set_unit(1, 50 * 1_000_000 / 80);
        layer.quote_ts[0] = 1;
        layer.quote_ts[1] = 2;
        let q0 = Quote {
            market: b.market,
            lp: cheap,
            capacity: 80,
            filled: 80,
            premium: 80,
            premium_owed: 80,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let q1 = Quote {
            market: b.market,
            lp: rich,
            capacity: 80,
            filled: 80,
            premium: 50,
            premium_owed: 50,
            ts: 2,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let mut lp1 = user(0, 0);
        assert_eq!(
            pay_surplus_lp_inner(&mut b, rich, &q1, Some(&layer), &mut lp1, 0).unwrap(),
            50
        );
        let mut lp0 = user(0, 0);
        assert_eq!(
            pay_surplus_lp_inner(&mut b, cheap, &q0, Some(&layer), &mut lp0, 0).unwrap(),
            50
        );
        assert_eq!(lp1.available, 50);
        assert_eq!(lp0.available, 50);
        assert_eq!(
            pay_surplus_lp_inner(&mut b, rich, &q1, Some(&layer), &mut lp1, 0).unwrap(),
            0
        );
    }

    #[test]
    fn cancelled_quote_cannot_claim_draw_surplus() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.trading_revenue = 10;
        b.liability = 100;
        b.rho_raw = Q64::ONE.raw();
        b.surplus_r = 40;
        let live_k = Pubkey::new_from_array([1; 32]);
        let dead_k = Pubkey::new_from_array([2; 32]);
        let mut layer = Layer {
            market: b.market,
            attachment: 0,
            thickness: 0,
            filled: 100,
            layer_id: 1,
            quote_count: 2,
            bump: 0,
            ..Layer::default()
        };
        layer.quote_live[0] = 1;
        layer.quote_live[1] = 0;
        layer.quote_keys[0] = live_k;
        layer.quote_keys[1] = dead_k;
        layer.quote_filled[0] = 50;
        layer.quote_filled[1] = 50;
        let dead = Quote {
            market: b.market,
            lp: dead_k,
            capacity: 100,
            filled: 50,
            premium: 0,
            premium_owed: 0,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: true,
            bump: 0,
        };
        let mut lp = user(0, 0);
        assert!(pay_surplus_lp_inner(&mut b, dead_k, &dead, Some(&layer), &mut lp, 0).is_err());
        assert_eq!(lp.available, 0);
        assert_eq!(b.paid_surplus, 0);
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
        // No shortfall → omit pool/tap is OK.
        assert_eq!(draw_c_p(30, 40, 0, 0, None, None, market).unwrap(), 0);
        // Shortfall without CP accounts must fail (cannot lock ρ too low forever).
        assert!(draw_c_p(100, 40, 0, 0, None, None, market).is_err());
        assert!(draw_c_p(100, 40, 0, 0, Some(&mut pool), None, market).is_err());
    }

    #[test]
    fn surplus_cover_funds_loss_pool_and_tops_up_negative_pnl() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.rho_raw = Q64::ONE.raw();
        b.surplus = 100;
        b.surplus_c = 20;
        b.surplus_r = 56;
        b.surplus_p = 24;
        let mut pool = LossPool {
            available: 0,
            bump: 1,
        };
        assert_eq!(pay_surplus_cover_inner(&mut b, &mut pool).unwrap(), 20);
        assert_eq!(pool.available, 20);
        assert_eq!(b.paid_cover, 20);
        let mut lp = user(0, 0);
        lp.risk_pnl = -50;
        assert_eq!(uncovered_loss(&lp), 50);
        assert_eq!(cover_lp_loss_inner(&mut pool, &mut lp).unwrap(), 20);
        assert_eq!(lp.available, 20);
        assert_eq!(lp.risk_pnl, -50);
        assert_eq!(lp.cover_paid, 20);
        assert_eq!(uncovered_loss(&lp), 30);
        assert_eq!(pool.available, 0);
        assert_eq!(cover_lp_loss_inner(&mut pool, &mut lp).unwrap(), 0);
        pool.available = 100;
        assert_eq!(cover_lp_loss_inner(&mut pool, &mut lp).unwrap(), 30);
        assert_eq!(lp.cover_paid, 50);
        assert_eq!(lp.risk_pnl, -50);
        assert_eq!(uncovered_loss(&lp), 0);
        assert_eq!(cover_lp_loss_inner(&mut pool, &mut lp).unwrap(), 0);
    }

    #[test]
    fn reconstructed_owed_ceils_truncated_unit() {
        // premium=1, capacity=3 → unit=333_333; floor path yields 0, ceil yields 1.
        assert_eq!(reconstructed_owed(333_333, 3), 1);
        assert_eq!(reconstructed_owed(1_000_000, 80), 80);
        assert_eq!(reconstructed_owed(0, 10), 0);
    }

    #[test]
    fn draw_rank_includes_cancelled_but_filled() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.trading_revenue = 0;
        b.liability = 30;
        let cheap = qkey(1);
        let rich = qkey(2);
        let mut layer = Layer {
            market: b.market,
            quote_count: 2,
            layer_id: 1,
            ..Layer::default()
        };
        layer.quote_keys[0] = rich;
        layer.quote_keys[1] = cheap;
        layer.quote_live[0] = 1;
        layer.quote_live[1] = 0; // cancelled leftover — filled must still rank
        layer.quote_filled[0] = 40;
        layer.quote_filled[1] = 40;
        layer.set_unit(0, 50 * 1_000_000 / 40);
        layer.set_unit(1, 10 * 1_000_000 / 40);
        layer.quote_ts[0] = 2;
        layer.quote_ts[1] = 1;
        let q_rich = Quote {
            market: b.market,
            lp: rich,
            capacity: 40,
            filled: 40,
            premium: 50,
            premium_owed: 50,
            ts: 2,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let mut lp_rich = user(100, 40);
        assert!(draw_quote(&mut b, &layer, rich, &q_rich, &mut lp_rich).is_err());
    }

    #[test]
    fn draw_requires_cheaper_quotes_first() {
        let mut b = board();
        b.phase = BOARD_SETTLED;
        b.trading_revenue = 0;
        b.liability = 30;
        let cheap = qkey(1);
        let rich = qkey(2);
        let mut layer = Layer {
            market: b.market,
            quote_count: 2,
            layer_id: 1,
            ..Layer::default()
        };
        layer.quote_keys[0] = rich;
        layer.quote_keys[1] = cheap;
        layer.quote_live[0] = 1;
        layer.quote_live[1] = 1;
        layer.quote_filled[0] = 40;
        layer.quote_filled[1] = 40;
        layer.set_unit(0, 50 * 1_000_000 / 40);
        layer.set_unit(1, 10 * 1_000_000 / 40);
        layer.quote_ts[0] = 2;
        layer.quote_ts[1] = 1;
        let q_rich = Quote {
            market: b.market,
            lp: rich,
            capacity: 40,
            filled: 40,
            premium: 50,
            premium_owed: 50,
            ts: 2,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let q_cheap = Quote {
            market: b.market,
            lp: cheap,
            capacity: 40,
            filled: 40,
            premium: 10,
            premium_owed: 10,
            ts: 1,
            profit_share_bps: 0,
            layer_id: 1,
            cancelled: false,
            bump: 0,
        };
        let mut lp_rich = user(100, 40);
        let mut lp_cheap = user(100, 40);
        assert!(draw_quote(&mut b, &layer, rich, &q_rich, &mut lp_rich).is_err());
        assert_eq!(draw_quote(&mut b, &layer, cheap, &q_cheap, &mut lp_cheap).unwrap(), 30);
        assert_eq!(draw_quote(&mut b, &layer, rich, &q_rich, &mut lp_rich).unwrap(), 0);
        assert_eq!(lp_rich.reserved, 0);
        assert_eq!(lp_rich.available, 100);
    }

    #[test]
    fn credit_er_requires_full_trade_pending() {
        let mut b = board();
        b.trading_revenue = 0;
        b.fees_accrued = 0;
        let mut u = user(100, 0);
        u.trade_pending = 30;
        assert!(credit_er_trade_inner(&mut b, &mut u, 40, 10).is_err());
        u.trade_pending = 50;
        credit_er_trade_inner(&mut b, &mut u, 40, 10).unwrap();
        assert_eq!(u.trade_pending, 0);
        assert_eq!(u.available, 50);
        assert_eq!(b.trading_revenue, 40);
        assert_eq!(b.fees_accrued, 10);
        // No free-margin heal: pending==0 must fail.
        b.trading_revenue = 0;
        b.fees_accrued = 0;
        let mut u2 = user(100, 0);
        u2.trade_pending = 0;
        assert!(credit_er_trade_inner(&mut b, &mut u2, 40, 10).is_err());
    }
}
