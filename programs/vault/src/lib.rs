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
pub use views::{PHASE_FAILED, PHASE_FINALIZED, PHASE_OPEN, PHASE_VOIDED};

declare_id!("VaULt11111111111111111111111111111111111111");

pub const VAULT_SEED: &[u8] = b"vault";
pub const USER_SEED: &[u8] = b"user";
pub const BOARD_SEED: &[u8] = b"board";
pub const CLAIM_SEED: &[u8] = b"claim";
pub const CPOOL_SEED: &[u8] = b"cpool";
pub const CPTAP_SEED: &[u8] = b"cptap";
pub const RLOSS_SEED: &[u8] = b"rloss";
pub const CBOND_SEED: &[u8] = b"cbond";
pub const CHBOND_SEED: &[u8] = b"chbond";

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
    /// Cannot withdraw `reserved`, `trade_pending`, or (when session not LIVE) `er_owed_usdc`.
    /// LIVE sessions require `trade_pending >= remaining + er_owed` (pending already locks that slice).
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        require_keys_eq!(ctx.accounts.user_ata.mint, USDC_MINT, VaultError::WrongMint);

        let mut locked = accounting::locked(
            ctx.accounts.user.reserved,
            ctx.accounts.user.trade_pending,
        )?;
        let session_ai = ctx.accounts.session.to_account_info();
        let (expected, _) =
            Pubkey::find_program_address(&[b"session", ctx.accounts.owner.key().as_ref()], &views::MARKET_ID);
        require_keys_eq!(*session_ai.key, expected, VaultError::NotOwner);
        if session_ai.owner == &views::MARKET_ID && !session_ai.data_is_empty() {
            let data = session_ai.try_borrow_data()?;
            // Greenfield Session: disc + body through unsynced_seats.
            const BODY: usize = 8 + 32 + 32 + 8 + 8 + 1 + 1 + 1 + 32 + 8 + 2;
            const REMAINING_OFF: usize = 8 + 32 + 32 + 8;
            const STATUS_OFF: usize = REMAINING_OFF + 8 + 1;
            const ER_OWED_OFF: usize = 8 + 32 + 32 + 8 + 8 + 1 + 1 + 1 + 32;
            const UNSYNCED_OFF: usize = ER_OWED_OFF + 8;
            require!(data.len() >= BODY, VaultError::BadSettle);
            let owner_pk = Pubkey::try_from(&data[8..40]).map_err(|_| error!(VaultError::BadSettle))?;
            require_keys_eq!(owner_pk, ctx.accounts.owner.key(), VaultError::NotOwner);
            let remaining = u64::from_le_bytes(data[REMAINING_OFF..REMAINING_OFF + 8].try_into().unwrap());
            let status = data[STATUS_OFF];
            let er_owed = u64::from_le_bytes(data[ER_OWED_OFF..ER_OWED_OFF + 8].try_into().unwrap());
            let unsynced_seats = u16::from_le_bytes(data[UNSYNCED_OFF..UNSYNCED_OFF + 2].try_into().unwrap());
            const SESSION_LIVE: u8 = 1;
            if status == SESSION_LIVE {
                let budget = remaining
                    .checked_add(er_owed)
                    .ok_or(error!(VaultError::Overflow))?;
                require!(
                    ctx.accounts.user.trade_pending >= budget,
                    VaultError::InsufficientTradePending
                );
            } else {
                require!(unsynced_seats == 0, VaultError::InsufficientTradePending);
                if er_owed > 0 {
                    locked = locked
                        .checked_add(er_owed)
                        .ok_or(error!(VaultError::Overflow))?;
                }
            }
        }

        let user = &mut ctx.accounts.user;
        user.available = accounting::debit_available(user.available, locked, amount)?;

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
        let locked = accounting::locked(user.reserved, user.trade_pending)?;
        user.reserved = accounting::lock_add(user.available, locked, user.reserved, amount)?;
        Ok(())
    }

    /// Unlock reserved margin. **Risk `vault_auth` PDA must sign** (CPI only).
    /// Users cannot call this directly — closes the reserve→release→withdraw bypass
    /// that left `RiskBook.c_r` counting unbacked capital (FR-RSK-03).
    pub fn release(ctx: Context<ReleaseReserve>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        let (expected, _) = Pubkey::find_program_address(&[b"vault_auth"], &views::RISK_ID);
        require_keys_eq!(ctx.accounts.authority.key(), expected, VaultError::NotOwner);
        require_keys_eq!(ctx.accounts.user.owner, ctx.accounts.owner.key(), VaultError::NotOwner);
        let user = &mut ctx.accounts.user;
        user.reserved = accounting::release(user.reserved, amount)?;
        Ok(())
    }

    /// Lock unused margin as ER / Session trade budget (`trade_pending`).
    /// **Market Protocol PDA must sign** (CPI only) — same gate as `unlock_trade`.
    pub fn lock_trade(ctx: Context<LockTrade>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        let (expected, _) = Pubkey::find_program_address(&[b"protocol"], &views::MARKET_ID);
        require_keys_eq!(ctx.accounts.authority.key(), expected, VaultError::NotOwner);
        require_keys_eq!(ctx.accounts.user.owner, ctx.accounts.owner.key(), VaultError::NotOwner);
        let user = &mut ctx.accounts.user;
        let locked = accounting::locked(user.reserved, user.trade_pending)?;
        user.trade_pending = accounting::lock_add(user.available, locked, user.trade_pending, amount)?;
        Ok(())
    }

    /// Unlock unused trade budget. **Market Protocol PDA must sign** (CPI only).
    /// Users cannot call this directly — closes the unlock→withdraw ER double-spend bypass.
    pub fn unlock_trade(ctx: Context<UnlockTrade>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        let (expected, _) = Pubkey::find_program_address(&[b"protocol"], &views::MARKET_ID);
        require_keys_eq!(ctx.accounts.authority.key(), expected, VaultError::NotOwner);
        require_keys_eq!(ctx.accounts.user.owner, ctx.accounts.owner.key(), VaultError::NotOwner);
        let user = &mut ctx.accounts.user;
        user.trade_pending = accounting::release(user.trade_pending, amount)?;
        Ok(())
    }

    /// Lock unused margin as this market's committee report bond. One lock per market.
    /// After ExtendOnce, only the current holder may unlock-and-relock; others must wait for
    /// `release_committee_bond` (reset path) then lock fresh — no third-party sniping.
    pub fn lock_committee_bond(ctx: Context<LockBond>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        let rec = &ctx.accounts.resolution;
        let who = ctx.accounts.owner.key();
        let member = rec.members.iter().take(rec.n as usize).any(|m| *m == who);
        let market_data = ctx.accounts.market.try_borrow_data()?;
        // Borsh layout: creator @ 16; `committee_bond` is the trailing u64 @ 540 (serialized len 548).
        // Account alloc may be Market::SIZE (600) with zero padding — do not read the last bytes.
        const COMMITTEE_BOND_OFF: usize = 540;
        require!(market_data.len() >= COMMITTEE_BOND_OFF + 8, VaultError::BadSettle);
        let creator = Pubkey::try_from(&market_data[16..48]).map_err(|_| error!(VaultError::BadSettle))?;
        let min_bond = u64::from_le_bytes(
            market_data[COMMITTEE_BOND_OFF..COMMITTEE_BOND_OFF + 8]
                .try_into()
                .unwrap(),
        );
        drop(market_data);
        let reported = rec.authorized_reporter != Pubkey::default() && who == rec.authorized_reporter;
        require!(member || who == creator || reported, VaultError::NotOwner);
        let reset = rec.phase == PHASE_OPEN
            && rec.extensions > 0
            && rec.proposer == Pubkey::default();
        // After ExtendOnce only the current holder may unlock-and-relock — no third-party sniping.
        if ctx.accounts.bond.amount > 0 {
            require!(
                reset && !ctx.accounts.bond.slashed && ctx.accounts.bond.holder == who,
                VaultError::BondLocked
            );
            let stale = ctx.accounts.bond.amount;
            let user = &mut ctx.accounts.user;
            user.bond = accounting::debit_bond(user.bond, stale)?;
            user.available = accounting::credit(user.available, stale)?;
            ctx.accounts.bond.amount = 0;
        }
        require!(ctx.accounts.bond.amount == 0, VaultError::BondLocked);
        require!(amount >= min_bond.max(1), VaultError::ZeroAmount);
        let user = &mut ctx.accounts.user;
        user.available = accounting::debit_available(
            user.available,
            accounting::locked(user.reserved, user.trade_pending)?,
            amount,
        )?;
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
    /// Not allowed during ExtendOnce Open — bond must stay locked so admin slash still has teeth.
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

    /// Lock unused margin as this market's challenger bond (`["chbond", market]`).
    /// After ExtendOnce, only the current holder may unlock-and-relock; others use
    /// `release_challenge_bond` then a fresh lock.
    pub fn lock_challenge_bond(ctx: Context<LockChallengeBond>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        let rec = &ctx.accounts.resolution;
        let who = ctx.accounts.owner.key();
        let member = rec.members.iter().take(rec.n as usize).any(|m| *m == who);
        require!(member, VaultError::NotOwner);
        let reset = rec.phase == PHASE_OPEN
            && rec.extensions > 0
            && rec.challenger == Pubkey::default();
        // After ExtendOnce only the current challenger-holder may unlock-and-relock.
        if ctx.accounts.bond.amount > 0 {
            require!(
                reset && !ctx.accounts.bond.slashed && ctx.accounts.bond.holder == who,
                VaultError::BondLocked
            );
            let stale = ctx.accounts.bond.amount;
            let user = &mut ctx.accounts.user;
            user.bond = accounting::debit_bond(user.bond, stale)?;
            user.available = accounting::credit(user.available, stale)?;
            ctx.accounts.bond.amount = 0;
        }
        require!(ctx.accounts.bond.amount == 0, VaultError::BondLocked);
        let market_data = ctx.accounts.market.try_borrow_data()?;
        const COMMITTEE_BOND_OFF: usize = 540;
        require!(market_data.len() >= COMMITTEE_BOND_OFF + 8, VaultError::BadSettle);
        let min_bond = u64::from_le_bytes(
            market_data[COMMITTEE_BOND_OFF..COMMITTEE_BOND_OFF + 8]
                .try_into()
                .unwrap(),
        );
        drop(market_data);
        require!(amount >= min_bond.max(1), VaultError::ZeroAmount);
        let user = &mut ctx.accounts.user;
        user.available = accounting::debit_available(
            user.available,
            accounting::locked(user.reserved, user.trade_pending)?,
            amount,
        )?;
        user.bond = accounting::credit(user.bond, amount)?;
        let bond = &mut ctx.accounts.bond;
        bond.market = ctx.accounts.market.key();
        bond.holder = who;
        bond.amount = amount;
        bond.slashed = false;
        bond.bump = ctx.bumps.bond;
        Ok(())
    }

    /// Platform takes challenger bond when the proposal stands after a vote.
    pub fn slash_challenge_bond(ctx: Context<SlashChallengeBond>) -> Result<()> {
        require!(
            ctx.accounts.authority.key() == ctx.accounts.market.platform,
            VaultError::NotOwner
        );
        let rec = &ctx.accounts.resolution;
        require!(rec.phase == PHASE_FINALIZED, VaultError::NotFinalized);
        require!(rec.challenger != Pubkey::default(), VaultError::NotFinalized);
        // Proposal won (final == proposed) ⇒ challenger loses bond.
        require!(
            outcome_eq(&rec.final_outcome, &rec.proposed),
            VaultError::NotFinalized
        );
        let bond = &mut ctx.accounts.bond;
        require!(!bond.slashed && bond.amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(bond.holder, rec.challenger, VaultError::NotOwner);
        require_keys_eq!(ctx.accounts.holder.owner, bond.holder, VaultError::NotOwner);
        let amount = bond.amount;
        ctx.accounts.holder.bond = accounting::debit_bond(ctx.accounts.holder.bond, amount)?;
        ctx.accounts.platform.available = accounting::credit(ctx.accounts.platform.available, amount)?;
        bond.slashed = true;
        Ok(())
    }

    /// Return challenger bond after challenge wins, VOID/Failed, or report window reset.
    pub fn release_challenge_bond(ctx: Context<ReleaseChallengeBond>) -> Result<()> {
        require_keys_eq!(ctx.accounts.owner.key(), ctx.accounts.user.owner, VaultError::NotOwner);
        let rec = &ctx.accounts.resolution;
        let phase = rec.phase;
        let challenge_won = phase == PHASE_FINALIZED && outcome_eq(&rec.final_outcome, &rec.challenged);
        let reset = phase == PHASE_OPEN && rec.challenger == Pubkey::default() && rec.extensions > 0;
        let ok = phase == PHASE_VOIDED
            || phase == PHASE_FAILED
            || challenge_won
            || reset;
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

    /// Consume `trade_pending` then debit available / credit Board (ER `sync_vault` path).
    /// Market PDA signs. Prevents withdraw-during-ER-fill double-spend.
    pub fn credit_er_trade(ctx: Context<CreditTrade>, cost: u64, fee: u64) -> Result<()> {
        require!(ctx.accounts.board.market == ctx.accounts.market.key(), VaultError::WrongBoard);
        settle::credit_er_trade_inner(&mut ctx.accounts.board, &mut ctx.accounts.user, cost, fee)
    }

    /// Credit unused margin from an in-board sell. Market PDA signs (CPI).
    /// `basis_cut` = min(position.cost_paid, proceeds) so VOID ρ tracks remaining basis.
    pub fn refund_trade(ctx: Context<CreditTrade>, cost: u64, basis_cut: u64) -> Result<()> {
        require!(ctx.accounts.board.market == ctx.accounts.market.key(), VaultError::WrongBoard);
        settle::refund_trade_inner(
            &mut ctx.accounts.board,
            &mut ctx.accounts.user,
            cost,
            basis_cut,
        )
    }

    /// Market CPI: move USDC ledger from trader → platform (Keeper sync gas rebate).
    /// Fee must already sit in `trade_pending` (session open reserve) — never drains free margin.
    pub fn pay_sync_fee(ctx: Context<PaySyncFee>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(ctx.accounts.user.owner, ctx.accounts.owner.key(), VaultError::NotOwner);
        require_keys_eq!(
            ctx.accounts.platform_user.owner,
            ctx.accounts.platform.key(),
            VaultError::NotOwner
        );
        {
            let data = ctx.accounts.market.try_borrow_data()?;
            let mut cur: &[u8] = &data;
            let market = views::Market::try_deserialize(&mut cur)
                .map_err(|_| error!(VaultError::WrongBoard))?;
            require_keys_eq!(
                ctx.accounts.platform.key(),
                market.platform,
                VaultError::WrongBoard
            );
        }
        let user = &mut ctx.accounts.user;
        require!(
            user.trade_pending >= amount,
            VaultError::InsufficientTradePending
        );
        user.trade_pending = accounting::release(user.trade_pending, amount)?;
        let locked = accounting::locked(user.reserved, user.trade_pending)?;
        user.available = accounting::debit_available(user.available, locked, amount)?;
        ctx.accounts.platform_user.available =
            accounting::credit(ctx.accounts.platform_user.available, amount)?;
        Ok(())
    }

    /// After `finalize`, lock $L=E(x^*)$, $C_{\max}$, one $\rho$.
    /// Requires Board revenue/fees to match Market (all ER seats synced).
    /// `risk_book` is required (open an empty book before settle if unused).
    pub fn begin_settle(ctx: Context<BeginSettle>) -> Result<()> {
        let pool = ctx.accounts.pool.as_mut().map(|a| &mut **a);
        let tap = ctx.accounts.tap.as_mut().map(|a| &mut **a);
        let grid_data = ctx.accounts.grid.try_borrow_data()?;
        let (grid_market, _, grid_start) = settle::parse_grid_meta(&grid_data)?;
        require!(grid_market == ctx.accounts.market.key(), VaultError::WrongBoard);
        // Real shard PDA only — rejects writable `gdump` (same owner, forged exposure).
        settle::require_grid_pda(ctx.accounts.grid.key, &ctx.accounts.market.key(), grid_start)?;
        settle::compute_begin(
            &mut ctx.accounts.board,
            &ctx.accounts.market,
            &grid_data,
            &ctx.accounts.record,
            &ctx.accounts.risk_book,
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
        settle::compute_refund(
            &mut ctx.accounts.board,
            &ctx.accounts.market,
            &ctx.accounts.record,
        )?;
        Ok(())
    }

    pub fn payout(ctx: Context<Payout>, set_mask: Vec<u8>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.market.key(),
            ctx.accounts.board.market,
            VaultError::WrongBoard
        );
        require_keys_eq!(
            ctx.accounts.record.market,
            ctx.accounts.board.market,
            VaultError::WrongBoard
        );
        require!(
            ctx.accounts.position.set_hash == settle::set_hash(&set_mask),
            VaultError::BadMask
        );
        let grid_data = ctx.accounts.grid.try_borrow_data()?;
        let (grid_market, _, grid_start) = settle::parse_grid_meta(&grid_data)?;
        require!(grid_market == ctx.accounts.board.market, VaultError::WrongBoard);
        settle::require_grid_pda(ctx.accounts.grid.key, &ctx.accounts.board.market, grid_start)?;
        let cell = ctx.accounts.board.cell as usize;
        let face = settle::mask_face(
            ctx.accounts.position.q,
            &set_mask,
            ctx.accounts.market.n as usize,
            cell,
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
        require_keys_eq!(
            ctx.accounts.market.key(),
            ctx.accounts.board.market,
            VaultError::WrongBoard
        );
        require_keys_eq!(
            ctx.accounts.record.market,
            ctx.accounts.board.market,
            VaultError::WrongBoard
        );
        require!(
            ctx.accounts.position.set_hash == settle::skellam_ticket(kind, a, b),
            VaultError::BadMask
        );
        let grid_data = ctx.accounts.grid.try_borrow_data()?;
        let (grid_market, _, grid_start) = settle::parse_grid_meta(&grid_data)?;
        require!(grid_market == ctx.accounts.board.market, VaultError::WrongBoard);
        settle::require_grid_pda(ctx.accounts.grid.key, &ctx.accounts.board.market, grid_start)?;
        // `record` is this market's resolution PDA (constrained below) — k_max matches settle.
        let face = settle::skellam_face(
            ctx.accounts.position.q,
            kind,
            a,
            b,
            ctx.accounts.record.k_max,
            ctx.accounts.board.cell as usize,
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
            ctx.accounts.quote.key(),
            &ctx.accounts.quote,
            &mut ctx.accounts.user,
        )?;
        Ok(())
    }

    pub fn pay_premium(ctx: Context<QuotePay>) -> Result<()> {
        let layer_ai = ctx.accounts.layer.to_account_info();
        let data = layer_ai.try_borrow_data()?;
        let layer = views::Layer::from_account(&data)?;
        settle::pay_premium_inner(
            &mut ctx.accounts.board,
            ctx.accounts.quote.key(),
            &ctx.accounts.quote,
            layer,
            &mut ctx.accounts.user,
        )?;
        Ok(())
    }

    /// `weight_sum` is ignored — draw-path weights are computed from the Layer on-chain.
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
        let layer_ai = ctx.accounts.layer.to_account_info();
        let data = layer_ai.try_borrow_data()?;
        let layer = views::Layer::from_account(&data)?;
        settle::release_quote(
            &mut ctx.accounts.board,
            layer,
            ctx.accounts.quote.key(),
            &ctx.accounts.quote,
            &mut ctx.accounts.user,
        )?;
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

    /// `reserved + trade_pending` — both slices are non-withdrawable.
    pub fn locked(reserved: u64, trade_pending: u64) -> Result<u64> {
        reserved
            .checked_add(trade_pending)
            .ok_or(error!(VaultError::Overflow))
    }

    /// Unused margin only. Locked (`reserved` / `trade_pending`) cannot be withdrawn.
    pub fn debit_available(available: u64, locked: u64, amount: u64) -> Result<u64> {
        let free = available
            .checked_sub(locked)
            .ok_or(error!(VaultError::InsufficientAvailable))?;
        require!(free >= amount, VaultError::InsufficientAvailable);
        available
            .checked_sub(amount)
            .ok_or(error!(VaultError::InsufficientAvailable))
    }

    /// Check free margin, then return `field + amount` for the lock being increased.
    pub fn lock_add(available: u64, locked_total: u64, field: u64, amount: u64) -> Result<u64> {
        let free = available
            .checked_sub(locked_total)
            .ok_or(error!(VaultError::InsufficientAvailable))?;
        require!(free >= amount, VaultError::InsufficientAvailable);
        field
            .checked_add(amount)
            .ok_or(error!(VaultError::Overflow))
    }

    /// Increase `reserved` when `trade_pending == 0` (unit tests / simple path).
    pub fn reserve(available: u64, reserved: u64, amount: u64) -> Result<u64> {
        lock_add(available, reserved, reserved, amount)
    }

    pub fn release(field: u64, amount: u64) -> Result<u64> {
        field
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
    /// CHECK: market `["session", owner]` — `er_owed_usdc` locks free if the PDA exists.
    /// Pass the PDA even when uncreated (system-owned); omit not allowed (key must match seeds).
    pub session: UncheckedAccount<'info>,
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

/// CPI-only lock: `authority` is the market program's Protocol PDA.
#[derive(Accounts)]
pub struct LockTrade<'info> {
    /// Market `["protocol"]` PDA. Must sign via market CPI seeds.
    pub authority: Signer<'info>,
    /// CHECK: main-wallet owner of the user vault (not a signer).
    pub owner: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        constraint = user.owner == owner.key() @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

/// CPI-only unlock: `authority` is the market program's Protocol PDA.
#[derive(Accounts)]
pub struct UnlockTrade<'info> {
    /// Market `["protocol"]` PDA. Must sign via market CPI seeds.
    pub authority: Signer<'info>,
    /// CHECK: main-wallet owner of the user vault (not a signer).
    pub owner: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        constraint = user.owner == owner.key() @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
}

/// CPI-only release of risk `reserved`: `authority` is the risk program's `vault_auth` PDA.
#[derive(Accounts)]
pub struct ReleaseReserve<'info> {
    /// Risk `["vault_auth"]` PDA. Must sign via risk CPI seeds.
    pub authority: Signer<'info>,
    /// CHECK: LP owner of the user vault (not required to sign).
    pub owner: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        constraint = user.owner == owner.key() @ VaultError::NotOwner
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
pub struct LockChallengeBond<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: owned by market; committee_bond at offset 540.
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
        seeds = [CHBOND_SEED, market.key().as_ref()],
        bump
    )]
    pub bond: Account<'info, CommitteeBond>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SlashChallengeBond<'info> {
    pub authority: Signer<'info>,
    #[account(owner = views::MARKET_ID)]
    pub market: Account<'info, views::Market>,
    #[account(owner = views::RESOLUTION_ID, constraint = resolution.market == market.key() @ VaultError::WrongBoard)]
    pub resolution: Box<Account<'info, views::Resolution>>,
    #[account(
        mut,
        seeds = [CHBOND_SEED, market.key().as_ref()],
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
pub struct ReleaseChallengeBond<'info> {
    pub owner: Signer<'info>,
    #[account(owner = views::RESOLUTION_ID)]
    pub resolution: Box<Account<'info, views::Resolution>>,
    #[account(
        mut,
        seeds = [CHBOND_SEED, resolution.market.as_ref()],
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

fn outcome_eq(a: &views::Outcome, b: &views::Outcome) -> bool {
    a.family == b.family
        && a.kind == b.kind
        && a.a == b.a
        && a.b == b.b
        && a.shares == b.shares
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
pub struct PaySyncFee<'info> {
    /// Market PDA. Only the market program can sign this (CPI seeds).
    pub market: Signer<'info>,
    /// CHECK: trader vault owner.
    pub owner: UncheckedAccount<'info>,
    /// CHECK: `Market.platform` destination owner.
    pub platform: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [USER_SEED, owner.key().as_ref()],
        bump = user.bump,
        constraint = user.owner == owner.key() @ VaultError::NotOwner
    )]
    pub user: Account<'info, UserVault>,
    #[account(
        mut,
        seeds = [USER_SEED, platform.key().as_ref()],
        bump = platform_user.bump,
        constraint = platform_user.owner == platform.key() @ VaultError::NotOwner
    )]
    pub platform_user: Account<'info, UserVault>,
}

#[derive(Accounts)]
pub struct BeginSettle<'info> {
    #[account(mut, seeds = [BOARD_SEED, market.key().as_ref()], bump = board.bump)]
    pub board: Box<Account<'info, Board>>,
    pub market: Box<Account<'info, views::Market>>,
    /// CHECK: market-program grid. Header + one exposure word; n=1024 is 65KB.
    #[account(owner = views::MARKET_ID)]
    pub grid: UncheckedAccount<'info>,
    #[account(
        seeds = [b"res", market.key().as_ref()],
        bump = record.bump,
        seeds::program = views::RESOLUTION_ID,
        constraint = record.market == market.key() @ VaultError::WrongBoard
    )]
    pub record: Box<Account<'info, views::Resolution>>,
    /// Required so \(C_R\) / premium cannot be silently zeroed by omitting the book.
    #[account(
        seeds = [b"risk", market.key().as_ref()],
        bump = risk_book.bump,
        seeds::program = views::RISK_ID,
        constraint = risk_book.market == market.key() @ VaultError::WrongBoard
    )]
    pub risk_book: Box<Account<'info, views::RiskBook>>,
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
    #[account(
        seeds = [b"res", market.key().as_ref()],
        bump = record.bump,
        seeds::program = views::RESOLUTION_ID,
        constraint = record.market == market.key() @ VaultError::WrongBoard
    )]
    pub record: Box<Account<'info, views::Resolution>>,
}

#[derive(Accounts)]
pub struct Payout<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(mut, seeds = [BOARD_SEED, board.market.as_ref()], bump = board.bump)]
    pub board: Box<Account<'info, Board>>,
    #[account(constraint = market.key() == board.market @ VaultError::WrongBoard)]
    pub market: Box<Account<'info, views::Market>>,
    /// CHECK: live `["grid", market]` / shard PDA containing `board.cell` (not `gdump`).
    #[account(owner = views::MARKET_ID)]
    pub grid: UncheckedAccount<'info>,
    #[account(
        seeds = [b"res", board.market.as_ref()],
        bump = record.bump,
        seeds::program = views::RESOLUTION_ID,
        constraint = record.market == board.market @ VaultError::WrongBoard
    )]
    pub record: Box<Account<'info, views::Resolution>>,
    #[account(
        seeds = [
            b"pos",
            board.market.as_ref(),
            position.owner.as_ref(),
            position.set_hash.as_ref()
        ],
        bump = position.bump,
        seeds::program = views::MARKET_ID,
        constraint = position.market == board.market @ VaultError::WrongBoard
    )]
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
    #[account(
        seeds = [b"quote", board.market.as_ref(), quote.lp.as_ref(), &[quote.layer_id]],
        bump = quote.bump,
        seeds::program = views::RISK_ID,
        constraint = quote.market == board.market @ VaultError::WrongBoard
    )]
    pub quote: Account<'info, views::Quote>,
    /// CHECK: risk `Layer` zero_copy; PDA + owner bind to this board/market.
    #[account(
        owner = views::RISK_ID,
        seeds = [b"layer", board.market.as_ref(), &[quote.layer_id]],
        bump,
        seeds::program = views::RISK_ID
    )]
    pub layer: UncheckedAccount<'info>,
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
    #[account(
        seeds = [b"quote", board.market.as_ref(), quote.lp.as_ref(), &[quote.layer_id]],
        bump = quote.bump,
        seeds::program = views::RISK_ID,
        constraint = quote.market == board.market @ VaultError::WrongBoard
    )]
    pub quote: Account<'info, views::Quote>,
    /// CHECK: risk `Layer` zero_copy; PDA + owner bind to this board/market.
    #[account(
        owner = views::RISK_ID,
        seeds = [b"layer", board.market.as_ref(), &[quote.layer_id]],
        bump,
        seeds::program = views::RISK_ID
    )]
    pub layer: UncheckedAccount<'info>,
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
    #[account(
        seeds = [
            b"pos",
            board.market.as_ref(),
            position.owner.as_ref(),
            position.set_hash.as_ref()
        ],
        bump = position.bump,
        seeds::program = views::MARKET_ID,
        constraint = position.market == board.market @ VaultError::WrongBoard
    )]
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
    /// Session / ER trade budget. Locked against `withdraw` until `credit_er_trade` or `unlock_trade`.
    pub trade_pending: u64,
}

impl UserVault {
    pub const SIZE: usize = 32 + 8 + 8 + 1 + 8 + 8 + 8 + 8;
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
    #[msg("board revenue/fees not synced with market — finish sync_vault first")]
    BoardNotSynced,
    #[msg("trade_pending is insufficient for this ER fill / sync")]
    InsufficientTradePending,
    #[msg("shortfall settle requires AdjustPool + BoardTap accounts")]
    MissingCpAccounts,
    #[msg("market has an in-flight or dirty wide fill — cancel/abort before settle")]
    DirtyWide,
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

    #[test]
    fn trade_pending_blocks_withdraw_like_reserved() {
        let locked_amt = locked(20, 50).unwrap();
        assert_eq!(locked_amt, 70);
        assert!(debit_available(100, locked_amt, 40).is_err());
        assert_eq!(debit_available(100, locked_amt, 30).unwrap(), 70);
        let pending = lock_add(100, locked_amt, 50, 10).unwrap();
        assert_eq!(pending, 60);
    }
}
