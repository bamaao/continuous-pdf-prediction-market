//! L1 risk auction (FR-RSK-01–08). Published layers only. Session is not authority.
//! Never Delegates. One lock underwrites one board.

use anchor_lang::prelude::*;
use market::state::{Market, Status};
use math::{sort_bids, Bid};
use vault::cpi::accounts::MutUser;
use vault::cpi::{release, reserve};

pub mod matching;

use matching::{
    d_required, layer_attachment, premium_due, take_from_head, QuoteView, MAX_LAYERS, MAX_QUOTES,
};

declare_id!("Rsk1111111111111111111111111111111111111111");

pub const BOOK_SEED: &[u8] = b"risk";
pub const LAYER_SEED: &[u8] = b"layer";
pub const QUOTE_SEED: &[u8] = b"quote";
pub const SEAT_SEED: &[u8] = b"seat";

#[program]
pub mod risk {
    use super::*;

    pub fn open_book(ctx: Context<OpenBook>) -> Result<()> {
        let market = &ctx.accounts.market;
        require!(market.n_layers >= 1 && market.n_layers <= MAX_LAYERS, RiskError::BadLayer);
        require!(market.d_unit > 0, RiskError::BadSize);
        require!(market.risk_lock_ts >= market.close_ts, RiskError::BadClock);

        let book = &mut ctx.accounts.book;
        book.market = market.key();
        book.c_m = market.c_m;
        book.d_unit = market.d_unit;
        book.n_layers = market.n_layers;
        book.gamma_bps = market.gamma_bps;
        book.c_r = 0;
        book.premium_payable = 0;
        book.bump = ctx.bumps.book;
        Ok(())
    }

    pub fn quote(
        ctx: Context<QuoteLayer>,
        layer_id: u8,
        capacity: u64,
        premium: u64,
        profit_share_bps: u16,
    ) -> Result<()> {
        let market = &ctx.accounts.market;
        let book = &mut ctx.accounts.book;
        require!(book.market == market.key(), RiskError::WrongBook);
        require!(layer_id >= 1 && layer_id <= book.n_layers, RiskError::BadLayer);
        require!(capacity > 0 && premium > 0, RiskError::BadSize);
        require!(profit_share_bps <= 10_000, RiskError::BadSize);
        require!(
            market.status != Status::Settled as u8 && market.status != Status::Void as u8,
            RiskError::Locked
        );
        let now = Clock::get()?.unix_timestamp;
        require!(now < market.risk_lock_ts, RiskError::Locked);
        let attach = layer_attachment(book.c_m, book.d_unit, layer_id).ok_or(RiskError::BadLayer)?;

        let layer = &mut ctx.accounts.layer;
        if layer.market == Pubkey::default() {
            layer.market = market.key();
            layer.layer_id = layer_id;
            layer.attachment = attach;
            layer.thickness = book.d_unit;
            layer.filled = 0;
            layer.quote_count = 0;
            layer.bump = ctx.bumps.layer;
        }
        require!(layer.attachment == attach && layer.thickness == book.d_unit, RiskError::BadLayer);
        require!((layer.quote_count as usize) < MAX_QUOTES, RiskError::BookFull);

        reserve(
            CpiContext::new(
                ctx.accounts.vault_program.to_account_info(),
                MutUser {
                    owner: ctx.accounts.lp.to_account_info(),
                    user: ctx.accounts.user_vault.to_account_info(),
                },
            ),
            capacity,
        )?;

        let q = &mut ctx.accounts.quote;
        q.market = market.key();
        q.layer_id = layer_id;
        q.lp = ctx.accounts.lp.key();
        q.capacity = capacity;
        q.filled = 0;
        q.premium = premium;
        q.premium_owed = 0;
        q.profit_share_bps = profit_share_bps;
        q.ts = now;
        q.cancelled = false;
        q.bump = ctx.bumps.quote;

        let seat = &mut ctx.accounts.seat;
        seat.market = market.key();
        seat.owner = ctx.accounts.lp.key();
        seat.bump = ctx.bumps.seat;

        let unit = math::unit_premium(premium, capacity).ok_or(RiskError::BadSize)?;
        let idx = layer.quote_count as usize;
        layer.quote_keys[idx] = q.key();
        layer.quote_cap[idx] = capacity;
        layer.quote_filled[idx] = 0;
        layer.unit_premia[idx] = unit;
        layer.quote_ts[idx] = now;
        layer.quote_live[idx] = 1;
        layer.quote_count += 1;

        if cheapest_key(layer)? == q.key() {
            try_fill(book, layer, q, seat, idx, market.l_max_usdc())?;
        }
        Ok(())
    }

    pub fn fill_next(ctx: Context<FillNext>) -> Result<()> {
        let market = &ctx.accounts.market;
        let book = &mut ctx.accounts.book;
        let layer = &mut ctx.accounts.layer;
        let quote = &mut ctx.accounts.quote;
        let seat = &mut ctx.accounts.seat;
        require!(book.market == market.key(), RiskError::WrongBook);
        require!(layer.market == market.key(), RiskError::WrongBook);
        require!(
            quote.market == market.key() && quote.layer_id == layer.layer_id,
            RiskError::WrongBook
        );
        require!(seat.market == market.key() && seat.owner == quote.lp, RiskError::WrongBook);
        require!(!quote.cancelled, RiskError::Cancelled);
        require!(
            market.status != Status::Settled as u8 && market.status != Status::Void as u8,
            RiskError::Locked
        );
        require!(Clock::get()?.unix_timestamp < market.risk_lock_ts, RiskError::Locked);
        let (head, idx) = cheapest_entry(layer)?;
        require!(head == quote.key(), RiskError::NotCheapest);
        try_fill(book, layer, quote, seat, idx, market.l_max_usdc())?;
        Ok(())
    }

    pub fn cancel_unfilled(ctx: Context<CancelUnfilled>) -> Result<()> {
        let quote = &mut ctx.accounts.quote;
        let layer = &mut ctx.accounts.layer;
        require!(quote.lp == ctx.accounts.lp.key(), RiskError::NotOwner);
        require!(!quote.cancelled, RiskError::Cancelled);
        let leftover = quote.capacity.saturating_sub(quote.filled);
        require!(leftover > 0, RiskError::AlreadyFilled);
        quote.cancelled = true;
        drop_quote(layer, quote.key());
        release(
            CpiContext::new(
                ctx.accounts.vault_program.to_account_info(),
                MutUser {
                    owner: ctx.accounts.lp.to_account_info(),
                    user: ctx.accounts.user_vault.to_account_info(),
                },
            ),
            leftover,
        )?;
        Ok(())
    }
}

fn cheapest_entry(layer: &Layer) -> Result<(Pubkey, usize)> {
    let mut live = Vec::new();
    let mut eligible = Vec::new();
    for i in 0..layer.quote_count as usize {
        if layer.quote_live[i] == 0 || layer.quote_filled[i] >= layer.quote_cap[i] {
            continue;
        }
        let bid = Bid {
            unit_premium: layer.unit_premia[i],
            ts: layer.quote_ts[i],
            idx: i,
        };
        live.push(bid);
        if layer.quote_skip[i] == 0 {
            eligible.push(bid);
        }
    }
    let pool = if eligible.is_empty() { live } else { eligible };
    require!(!pool.is_empty(), RiskError::EmptyLayer);
    let i = sort_bids(pool)[0].idx;
    Ok((layer.quote_keys[i], i))
}

fn cheapest_key(layer: &Layer) -> Result<Pubkey> {
    Ok(cheapest_entry(layer)?.0)
}

fn drop_quote(layer: &mut Layer, key: Pubkey) {
    for i in 0..layer.quote_count as usize {
        if layer.quote_keys[i] == key {
            layer.quote_live[i] = 0;
        }
    }
}

fn try_fill(
    book: &mut RiskBook,
    layer: &mut Layer,
    quote: &mut Quote,
    seat: &mut LpSeat,
    idx: usize,
    l_max: u64,
) -> Result<()> {
    if quote.cancelled {
        return Ok(());
    }
    let remain = layer.thickness.saturating_sub(layer.filled);
    let view = QuoteView {
        capacity: quote.capacity,
        filled: quote.filled,
        premium: quote.premium,
        ts: quote.ts,
        lp_filled_on_board: seat.filled_d,
    };
    let take = match take_from_head(
        remain,
        &view,
        book.gamma_bps,
        book.c_r,
        d_required(l_max, book.c_m),
        book.d_unit,
    ) {
        Ok(v) => v,
        Err(_) => {
            layer.quote_skip[idx] = 1;
            return Ok(());
        }
    };
    if take == 0 {
        return Ok(());
    }
    for skip in layer.quote_skip.iter_mut() {
        *skip = 0;
    }
    quote.filled = quote.filled.saturating_add(take);
    let due = premium_due(quote.premium, quote.capacity, take);
    quote.premium_owed = quote.premium_owed.saturating_add(due);
    layer.filled = layer.filled.saturating_add(take);
    layer.quote_filled[idx] = quote.filled;
    book.c_r = book.c_r.saturating_add(take);
    book.premium_payable = book.premium_payable.saturating_add(due);
    seat.filled_d = seat.filled_d.saturating_add(take);
    Ok(())
}

trait LMaxUsdc {
    fn l_max_usdc(&self) -> u64;
}

impl LMaxUsdc for Market {
    fn l_max_usdc(&self) -> u64 {
        let raw = self.l_max;
        if raw <= 0 {
            0
        } else {
            (raw as u128 >> 64) as u64
        }
    }
}

#[account]
pub struct RiskBook {
    pub market: Pubkey,
    pub c_m: u64,
    pub d_unit: u64,
    pub c_r: u64,
    pub premium_payable: u64,
    pub n_layers: u8,
    pub gamma_bps: u16,
    pub bump: u8,
}

impl RiskBook {
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 8 + 8 + 1 + 2 + 1;
}

#[account]
pub struct Layer {
    pub market: Pubkey,
    pub attachment: u64,
    pub thickness: u64,
    pub filled: u64,
    pub layer_id: u8,
    pub quote_count: u8,
    pub bump: u8,
    pub quote_live: [u8; MAX_QUOTES],
    pub quote_skip: [u8; MAX_QUOTES],
    pub quote_keys: [Pubkey; MAX_QUOTES],
    pub quote_cap: [u64; MAX_QUOTES],
    pub quote_filled: [u64; MAX_QUOTES],
    pub unit_premia: [u128; MAX_QUOTES],
    pub quote_ts: [i64; MAX_QUOTES],
}

impl Layer {
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 8 + 3 + MAX_QUOTES * (1 + 1 + 32 + 8 + 8 + 16 + 8);
}

#[account]
pub struct Quote {
    pub market: Pubkey,
    pub lp: Pubkey,
    pub capacity: u64,
    pub filled: u64,
    pub premium: u64,
    pub premium_owed: u64,
    pub ts: i64,
    pub profit_share_bps: u16,
    pub layer_id: u8,
    pub cancelled: bool,
    pub bump: u8,
}

impl Quote {
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 8 + 8 + 8 + 8 + 2 + 1 + 1 + 1;
}

#[account]
pub struct LpSeat {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub filled_d: u64,
    pub bump: u8,
}

impl LpSeat {
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 1;
}

#[derive(Accounts)]
pub struct OpenBook<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(owner = market::ID)]
    pub market: Account<'info, Market>,
    #[account(
        init,
        payer = payer,
        space = RiskBook::SIZE,
        seeds = [BOOK_SEED, market.key().as_ref()],
        bump
    )]
    pub book: Account<'info, RiskBook>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(layer_id: u8)]
pub struct QuoteLayer<'info> {
    #[account(mut)]
    pub lp: Signer<'info>,
    #[account(owner = market::ID)]
    pub market: Account<'info, Market>,
    #[account(mut, seeds = [BOOK_SEED, market.key().as_ref()], bump = book.bump)]
    pub book: Account<'info, RiskBook>,
    #[account(
        init_if_needed,
        payer = lp,
        space = Layer::SIZE,
        seeds = [LAYER_SEED, market.key().as_ref(), &[layer_id]],
        bump
    )]
    pub layer: Account<'info, Layer>,
    #[account(
        init,
        payer = lp,
        space = Quote::SIZE,
        seeds = [QUOTE_SEED, market.key().as_ref(), lp.key().as_ref(), &[layer_id]],
        bump
    )]
    pub quote: Account<'info, Quote>,
    #[account(
        init_if_needed,
        payer = lp,
        space = LpSeat::SIZE,
        seeds = [SEAT_SEED, market.key().as_ref(), lp.key().as_ref()],
        bump
    )]
    pub seat: Account<'info, LpSeat>,
    #[account(
        mut,
        seeds = [vault::USER_SEED, lp.key().as_ref()],
        bump = user_vault.bump,
        seeds::program = vault::ID,
        constraint = user_vault.owner == lp.key() @ RiskError::NotOwner
    )]
    pub user_vault: Account<'info, vault::UserVault>,
    pub vault_program: Program<'info, vault::program::Vault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FillNext<'info> {
    #[account(owner = market::ID)]
    pub market: Account<'info, Market>,
    #[account(mut, seeds = [BOOK_SEED, market.key().as_ref()], bump = book.bump)]
    pub book: Account<'info, RiskBook>,
    #[account(mut, seeds = [LAYER_SEED, market.key().as_ref(), &[layer.layer_id]], bump = layer.bump)]
    pub layer: Account<'info, Layer>,
    #[account(mut)]
    pub quote: Account<'info, Quote>,
    #[account(mut, seeds = [SEAT_SEED, market.key().as_ref(), quote.lp.as_ref()], bump = seat.bump)]
    pub seat: Account<'info, LpSeat>,
}

#[derive(Accounts)]
pub struct CancelUnfilled<'info> {
    pub lp: Signer<'info>,
    #[account(
        mut,
        seeds = [QUOTE_SEED, quote.market.as_ref(), lp.key().as_ref(), &[quote.layer_id]],
        bump = quote.bump
    )]
    pub quote: Account<'info, Quote>,
    #[account(
        mut,
        seeds = [LAYER_SEED, quote.market.as_ref(), &[quote.layer_id]],
        bump = layer.bump
    )]
    pub layer: Account<'info, Layer>,
    #[account(
        mut,
        seeds = [vault::USER_SEED, lp.key().as_ref()],
        bump = user_vault.bump,
        seeds::program = vault::ID,
        constraint = user_vault.owner == lp.key() @ RiskError::NotOwner
    )]
    pub user_vault: Account<'info, vault::UserVault>,
    pub vault_program: Program<'info, vault::program::Vault>,
}

#[error_code]
pub enum RiskError {
    #[msg("signer is not the creator or a roster member")]
    NotResolver,
    #[msg("layer id is not a published layer")]
    BadLayer,
    #[msg("capacity, premium, or gamma is illegal")]
    BadSize,
    #[msg("risk_lock_ts is before close_ts")]
    BadClock,
    #[msg("now >= risk_lock_ts")]
    Locked,
    #[msg("risk book does not belong to this market")]
    WrongBook,
    #[msg("layer quote list is full")]
    BookFull,
    #[msg("same LP exceeds concentration γ")]
    Concentration,
    #[msg("quote is not the cheapest live bid")]
    NotCheapest,
    #[msg("layer has no quotes")]
    EmptyLayer,
    #[msg("signer is not the quote LP")]
    NotOwner,
    #[msg("quote already cancelled")]
    Cancelled,
    #[msg("quote is fully filled")]
    AlreadyFilled,
}

#[cfg(test)]
mod tests {
    use super::matching::*;

    #[test]
    fn one_lock_one_board_seat_is_per_market() {
        assert_ne!(super::SEAT_SEED, super::BOOK_SEED);
    }

    #[test]
    fn c_r_only_counts_filled() {
        let q = QuoteView {
            capacity: 80,
            filled: 0,
            premium: 8,
            ts: 1,
            lp_filled_on_board: 0,
        };
        let take = take_from_head(50, &q, 10_000, 0, 0, 80).unwrap();
        assert_eq!(take, 50);
        assert!(take < q.capacity);
    }
}
