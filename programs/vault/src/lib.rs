//! L1 USDC vault (FR-WAL-03, FR-SET-01–06, CR-05, CR-08, IR-04).
//!
//! The USDC mint is Circle's official SPL mint (`USDC_MINT`). This program
//! never creates that mint. `initialize` only opens the vault token account.
//! Allowed outflows: unused-margin withdraw, settlement payout, LP draw,
//! $C_P$ allocation, surplus split, platform fee claim, VOID / failed-resolution refunds.
//! Fees never enter $C_P$. There is no `admin_withdraw`. Session keys are not authority.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

pub mod mint;
pub mod settle;
pub mod views;

pub use mint::{CIRCLE_USDC_DEVNET, CIRCLE_USDC_MAINNET, USDC_MINT};
pub use settle::{AdjustPool, Board, BoardTap, Claim, LossPool};
pub use views::{PHASE_FAILED, PHASE_FINALIZED, PHASE_VOIDED};

declare_id!("VaULt11111111111111111111111111111111111111");

pub const VAULT_SEED: &[u8] = b"vault";
pub const USER_SEED: &[u8] = b"user";
pub const BOARD_SEED: &[u8] = b"board";
pub const CLAIM_SEED: &[u8] = b"claim";
pub const CPOOL_SEED: &[u8] = b"cpool";
pub const CPTAP_SEED: &[u8] = b"cptap";
pub const RLOSS_SEED: &[u8] = b"rloss";
pub const CBOND_SEED: &[u8] = b"cbond";

#[program]
pub mod vault {
    use super::*;

    /// Create the vault config and its USDC token account.
    /// The mint account must already be Circle USDC (`USDC_MINT`).
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.usdc_mint = USDC_MINT;
        cfg.token_account = ctx.accounts.vault_ata.key();
        cfg.bump = ctx.bumps.config;
        Ok(())
    }

    /// Main-wallet deposit. Credits `available`. Mint must be `USDC_MINT`.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.usdc_mint.key(), USDC_MINT, VaultError::WrongMint);
        require_keys_eq!(ctx.accounts.user_ata.mint, USDC_MINT, VaultError::WrongMint);

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_ata.to_account_info(),
                    to: ctx.accounts.vault_ata.to_account_info(),
                    authority: ctx.accounts.owner.to_account_info(),
                },
            ),
            amount,
        )?;

        let user = &mut ctx.accounts.user;
        user.owner = ctx.accounts.owner.key();
        user.available = accounting::credit(user.available, amount)?;
        user.bump = ctx.bumps.user;
        Ok(())
    }

    /// Withdraw unused margin. Signer must be the user owner (main wallet).
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        require_keys_eq!(ctx.accounts.user_ata.mint, USDC_MINT, VaultError::WrongMint);

        let user = &mut ctx.accounts.user;
        user.available = accounting::debit_available(user.available, user.reserved, amount)?;

        let bump = ctx.accounts.config.bump;
        let signer_seeds: &[&[u8]] = &[VAULT_SEED, &[bump]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault_ata.to_account_info(),
                    to: ctx.accounts.user_ata.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                &[signer_seeds],
            ),
            amount,
        )?;
        Ok(())
    }

    /// Lock unused margin so it cannot be withdrawn. Used by risk quotes (FR-RSK-03).
    pub fn reserve(ctx: Context<MutUser>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        let user = &mut ctx.accounts.user;
        user.reserved = accounting::reserve(user.available, user.reserved, amount)?;
        Ok(())
    }

    /// Unlock reserved margin. Owner only. Settlement draw is a later instruction.
    pub fn release(ctx: Context<MutUser>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        let user = &mut ctx.accounts.user;
        user.reserved = accounting::release(user.reserved, amount)?;
        Ok(())
    }

    /// Lock unused margin as this market's committee report bond. One lock per market.
    pub fn lock_committee_bond(ctx: Context<LockBond>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        require!(ctx.accounts.bond.amount == 0, VaultError::BondLocked);
        let rec = &ctx.accounts.resolution;
        let who = ctx.accounts.owner.key();
        let member = rec.members.iter().take(rec.n as usize).any(|m| *m == who);
        let market_data = ctx.accounts.market.try_borrow_data()?;
        require!(market_data.len() >= 48, VaultError::BadSettle);
        let creator = Pubkey::try_from(&market_data[16..48]).map_err(|_| error!(VaultError::BadSettle))?;
        drop(market_data);
        require!(member || who == creator, VaultError::NotOwner);
        let user = &mut ctx.accounts.user;
        user.available = accounting::debit_available(user.available, user.reserved, amount)?;
        user.bond = accounting::credit(user.bond, amount)?;
        let bond = &mut ctx.accounts.bond;
        bond.market = ctx.accounts.market.key();
        bond.holder = who;
        bond.amount = amount;
        bond.slashed = false;
        bond.bump = ctx.bumps.bond;
        Ok(())
    }

    /// Platform takes the locked bond after `admin_submit_result` (`slash_due`). Holder is `CommitteeBond.holder`.
    pub fn slash_committee_bond(ctx: Context<SlashBond>) -> Result<()> {
        require!(
            ctx.accounts.authority.key() == ctx.accounts.market.platform,
            VaultError::NotOwner
        );
        require!(ctx.accounts.resolution.slash_due, VaultError::NotFinalized);
        require!(ctx.accounts.resolution.phase == PHASE_FINALIZED, VaultError::NotFinalized);
        let bond = &mut ctx.accounts.bond;
        require!(!bond.slashed && bond.amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.holder.owner, bond.holder, VaultError::NotOwner);
        let amount = bond.amount;
        ctx.accounts.holder.bond = accounting::debit_bond(ctx.accounts.holder.bond, amount)?;
        ctx.accounts.platform.available = accounting::credit(ctx.accounts.platform.available, amount)?;
        bond.slashed = true;
        Ok(())
    }

    /// Return this market's bond after VOID / committee finalize (not slashed).
    pub fn release_committee_bond(ctx: Context<ReleaseBond>) -> Result<()> {
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        let slash = ctx.accounts.resolution.slash_due;
        let phase = ctx.accounts.resolution.phase;
        let ok = !slash
            && (phase == PHASE_VOIDED || phase == PHASE_FAILED || phase == PHASE_FINALIZED);
        require!(ok, VaultError::NotFinalized);
        let bond = &mut ctx.accounts.bond;
        require!(!bond.slashed && bond.amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(bond.holder, ctx.accounts.owner.key(), VaultError::NotOwner);
        let amount = bond.amount;
        ctx.accounts.user.bond = accounting::debit_bond(ctx.accounts.user.bond, amount)?;
        ctx.accounts.user.available = accounting::credit(ctx.accounts.user.available, amount)?;
        bond.amount = 0;
        Ok(())
    }

    /// Open this board's Vault account. Amount MUST be 0 — there is no seed reserve.
    pub fn fund_cm(ctx: Context<FundCm>, amount: u64) -> Result<()> {
        let board = &mut ctx.accounts.board;
        if board.market == Pubkey::default() {
            board.market = ctx.accounts.market_key.key();
            board.bump = ctx.bumps.board;
        }
        settle::fund_cm_inner(
            board,
            ctx.accounts.market_key.key(),
            ctx.accounts.market.c_m,
            &mut ctx.accounts.user,
            amount,
        )?;
        Ok(())
    }

    /// Debit trader unused margin: $C_S$ into the pot, fee aside (not in $C_{\max}$).
    /// The market PDA must sign (CPI). The user owner is not a signer — Session
    /// may authorize the fill; this instruction still never treats a Session as vault authority.
    pub fn credit_trade(ctx: Context<CreditTrade>, cost: u64, fee: u64) -> Result<()> {
        require!(ctx.accounts.board.market == ctx.accounts.market.key(), VaultError::WrongBoard);
        settle::credit_trade_inner(&mut ctx.accounts.board, &mut ctx.accounts.user, cost, fee)
    }

    /// Credit unused margin from an in-board sell. Market PDA signs (CPI).
    pub fn refund_trade(ctx: Context<CreditTrade>, cost: u64) -> Result<()> {
        require!(ctx.accounts.board.market == ctx.accounts.market.key(), VaultError::WrongBoard);
        settle::refund_trade_inner(&mut ctx.accounts.board, &mut ctx.accounts.user, cost)
    }

    /// After `finalize`, lock $L=E(x^*)$, $C_{\max}$, one $\rho$.
    pub fn begin_settle(ctx: Context<BeginSettle>) -> Result<()> {
        let book = ctx.accounts.risk_book.as_ref().map(|a| a.as_ref().as_ref());
        let pool = ctx.accounts.pool.as_mut().map(|a| &mut **a);
        let tap = ctx.accounts.tap.as_mut().map(|a| &mut **a);
        let grid_data = ctx.accounts.grid.try_borrow_data()?;
        settle::compute_begin(
            &mut ctx.accounts.board,
            &ctx.accounts.market,
            &grid_data,
            &ctx.accounts.record,
            book,
            pool,
            tap,
        )?;
        Ok(())
    }

    pub fn init_pool(ctx: Context<InitPool>) -> Result<()> {
        ctx.accounts.pool.bump = ctx.bumps.pool;
        ctx.accounts.loss_pool.bump = ctx.bumps.loss_pool;
        Ok(())
    }

    pub fn fund_pool(ctx: Context<FundPool>, amount: u64) -> Result<()> {
        settle::fund_pool_inner(&mut ctx.accounts.pool, &mut ctx.accounts.user, amount)
    }

    pub fn set_tap(ctx: Context<SetTap>, cap: u64) -> Result<()> {
        let m = &ctx.accounts.market;
        require!(ctx.accounts.authority.key() == m.platform, VaultError::NotOwner);
        let tap = &mut ctx.accounts.tap;
        tap.market = m.key();
        tap.cap = cap;
        tap.bump = ctx.bumps.tap;
        Ok(())
    }

    pub fn claim_fees(ctx: Context<ClaimFees>) -> Result<()> {
        let m = &ctx.accounts.market;
        require!(ctx.accounts.authority.key() == m.platform, VaultError::NotOwner);
        settle::claim_fees_inner(&mut ctx.accounts.board, &mut ctx.accounts.user)?;
        Ok(())
    }

    pub fn begin_refund(ctx: Context<BeginRefund>) -> Result<()> {
        settle::compute_refund(&mut ctx.accounts.board, &ctx.accounts.record)?;
        Ok(())
    }

    pub fn payout(ctx: Context<Payout>, set_mask: Vec<u8>) -> Result<()> {
        let board = &ctx.accounts.board;
        require!(
            ctx.accounts.position.set_hash == settle::set_hash(&set_mask),
            VaultError::BadMask
        );
        let grid_data = ctx.accounts.grid.try_borrow_data()?;
        let (grid_market, _, _) = settle::parse_grid_meta(&grid_data)?;
        require!(grid_market == ctx.accounts.board.market, VaultError::WrongBoard);
        let face = settle::mask_face(
            ctx.accounts.position.q,
            &set_mask,
            ctx.accounts.market.n as usize,
            board.cell as usize,
        )?;
        let paid = settle::pay_winner_clean(
            &mut ctx.accounts.board,
            &ctx.accounts.position,
            face,
            &mut ctx.accounts.user,
        )?;
        ctx.accounts.claim.position = ctx.accounts.position.key();
        ctx.accounts.claim.paid = paid;
        ctx.accounts.claim.bump = ctx.bumps.claim;
        Ok(())
    }

    pub fn payout_skellam(ctx: Context<Payout>, kind: u8, a: i16, b: i16) -> Result<()> {
        let board = &ctx.accounts.board;
        require!(
            ctx.accounts.position.set_hash == settle::skellam_ticket(kind, a, b),
            VaultError::BadMask
        );
        let face = settle::skellam_face(
            ctx.accounts.position.q,
            kind,
            a,
            b,
            ctx.accounts.record.k_max,
            board.cell as usize,
        )?;
        let paid = settle::pay_winner_clean(
            &mut ctx.accounts.board,
            &ctx.accounts.position,
            face,
            &mut ctx.accounts.user,
        )?;
        ctx.accounts.claim.position = ctx.accounts.position.key();
        ctx.accounts.claim.paid = paid;
        ctx.accounts.claim.bump = ctx.bumps.claim;
        Ok(())
    }

    pub fn draw_lp(ctx: Context<DrawLp>) -> Result<()> {
        let layer_ai = ctx.accounts.layer.to_account_info();
        let data = layer_ai.try_borrow_data()?;
        let layer = views::Layer::from_account(&data)?;
        settle::draw_quote(
            &mut ctx.accounts.board,
            layer,
            &ctx.accounts.quote,
            &mut ctx.accounts.user,
        )?;
        Ok(())
    }

    pub fn pay_premium(ctx: Context<QuotePay>) -> Result<()> {
        settle::pay_premium_inner(
            &mut ctx.accounts.board,
            ctx.accounts.quote.key(),
            &ctx.accounts.quote,
            &mut ctx.accounts.user,
        )?;
        Ok(())
    }

    pub fn pay_surplus_lp(ctx: Context<QuotePay>, weight_sum: u64) -> Result<()> {
        let layer_ai = ctx.accounts.layer.to_account_info();
        let data = layer_ai.try_borrow_data()?;
        let layer = views::Layer::from_account(&data)?;
        settle::pay_surplus_lp_inner(
            &mut ctx.accounts.board,
            ctx.accounts.quote.key(),
            &ctx.accounts.quote,
            Some(layer),
            &mut ctx.accounts.user,
            weight_sum,
        )?;
        Ok(())
    }

    /// Permissionless credit of \(S_P\) into `market.platform`'s UserVault. No platform signer.
    /// Withdraw from that vault still requires the platform owner (`withdraw`).
    pub fn pay_surplus_platform(ctx: Context<PlatformPay>) -> Result<()> {
        settle::pay_surplus_platform_inner(&mut ctx.accounts.board, &mut ctx.accounts.user)?;
        Ok(())
    }

    pub fn pay_surplus_cover(ctx: Context<CoverPoolPay>) -> Result<()> {
        settle::pay_surplus_cover_inner(&mut ctx.accounts.board, &mut ctx.accounts.loss_pool)?;
        Ok(())
    }

    pub fn cover_lp_loss(ctx: Context<CoverLp>) -> Result<()> {
        settle::cover_lp_loss_inner(&mut ctx.accounts.loss_pool, &mut ctx.accounts.user)?;
        Ok(())
    }

    pub fn refund_position(ctx: Context<RefundPosition>) -> Result<()> {
        let paid = settle::refund_position_inner(
            &mut ctx.accounts.board,
            &ctx.accounts.position,
            &mut ctx.accounts.user,
        )?;
        ctx.accounts.claim.position = ctx.accounts.position.key();
        ctx.accounts.claim.paid = paid;
        ctx.accounts.claim.bump = ctx.bumps.claim;
        Ok(())
    }

    pub fn release_lp(ctx: Context<QuotePay>) -> Result<()> {
        settle::release_quote(&ctx.accounts.board, &ctx.accounts.quote, &mut ctx.accounts.user)?;
        Ok(())
    }
}

pub mod accounting {
    use super::*;

    pub fn credit(available: u64, amount: u64) -> Result<u64> {
        available
            .checked_add(amount)
            .ok_or(VaultError::Overflow.into())
    }

    /// Unused margin only. Reserved (open exposure / locks) cannot be withdrawn.
    pub fn debit_available(available: u64, reserved: u64, amount: u64) -> Result<u64> {
        let free = available
            .checked_sub(reserved)
            .ok_or(error!(VaultError::InsufficientAvailable))?;
        require!(free >= amount, VaultError::InsufficientAvailable);
        available
            .checked_sub(amount)
            .ok_or(error!(VaultError::InsufficientAvailable))
    }

    pub fn reserve(available: u64, reserved: u64, amount: u64) -> Result<u64> {
        let free = available
            .checked_sub(reserved)
            .ok_or(error!(VaultError::InsufficientAvailable))?;
        require!(free >= amount, VaultError::InsufficientAvailable);
        reserved
            .checked_add(amount)
            .ok_or(error!(VaultError::Overflow))
    }

    pub fn release(reserved: u64, amount: u64) -> Result<u64> {
        reserved
            .checked_sub(amount)
            .ok_or(error!(VaultError::InsufficientAvailable))
    }

    pub fn debit_bond(bond: u64, amount: u64) -> Result<u64> {
        require!(bond >= amount, VaultError::InsufficientAvailable);
        bond.checked_sub(amount)
            .ok_or(error!(VaultError::InsufficientAvailable))
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// Circle official USDC. Passed in; never created by this program.
    #[account(address = USDC_MINT @ VaultError::WrongMint)]
    pub usdc_mint: Account<'info, Mint>,
    #[account(
        init,
        payer = payer,
        space = 8 + VaultConfig::SIZE,
        seeds = [VAULT_SEED],
        bump
    )]
    pub config: Account<'info, VaultConfig>,
    #[account(
        init,
        payer = payer,
        associated_token::mint = usdc_mint,
        associated_token::authority = config
    )]
    pub vault_ata: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(seeds = [VAULT_SEED], bump = config.bump)]
    pub config: Account<'info, VaultConfig>,
    #[account(address = USDC_MINT @ VaultError::WrongMint)]
    pub usdc_mint: Account<'info, Mint>,
    #[account(
        mut,
        constraint = vault_ata.key() == config.token_account @ VaultError::WrongVaultAta,
        constraint = vault_ata.mint == USDC_MINT @ VaultError::WrongMint
    )]
    pub vault_ata: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = user_ata.owner == owner.key() @ VaultError::NotOwner,
        constraint = user_ata.mint == USDC_MINT @ VaultError::WrongMint
    )]
    pub user_ata: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + UserVault::SIZE,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump
    )]
    pub user: Account<'info, UserVault>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    pub owner: Signer<'info>,
    #[account(seeds = [VAULT_SEED], bump = config.bump)]
    pub config: Account<'info, VaultConfig>,
    #[account(
        mut,
        constraint = vault_ata.key() == config.token_account @ VaultError::WrongVaultAta,
        constraint = vault_ata.mint == USDC_MINT @ VaultError::WrongMint
    )]
    pub vault_ata: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = user_ata.owner == owner.key() @ VaultError::NotOwner,
        constraint = user_ata.mint == USDC_MINT @ VaultError::WrongMint
    )]
    pub user_ata: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        has_one = owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct MutUser<'info> {
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        has_one = owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct LockBond<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: owned by market; creator pubkey is at byte offset 16 (disc + header).
    #[account(owner = views::MARKET_ID)]
    pub market: UncheckedAccount<'info>,
    #[account(owner = views::RESOLUTION_ID, constraint = resolution.market == market.key() @ VaultError::WrongBoard)]
    pub resolution: Box<Account<'info, views::Resolution>>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        has_one = owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + CommitteeBond::SIZE,
        seeds = [CBOND_SEED, market.key().as_ref()],
        bump
    )]
    pub bond: Account<'info, CommitteeBond>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SlashBond<'info> {
    pub authority: Signer<'info>,
    #[account(owner = views::MARKET_ID)]
    pub market: Account<'info, views::Market>,
    #[account(owner = views::RESOLUTION_ID, constraint = resolution.market == market.key() @ VaultError::WrongBoard)]
    pub resolution: Box<Account<'info, views::Resolution>>,
    #[account(
        mut,
        seeds = [CBOND_SEED, market.key().as_ref()],
        bump = bond.bump
    )]
    pub bond: Account<'info, CommitteeBond>,
    #[account(
        mut,
        seeds = [USER_SEED, holder.owner.as_ref()],
        bump = holder.bump,
        constraint = holder.owner == bond.holder @ VaultError::NotOwner
    )]
    pub holder: Account<'info, UserVault>,
    #[account(
        mut,
        seeds = [USER_SEED, platform.owner.as_ref()],
        bump = platform.bump,
        constraint = platform.owner == market.platform @ VaultError::NotOwner
    )]
    pub platform: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct ReleaseBond<'info> {
    pub owner: Signer<'info>,
    #[account(owner = views::RESOLUTION_ID)]
    pub resolution: Box<Account<'info, views::Resolution>>,
    #[account(
        mut,
        seeds = [CBOND_SEED, resolution.market.as_ref()],
        bump = bond.bump
    )]
    pub bond: Account<'info, CommitteeBond>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        has_one = owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct FundCm<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    pub market: Account<'info, views::Market>,
    /// CHECK: board PDA seed; must be the listed market.
    #[account(constraint = market_key.key() == market.key() @ VaultError::WrongBoard)]
    pub market_key: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        payer = owner,
        space = Board::SIZE,
        seeds = [BOARD_SEED, market_key.key().as_ref()],
        bump
    )]
    pub board: Account<'info, Board>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        has_one = owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreditTrade<'info> {
    /// Market PDA. Only the market program can sign this (CPI seeds).
    pub market: Signer<'info>,
    /// CHECK: main wallet that owns the user vault. Not a signer.
    pub owner: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [BOARD_SEED, market.key().as_ref()],
        bump = board.bump,
        constraint = board.market == market.key() @ VaultError::WrongBoard
    )]
    pub board: Account<'info, Board>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        constraint = user.owner == owner.key() @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct BeginSettle<'info> {
    #[account(mut, seeds = [BOARD_SEED, market.key().as_ref()], bump = board.bump)]
    pub board: Box<Account<'info, Board>>,
    pub market: Box<Account<'info, views::Market>>,
    /// CHECK: market-program grid. Header + one exposure word; n=1024 is 65KB.
    #[account(owner = views::MARKET_ID)]
    pub grid: UncheckedAccount<'info>,
    #[account(constraint = record.market == market.key() @ VaultError::WrongBoard)]
    pub record: Box<Account<'info, views::Resolution>>,
    pub risk_book: Option<Box<Account<'info, views::RiskBook>>>,
    #[account(mut, seeds = [CPOOL_SEED], bump = pool.bump)]
    pub pool: Option<Account<'info, AdjustPool>>,
    #[account(mut, seeds = [CPTAP_SEED, market.key().as_ref()], bump = tap.bump)]
    pub tap: Option<Account<'info, BoardTap>>,
}

#[derive(Accounts)]
pub struct InitPool<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(init_if_needed, payer = payer, space = AdjustPool::SIZE, seeds = [CPOOL_SEED], bump)]
    pub pool: Account<'info, AdjustPool>,
    #[account(init_if_needed, payer = payer, space = LossPool::SIZE, seeds = [RLOSS_SEED], bump)]
    pub loss_pool: Account<'info, LossPool>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FundPool<'info> {
    pub owner: Signer<'info>,
    #[account(mut, seeds = [CPOOL_SEED], bump = pool.bump)]
    pub pool: Account<'info, AdjustPool>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        has_one = owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct SetTap<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    pub market: Account<'info, views::Market>,
    #[account(
        init_if_needed,
        payer = authority,
        space = BoardTap::SIZE,
        seeds = [CPTAP_SEED, market.key().as_ref()],
        bump
    )]
    pub tap: Account<'info, BoardTap>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimFees<'info> {
    pub authority: Signer<'info>,
    pub market: Account<'info, views::Market>,
    #[account(mut, seeds = [BOARD_SEED, market.key().as_ref()], bump = board.bump)]
    pub board: Account<'info, Board>,
    #[account(
        mut,
        seeds = [USER_SEED, market.platform.as_ref()],
        bump = user.bump,
        constraint = user.owner == market.platform @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct BeginRefund<'info> {
    #[account(mut, seeds = [BOARD_SEED, market.key().as_ref()], bump = board.bump)]
    pub board: Box<Account<'info, Board>>,
    pub market: Box<Account<'info, views::Market>>,
    #[account(constraint = record.market == market.key() @ VaultError::WrongBoard)]
    pub record: Box<Account<'info, views::Resolution>>,
}

#[derive(Accounts)]
pub struct Payout<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut, seeds = [BOARD_SEED, board.market.as_ref()], bump = board.bump)]
    pub board: Box<Account<'info, Board>>,
    pub market: Box<Account<'info, views::Market>>,
    /// CHECK: market-program grid shard that contains `board.cell`.
    #[account(owner = views::MARKET_ID)]
    pub grid: UncheckedAccount<'info>,
    pub record: Box<Account<'info, views::Resolution>>,
    #[account(constraint = position.market == board.market @ VaultError::WrongBoard)]
    pub position: Box<Account<'info, views::Position>>,
    #[account(
        init,
        payer = payer,
        space = Claim::SIZE,
        seeds = [CLAIM_SEED, position.key().as_ref()],
        bump
    )]
    pub claim: Account<'info, Claim>,
    #[account(
        mut,
        seeds = [USER_SEED, position.owner.as_ref()],
        bump = user.bump,
        constraint = user.owner == position.owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DrawLp<'info> {
    #[account(mut, seeds = [BOARD_SEED, board.market.as_ref()], bump = board.bump)]
    pub board: Account<'info, Board>,
    /// CHECK: risk `Layer` zero_copy account; owner + layout checked in the handler.
    #[account(owner = views::RISK_ID)]
    pub layer: UncheckedAccount<'info>,
    pub quote: Account<'info, views::Quote>,
    #[account(
        mut,
        seeds = [USER_SEED, quote.lp.as_ref()],
        bump = user.bump,
        constraint = user.owner == quote.lp @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct QuotePay<'info> {
    #[account(mut, seeds = [BOARD_SEED, board.market.as_ref()], bump = board.bump)]
    pub board: Account<'info, Board>,
    /// CHECK: risk `Layer` zero_copy account; owner + layout checked in the handler.
    #[account(owner = views::RISK_ID)]
    pub layer: UncheckedAccount<'info>,
    pub quote: Account<'info, views::Quote>,
    #[account(
        mut,
        seeds = [USER_SEED, quote.lp.as_ref()],
        bump = user.bump,
        constraint = user.owner == quote.lp @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct PlatformPay<'info> {
    #[account(mut, seeds = [BOARD_SEED, board.market.as_ref()], bump = board.bump)]
    pub board: Account<'info, Board>,
    #[account(constraint = market.key() == board.market @ VaultError::WrongBoard)]
    pub market: Box<Account<'info, views::Market>>,
    /// Destination is always `Market.platform`. No signer on this ix.
    #[account(
        mut,
        seeds = [USER_SEED, market.platform.as_ref()],
        bump = user.bump,
        constraint = user.owner == market.platform @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct CoverPoolPay<'info> {
    #[account(mut, seeds = [BOARD_SEED, board.market.as_ref()], bump = board.bump)]
    pub board: Account<'info, Board>,
    #[account(mut, seeds = [RLOSS_SEED], bump = loss_pool.bump)]
    pub loss_pool: Account<'info, LossPool>,
}

#[derive(Accounts)]
pub struct CoverLp<'info> {
    /// Protocol platform. Timing is operational; the chain only books Π and cover paid.
    pub authority: Signer<'info>,
    #[account(constraint = authority.key() == market.platform @ VaultError::NotOwner)]
    pub market: Box<Account<'info, views::Market>>,
    #[account(mut, seeds = [RLOSS_SEED], bump = loss_pool.bump)]
    pub loss_pool: Account<'info, LossPool>,
    #[account(
        mut,
        seeds = [USER_SEED, user.owner.as_ref()],
        bump = user.bump
    )]
    pub user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct RefundPosition<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut, seeds = [BOARD_SEED, board.market.as_ref()], bump = board.bump)]
    pub board: Account<'info, Board>,
    #[account(constraint = position.market == board.market @ VaultError::WrongBoard)]
    pub position: Account<'info, views::Position>,
    #[account(
        init,
        payer = payer,
        space = Claim::SIZE,
        seeds = [CLAIM_SEED, position.key().as_ref()],
        bump
    )]
    pub claim: Account<'info, Claim>,
    #[account(
        mut,
        seeds = [USER_SEED, position.owner.as_ref()],
        bump = user.bump,
        constraint = user.owner == position.owner @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
    pub system_program: Program<'info, System>,
}

#[account]
pub struct VaultConfig {
    pub usdc_mint: Pubkey,
    pub token_account: Pubkey,
    pub bump: u8,
}

impl VaultConfig {
    pub const SIZE: usize = 32 + 32 + 1;
}

#[account]
pub struct UserVault {
    pub owner: Pubkey,
    pub available: u64,
    pub reserved: u64,
    pub bump: u8,
    /// Lifetime operating risk P&L: −H, +premium, +S_R. Cover does not rewrite this.
    pub risk_pnl: i64,
    /// Committee report bond. Not trading margin. Slashed to the platform after a missed report.
    pub bond: u64,
    /// Cumulative cover reimbursed from LossPool. Never exceeds (−Π)⁺.
    pub cover_paid: u64,
}

impl UserVault {
    pub const SIZE: usize = 32 + 8 + 8 + 1 + 8 + 8 + 8;
}

#[account]
pub struct CommitteeBond {
    pub market: Pubkey,
    pub holder: Pubkey,
    pub amount: u64,
    pub slashed: bool,
    pub bump: u8,
}

impl CommitteeBond {
    pub const SIZE: usize = 32 + 32 + 8 + 1 + 1;
}

#[error_code]
pub enum VaultError {
    #[msg("amount must be > 0")]
    ZeroAmount,
    #[msg("mint is not Circle official USDC")]
    WrongMint,
    #[msg("signer is not the user vault owner")]
    NotOwner,
    #[msg("insufficient unused margin")]
    InsufficientAvailable,
    #[msg("arithmetic overflow")]
    Overflow,
    #[msg("vault token account mismatch")]
    WrongVaultAta,
    #[msg("settlement inputs do not match the board")]
    BadSettle,
    #[msg("board is already settled or refunding")]
    AlreadySettled,
    #[msg("account does not belong to this board")]
    WrongBoard,
    #[msg("resolution is not in the required terminal phase")]
    NotFinalized,
    #[msg("committee bond already locked for this market")]
    BondLocked,
    #[msg("refunds_due does not match this path")]
    RefundsDue,
    #[msg("x* does not map onto the grid")]
    BadOutcome,
    #[msg("position already claimed")]
    AlreadyClaimed,
    #[msg("set mask or skellam ticket does not match the position")]
    BadMask,
    #[msg("surplus is zero because rho < 1")]
    NoSurplus,
}

#[cfg(test)]
mod tests {
    use super::accounting::*;
    use super::{CIRCLE_USDC_DEVNET, CIRCLE_USDC_MAINNET, USDC_MINT};

    #[test]
    fn usdc_mint_is_circle_mainnet_by_default() {
        assert_eq!(USDC_MINT, CIRCLE_USDC_MAINNET);
        assert_eq!(
            USDC_MINT.to_string(),
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        );
        assert_eq!(
            CIRCLE_USDC_DEVNET.to_string(),
            "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"
        );
    }

    #[test]
    fn credit_then_withdraw_unused() {
        let a = credit(0, 100).unwrap();
        let a = debit_available(a, 0, 40).unwrap();
        assert_eq!(a, 60);
    }

    #[test]
    fn cannot_withdraw_reserved() {
        assert!(debit_available(100, 80, 30).is_err());
        assert_eq!(debit_available(100, 80, 20).unwrap(), 80);
    }

    #[test]
    fn no_overdraft() {
        assert!(debit_available(10, 0, 11).is_err());
    }

    #[test]
    fn reserve_then_cannot_withdraw_that_slice() {
        let r = reserve(100, 0, 40).unwrap();
        assert_eq!(r, 40);
        assert!(debit_available(100, r, 70).is_err());
        assert_eq!(debit_available(100, r, 60).unwrap(), 40);
        assert_eq!(release(r, 40).unwrap(), 0);
    }
}
