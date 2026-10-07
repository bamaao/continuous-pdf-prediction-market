//! L1 risk auction (FR-RSK-01–08). Single pool. Session is not authority.
//! Never Delegates. One lock underwrites one prediction market.

use anchor_lang::prelude::*;
use market::state::{Market, Status};
use vault::cpi::accounts::MutUser;
use vault::cpi::{release, reserve};

pub mod matching;

use matching::{
    layer_attachment, premium_due, take_from_head, QuoteView, POOL_LAYER, MAX_QUOTES,
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
        require!(market.d_unit > 0, RiskError::BadSize);
        require!(market.risk_lock_ts <= market.close_ts, RiskError::BadClock);

        let book = &mut ctx.accounts.book;
        book.market = market.key();
        book.c_m = 0;
        book.d_unit = market.d_unit;
        book.n_layers = 1;
        book.gamma_bps = 0;
        book.c_r = 0;
        book.premium_payable = 0;
        book.head_quote = Pubkey::default();
        book.head_premium = 0;
        book.head_unit = 0;
        book.head_ts = 0;
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
        require!(layer_id == POOL_LAYER, RiskError::BadLayer);
        require!(capacity >= book.d_unit && premium > 0, RiskError::BadSize);
        require!(profit_share_bps <= 10_000, RiskError::BadSize);
        require!(
            market.status != Status::Settled as u8 && market.status != Status::Void as u8,
            RiskError::Locked
        );
        let now = Clock::get()?.unix_timestamp;
        require!(now < market.close_ts && now < market.risk_lock_ts, RiskError::Locked);
        let attach = layer_attachment(book.d_unit, layer_id).ok_or(RiskError::BadLayer)?;

        let mut layer = ctx
            .accounts
            .layer
            .load_mut()
            .or_else(|_| ctx.accounts.layer.load_init())?;
        if layer.market == Pubkey::default() {
            layer.market = market.key();
            layer.layer_id = layer_id;
            layer.attachment = attach;
            layer.thickness = 0;
            layer.filled = 0;
            layer.quote_count = 0;
            layer.bump = ctx.bumps.layer;
        }
        require!(layer.attachment == 0 && layer.layer_id == POOL_LAYER, RiskError::BadLayer);
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

        let qk = ctx.accounts.quote.key();
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
        layer.quote_keys[idx] = qk;
        layer.quote_cap[idx] = capacity;
        layer.quote_filled[idx] = 0;
        layer.set_unit(idx, unit);
        layer.quote_ts[idx] = now;
        layer.quote_live[idx] = 1;
        layer.quote_count += 1;

        try_fill(book, &mut layer, q, seat, idx, qk)?;
        Ok(())
    }

    pub fn fill_next(ctx: Context<FillNext>) -> Result<()> {
        let market = &ctx.accounts.market;
        let qk = ctx.accounts.quote.key();
        let book = &mut ctx.accounts.book;
        let mut layer = ctx.accounts.layer.load_mut()?;
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
        let now = Clock::get()?.unix_timestamp;
        require!(now < market.close_ts && now < market.risk_lock_ts, RiskError::Locked);
        let idx = quote_index(&layer, qk)?;
        try_fill(book, &mut layer, quote, seat, idx, qk)?;
        Ok(())
    }

    pub fn cancel_unfilled(ctx: Context<CancelUnfilled>) -> Result<()> {
        let quote = &mut ctx.accounts.quote;
        let mut layer = ctx.accounts.layer.load_mut()?;
        require!(quote.lp == ctx.accounts.lp.key(), RiskError::NotOwner);
        require!(!quote.cancelled, RiskError::Cancelled);
        let leftover = quote.capacity.saturating_sub(quote.filled);
        require!(leftover > 0, RiskError::AlreadyFilled);
        quote.cancelled = true;
        drop_quote(&mut layer, quote.key());
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

fn quote_index(layer: &Layer, key: Pubkey) -> Result<usize> {
    for i in 0..layer.quote_count as usize {
        if layer.quote_keys[i] == key && layer.quote_live[i] != 0 {
            return Ok(i);
        }
    }
    err!(RiskError::EmptyLayer)
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
    quote_key: Pubkey,
) -> Result<()> {
    if quote.cancelled {
        return Ok(());
    }
    let leftover = quote.capacity.saturating_sub(quote.filled);
    let view = QuoteView {
        capacity: quote.capacity,
        filled: quote.filled,
        premium: quote.premium,
        ts: quote.ts,
        lp_filled_on_board: seat.filled_d,
    };
    let take = take_from_head(leftover, &view).unwrap_or(0);
    if take == 0 {
        return Ok(());
    }
    quote.filled = quote.filled.saturating_add(take);
    let due = premium_due(quote.premium, quote.capacity, take);
    quote.premium_owed = quote.premium_owed.saturating_add(due);
    layer.filled = layer.filled.saturating_add(take);
    layer.quote_filled[idx] = quote.filled;
    book.c_r = book.c_r.saturating_add(take);
    book.premium_payable = book.premium_payable.saturating_add(due);
    seat.filled_d = seat.filled_d.saturating_add(take);
    let unit = math::unit_premium(quote.premium, quote.capacity).unwrap_or(u128::MAX);
    touch_head(book, quote_key, unit, quote.ts, quote.premium_owed);
    Ok(())
}

fn touch_head(book: &mut RiskBook, quote_key: Pubkey, unit: u128, ts: i64, premium_owed: u64) {
    let better = book.head_quote == Pubkey::default()
        || unit < book.head_unit
        || (unit == book.head_unit && ts < book.head_ts);
    if better {
        book.head_quote = quote_key;
        book.head_unit = unit;
        book.head_ts = ts;
        book.head_premium = premium_owed;
    } else if book.head_quote == quote_key {
        book.head_premium = premium_owed;
    }
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
    pub head_quote: Pubkey,
    pub head_premium: u64,
    pub head_unit: u128,
    pub head_ts: i64,
}

impl RiskBook {
    pub const SIZE: usize = 8 + 32 + 8 + 8 + 8 + 8 + 1 + 2 + 1 + 32 + 8 + 16 + 8;
}

#[account(zero_copy)]
#[repr(C)]
pub struct Layer {
    pub market: Pubkey,
    pub attachment: u64,
    pub thickness: u64,
    pub filled: u64,
    pub layer_id: u8,
    pub quote_count: u8,
    pub bump: u8,
    pub _pad: [u8; 5],
    pub quote_live: [u8; MAX_QUOTES],
    pub quote_skip: [u8; MAX_QUOTES],
    pub quote_keys: [Pubkey; MAX_QUOTES],
    pub quote_cap: [u64; MAX_QUOTES],
    pub quote_filled: [u64; MAX_QUOTES],
    /// Packed `u128` unit premia as `[lo, hi]` so the account stays 8-byte aligned
    /// after the 8-byte discriminator (a real `u128` field would be 16-aligned and
    /// fail `bytemuck` / `AccountLoader` at offset 8).
    pub unit_premia: [[u64; 2]; MAX_QUOTES],
    pub quote_ts: [i64; MAX_QUOTES],
}

impl Layer {
    pub const SIZE: usize = 8 + core::mem::size_of::<Self>();

    pub fn unit_at(&self, i: usize) -> u128 {
        let [lo, hi] = self.unit_premia[i];
        (lo as u128) | ((hi as u128) << 64)
    }

    pub fn set_unit(&mut self, i: usize, v: u128) {
        self.unit_premia[i] = [v as u64, (v >> 64) as u64];
    }

    pub fn from_account_data(data: &[u8]) -> std::result::Result<Self, &'static str> {
        if data.len() < Self::SIZE {
            return Err("layer account too small");
        }
        let body = &data[8..Self::SIZE];
        if body.len() != core::mem::size_of::<Self>() {
            return Err("layer layout");
        }
        Ok(bytemuck::pod_read_unaligned(body))
    }
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
    pub market: Box<Account<'info, Market>>,
    #[account(
        init,
        payer = payer,
        space = RiskBook::SIZE,
        seeds = [BOOK_SEED, market.key().as_ref()],
        bump
    )]
    pub book: Box<Account<'info, RiskBook>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(layer_id: u8)]
pub struct QuoteLayer<'info> {
    #[account(mut)]
    pub lp: Signer<'info>,
    #[account(owner = market::ID)]
    pub market: Box<Account<'info, Market>>,
    #[account(mut, seeds = [BOOK_SEED, market.key().as_ref()], bump = book.bump)]
    pub book: Box<Account<'info, RiskBook>>,
    #[account(
        init_if_needed,
        payer = lp,
        space = Layer::SIZE,
        seeds = [LAYER_SEED, market.key().as_ref(), &[layer_id]],
        bump
    )]
    pub layer: AccountLoader<'info, Layer>,
    #[account(
        init,
        payer = lp,
        space = Quote::SIZE,
        seeds = [QUOTE_SEED, market.key().as_ref(), lp.key().as_ref(), &[layer_id]],
        bump
    )]
    pub quote: Box<Account<'info, Quote>>,
    #[account(
        init_if_needed,
        payer = lp,
        space = LpSeat::SIZE,
        seeds = [SEAT_SEED, market.key().as_ref(), lp.key().as_ref()],
        bump
    )]
    pub seat: Box<Account<'info, LpSeat>>,
    #[account(
        mut,
        seeds = [vault::USER_SEED, lp.key().as_ref()],
        bump = user_vault.bump,
        seeds::program = vault::ID,
        constraint = user_vault.owner == lp.key() @ RiskError::NotOwner
    )]
    pub user_vault: Box<Account<'info, vault::UserVault>>,
    pub vault_program: Program<'info, vault::program::Vault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FillNext<'info> {
    #[account(owner = market::ID)]
    pub market: Box<Account<'info, Market>>,
    #[account(mut, seeds = [BOOK_SEED, market.key().as_ref()], bump = book.bump)]
    pub book: Box<Account<'info, RiskBook>>,
    #[account(mut, seeds = [LAYER_SEED, market.key().as_ref(), &[POOL_LAYER]], bump)]
    pub layer: AccountLoader<'info, Layer>,
    #[account(mut)]
    pub quote: Box<Account<'info, Quote>>,
    #[account(mut, seeds = [SEAT_SEED, market.key().as_ref(), quote.lp.as_ref()], bump = seat.bump)]
    pub seat: Box<Account<'info, LpSeat>>,
}

#[derive(Accounts)]
pub struct CancelUnfilled<'info> {
    pub lp: Signer<'info>,
    #[account(
        mut,
        seeds = [QUOTE_SEED, quote.market.as_ref(), lp.key().as_ref(), &[quote.layer_id]],
        bump = quote.bump
    )]
    pub quote: Box<Account<'info, Quote>>,
    #[account(
        mut,
        seeds = [LAYER_SEED, quote.market.as_ref(), &[quote.layer_id]],
        bump
    )]
    pub layer: AccountLoader<'info, Layer>,
    #[account(
        mut,
        seeds = [vault::USER_SEED, lp.key().as_ref()],
        bump = user_vault.bump,
        seeds::program = vault::ID,
        constraint = user_vault.owner == lp.key() @ RiskError::NotOwner
    )]
    pub user_vault: Box<Account<'info, vault::UserVault>>,
    pub vault_program: Program<'info, vault::program::Vault>,
}

#[error_code]
pub enum RiskError {
    #[msg("signer is not the creator or a roster member")]
    NotResolver,
    #[msg("layer id is not a published layer")]
    BadLayer,
    #[msg("capacity below min D, or premium is illegal")]
    BadSize,
    #[msg("risk_lock_ts is after close_ts")]
    BadClock,
    #[msg("now >= risk_lock_ts")]
    Locked,
    #[msg("risk book does not belong to this market")]
    WrongBook,
    #[msg("layer quote list is full")]
    BookFull,
    #[msg("same LP exceeds concentration (unused)")]
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
    fn standing_quotes_cap_fits_one_create() {
        assert_eq!(MAX_QUOTES, 64);
        assert_eq!(core::mem::align_of::<super::Layer>(), 8);
        assert_eq!(core::mem::size_of::<super::Layer>() % 8, 0);
        assert!(
            super::Layer::SIZE > 4_000 && super::Layer::SIZE <= 10_240,
            "Layer::SIZE {}",
            super::Layer::SIZE
        );
        let mut raw = vec![0u8; super::Layer::SIZE];
        raw[64] = 7;
        let layer = super::Layer::from_account_data(&raw).expect("zero_copy layout");
        assert_eq!(layer.layer_id, 7);
        assert_eq!(layer.quote_count, 0);
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
        let take = take_from_head(50, &q).unwrap();
        assert_eq!(take, 50);
        assert!(take < q.capacity);
    }
}
