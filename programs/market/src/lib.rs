//! Market program: create by distribution family, `p0` once, L1 `buy_set`.
//! Product sells are closed (`sell_set` / `sell_skellam_set` / wide `is_buy=false`).
//! Listing names (CPI, election, BTC) are metadata, not instructions.
//! Session PDA authorizes in-board fills (FR-WAL-04–06). After `delegate_book`, L1 fills return `Delegated`.

use anchor_lang::prelude::*;
use ephemeral_rollups_sdk::anchor::{delegate, ephemeral};
use ephemeral_rollups_sdk::cpi::{cpi_delegate, DelegateConfig};
use ephemeral_rollups_sdk::ephem::{FoldableIntentBuilder, MagicIntentBundleBuilder};
use ephemeral_rollups_sdk::types::DelegateAccountArgs;
use ephemeral_rollups_sdk::utils::close_pda_with_system_transfer;
use math::prior;
use math::settle::{usdc, usdc_charge};
use math::Q64;
use vault::cpi::accounts::CreditTrade;
use vault::cpi::{credit_trade, refund_trade};

pub mod ids;
pub mod mask;
pub mod session;
pub mod state;

use session::{
    check_trader, is_replay, require_next, session_auth_or, FillNonce, Session as ProtocolSession,
    SessionError, SessionTokenV2, IX_ALL_TRADES, IX_BUY_SET, IX_BUY_SKELLAM, IX_SELL_SET,
    IX_SELL_SKELLAM, NONCE_SEED, SESSION_LIVE, SESSION_REVOKED, SESSION_SEED,
};
// MagicBlock `#[derive(Session)]` / `#[session_auth_or]` (name collides with protocol Session account).
use session_keys::Session;
use state::*;

declare_id!("Market1111111111111111111111111111111111111");

/// Local `ephemeral-validator` identity (docs: localhost:7799).
pub const LOCAL_ER_VALIDATOR: Pubkey = pubkey!("mAGicPQYBMvcYveUZA5F5UNNwyHvfYh5xkLS2Fr1mev");
/// MagicBlock Delegation program (loaded on localnet from the validator dump).
pub const DELEGATION_PROGRAM_ID: Pubkey = pubkey!("DELeGGvXpWV2fqJUhqcF5ZSYMS4JTLjteaAMARRSaeSh");
/// MagicBlock magic program id used by `ephemeral-rollups-sdk` 0.17 CPI.
pub const MAGIC_PROGRAM_ID: Pubkey = pubkey!("Magic11111111111111111111111111111111111111");
/// MagicBlock magic context account (present and owned by the magic program on ER).
pub const MAGIC_CONTEXT_ID: Pubkey = pubkey!("MagicContext1111111111111111111111111111111");

#[ephemeral]
#[program]
pub mod market {
    use super::*;

    pub fn init_committee(ctx: Context<InitCommittee>, members: Vec<Pubkey>, m: u8) -> Result<()> {
        write_roster(&mut ctx.accounts.committee, ctx.accounts.authority.key(), ctx.bumps.committee, members, m)
    }

    pub fn set_roster(ctx: Context<SetRoster>, members: Vec<Pubkey>, m: u8) -> Result<()> {
        let committee = &mut ctx.accounts.committee;
        require!(ctx.accounts.authority.key() == committee.authority, MarketError::BadCommittee);
        let authority = committee.authority;
        let bump = committee.bump;
        let epoch = committee.epoch.saturating_add(1);
        write_roster(committee, authority, bump, members, m)?;
        committee.epoch = epoch;
        Ok(())
    }

    /// One-time official policy. Signer MUST be `args.platform`.
    pub fn init_protocol(ctx: Context<InitProtocol>, args: ProtocolArgs) -> Result<()> {
        require!(ctx.accounts.authority.key() == args.platform, MarketError::NotOfficial);
        require!(args.platform != Pubkey::default(), MarketError::BadCapital);
        require!(args.fee_bps <= 10_000, MarketError::BadFee);
        require!(
            args.fee_timing == FEE_ON_FILL || args.fee_timing == FEE_ON_CLAIM,
            MarketError::BadFee
        );
        require!(args.report_window_secs > 0 && args.challenge_secs > 0, MarketError::BadClock);
        require!(args.committee_bond > 0, MarketError::BadCapital);
        require!(args.alpha_r_bps <= 10_000, MarketError::BadFee);
        let p = &mut ctx.accounts.protocol;
        p.platform = args.platform;
        p.fee_bps = args.fee_bps;
        p.fee_timing = args.fee_timing;
        p.report_window_secs = args.report_window_secs;
        p.challenge_secs = args.challenge_secs;
        p.committee_bond = args.committee_bond;
        p.tap_cap_max = args.tap_cap_max;
        p.alpha_r_bps = args.alpha_r_bps;
        p.bump = ctx.bumps.protocol;
        Ok(())
    }

    /// Platform may update policy numbers. `platform` stays the init key.
    pub fn set_protocol(ctx: Context<SetProtocol>, args: ProtocolArgs) -> Result<()> {
        let p = &mut ctx.accounts.protocol;
        require!(ctx.accounts.authority.key() == p.platform, MarketError::NotOfficial);
        require!(args.platform == p.platform, MarketError::NotOfficial);
        require!(args.fee_bps <= 10_000, MarketError::BadFee);
        require!(
            args.fee_timing == FEE_ON_FILL || args.fee_timing == FEE_ON_CLAIM,
            MarketError::BadFee
        );
        require!(args.report_window_secs > 0 && args.challenge_secs > 0, MarketError::BadClock);
        require!(args.committee_bond > 0, MarketError::BadCapital);
        require!(args.alpha_r_bps <= 10_000, MarketError::BadFee);
        p.fee_bps = args.fee_bps;
        p.fee_timing = args.fee_timing;
        p.report_window_secs = args.report_window_secs;
        p.challenge_secs = args.challenge_secs;
        p.committee_bond = args.committee_bond;
        p.tap_cap_max = args.tap_cap_max;
        p.alpha_r_bps = args.alpha_r_bps;
        Ok(())
    }

    pub fn create_skellam_market(
        mut ctx: Context<CreateBoard>,
        id_hash: [u8; 32],
        n: u16,
        args: SkellamArgs,
    ) -> Result<()> {
        check_common(&id_hash, n, &args.common)?;
        require!(n == FOOTBALL_N, MarketError::BadGrid);
        require!(
            id_hash == ids::skellam(&args.topic, args.score_scope),
            MarketError::IdMismatch
        );
        require!(args.kickoff_ts <= args.common.close_ts, MarketError::BadClock);
        let p0 = match args.prior_kind {
            0 => prior::independent_poisson_2d(
                FOOTBALL_K_MAX as u32,
                Q64::from_raw(args.lambda_home),
                Q64::from_raw(args.lambda_away),
            ),
            1 => prior::dixon_coles_2d(
                FOOTBALL_K_MAX as u32,
                Q64::from_raw(args.lambda_home),
                Q64::from_raw(args.lambda_away),
                Q64::from_raw(args.dc_rho),
            ),
            2 => prior::football_uniform(FOOTBALL_K_MAX as u32),
            _ => return err!(MarketError::BadPrior),
        };
        open_board(
            &mut ctx,
            Family::Skellam,
            &args.common,
            p0,
            FamilyExtra {
                a: args.lambda_home,
                b: args.lambda_away,
                c: args.dc_rho,
                e: args.kickoff_ts,
                u0: args.score_scope,
                u1: args.prior_kind,
                u2: FOOTBALL_K_MAX,
                ..Default::default()
            },
        )
    }

    pub fn create_gaussian_market(
        mut ctx: Context<CreateBoard>,
        id_hash: [u8; 32],
        n: u16,
        args: IntervalArgs,
    ) -> Result<()> {
        open_interval(&mut ctx, id_hash, n, Family::Gaussian, args)
    }

    pub fn create_lognormal_market(
        mut ctx: Context<CreateBoard>,
        id_hash: [u8; 32],
        n: u16,
        args: IntervalArgs,
    ) -> Result<()> {
        open_interval(&mut ctx, id_hash, n, Family::Lognormal, args)
    }

    pub fn create_dirichlet_market(
        mut ctx: Context<CreateBoard>,
        id_hash: [u8; 32],
        n: u16,
        args: DirichletArgs,
    ) -> Result<()> {
        check_common(&id_hash, n, &args.common)?;
        require!(
            id_hash == ids::dirichlet(&args.topic, args.layout, args.top_n, args.bins),
            MarketError::IdMismatch
        );
        require!(args.alpha.len() >= 2, MarketError::BadPrior);
        let over = Grid::space(n as usize) > Grid::CREATE_CAP;
        if over {
            require!(args.alpha.len() <= 4, MarketError::BadGrid);
        }
        let alphas: Vec<Q64> = args.alpha.iter().copied().map(Q64::from_raw).collect();
        let p0 = if over {
            match args.layout {
                        DIRICHLET_ATOMS => {
                    // extra.a–d hold at most 4 alphas; atoms over CREATE_CAP cannot be rebuilt.
                    return err!(MarketError::BadGrid);
                }
                DIRICHLET_TOP_N => {
                    let k = args.alpha.len() as u32;
                    require!(args.top_n >= 1 && (args.top_n as u32) < k, MarketError::BadPrior);
                    let atoms = prior::binom(k, args.top_n as u32).ok_or(MarketError::BadGrid)?;
                    require!(atoms == n as u32 && atoms <= MAX_N as u32, MarketError::BadGrid);
                }
                DIRICHLET_SIMPLEX => {
                    let k = args.alpha.len() as u32;
                    let cells =
                        prior::simplex_cell_count(k, args.bins as u32).ok_or(MarketError::BadGrid)?;
                    require!(cells == n as u32 && cells <= MAX_N as u32, MarketError::BadGrid);
                }
                _ => return err!(MarketError::BadPrior),
            }
            vec![Q64::ZERO; n as usize]
        } else {
            match args.layout {
                DIRICHLET_ATOMS => {
                    require!(args.alpha.len() == n as usize, MarketError::BadGrid);
                    prior::dirichlet(&alphas)
                }
                DIRICHLET_TOP_N => {
                    let k = args.alpha.len() as u32;
                    require!(args.top_n >= 1 && (args.top_n as u32) < k, MarketError::BadPrior);
                    let atoms = prior::binom(k, args.top_n as u32).ok_or(MarketError::BadGrid)?;
                    require!(atoms == n as u32 && atoms <= MAX_N as u32, MarketError::BadGrid);
                    math::uniform_prior(atoms as usize)
                }
                DIRICHLET_SIMPLEX => {
                    let k = args.alpha.len() as u32;
                    let cells =
                        prior::simplex_cell_count(k, args.bins as u32).ok_or(MarketError::BadGrid)?;
                    require!(cells == n as u32 && cells <= MAX_N as u32, MarketError::BadGrid);
                    prior::vote_share_simplex(k as usize, args.bins as usize, &alphas)
                }
                _ => return err!(MarketError::BadPrior),
            }
        };
        open_board(
            &mut ctx,
            Family::Dirichlet,
            &args.common,
            p0,
            FamilyExtra {
                a: *args.alpha.first().unwrap_or(&0),
                b: *args.alpha.get(1).unwrap_or(&0),
                c: *args.alpha.get(2).unwrap_or(&0),
                d: *args.alpha.get(3).unwrap_or(&0),
                e: args.bins as i64,
                u0: args.layout,
                u1: args.top_n,
                u2: args.alpha.len() as u8,
                ..Default::default()
            },
        )
    }

    pub fn create_bernoulli_market(
        mut ctx: Context<CreateBoard>,
        id_hash: [u8; 32],
        n: u16,
        args: BernoulliArgs,
    ) -> Result<()> {
        check_common(&id_hash, n, &args.common)?;
        require!(n == 2, MarketError::BadGrid);
        require!(
            id_hash == ids::bernoulli(&args.topic, &args.tag),
            MarketError::IdMismatch
        );
        let p0 = prior::binary(Q64::from_raw(args.alpha_yes), Q64::from_raw(args.alpha_no));
        open_board(
            &mut ctx,
            Family::Bernoulli,
            &args.common,
            p0,
            FamilyExtra {
                a: args.alpha_yes,
                b: args.alpha_no,
                e: args.deadline_ts,
                u0: u8::from(args.early_resolve),
                ..Default::default()
            },
        )
    }

    pub fn open_session(
        ctx: Context<OpenSession>,
        authority: Pubkey,
        expires_ts: i64,
        remaining_usdc: u64,
        allowed_ix: u8,
        whitelist: Pubkey,
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        require!(authority != Pubkey::default(), MarketError::SessionUnauthorized);
        require!(expires_ts > now, MarketError::BadClock);
        require!(allowed_ix > 0 && allowed_ix & !IX_ALL_TRADES == 0, MarketError::SessionUnauthorized);
        let s = &mut ctx.accounts.session;
        if s.status == SESSION_LIVE && now < s.expires_ts {
            return err!(MarketError::SessionLive);
        }
        s.owner = ctx.accounts.owner.key();
        s.authority = authority;
        s.expires_ts = expires_ts;
        s.remaining_usdc = remaining_usdc;
        s.allowed_ix = allowed_ix;
        s.status = SESSION_LIVE;
        s.bump = ctx.bumps.session;
        s.whitelist = whitelist;
        Ok(())
    }

    pub fn renew_session(ctx: Context<MutSession>, expires_ts: i64, remaining_usdc: u64) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        require!(expires_ts > now, MarketError::BadClock);
        let s = &mut ctx.accounts.session;
        require!(s.status == SESSION_LIVE, MarketError::SessionUnauthorized);
        s.expires_ts = expires_ts;
        s.remaining_usdc = remaining_usdc;
        Ok(())
    }

    pub fn revoke_session(ctx: Context<MutSession>) -> Result<()> {
        let s = &mut ctx.accounts.session;
        s.status = SESSION_REVOKED;
        s.authority = Pubkey::default();
        s.remaining_usdc = 0;
        Ok(())
    }

    /// L1 path used before Delegate. After `delegate_book`, fills must run on ER
    /// (pass remaining `MAGIC_PROGRAM_ID`). Trader may be the owner or a live Session.
    /// CR-04: SessionTokenV2 via `#[session_auth_or]`, or owner / protocol Session authority.
    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn buy_set(mut ctx: Context<Trade>, set_mask: Vec<u8>, q_raw: i128, nonce: u64) -> Result<()> {
        fill(&mut ctx, &set_mask, q_raw, nonce, true)
    }

    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn sell_set(mut ctx: Context<Trade>, set_mask: Vec<u8>, q_raw: i128, nonce: u64) -> Result<()> {
        fill(&mut ctx, &set_mask, q_raw, nonce, false)
    }

    /// n=1024 full mask cannot lock 63 extras + Trade accounts in one tx (Solana 64-account cap).
    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn wide_begin(
        mut ctx: Context<Trade>,
        set_mask: Vec<u8>,
        q_raw: i128,
        nonce: u64,
        is_buy: bool,
    ) -> Result<()> {
        wide_begin_fill(&mut ctx, &set_mask, q_raw, nonce, is_buy)
    }

    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn wide_accum(
        mut ctx: Context<Trade>,
        set_mask: Vec<u8>,
        q_raw: i128,
        nonce: u64,
        is_buy: bool,
    ) -> Result<()> {
        wide_accum_fill(&mut ctx, &set_mask, q_raw, nonce, is_buy)
    }

    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn wide_apply(
        mut ctx: Context<Trade>,
        set_mask: Vec<u8>,
        q_raw: i128,
        nonce: u64,
        is_buy: bool,
    ) -> Result<()> {
        wide_apply_fill(&mut ctx, &set_mask, q_raw, nonce, is_buy)
    }

    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn wide_finish(
        mut ctx: Context<Trade>,
        set_mask: Vec<u8>,
        q_raw: i128,
        nonce: u64,
        is_buy: bool,
    ) -> Result<()> {
        wide_finish_fill(&mut ctx, &set_mask, q_raw, nonce, is_buy)
    }

    /// Allocates position + nonce on L1. Required before `delegate_seat` (ER cannot pay rent).
    pub fn open_seat(ctx: Context<OpenSeat>, set_hash: [u8; 32]) -> Result<()> {
        let pos = &mut ctx.accounts.position;
        if pos.market == Pubkey::default() {
            pos.market = ctx.accounts.market.key();
            pos.owner = ctx.accounts.owner.key();
            pos.set_hash = set_hash;
            pos.bump = ctx.bumps.position;
        }
        if ctx.accounts.nonce_acc.bump == 0 {
            ctx.accounts.nonce_acc.bump = ctx.bumps.nonce_acc;
        }
        Ok(())
    }

    /// Delegates the trader's position + nonce so ER `buy_set` can write them.
    pub fn delegate_seat(ctx: Context<DelegateSeat>, set_hash: [u8; 32]) -> Result<()> {
        {
            let owner = ctx.accounts.market.owner;
            if owner == &crate::ID {
                let data = ctx.accounts.market.try_borrow_data()?;
                let mut cur: &[u8] = &data;
                let market = Market::try_deserialize(&mut cur)?;
                require!(market.delegated, MarketError::NotDelegated);
            } else {
                require_keys_eq!(*owner, DELEGATION_PROGRAM_ID, MarketError::NotDelegated);
            }
        }
        let validator = ctx.remaining_accounts.first().map(|a| *a.key);
        if ctx.accounts.delegation_program.executable {
            ctx.accounts.delegate_position(
                &ctx.accounts.trader,
                &[
                    POS_SEED,
                    ctx.accounts.market.key().as_ref(),
                    ctx.accounts.owner.key().as_ref(),
                    set_hash.as_ref(),
                ],
                DelegateConfig {
                    validator,
                    ..Default::default()
                },
            )?;
            ctx.accounts.delegate_nonce_acc(
                &ctx.accounts.trader,
                &[
                    NONCE_SEED,
                    ctx.accounts.owner.key().as_ref(),
                    ctx.accounts.market.key().as_ref(),
                ],
                DelegateConfig {
                    validator,
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }

    /// 1X2 / handicap / totals / exact score on the shared Skellam grid.
    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn buy_skellam_set(
        mut ctx: Context<TradeSkellam>,
        contract: SkellamContract,
        q_raw: i128,
        nonce: u64,
    ) -> Result<()> {
        fill_skellam(&mut ctx, contract, q_raw, nonce, true)
    }

    #[session_auth_or(
        ctx.accounts.trader.key() == ctx.accounts.owner.key()
            || ctx
                .accounts
                .session
                .as_ref()
                .map(|s| s.authority == ctx.accounts.trader.key())
                .unwrap_or(false),
        SessionError::InvalidToken
    )]
    pub fn sell_skellam_set(
        mut ctx: Context<TradeSkellam>,
        contract: SkellamContract,
        q_raw: i128,
        nonce: u64,
    ) -> Result<()> {
        fill_skellam(&mut ctx, contract, q_raw, nonce, false)
    }

    /// Grow a Grid that did not fit in one `create_account` (n=256 needs one grow; n=1024 needs several).
    /// Only resizes. $P_0$ is written in `write_grid_mass` chunks then `seal_grid`.
    pub fn grow_grid(ctx: Context<GrowGrid>) -> Result<()> {
        expand_grid(ctx)
    }

    /// Append the next `PRIOR_CHUNK` unnormalized interval masses from `Market.extra`.
    pub fn write_grid_mass(ctx: Context<GrowGrid>) -> Result<()> {
        append_interval_mass(ctx)
    }

    /// Normalize chunked masses into $P_0$ / weights / $z$. Trading stays blocked until this.
    pub fn seal_grid(ctx: Context<GrowGrid>) -> Result<()> {
        seal_interval_p0(ctx)
    }

    /// Add raw $P_0$ of shard 0 + remaining extras into `market.p0_sum`. Idempotent per shard bit.
    pub fn accum_seal(ctx: Context<GrowGrid>) -> Result<()> {
        accum_seal_p0(ctx)
    }

    /// Write normalized $P_0$ / weights for shard 0 + remaining extras. Requires every shard bit set.
    pub fn apply_seal(ctx: Context<GrowGrid>) -> Result<()> {
        apply_seal_p0(ctx)
    }

    pub fn create_grid_shard(ctx: Context<CreateGridShard>, ix: u8) -> Result<()> {
        require!(ix >= 1, MarketError::BadGrid);
        let n = ctx.accounts.market.n;
        require!((ix as u16) < Grid::shard_count(n), MarketError::BadGrid);
        let local = Grid::shard_len(n, ix as u16);
        let grid = &mut ctx.accounts.shard;
        grid.market = ctx.accounts.market.key();
        grid.n = local;
        grid.start = Grid::shard_start(ix as u16);
        grid.bump = ctx.bumps.shard;
        grid.p0 = vec![0; local as usize];
        grid.theta = Vec::new();
        grid.exposure = Vec::new();
        grid.weights = Vec::new();
        grid.z = 0;
        Ok(())
    }

    pub fn write_grid_shard(ctx: Context<WriteGridShard>, ix: u8) -> Result<()> {
        let n = ctx.accounts.market.n;
        require!((ix as u16) < Grid::shard_count(n), MarketError::BadGrid);
        let local = Grid::shard_len(n, ix as u16) as usize;
        let start = Grid::shard_start(ix as u16) as usize;
        let info = ctx.accounts.shard.to_account_info();
        require!(info.data_len() >= Grid::space(local), MarketError::GridNotReady);
        let mut data = info.try_borrow_mut_data()?;
        let (g_market, g_n, g_start, _, z) = grid_header(&data)?;
        require!(g_market == ctx.accounts.market.key(), MarketError::WrongGrid);
        require!(g_n == local as u16 && g_start == start as u16, MarketError::WrongGrid);
        let (p0_len, _, _, w_len) = grid_vec_lens(&data)?;
        if p0_len == local && w_len == local && z != 0 {
            return Ok(());
        }
        require!(p0_len == local, MarketError::GridNotReady);
        write_p0_range(&mut data, &ctx.accounts.market, start, start + local)
    }

    /// Creator or roster member stops fills (VOID, including Bernoulli early occurrence, or after close).
    pub fn halt(ctx: Context<Halt>) -> Result<()> {
        require_book_keys(
            &ctx.accounts.authority.key(),
            &ctx.accounts.market,
            &ctx.accounts.committee,
            &ctx.accounts.committee.key(),
        )?;
        let market = &mut ctx.accounts.market;
        require!(market.status == Status::Trading as u8, MarketError::NotTrading);
        market.status = Status::Halted as u8;
        Ok(())
    }

    /// Marks the board delegated and CPIs MagicBlock Delegation for market + grid.
    pub fn delegate_book(ctx: Context<DelegateBook>) -> Result<()> {
        let id_hash = {
            let mut data = ctx.accounts.market.try_borrow_mut_data()?;
            let mut cur: &[u8] = &data;
            let mut market = Market::try_deserialize(&mut cur)?;
            require_book_keys(
                &ctx.accounts.authority.key(),
                &market,
                &ctx.accounts.committee,
                &ctx.accounts.committee.key(),
            )?;
            require!(market.status == Status::Trading as u8, MarketError::NotTrading);
            require!(!market.delegated, MarketError::AlreadyDelegated);
            let id_hash = market.id_hash;
            let (expected, _) =
                Pubkey::find_program_address(&[MARKET_SEED, id_hash.as_ref()], &crate::ID);
            require_keys_eq!(ctx.accounts.market.key(), expected, MarketError::IdMismatch);
            market.delegated = true;
            let mut out: &mut [u8] = &mut data;
            market.try_serialize(&mut out)?;
            id_hash
        };
        let validator = ctx.remaining_accounts.first().map(|a| *a.key);
        if ctx.accounts.delegation_program.executable {
            let grid_len = ctx.accounts.grid.data_len();
            let commit_frequency_ms = if grid_len > Grid::CREATE_CAP {
                u32::MAX
            } else {
                0
            };
            ctx.accounts.delegate_market(
                &ctx.accounts.authority,
                &[MARKET_SEED, id_hash.as_ref()],
                DelegateConfig {
                    validator,
                    commit_frequency_ms,
                    ..Default::default()
                },
            )?;
            let market_key = ctx.accounts.market.key();
            let grid_seeds: &[&[u8]] = &[GRID_SEED, market_key.as_ref()];
            if grid_len > Grid::CREATE_CAP {
                delegate_presized(
                    &ctx.accounts.authority,
                    &ctx.accounts.grid,
                    &ctx.accounts.buffer_grid.to_account_info(),
                    &ctx.accounts.owner_program.to_account_info(),
                    &ctx.accounts.delegation_record_grid.to_account_info(),
                    &ctx.accounts.delegation_metadata_grid.to_account_info(),
                    &ctx.accounts.delegation_program.to_account_info(),
                    &ctx.accounts.system_program.to_account_info(),
                    grid_seeds,
                    DelegateConfig {
                        validator,
                        commit_frequency_ms,
                        ..Default::default()
                    },
                )?;
            } else {
                ctx.accounts.delegate_grid(
                    &ctx.accounts.authority,
                    grid_seeds,
                    DelegateConfig {
                        validator,
                        commit_frequency_ms,
                        ..Default::default()
                    },
                )?;
            }
        }
        Ok(())
    }

    /// Grow the MagicBlock delegate buffer in 10 240-byte steps (top-level, not CPI).
    /// Required before `delegate_book` when the grid is larger than one inner `create_account`.
    pub fn prepare_delegate_buffer(ctx: Context<PrepareDelegateBuffer>) -> Result<()> {
        let src_len = ctx.accounts.source.data_len();
        require!(src_len > 0, MarketError::BadGrid);
        let buf = &ctx.accounts.buffer;
        let cur = buf.data_len();
        require!(cur < src_len, MarketError::BufferNotReady);
        let next = (cur.saturating_add(Grid::CREATE_CAP)).min(src_len);
        let rent = Rent::get()?.minimum_balance(next);
        let tag = ephemeral_rollups_sdk::pda::DELEGATE_BUFFER_TAG;
        let src_key = ctx.accounts.source.key();
        let (expected, bump) = Pubkey::find_program_address(&[tag, src_key.as_ref()], &crate::ID);
        require_keys_eq!(expected, buf.key(), MarketError::IdMismatch);
        let bump_slice = [bump];
        let seeds: &[&[u8]] = &[tag, src_key.as_ref(), bump_slice.as_slice()];
        if buf.lamports() == 0 {
            let space = src_len.min(Grid::CREATE_CAP);
            let rent = Rent::get()?.minimum_balance(space);
            anchor_lang::solana_program::program::invoke_signed(
                &anchor_lang::solana_program::system_instruction::create_account(
                    ctx.accounts.payer.key,
                    buf.key,
                    rent,
                    space as u64,
                    &crate::ID,
                ),
                &[
                    ctx.accounts.payer.to_account_info(),
                    buf.clone(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                &[seeds],
            )?;
        } else {
            let need = rent.saturating_sub(buf.lamports());
            if need > 0 {
                anchor_lang::solana_program::program::invoke(
                    &anchor_lang::solana_program::system_instruction::transfer(
                        ctx.accounts.payer.key,
                        buf.key,
                        need,
                    ),
                    &[
                        ctx.accounts.payer.to_account_info(),
                        buf.clone(),
                        ctx.accounts.system_program.to_account_info(),
                    ],
                )?;
            }
            buf.resize(next)?;
        }
        Ok(())
    }

    /// Grow a market-owned grid dump in 10 240-byte steps (committor cannot return a 16KB grid).
    pub fn prepare_grid_dump(ctx: Context<PrepareGridDump>) -> Result<()> {
        let n = {
            let data = ctx.accounts.market.try_borrow_data()?;
            let mut cur: &[u8] = &data;
            Market::try_deserialize(&mut cur)?.n as usize
        };
        let need = Grid::space(n);
        require!(need > 0, MarketError::BadGrid);
        let dump = &ctx.accounts.dump;
        let cur = dump.data_len();
        require!(cur < need, MarketError::BufferNotReady);
        let next = (cur.saturating_add(Grid::CREATE_CAP)).min(need);
        let rent = Rent::get()?.minimum_balance(next);
        let market_key = ctx.accounts.market.key();
        let (expected, bump) = Pubkey::find_program_address(&[GRID_DUMP_SEED, market_key.as_ref()], &crate::ID);
        require_keys_eq!(expected, dump.key(), MarketError::IdMismatch);
        let bump_slice = [bump];
        let seeds: &[&[u8]] = &[GRID_DUMP_SEED, market_key.as_ref(), bump_slice.as_slice()];
        if dump.lamports() == 0 {
            let space = need.min(Grid::CREATE_CAP);
            let rent = Rent::get()?.minimum_balance(space);
            anchor_lang::solana_program::program::invoke_signed(
                &anchor_lang::solana_program::system_instruction::create_account(
                    ctx.accounts.payer.key,
                    dump.key,
                    rent,
                    space as u64,
                    &crate::ID,
                ),
                &[
                    ctx.accounts.payer.to_account_info(),
                    dump.clone(),
                    ctx.accounts.system_program.to_account_info(),
                ],
                &[seeds],
            )?;
        } else {
            let need_lamports = rent.saturating_sub(dump.lamports());
            if need_lamports > 0 {
                anchor_lang::solana_program::program::invoke(
                    &anchor_lang::solana_program::system_instruction::transfer(
                        ctx.accounts.payer.key,
                        dump.key,
                        need_lamports,
                    ),
                    &[
                        ctx.accounts.payer.to_account_info(),
                        dump.clone(),
                        ctx.accounts.system_program.to_account_info(),
                    ],
                )?;
            }
            dump.resize(next)?;
        }
        Ok(())
    }

    /// Copy one chunk of ER grid bytes onto the L1 dump (vault reads this when grid is still DLP).
    pub fn write_grid_dump(ctx: Context<WriteGridDump>, offset: u32, data: Vec<u8>) -> Result<()> {
        require!(!data.is_empty() && data.len() <= 900, MarketError::BadGrid);
        let dump = &ctx.accounts.dump;
        let start = offset as usize;
        let end = start.checked_add(data.len()).ok_or_else(|| error!(MarketError::Overflow))?;
        require!(end <= dump.data_len(), MarketError::BadGrid);
        dump.try_borrow_mut_data()?[start..end].copy_from_slice(&data);
        Ok(())
    }

    /// Writes a journal checkpoint. On ER, also commits PDAs to L1. Does not move Vault USDC.
    pub fn commit_book(ctx: Context<CommitBook>, trades_root: [u8; 32]) -> Result<()> {
        require_book_keys(
            &ctx.accounts.authority.key(),
            &ctx.accounts.market,
            &ctx.accounts.committee,
            &ctx.accounts.committee.key(),
        )?;
        let market = &mut ctx.accounts.market;
        require!(market.delegated, MarketError::NotDelegated);
        market.trades_root = trades_root;
        market.commit_ts = Clock::get()?.unix_timestamp;
        market.exit(&crate::ID)?;
        // One MagicContext intent per tx. Grid is `undelegate_shard` after
        // this market commit has landed on L1.
        invoke_magic_commit(
            &ctx.accounts.authority,
            &ctx.accounts.magic_context,
            &ctx.accounts.magic_program,
            &[ctx.accounts.market.to_account_info()],
            false,
        )?;
        Ok(())
    }

    /// Commits and undelegates after halt or `close_ts`.
    pub fn undelegate_book(ctx: Context<CommitBook>) -> Result<()> {
        require_book_keys(
            &ctx.accounts.authority.key(),
            &ctx.accounts.market,
            &ctx.accounts.committee,
            &ctx.accounts.committee.key(),
        )?;
        let market = &mut ctx.accounts.market;
        require!(market.delegated, MarketError::NotDelegated);
        let now = Clock::get()?.unix_timestamp;
        require!(
            market.status == Status::Halted as u8 || now >= market.close_ts,
            MarketError::StillOpen
        );
        market.delegated = false;
        market.exit(&crate::ID)?;
        // MagicContext holds one intent per tx. Putting grid in a second
        // commit_and_undelegate here dropped it; putting market+grid in the
        // same intent made the committor drop both. Market only; grid is a
        // later `undelegate_shard` after L1 owner is Market.
        invoke_magic_commit(
            &ctx.accounts.authority,
            &ctx.accounts.magic_context,
            &ctx.accounts.magic_program,
            &[ctx.accounts.market.to_account_info()],
            true,
        )?;
        Ok(())
    }

    pub fn delegate_grid_shard(ctx: Context<DelegateShard>, ix: u8) -> Result<()> {
        require!(ix >= 1, MarketError::BadGrid);
        let validator = ctx.remaining_accounts.first().map(|a| *a.key);
        if ctx.accounts.delegation_program.executable {
            let ix_seed = [ix];
            ctx.accounts.delegate_shard(
                &ctx.accounts.authority,
                &[GRID_SEED, ctx.accounts.market.key().as_ref(), ix_seed.as_slice()],
                DelegateConfig {
                    validator,
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }

    pub fn undelegate_shard(ctx: Context<CommitShard>) -> Result<()> {
        invoke_magic_commit(
            &ctx.accounts.authority,
            &ctx.accounts.magic_context,
            &ctx.accounts.magic_program,
            &[ctx.accounts.shard.to_account_info()],
            true,
        )
    }

    /// Commits a delegated position + nonce to L1. Does not move Vault USDC.
    pub fn commit_seat(ctx: Context<CommitSeat>, set_hash: [u8; 32]) -> Result<()> {
        let _ = set_hash;
        invoke_magic_commit(
            &ctx.accounts.trader,
            &ctx.accounts.magic_context,
            &ctx.accounts.magic_program,
            &[
                ctx.accounts.position.to_account_info(),
                ctx.accounts.nonce_acc.to_account_info(),
            ],
            false,
        )
    }

    /// Commits and undelegates a seat so L1 `sync_vault` can run.
    pub fn undelegate_seat(ctx: Context<CommitSeat>, set_hash: [u8; 32]) -> Result<()> {
        let _ = set_hash;
        invoke_magic_commit(
            &ctx.accounts.trader,
            &ctx.accounts.magic_context,
            &ctx.accounts.magic_program,
            &[
                ctx.accounts.position.to_account_info(),
                ctx.accounts.nonce_acc.to_account_info(),
            ],
            true,
        )
    }

    /// After undelegate, debit `user_vault` for ER fills recorded in `vault_owed_*`.
    pub fn sync_vault(ctx: Context<SyncVault>, set_hash: [u8; 32]) -> Result<()> {
        let _ = set_hash;
        require!(
            !ctx.accounts.market.delegated,
            MarketError::Delegated
        );
        let cost = ctx.accounts.position.vault_owed_cost;
        let fee = ctx.accounts.position.vault_owed_fee;
        let credit = ctx.accounts.position.vault_owed_credit;
        if cost == 0 && fee == 0 && credit == 0 {
            return Ok(());
        }
        let owner_key = ctx.accounts.owner.key();
        require_keys_eq!(ctx.accounts.position.owner, owner_key, MarketError::BadVault);
        require_keys_eq!(ctx.accounts.position.market, ctx.accounts.market.key(), MarketError::IdMismatch);
        let need = cost.saturating_add(fee);
        if need > 0 {
            require_user_vault_cover(
                &ctx.accounts.user_vault.to_account_info(),
                &owner_key,
                need,
            )?;
            charge_vault(
                ctx.accounts.vault_program.to_account_info(),
                ctx.accounts.owner.to_account_info(),
                ctx.accounts.market.to_account_info(),
                ctx.accounts.board.to_account_info(),
                ctx.accounts.user_vault.to_account_info(),
                ctx.accounts.market.id_hash,
                ctx.accounts.market.bump,
                cost,
                fee,
            )?;
        }
        if credit > 0 {
            refund_vault(
                ctx.accounts.vault_program.to_account_info(),
                ctx.accounts.owner.to_account_info(),
                ctx.accounts.market.to_account_info(),
                ctx.accounts.board.to_account_info(),
                ctx.accounts.user_vault.to_account_info(),
                ctx.accounts.market.id_hash,
                ctx.accounts.market.bump,
                credit,
            )?;
        }
        ctx.accounts.position.vault_owed_cost = 0;
        ctx.accounts.position.vault_owed_fee = 0;
        ctx.accounts.position.vault_owed_credit = 0;
        Ok(())
    }

    /// Delegates the owner's Session PDA so ER session fills can write `remaining_usdc`.
    pub fn delegate_session(ctx: Context<DelegateSession>) -> Result<()> {
        let validator = ctx.remaining_accounts.first().map(|a| *a.key);
        if ctx.accounts.delegation_program.executable {
            ctx.accounts.delegate_session(
                &ctx.accounts.owner,
                &[SESSION_SEED, ctx.accounts.owner.key().as_ref()],
                DelegateConfig {
                    validator,
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }
}

fn open_interval(
    ctx: &mut Context<CreateBoard>,
    id_hash: [u8; 32],
    n: u16,
    family: Family,
    args: IntervalArgs,
) -> Result<()> {
    check_common(&id_hash, n, &args.common)?;
    require!(
        id_hash == ids::interval(family as u8, &args.topic, &args.tag),
        MarketError::IdMismatch
    );
    require!((8..=MAX_N).contains(&n), MarketError::BadGrid);
    // Over CREATE_CAP the account cannot hold P0; a full 256-point exp here also blows the 1.4M CU cap.
    // write_grid_mass / seal_grid rebuild P0 from extra.{a,b,c,d}.
    let p0 = if Grid::space(n as usize) > Grid::CREATE_CAP {
        require!(
            family == Family::Gaussian || family == Family::Lognormal,
            MarketError::BadGrid
        );
        vec![Q64::ZERO; n as usize]
    } else {
        match family {
            Family::Gaussian => prior::truncated_normal(
                n as usize,
                Q64::from_raw(args.x_min),
                Q64::from_raw(args.x_max),
                Q64::from_raw(args.mu),
                Q64::from_raw(args.sigma),
            ),
            Family::Lognormal => prior::truncated_lognormal(
                n as usize,
                Q64::from_raw(args.x_min),
                Q64::from_raw(args.x_max),
                Q64::from_raw(args.mu),
                Q64::from_raw(args.sigma),
            ),
            _ => return err!(MarketError::BadPrior),
        }
    };
    open_board(
        ctx,
        family,
        &args.common,
        p0,
        FamilyExtra {
            a: args.x_min,
            b: args.x_max,
            c: args.mu,
            d: args.sigma,
            ..Default::default()
        },
    )
}

fn open_board(
    ctx: &mut Context<CreateBoard>,
    family: Family,
    common: &CreateCommon,
    p0: Vec<Q64>,
    extra: FamilyExtra,
) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    require!(common.n as usize == p0.len(), MarketError::BadGrid);
    require!((2..=MAX_N).contains(&common.n), MarketError::BadGrid);
    require!(common.beta > 0, MarketError::BadBeta);
    require!(now < common.close_ts, MarketError::BadClock);
    require!(common.risk_lock_ts <= common.close_ts, MarketError::BadClock);
    lock_clocks(common)?;
    lock_layers(common)?;
    let protocol = &ctx.accounts.protocol;
    lock_protocol(protocol)?;
    let fee_bps = protocol.fee_bps;
    let fee_timing = protocol.fee_timing;
    let report_window_secs = protocol.report_window_secs;
    let challenge_secs = protocol.challenge_secs;
    let committee_bond = protocol.committee_bond;
    let alpha_r_bps = protocol.alpha_r_bps;
    let platform = protocol.platform;

    let market = &mut ctx.accounts.market;
    market.family = family as u8;
    market.status = Status::Trading as u8;
    market.bump = ctx.bumps.market;
    market.grid_bump = ctx.bumps.grid;
    market.n = common.n;
    market.fee_bps = fee_bps;
    market.fee_timing = fee_timing;
    market.creator = ctx.accounts.creator.key();
    market.committee = Pubkey::find_program_address(&[COMMITTEE_SEED], &crate::ID).0;
    market.authorized_reporter = common.authorized_reporter;
    market.close_ts = common.close_ts;
    market.risk_lock_ts = common.risk_lock_ts;
    market.report_open_ts = common.report_open_ts;
    market.committee_bond = committee_bond;
    market.report_window_secs = report_window_secs;
    market.challenge_secs = challenge_secs;
    market.n_layers = 1;
    market.d_unit = common.d_unit;
    market.gamma_bps = 0;
    market.beta = common.beta;
    market.c_m = 0;
    market.fees_accrued = 0;
    market.trading_revenue = 0;
    market.alpha_r_bps = alpha_r_bps;
    market.platform = platform;
    market.l_max = 0;
    market.id_hash = common.id_hash;
    market.extra = extra;
    market.delegated = false;
    market.trades_root = [0u8; 32];
    market.commit_ts = 0;
    market.p0_sum = 0;
    market.seal_bits = 0;
    market.wide_z0 = 0;
    market.wide_z = 0;
    market.wide_q = 0;
    market.wide_nonce = 0;
    market.wide_read = 0;
    market.wide_write = 0;
    market.wide_tag = 0;
    market.wide_flags = 0;

    let grid = &mut ctx.accounts.grid;
    let local = Grid::shard_len(common.n, 0) as usize;
    grid.market = market.key();
    grid.n = local as u16;
    grid.start = 0;
    grid.bump = ctx.bumps.grid;
    let first: Vec<Q64> = p0.iter().copied().take(local).collect();
    if common.n <= Grid::SHARD_CELLS {
        write_grid_prior(grid, &first);
    } else {
        grid.p0 = first.iter().map(|q| q.raw()).collect();
        grid.theta = Vec::new();
        grid.exposure = Vec::new();
        grid.weights = Vec::new();
        grid.z = 0;
    }
    Ok(())
}

fn write_grid_prior(grid: &mut Grid, p0: &[Q64]) {
    grid.p0 = p0.iter().map(|q| q.raw()).collect();
    grid.theta = vec![0; p0.len()];
    grid.exposure = vec![0; p0.len()];
    grid.weights = p0.iter().map(|q| q.raw()).collect();
    grid.z = p0.iter().fold(Q64::ZERO, |a, p| a.saturating_add(*p)).raw();
}

fn dirichlet_alphas(market: &Market) -> Result<Vec<Q64>> {
    let k = market.extra.u2 as usize;
    require!((2..=4).contains(&k), MarketError::BadPrior);
    let raw = [
        market.extra.a,
        market.extra.b,
        market.extra.c,
        market.extra.d,
    ];
    Ok(raw[..k].iter().copied().map(Q64::from_raw).collect())
}

fn interval_log_bounds(market: &Market) -> Result<(Q64, Q64)> {
    let xmin = Q64::from_raw(market.extra.a);
    let xmax = Q64::from_raw(market.extra.b);
    match market.family {
        x if x == Family::Gaussian as u8 => Ok((xmin, xmax)),
        x if x == Family::Lognormal as u8 => {
            require!(xmin.raw() > 0 && xmax > xmin, MarketError::BadGrid);
            Ok((xmin.ln(), xmax.ln()))
        }
        _ => err!(MarketError::BadGrid),
    }
}

const GRID_HDR: usize = 8 + 32 + 2 + 2 + 1 + 16;
const GRID_Z_OFF: usize = 45;

fn grid_i128(data: &[u8], off: usize) -> Result<i128> {
    require!(data.len() >= off + 16, MarketError::BadGrid);
    let bytes: [u8; 16] = data[off..off + 16]
        .try_into()
        .map_err(|_| error!(MarketError::BadGrid))?;
    Ok(i128::from_le_bytes(bytes))
}

fn set_grid_i128(data: &mut [u8], off: usize, v: i128) -> Result<()> {
    require!(data.len() >= off + 16, MarketError::BadGrid);
    data[off..off + 16].copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn grid_header(data: &[u8]) -> Result<(Pubkey, u16, u16, u8, i128)> {
    require!(data.len() >= GRID_HDR + 4, MarketError::BadGrid);
    require!(&data[..8] == Grid::DISCRIMINATOR, MarketError::BadGrid);
    let market_bytes: [u8; 32] = data[8..40].try_into().map_err(|_| error!(MarketError::BadGrid))?;
    let n_bytes: [u8; 2] = data[40..42].try_into().map_err(|_| error!(MarketError::BadGrid))?;
    let start_bytes: [u8; 2] = data[42..44].try_into().map_err(|_| error!(MarketError::BadGrid))?;
    Ok((
        Pubkey::from(market_bytes),
        u16::from_le_bytes(n_bytes),
        u16::from_le_bytes(start_bytes),
        data[44],
        grid_i128(data, GRID_Z_OFF)?,
    ))
}

fn grid_vec_lens(data: &[u8]) -> Result<(usize, usize, usize, usize)> {
    require!(data.len() >= GRID_HDR + 4, MarketError::BadGrid);
    let mut off = GRID_HDR;
    let read_len = |data: &[u8], off: &mut usize| -> Result<usize> {
        require!(data.len() >= *off + 4, MarketError::BadGrid);
        let n = u32::from_le_bytes(data[*off..*off + 4].try_into().map_err(|_| error!(MarketError::BadGrid))?)
            as usize;
        *off += 4 + n.saturating_mul(16);
        require!(data.len() >= *off, MarketError::BadGrid);
        Ok(n)
    };
    let p0 = read_len(data, &mut off)?;
    let theta = read_len(data, &mut off)?;
    let exposure = read_len(data, &mut off)?;
    let weights = read_len(data, &mut off)?;
    Ok((p0, theta, exposure, weights))
}

/// Data offsets of the four `Vec<i128>` payloads when every vec has length `n`.
fn sealed_vec_offs(n: usize) -> (usize, usize, usize, usize) {
    let p0 = GRID_HDR + 4;
    let theta = p0 + n * 16 + 4;
    let exp = theta + n * 16 + 4;
    let w = exp + n * 16 + 4;
    (p0, theta, exp, w)
}

fn require_sealed_bytes(data: &[u8], market: Pubkey, n: u16) -> Result<(usize, usize, usize)> {
    let (g_market, g_n, _, _, z) = grid_header(data)?;
    require!(g_market == market, MarketError::WrongGrid);
    require!(g_n == n, MarketError::WrongGrid);
    require!(z != 0, MarketError::GridNotReady);
    let nu = n as usize;
    require!(data.len() >= Grid::space(nu), MarketError::GridNotReady);
    let (p0_len, theta_len, exp_len, w_len) = grid_vec_lens(data)?;
    require!(
        p0_len == nu && theta_len == nu && exp_len == nu && w_len == nu,
        MarketError::GridNotReady
    );
    let (_, theta, exp, w) = sealed_vec_offs(nu);
    Ok((theta, exp, w))
}

fn grid_l_max_bytes(data: &[u8], exp_off: usize, n: usize) -> Result<i128> {
    let mut m = 0i128;
    for i in 0..n {
        let v = grid_i128(data, exp_off + i * 16)?;
        if v > m {
            m = v;
        }
    }
    Ok(m)
}

fn apply_cached_on_bytes(
    data: &mut [u8],
    n: usize,
    theta_off: usize,
    exp_off: usize,
    w_off: usize,
    beta: Q64,
    in_set: impl Fn(usize) -> bool,
    signed_q: i128,
) -> Result<Q64> {
    let z0 = Q64::from_raw(grid_i128(data, GRID_Z_OFF)?);
    require!(z0.raw() != 0, MarketError::GridNotReady);
    let q = Q64::from_raw(signed_q);
    let mut num = Q64::ZERO;
    for i in 0..n {
        if in_set(i) {
            num = num.saturating_add(Q64::from_raw(grid_i128(data, w_off + i * 16)?));
        }
    }
    let p = num.checked_div(z0).unwrap_or(Q64::ZERO);
    let cost = math::lmsr::lmsr_cost(beta, p, q);
    let e = q.checked_div(beta).unwrap_or(Q64::ZERO).exp();
    let mut z_now = z0;
    for i in 0..n {
        if in_set(i) {
            let w_old = Q64::from_raw(grid_i128(data, w_off + i * 16)?);
            let w_new = w_old.saturating_mul(e);
            z_now = z_now.saturating_sub(w_old).saturating_add(w_new);
            set_grid_i128(data, w_off + i * 16, w_new.raw())?;
            let th = Q64::from_raw(grid_i128(data, theta_off + i * 16)?);
            set_grid_i128(data, theta_off + i * 16, th.saturating_add(q).raw())?;
            let ex = Q64::from_raw(grid_i128(data, exp_off + i * 16)?);
            set_grid_i128(data, exp_off + i * 16, ex.saturating_add(q).raw())?;
        }
    }
    set_grid_i128(data, GRID_Z_OFF, z_now.raw())?;
    Ok(cost)
}

fn extra_shard_count(n: u16) -> usize {
    Grid::shard_count(n).saturating_sub(1) as usize
}

fn shard_bit(ix: usize) -> u128 {
    1u128 << ix
}

fn seal_bit_mask(n: u16) -> u128 {
    let c = Grid::shard_count(n);
    if c == 0 {
        0
    } else if c >= 128 {
        u128::MAX
    } else {
        (1u128 << c) - 1
    }
}

fn shard_pda(market: &Pubkey, ix: u8) -> Pubkey {
    if ix == 0 {
        Pubkey::find_program_address(&[GRID_SEED, market.as_ref()], &crate::ID).0
    } else {
        Pubkey::find_program_address(&[GRID_SEED, market.as_ref(), &[ix]], &crate::ID).0
    }
}

fn parse_extra_ix(info: &AccountInfo, market: Pubkey, n: u16) -> Result<usize> {
    let data = info.try_borrow_data()?;
    let (g_market, g_n, start, _, _) = grid_header(&data)?;
    require!(g_market == market, MarketError::WrongGrid);
    let ix = (start / Grid::SHARD_CELLS) as usize;
    require!(ix >= 1 && (ix as u16) < Grid::shard_count(n), MarketError::WrongGrid);
    require!(start == Grid::shard_start(ix as u16), MarketError::WrongGrid);
    require!(g_n == Grid::shard_len(n, ix as u16), MarketError::WrongGrid);
    require_keys_eq!(*info.key, shard_pda(&market, ix as u8), MarketError::WrongGrid);
    Ok(ix)
}

fn extras_end(remaining: &[AccountInfo], market: Pubkey, n: u16) -> usize {
    let mut i = 0usize;
    while i < remaining.len() {
        if remaining[i].key == &MAGIC_PROGRAM_ID {
            break;
        }
        if parse_extra_ix(&remaining[i], market, n).is_err() {
            break;
        }
        i += 1;
    }
    i
}

fn needed_extra_bits(n: u16, in_set: impl Fn(usize) -> bool) -> u128 {
    let mut bits = 0u128;
    for cell in 0..n as usize {
        if in_set(cell) {
            let ix = cell / Grid::SHARD_CELLS as usize;
            if ix >= 1 {
                bits |= shard_bit(ix);
            }
        }
    }
    bits
}

fn apply_one_shard_num(
    data: &[u8],
    market: Pubkey,
    n: usize,
    ix: usize,
    in_set: impl Fn(usize) -> bool,
) -> Result<(u16, i128, Q64)> {
    let local = Grid::shard_len(n as u16, ix as u16);
    let (_, _, w_off) = require_sealed_bytes(data, market, local)?;
    let (_, _, start, _, z) = grid_header(data)?;
    require!(start == Grid::shard_start(ix as u16), MarketError::WrongGrid);
    let mut num = Q64::ZERO;
    for i in 0..local as usize {
        if in_set(start as usize + i) {
            num = num.saturating_add(Q64::from_raw(grid_i128(data, w_off + i * 16)?));
        }
    }
    Ok((start, z, num))
}

fn apply_one_shard_write(
    data: &mut [u8],
    market: Pubkey,
    n: usize,
    ix: usize,
    in_set: impl Fn(usize) -> bool,
    q: Q64,
    e: Q64,
    mut z_now: Q64,
    mut l_max: i128,
) -> Result<(Q64, i128)> {
    let local = Grid::shard_len(n as u16, ix as u16) as usize;
    let (theta_off, exp_off, w_off) = require_sealed_bytes(data, market, local as u16)?;
    let start = Grid::shard_start(ix as u16) as usize;
    for i in 0..local {
        if in_set(start + i) {
            let w_old = Q64::from_raw(grid_i128(data, w_off + i * 16)?);
            let w_new = w_old.saturating_mul(e);
            z_now = z_now.saturating_sub(w_old).saturating_add(w_new);
            set_grid_i128(data, w_off + i * 16, w_new.raw())?;
            let th = Q64::from_raw(grid_i128(data, theta_off + i * 16)?);
            set_grid_i128(data, theta_off + i * 16, th.saturating_add(q).raw())?;
            let ex = Q64::from_raw(grid_i128(data, exp_off + i * 16)?);
            set_grid_i128(data, exp_off + i * 16, ex.saturating_add(q).raw())?;
        }
        let ex = grid_i128(data, exp_off + i * 16)?;
        if ex > l_max {
            l_max = ex;
        }
    }
    if ix == 0 {
        set_grid_i128(data, GRID_Z_OFF, z_now.raw())?;
    }
    Ok((z_now, l_max))
}

fn apply_cached_on_shards(
    grid: &AccountInfo,
    extras: &[AccountInfo],
    market: Pubkey,
    n: usize,
    beta: Q64,
    in_set: impl Fn(usize) -> bool,
    signed_q: i128,
) -> Result<(Q64, i128)> {
    let need = needed_extra_bits(n as u16, &in_set);
    let mut got = 0u128;
    let mut extra_ix: Vec<usize> = Vec::with_capacity(extras.len());
    for shard in extras {
        let ix = parse_extra_ix(shard, market, n as u16)?;
        got |= shard_bit(ix);
        extra_ix.push(ix);
    }
    require!(got & need == need, MarketError::GridNotReady);
    let mut z0 = Q64::ZERO;
    let mut num = Q64::ZERO;
    {
        let data = grid.try_borrow_data()?;
        let (_, z, part) = apply_one_shard_num(&data, market, n, 0, &in_set)?;
        z0 = Q64::from_raw(z);
        num = num.saturating_add(part);
    }
    for (shard, ix) in extras.iter().zip(extra_ix.iter()) {
        let data = shard.try_borrow_data()?;
        let (_, _, part) = apply_one_shard_num(&data, market, n, *ix, &in_set)?;
        num = num.saturating_add(part);
    }
    require!(z0.raw() != 0, MarketError::GridNotReady);
    let q = Q64::from_raw(signed_q);
    let p = num.checked_div(z0).unwrap_or(Q64::ZERO);
    let cost = math::lmsr::lmsr_cost(beta, p, q);
    let e = q.checked_div(beta).unwrap_or(Q64::ZERO).exp();
    let mut z_now = z0;
    let mut l_max = 0i128;
    {
        let mut data = grid.try_borrow_mut_data()?;
        let out = apply_one_shard_write(&mut data, market, n, 0, &in_set, q, e, z_now, l_max)?;
        z_now = out.0;
        l_max = out.1;
    }
    for (shard, ix) in extras.iter().zip(extra_ix.iter()) {
        let mut data = shard.try_borrow_mut_data()?;
        let out = apply_one_shard_write(&mut data, market, n, *ix, &in_set, q, e, z_now, l_max)?;
        z_now = out.0;
        l_max = out.1;
    }
    Ok((cost, l_max))
}

fn expand_grid(ctx: Context<GrowGrid>) -> Result<()> {
    let local = Grid::shard_len(ctx.accounts.market.n, 0) as usize;
    let info = ctx.accounts.grid.to_account_info();
    {
        let data = info.try_borrow_data()?;
        let (g_market, g_n, _, _, _) = grid_header(&data)?;
        require!(g_market == ctx.accounts.market.key(), MarketError::WrongGrid);
        require!(g_n == local as u16, MarketError::WrongGrid);
    }
    let need = Grid::space(local);
    if info.data_len() >= need {
        return Ok(());
    }
    let next = info.data_len().saturating_add(Grid::CREATE_CAP).min(need);
    let rent = Rent::get()?.minimum_balance(next);
    let extra = rent.saturating_sub(info.lamports());
    if extra > 0 {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.creator.to_account_info(),
                    to: info.clone(),
                },
            ),
            extra,
        )?;
    }
    info.resize(next)?;
    Ok(())
}

fn write_p0_range(data: &mut [u8], market: &Market, start: usize, end: usize) -> Result<()> {
    let n = market.n as usize;
    let p0_off = GRID_HDR + 4;
    if market.family == Family::Dirichlet as u8 {
        match market.extra.u0 {
            DIRICHLET_SIMPLEX => {
                let alphas = dirichlet_alphas(market)?;
                let bins = market.extra.e as usize;
                let chunk = prior::simplex_chunk_raw(alphas.len(), bins, &alphas, start, end);
                require!(chunk.len() == end - start, MarketError::BadGrid);
                for (i, raw) in chunk.iter().enumerate() {
                    set_grid_i128(data, p0_off + i * 16, *raw)?;
                }
            }
            DIRICHLET_TOP_N => {
                for i in 0..(end - start) {
                    set_grid_i128(data, p0_off + i * 16, Q64::ONE.raw())?;
                }
            }
            _ => return err!(MarketError::BadGrid),
        }
        return Ok(());
    }
    if market.family == Family::Skellam as u8 {
        let k_max = if market.extra.u2 == 0 {
            10
        } else {
            market.extra.u2 as u32
        };
        let p0 = match market.extra.u1 {
            0 => prior::independent_poisson_2d(
                k_max,
                Q64::from_raw(market.extra.a),
                Q64::from_raw(market.extra.b),
            ),
            1 => prior::dixon_coles_2d(
                k_max,
                Q64::from_raw(market.extra.a),
                Q64::from_raw(market.extra.b),
                Q64::from_raw(market.extra.c),
            ),
            2 => prior::football_uniform(k_max),
            _ => return err!(MarketError::BadPrior),
        };
        require!(p0.len() == n && end <= n, MarketError::BadGrid);
        for i in start..end {
            set_grid_i128(data, p0_off + (i - start) * 16, p0[i].raw())?;
        }
        return Ok(());
    }
    let (lo, hi) = interval_log_bounds(market)?;
    let kn = prior::TruncatedNormal::new(
        n,
        lo,
        hi,
        Q64::from_raw(market.extra.c),
        Q64::from_raw(market.extra.d),
    );
    for i in start..end {
        set_grid_i128(data, p0_off + (i - start) * 16, kn.weight_at(i).raw())?;
    }
    Ok(())
}

fn append_interval_mass(ctx: Context<GrowGrid>) -> Result<()> {
    let local = Grid::shard_len(ctx.accounts.market.n, 0) as usize;
    let info = ctx.accounts.grid.to_account_info();
    require!(info.data_len() >= Grid::space(local), MarketError::GridNotReady);
    let mut data = info.try_borrow_mut_data()?;
    let (g_market, g_n, _, _, z) = grid_header(&data)?;
    require!(g_market == ctx.accounts.market.key(), MarketError::WrongGrid);
    require!(g_n == local as u16, MarketError::WrongGrid);
    let (p0_len, theta_len, exp_len, w_len) = grid_vec_lens(&data)?;
    if p0_len == local && w_len == local && z != 0 {
        return Ok(());
    }
    require!(
        z == 0 && w_len == 0 && theta_len == 0 && exp_len == 0,
        MarketError::BadGrid
    );
    require!(p0_len == local, MarketError::GridNotReady);
    write_p0_range(&mut data, &ctx.accounts.market, 0, local)
}

fn seal_one_shard(data: &mut [u8], p0: &[i128], z: i128, write_z: bool) -> Result<()> {
    let n = p0.len();
    require!(data.len() >= Grid::space(n), MarketError::GridNotReady);
    if write_z {
        data[GRID_Z_OFF..GRID_Z_OFF + 16].copy_from_slice(&z.to_le_bytes());
    } else {
        data[GRID_Z_OFF..GRID_Z_OFF + 16].copy_from_slice(&1i128.to_le_bytes());
    }
    let p0_off = GRID_HDR + 4;
    for (i, v) in p0.iter().enumerate() {
        data[p0_off + i * 16..p0_off + i * 16 + 16].copy_from_slice(&v.to_le_bytes());
    }
    let mut off = p0_off + n * 16;
    let n32 = (n as u32).to_le_bytes();
    data[off..off + 4].copy_from_slice(&n32);
    off += 4;
    data[off..off + n * 16].fill(0);
    off += n * 16;
    data[off..off + 4].copy_from_slice(&n32);
    off += 4;
    data[off..off + n * 16].fill(0);
    off += n * 16;
    data[off..off + 4].copy_from_slice(&n32);
    off += 4;
    for (i, v) in p0.iter().enumerate() {
        data[off + i * 16..off + i * 16 + 16].copy_from_slice(&v.to_le_bytes());
    }
    Ok(())
}

fn read_shard_p0(data: &[u8], local: usize) -> Result<Vec<i128>> {
    let p0_off = GRID_HDR + 4;
    let mut p0 = vec![0i128; local];
    for (i, slot) in p0.iter_mut().enumerate() {
        *slot = grid_i128(data, p0_off + i * 16)?;
    }
    Ok(p0)
}

fn read_seal_shard(data: &[u8], market: Pubkey, n: u16, ix: usize) -> Result<(bool, Vec<i128>)> {
    let local = Grid::shard_len(n, ix as u16) as usize;
    let (g_market, g_n, start, _, z) = grid_header(data)?;
    require!(g_market == market, MarketError::WrongGrid);
    require!(g_n == local as u16 && start == Grid::shard_start(ix as u16), MarketError::WrongGrid);
    let (p0_len, _, _, w_len) = grid_vec_lens(data)?;
    require!(p0_len == local, MarketError::GridNotReady);
    let ready = p0_len == local && w_len == local && (ix != 0 || z != 0);
    Ok((ready, read_shard_p0(data, local)?))
}

fn seal_interval_p0(ctx: Context<GrowGrid>) -> Result<()> {
    let n_u = ctx.accounts.market.n;
    let n = n_u as usize;
    let extra = extra_shard_count(n_u);
    require!(ctx.remaining_accounts.len() >= extra, MarketError::GridNotReady);
    let market_key = ctx.accounts.market.key();
    let mut all = Vec::with_capacity(n);
    let mut sealed = true;
    {
        let data = ctx.accounts.grid.try_borrow_data()?;
        let (ready, p0) = read_seal_shard(&data, market_key, n_u, 0)?;
        sealed &= ready;
        all.extend(p0);
    }
    for (i, info) in ctx.remaining_accounts.iter().take(extra).enumerate() {
        let data = info.try_borrow_data()?;
        let (ready, p0) = read_seal_shard(&data, market_key, n_u, i + 1)?;
        sealed &= ready;
        all.extend(p0);
    }
    if sealed {
        return Ok(());
    }
    require!(all.len() == n, MarketError::BadGrid);
    prior::normalize_i128(&mut all);
    let z_sum = all
        .iter()
        .fold(Q64::ZERO, |a, p| a.saturating_add(Q64::from_raw(*p)))
        .raw();
    let mut off = 0usize;
    {
        let local = Grid::shard_len(n_u, 0) as usize;
        let mut data = ctx.accounts.grid.try_borrow_mut_data()?;
        seal_one_shard(&mut data, &all[off..off + local], z_sum, true)?;
        off += local;
    }
    for (i, info) in ctx.remaining_accounts.iter().take(extra).enumerate() {
        let local = Grid::shard_len(n_u, (i + 1) as u16) as usize;
        let mut data = info.try_borrow_mut_data()?;
        seal_one_shard(&mut data, &all[off..off + local], z_sum, false)?;
        off += local;
    }
    Ok(())
}

fn accum_one_p0(data: &[u8], market_key: Pubkey, n: u16, ix: usize, mkt: &mut Market) -> Result<()> {
    if mkt.seal_bits & shard_bit(ix) != 0 {
        return Ok(());
    }
    let local = Grid::shard_len(n, ix as u16) as usize;
    let (g_market, g_n, start, _, _) = grid_header(data)?;
    require!(g_market == market_key, MarketError::WrongGrid);
    require!(g_n == local as u16 && start == Grid::shard_start(ix as u16), MarketError::WrongGrid);
    let (p0_len, _, _, _) = grid_vec_lens(data)?;
    require!(p0_len == local, MarketError::GridNotReady);
    for v in read_shard_p0(data, local)? {
        mkt.p0_sum = mkt.p0_sum.checked_add(v).ok_or(MarketError::Overflow)?;
    }
    mkt.seal_bits |= shard_bit(ix);
    Ok(())
}

fn normalize_chunk(p0: &mut [i128], sum: i128, dust: bool) {
    let inv = Q64::ONE.checked_div(Q64::from_raw(sum)).unwrap_or(Q64::ZERO);
    let mut acc = Q64::ZERO;
    for x in p0.iter_mut() {
        let p = Q64::from_raw(*x).saturating_mul(inv);
        *x = p.raw();
        acc = acc.saturating_add(p);
    }
    if dust && acc != Q64::ONE && !p0.is_empty() {
        p0[0] = Q64::from_raw(p0[0])
            .saturating_add(Q64::ONE.saturating_sub(acc))
            .raw();
    }
}

fn apply_one_p0(data: &mut [u8], market_key: Pubkey, n: u16, ix: usize, sum: i128) -> Result<()> {
    let local = Grid::shard_len(n, ix as u16) as usize;
    let (ready, mut p0) = read_seal_shard(data, market_key, n, ix)?;
    if ready {
        return Ok(());
    }
    require!(sum > 0, MarketError::BadGrid);
    normalize_chunk(&mut p0, sum, ix == 0);
    seal_one_shard(data, &p0, if ix == 0 { Q64::ONE.raw() } else { 1 }, ix == 0)
}

fn accum_seal_p0(ctx: Context<GrowGrid>) -> Result<()> {
    let n = ctx.accounts.market.n;
    let market_key = ctx.accounts.market.key();
    {
        let data = ctx.accounts.grid.try_borrow_data()?;
        accum_one_p0(&data, market_key, n, 0, &mut ctx.accounts.market)?;
    }
    for info in ctx.remaining_accounts.iter() {
        let ix = parse_extra_ix(info, market_key, n)?;
        let data = info.try_borrow_data()?;
        accum_one_p0(&data, market_key, n, ix, &mut ctx.accounts.market)?;
    }
    Ok(())
}

fn apply_seal_p0(ctx: Context<GrowGrid>) -> Result<()> {
    let n = ctx.accounts.market.n;
    require!(
        ctx.accounts.market.seal_bits == seal_bit_mask(n),
        MarketError::GridNotReady
    );
    let market_key = ctx.accounts.market.key();
    let sum = ctx.accounts.market.p0_sum;
    {
        let mut data = ctx.accounts.grid.try_borrow_mut_data()?;
        apply_one_p0(&mut data, market_key, n, 0, sum)?;
    }
    for info in ctx.remaining_accounts.iter() {
        let ix = parse_extra_ix(info, market_key, n)?;
        let mut data = info.try_borrow_mut_data()?;
        apply_one_p0(&mut data, market_key, n, ix, sum)?;
    }
    Ok(())
}

fn require_buy(is_buy: bool) -> Result<()> {
    require!(is_buy, MarketError::SellsClosed);
    Ok(())
}

fn fill(ctx: &mut Context<Trade>, set_mask: &[u8], q_raw: i128, nonce: u64, is_buy: bool) -> Result<()> {
    require_buy(is_buy)?;
    require!(q_raw > 0, MarketError::ZeroQty);
    require!(
        ctx.accounts.market.status == Status::Trading as u8,
        MarketError::NotTrading
    );
    require!(
        ctx.accounts.market.wide_flags & 1 == 0,
        MarketError::WideFillBusy
    );
    let n = ctx.accounts.market.n;
    let market_key = ctx.accounts.market.key();
    let extra = extras_end(ctx.remaining_accounts, market_key, n);
    require_er_fill(&ctx.accounts.market, &ctx.remaining_accounts[extra..])?;
    let now = Clock::get()?.unix_timestamp;
    require!(now < ctx.accounts.market.close_ts, MarketError::Closed);
    let owner_key = ctx.accounts.owner.key();
    let trader_key = ctx.accounts.trader.key();
    let ix = if is_buy { IX_BUY_SET } else { IX_SELL_SET };
    check_trader(
        &trader_key,
        &owner_key,
        ctx.accounts.session.as_ref().map(|s| s.as_ref().as_ref()),
        &market_key,
        now,
        ix,
    )?;
    if is_replay(&ctx.accounts.nonce_acc, nonce) {
        return Ok(());
    }
    require_next(&ctx.accounts.nonce_acc, nonce)?;
    let n_cells = n as usize;
    mask::check(set_mask, n_cells)?;
    let signed = if is_buy { q_raw } else { -q_raw };
    if !is_buy {
        require!(ctx.accounts.position.q >= q_raw, MarketError::NoInventory);
    }
    let (cost, l_max) = apply_cached_on_shards(
        &ctx.accounts.grid.to_account_info(),
        &ctx.remaining_accounts[..extra],
        market_key,
        n_cells,
        Q64::from_raw(ctx.accounts.market.beta),
        |i| mask::bit(set_mask, i),
        signed,
    )?;

    ctx.accounts.market.l_max = ctx.accounts.market.l_max.max(l_max);
    let fee_bps = ctx.accounts.market.fee_bps;
    let fee = if is_buy && ctx.accounts.market.fee_on_fill() && fee_bps > 0 {
        cost.saturating_mul(Q64::from_int(fee_bps as i64))
            .checked_div(Q64::from_int(10_000))
            .unwrap_or(Q64::ZERO)
    } else {
        Q64::ZERO
    };
    let cost_usdc = fill_usdc(is_buy, cost);
    let fee_usdc = fill_usdc(is_buy, fee);
    if is_buy {
        ctx.accounts.market.fees_accrued = ctx
            .accounts
            .market
            .fees_accrued
            .saturating_add(fee_usdc);
        ctx.accounts.market.trading_revenue = ctx
            .accounts
            .market
            .trading_revenue
            .saturating_add(cost_usdc);
    } else if cost_usdc > 0 {
        ctx.accounts.market.trading_revenue = ctx
            .accounts
            .market
            .trading_revenue
            .saturating_sub(cost_usdc);
    }

    let pos = &mut ctx.accounts.position;
    pos.market = market_key;
    pos.owner = owner_key;
    pos.set_hash = ids::set_hash(set_mask);
    pos.bump = ctx.bumps.position;
    if is_buy {
        pos.q = pos.q.checked_add(q_raw).ok_or(MarketError::Overflow)?;
        pos.cost_paid = pos.cost_paid.saturating_add(cost_usdc);
    } else {
        pos.q = pos.q.checked_sub(q_raw).ok_or(MarketError::NoInventory)?;
        pos.cost_paid = pos.cost_paid.saturating_sub(cost_usdc);
    }

    let id_hash = ctx.accounts.market.id_hash;
    let bump = ctx.accounts.market.bump;
    settle_vault_fill(
        &mut ctx.accounts.session,
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.market.to_account_info(),
        ctx.accounts.board.to_account_info(),
        ctx.accounts.user_vault.to_account_info(),
        ctx.accounts.vault_program.to_account_info(),
        ctx.remaining_accounts,
        &mut **pos,
        id_hash,
        bump,
        is_buy,
        cost_usdc,
        fee_usdc,
        trader_key,
        owner_key,
    )?;
    ctx.accounts.nonce_acc.last = nonce;
    ctx.accounts.nonce_acc.bump = ctx.bumps.nonce_acc;

    emit!(FillEvent {
        market: market_key,
        owner: owner_key,
        buy: is_buy,
        q: q_raw,
        cost: cost.raw(),
        fee: fee.raw(),
        l_max,
    });
    Ok(())
}

const WIDE_ACTIVE: u8 = 1;
const WIDE_BUY: u8 = 2;

fn mask_tag(set_mask: &[u8]) -> u64 {
    let h = ids::set_hash(set_mask);
    u64::from_le_bytes(h[..8].try_into().unwrap())
}

fn wide_need(n: u16, set_mask: &[u8]) -> u128 {
    1u128 | needed_extra_bits(n, |i| mask::bit(set_mask, i))
}

fn wide_clear(m: &mut Market) {
    m.p0_sum = 0;
    m.wide_z0 = 0;
    m.wide_z = 0;
    m.wide_q = 0;
    m.wide_nonce = 0;
    m.wide_read = 0;
    m.wide_write = 0;
    m.wide_tag = 0;
    m.wide_flags = 0;
}

fn wide_params_ok(m: &Market, set_mask: &[u8], q_raw: i128, nonce: u64, is_buy: bool) -> Result<()> {
    require!(m.wide_flags & WIDE_ACTIVE != 0, MarketError::GridNotReady);
    require!(m.wide_tag == mask_tag(set_mask), MarketError::BadMask);
    require!(m.wide_q == q_raw, MarketError::ZeroQty);
    require!(m.wide_nonce == nonce, MarketError::NonceReplay);
    require!((m.wide_flags & WIDE_BUY != 0) == is_buy, MarketError::BadMask);
    Ok(())
}

fn wide_gate(ctx: &Context<Trade>, set_mask: &[u8], q_raw: i128, nonce: u64, is_buy: bool) -> Result<bool> {
    require_buy(is_buy)?;
    require!(q_raw > 0, MarketError::ZeroQty);
    require!(
        ctx.accounts.market.status == Status::Trading as u8,
        MarketError::NotTrading
    );
    let n = ctx.accounts.market.n;
    let market_key = ctx.accounts.market.key();
    let extra = extras_end(ctx.remaining_accounts, market_key, n);
    require_er_fill(&ctx.accounts.market, &ctx.remaining_accounts[extra..])?;
    let now = Clock::get()?.unix_timestamp;
    require!(now < ctx.accounts.market.close_ts, MarketError::Closed);
    let ix = if is_buy { IX_BUY_SET } else { IX_SELL_SET };
    check_trader(
        &ctx.accounts.trader.key(),
        &ctx.accounts.owner.key(),
        ctx.accounts.session.as_ref().map(|s| s.as_ref().as_ref()),
        &market_key,
        now,
        ix,
    )?;
    if is_replay(&ctx.accounts.nonce_acc, nonce) {
        return Ok(true);
    }
    require_next(&ctx.accounts.nonce_acc, nonce)?;
    mask::check(set_mask, n as usize)?;
    if !is_buy {
        require!(ctx.accounts.position.q >= q_raw, MarketError::NoInventory);
    }
    Ok(false)
}

fn wide_accum_extras(ctx: &mut Context<Trade>, set_mask: &[u8]) -> Result<()> {
    let n = ctx.accounts.market.n;
    let market_key = ctx.accounts.market.key();
    let extra = extras_end(ctx.remaining_accounts, market_key, n);
    let need = wide_need(n, set_mask);
    let in_set = |i: usize| mask::bit(set_mask, i);
    let m = &mut ctx.accounts.market;
    if m.wide_read & 1 == 0 {
        let data = ctx.accounts.grid.try_borrow_data()?;
        let (_, z, part) = apply_one_shard_num(&data, market_key, n as usize, 0, in_set)?;
        require!(z != 0, MarketError::GridNotReady);
        m.wide_z0 = z;
        m.p0_sum = part.raw();
        m.wide_read |= 1;
    }
    for shard in &ctx.remaining_accounts[..extra] {
        let ix = parse_extra_ix(shard, market_key, n)?;
        let bit = shard_bit(ix);
        require!(need & bit != 0, MarketError::WrongGrid);
        if m.wide_read & bit != 0 {
            continue;
        }
        let data = shard.try_borrow_data()?;
        let (_, _, part) = apply_one_shard_num(&data, market_key, n as usize, ix, in_set)?;
        m.p0_sum = m.p0_sum.saturating_add(part.raw());
        m.wide_read |= bit;
    }
    Ok(())
}

fn wide_begin_fill(
    ctx: &mut Context<Trade>,
    set_mask: &[u8],
    q_raw: i128,
    nonce: u64,
    is_buy: bool,
) -> Result<()> {
    if wide_gate(ctx, set_mask, q_raw, nonce, is_buy)? {
        return Ok(());
    }
    {
        let m = &ctx.accounts.market;
        if m.wide_flags & WIDE_ACTIVE != 0 {
            wide_params_ok(m, set_mask, q_raw, nonce, is_buy)?;
        } else {
            let m = &mut ctx.accounts.market;
            m.wide_flags = WIDE_ACTIVE | if is_buy { WIDE_BUY } else { 0 };
            m.wide_q = q_raw;
            m.wide_nonce = nonce;
            m.wide_tag = mask_tag(set_mask);
            m.wide_read = 0;
            m.wide_write = 0;
            m.wide_z = 0;
            m.p0_sum = 0;
        }
    }
    wide_accum_extras(ctx, set_mask)
}

fn wide_accum_fill(
    ctx: &mut Context<Trade>,
    set_mask: &[u8],
    q_raw: i128,
    nonce: u64,
    is_buy: bool,
) -> Result<()> {
    if wide_gate(ctx, set_mask, q_raw, nonce, is_buy)? {
        return Ok(());
    }
    wide_params_ok(&ctx.accounts.market, set_mask, q_raw, nonce, is_buy)?;
    wide_accum_extras(ctx, set_mask)
}

fn wide_apply_fill(
    ctx: &mut Context<Trade>,
    set_mask: &[u8],
    q_raw: i128,
    nonce: u64,
    is_buy: bool,
) -> Result<()> {
    if wide_gate(ctx, set_mask, q_raw, nonce, is_buy)? {
        return Ok(());
    }
    wide_params_ok(&ctx.accounts.market, set_mask, q_raw, nonce, is_buy)?;
    let n = ctx.accounts.market.n;
    let need = wide_need(n, set_mask);
    require!(
        ctx.accounts.market.wide_read == need,
        MarketError::GridNotReady
    );
    let market_key = ctx.accounts.market.key();
    let extra = extras_end(ctx.remaining_accounts, market_key, n);
    let signed = if is_buy { q_raw } else { -q_raw };
    let q = Q64::from_raw(signed);
    let beta = Q64::from_raw(ctx.accounts.market.beta);
    let e = q.checked_div(beta).unwrap_or(Q64::ZERO).exp();
    let in_set = |i: usize| mask::bit(set_mask, i);
    let mut z_now = if ctx.accounts.market.wide_write == 0 {
        Q64::from_raw(ctx.accounts.market.wide_z0)
    } else {
        Q64::from_raw(ctx.accounts.market.wide_z)
    };
    let mut l_max = ctx.accounts.market.l_max;
    for shard in &ctx.remaining_accounts[..extra] {
        let ix = parse_extra_ix(shard, market_key, n)?;
        let bit = shard_bit(ix);
        require!(need & bit != 0, MarketError::WrongGrid);
        if ctx.accounts.market.wide_write & bit != 0 {
            continue;
        }
        let mut data = shard.try_borrow_mut_data()?;
        let out = apply_one_shard_write(
            &mut data,
            market_key,
            n as usize,
            ix,
            in_set,
            q,
            e,
            z_now,
            l_max,
        )?;
        z_now = out.0;
        l_max = out.1;
        ctx.accounts.market.wide_write |= bit;
    }
    let extra_need = need & !1u128;
    if ctx.accounts.market.wide_write & extra_need == extra_need
        && ctx.accounts.market.wide_write & 1 == 0
    {
        let mut data = ctx.accounts.grid.try_borrow_mut_data()?;
        let out = apply_one_shard_write(
            &mut data,
            market_key,
            n as usize,
            0,
            in_set,
            q,
            e,
            z_now,
            l_max,
        )?;
        z_now = out.0;
        l_max = out.1;
        ctx.accounts.market.wide_write |= 1;
    }
    ctx.accounts.market.wide_z = z_now.raw();
    ctx.accounts.market.l_max = ctx.accounts.market.l_max.max(l_max);
    Ok(())
}

fn wide_finish_fill(
    ctx: &mut Context<Trade>,
    set_mask: &[u8],
    q_raw: i128,
    nonce: u64,
    is_buy: bool,
) -> Result<()> {
    if wide_gate(ctx, set_mask, q_raw, nonce, is_buy)? {
        return Ok(());
    }
    wide_params_ok(&ctx.accounts.market, set_mask, q_raw, nonce, is_buy)?;
    let n = ctx.accounts.market.n;
    let need = wide_need(n, set_mask);
    require!(
        ctx.accounts.market.wide_write == need,
        MarketError::GridNotReady
    );
    let market_key = ctx.accounts.market.key();
    let extra = extras_end(ctx.remaining_accounts, market_key, n);
    require_er_fill(&ctx.accounts.market, &ctx.remaining_accounts[extra..])?;
    let signed = if is_buy { q_raw } else { -q_raw };
    let q = Q64::from_raw(signed);
    let z0 = Q64::from_raw(ctx.accounts.market.wide_z0);
    require!(z0.raw() != 0, MarketError::GridNotReady);
    let num = Q64::from_raw(ctx.accounts.market.p0_sum);
    let p = num.checked_div(z0).unwrap_or(Q64::ZERO);
    let beta = Q64::from_raw(ctx.accounts.market.beta);
    let cost = math::lmsr::lmsr_cost(beta, p, q);
    let l_max = ctx.accounts.market.l_max;
    let fee_bps = ctx.accounts.market.fee_bps;
    let fee = if is_buy && ctx.accounts.market.fee_on_fill() && fee_bps > 0 {
        cost.saturating_mul(Q64::from_int(fee_bps as i64))
            .checked_div(Q64::from_int(10_000))
            .unwrap_or(Q64::ZERO)
    } else {
        Q64::ZERO
    };
    let cost_usdc = fill_usdc(is_buy, cost);
    let fee_usdc = fill_usdc(is_buy, fee);
    if is_buy {
        ctx.accounts.market.fees_accrued = ctx
            .accounts
            .market
            .fees_accrued
            .saturating_add(fee_usdc);
        ctx.accounts.market.trading_revenue = ctx
            .accounts
            .market
            .trading_revenue
            .saturating_add(cost_usdc);
    } else if cost_usdc > 0 {
        ctx.accounts.market.trading_revenue = ctx
            .accounts
            .market
            .trading_revenue
            .saturating_sub(cost_usdc);
    }
    let owner_key = ctx.accounts.owner.key();
    let trader_key = ctx.accounts.trader.key();
    let pos = &mut ctx.accounts.position;
    pos.market = market_key;
    pos.owner = owner_key;
    pos.set_hash = ids::set_hash(set_mask);
    pos.bump = ctx.bumps.position;
    if is_buy {
        pos.q = pos.q.checked_add(q_raw).ok_or(MarketError::Overflow)?;
        pos.cost_paid = pos.cost_paid.saturating_add(cost_usdc);
    } else {
        pos.q = pos.q.checked_sub(q_raw).ok_or(MarketError::NoInventory)?;
        pos.cost_paid = pos.cost_paid.saturating_sub(cost_usdc);
    }
    let id_hash = ctx.accounts.market.id_hash;
    let bump = ctx.accounts.market.bump;
    settle_vault_fill(
        &mut ctx.accounts.session,
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.market.to_account_info(),
        ctx.accounts.board.to_account_info(),
        ctx.accounts.user_vault.to_account_info(),
        ctx.accounts.vault_program.to_account_info(),
        ctx.remaining_accounts,
        &mut **pos,
        id_hash,
        bump,
        is_buy,
        cost_usdc,
        fee_usdc,
        trader_key,
        owner_key,
    )?;
    ctx.accounts.nonce_acc.last = nonce;
    ctx.accounts.nonce_acc.bump = ctx.bumps.nonce_acc;
    wide_clear(&mut ctx.accounts.market);
    emit!(FillEvent {
        market: market_key,
        owner: owner_key,
        buy: is_buy,
        q: q_raw,
        cost: cost.raw(),
        fee: fee.raw(),
        l_max,
    });
    Ok(())
}

fn expand_skellam(contract: SkellamContract, k_max: u32) -> Result<Vec<Vec<bool>>> {
    let (kind, a, b) = contract.ticket_key();
    math::football::skellam_masks(kind, a, b, k_max).ok_or_else(|| error!(MarketError::BadMask))
}

fn fill_skellam(
    ctx: &mut Context<TradeSkellam>,
    contract: SkellamContract,
    q_raw: i128,
    nonce: u64,
    is_buy: bool,
) -> Result<()> {
    require_buy(is_buy)?;
    require!(q_raw > 0, MarketError::ZeroQty);
    require!(
        ctx.accounts.market.family == Family::Skellam as u8,
        MarketError::WrongFamily
    );
    require!(
        ctx.accounts.market.status == Status::Trading as u8,
        MarketError::NotTrading
    );
    let n = ctx.accounts.market.n;
    let market_key = ctx.accounts.market.key();
    require!(
        ctx.accounts.market.wide_flags & 1 == 0,
        MarketError::WideFillBusy
    );
    let extra = extras_end(ctx.remaining_accounts, market_key, n);
    require_er_fill(&ctx.accounts.market, &ctx.remaining_accounts[extra..])?;
    let now = Clock::get()?.unix_timestamp;
    require!(now < ctx.accounts.market.close_ts, MarketError::Closed);
    let owner_key = ctx.accounts.owner.key();
    let trader_key = ctx.accounts.trader.key();
    let ix = if is_buy { IX_BUY_SKELLAM } else { IX_SELL_SKELLAM };
    check_trader(
        &trader_key,
        &owner_key,
        ctx.accounts.session.as_ref().map(|s| s.as_ref().as_ref()),
        &market_key,
        now,
        ix,
    )?;
    if is_replay(&ctx.accounts.nonce_acc, nonce) {
        return Ok(());
    }
    require_next(&ctx.accounts.nonce_acc, nonce)?;
    if !is_buy {
        require!(ctx.accounts.position.q >= q_raw, MarketError::NoInventory);
    }
    let k_max = if ctx.accounts.market.extra.u2 == 0 {
        10
    } else {
        ctx.accounts.market.extra.u2 as u32
    };
    let masks = expand_skellam(contract, k_max)?;
    let parts = masks.len() as i128;
    require!(parts >= 1 && q_raw % parts == 0, MarketError::ZeroQty);
    let part = q_raw / parts;
    let signed = if is_buy { part } else { -part };
    let n_cells = n as usize;
    let beta = Q64::from_raw(ctx.accounts.market.beta);
    let fee_bps = ctx.accounts.market.fee_bps;
    let charge_fee = is_buy && ctx.accounts.market.fee_on_fill() && fee_bps > 0;
    let mut cost_sum = 0i128;
    let mut fee_sum = 0i128;
    let mut l_max = 0i128;
    for m in &masks {
        require!(m.len() == n_cells, MarketError::BadMask);
        let (cost, shard_max) = apply_cached_on_shards(
            &ctx.accounts.grid.to_account_info(),
            &ctx.remaining_accounts[..extra],
            market_key,
            n_cells,
            beta,
            |i| m[i],
            signed,
        )?;
        if shard_max > l_max {
            l_max = shard_max;
        }
        let fee = if charge_fee {
            cost.saturating_mul(Q64::from_int(fee_bps as i64))
                .checked_div(Q64::from_int(10_000))
                .unwrap_or(Q64::ZERO)
        } else {
            Q64::ZERO
        };
        if is_buy {
            ctx.accounts.market.fees_accrued = ctx
                .accounts
                .market
                .fees_accrued
                .saturating_add(usdc_charge(fee));
            ctx.accounts.market.trading_revenue = ctx
                .accounts
                .market
                .trading_revenue
                .saturating_add(usdc_charge(cost));
        } else {
            ctx.accounts.market.trading_revenue = ctx
                .accounts
                .market
                .trading_revenue
                .saturating_sub(fill_usdc(false, cost));
        }
        cost_sum = cost_sum.checked_add(cost.raw()).ok_or(MarketError::Overflow)?;
        fee_sum = fee_sum.checked_add(fee.raw()).ok_or(MarketError::Overflow)?;
    }
    ctx.accounts.market.l_max = ctx.accounts.market.l_max.max(l_max);
    let (kind, a, b) = contract.ticket_key();
    let pos = &mut ctx.accounts.position;
    pos.market = market_key;
    pos.owner = owner_key;
    pos.set_hash = ids::skellam_ticket(kind, a, b);
    pos.bump = ctx.bumps.position;
    let cost_q = Q64::from_raw(cost_sum);
    let fee_q = Q64::from_raw(fee_sum);
    let cost_usdc = fill_usdc(is_buy, cost_q);
    let fee_usdc = fill_usdc(is_buy, fee_q);
    if is_buy {
        pos.q = pos.q.checked_add(q_raw).ok_or(MarketError::Overflow)?;
        pos.cost_paid = pos.cost_paid.saturating_add(cost_usdc);
    } else {
        pos.q = pos.q.checked_sub(q_raw).ok_or(MarketError::NoInventory)?;
        pos.cost_paid = pos.cost_paid.saturating_sub(cost_usdc);
    }
    let id_hash = ctx.accounts.market.id_hash;
    let bump = ctx.accounts.market.bump;
    settle_vault_fill(
        &mut ctx.accounts.session,
        ctx.accounts.owner.to_account_info(),
        ctx.accounts.market.to_account_info(),
        ctx.accounts.board.to_account_info(),
        ctx.accounts.user_vault.to_account_info(),
        ctx.accounts.vault_program.to_account_info(),
        ctx.remaining_accounts,
        &mut **pos,
        id_hash,
        bump,
        is_buy,
        cost_usdc,
        fee_usdc,
        trader_key,
        owner_key,
    )?;
    ctx.accounts.nonce_acc.last = nonce;
    ctx.accounts.nonce_acc.bump = ctx.bumps.nonce_acc;
    emit!(FillEvent {
        market: market_key,
        owner: owner_key,
        buy: is_buy,
        q: q_raw,
        cost: cost_sum,
        fee: fee_sum,
        l_max,
    });
    Ok(())
}

fn check_common(id_hash: &[u8; 32], n: u16, common: &CreateCommon) -> Result<()> {
    require!(*id_hash == common.id_hash, MarketError::IdMismatch);
    require!(n == common.n, MarketError::BadGrid);
    Ok(())
}

fn lock_clocks(common: &CreateCommon) -> Result<()> {
    require!(
        common.report_open_ts >= common.close_ts,
        MarketError::BadClock
    );
    Ok(())
}

fn lock_protocol(protocol: &Protocol) -> Result<()> {
    require!(protocol.platform != Pubkey::default(), MarketError::BadCapital);
    require!(protocol.fee_bps <= 10_000, MarketError::BadFee);
    require!(
        protocol.fee_timing == crate::state::FEE_ON_FILL || protocol.fee_timing == crate::state::FEE_ON_CLAIM,
        MarketError::BadFee
    );
    require!(
        protocol.report_window_secs > 0 && protocol.challenge_secs > 0,
        MarketError::BadClock
    );
    require!(protocol.committee_bond > 0, MarketError::BadCapital);
    require!(protocol.alpha_r_bps <= 10_000, MarketError::BadFee);
    Ok(())
}

fn fill_usdc(is_buy: bool, amount: Q64) -> u64 {
    if is_buy {
        return usdc_charge(amount);
    }
    let raw = amount.raw();
    if raw < 0 {
        usdc(Q64::from_raw(-raw))
    } else {
        usdc(amount)
    }
}

fn executing_on_er(remaining: &[AccountInfo]) -> bool {
    remaining
        .iter()
        .any(|a| a.key == &MAGIC_PROGRAM_ID && a.executable)
}

fn require_user_vault_cover(user_vault: &AccountInfo, owner: &Pubkey, need: u64) -> Result<()> {
    let data = user_vault.try_borrow_data()?;
    let mut cur: &[u8] = &data;
    let uv = vault::UserVault::try_deserialize(&mut cur).map_err(|_| error!(MarketError::BadVault))?;
    require_keys_eq!(uv.owner, *owner, MarketError::BadVault);
    require!(uv.available >= need, MarketError::InsufficientVault);
    Ok(())
}

fn require_er_fill(market: &Market, remaining: &[AccountInfo]) -> Result<()> {
    if !market.delegated {
        return Ok(());
    }
    require!(executing_on_er(remaining), MarketError::Delegated);
    Ok(())
}

fn delegate_presized<'info>(
    payer: &Signer<'info>,
    pda: &AccountInfo<'info>,
    buffer: &AccountInfo<'info>,
    owner_program: &AccountInfo<'info>,
    delegation_record: &AccountInfo<'info>,
    delegation_metadata: &AccountInfo<'info>,
    _delegation_program: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    pda_seeds: &[&[u8]],
    config: DelegateConfig,
) -> Result<()> {
    let data_len = pda.data_len();
    require!(buffer.data_len() == data_len && buffer.lamports() > 0, MarketError::BufferNotReady);
    {
        let src = pda.try_borrow_data()?;
        let mut dst = buffer.try_borrow_mut_data()?;
        dst.copy_from_slice(&src);
    }
    {
        let mut data = pda.try_borrow_mut_data()?;
        data.fill(0);
    }
    let (expected, bump) = Pubkey::find_program_address(pda_seeds, &crate::ID);
    require_keys_eq!(expected, *pda.key, MarketError::IdMismatch);
    let bump_slice = [bump];
    let mut seed_store: Vec<&[u8]> = pda_seeds.to_vec();
    seed_store.push(bump_slice.as_slice());
    let signer: &[&[&[u8]]] = &[seed_store.as_slice()];
    if pda.owner != &anchor_lang::system_program::ID {
        pda.assign(&anchor_lang::system_program::ID);
    }
    if pda.owner != &DELEGATION_PROGRAM_ID {
        anchor_lang::solana_program::program::invoke_signed(
            &anchor_lang::solana_program::system_instruction::assign(pda.key, &DELEGATION_PROGRAM_ID),
            &[pda.clone(), system_program.clone()],
            signer,
        )?;
    }
    let args = DelegateAccountArgs {
        commit_frequency_ms: config.commit_frequency_ms,
        seeds: pda_seeds.iter().map(|s| s.to_vec()).collect(),
        validator: config.validator,
    };
    cpi_delegate(
        &payer.to_account_info(),
        pda,
        owner_program,
        buffer,
        delegation_record,
        delegation_metadata,
        system_program,
        signer,
        args,
    )
    .map_err(|_| error!(MarketError::BadGrid))?;
    let tag = ephemeral_rollups_sdk::pda::DELEGATE_BUFFER_TAG;
    let (buf_expected, buf_bump) = Pubkey::find_program_address(&[tag, pda.key.as_ref()], &crate::ID);
    require_keys_eq!(buf_expected, buffer.key(), MarketError::IdMismatch);
    let buf_bump_slice = [buf_bump];
    let buf_seeds: &[&[u8]] = &[tag, pda.key.as_ref(), buf_bump_slice.as_slice()];
    close_pda_with_system_transfer(buffer, &[buf_seeds], &payer.to_account_info(), system_program)
        .map_err(|_| error!(MarketError::BadGrid))?;
    Ok(())
}

fn invoke_magic_commit<'info>(
    payer: &Signer<'info>,
    magic_context: &AccountInfo<'info>,
    magic_program: &AccountInfo<'info>,
    accounts: &[AccountInfo<'info>],
    undelegate: bool,
) -> Result<()> {
    if !magic_program.executable {
        return Ok(());
    }
    let builder = MagicIntentBundleBuilder::new(
        payer.to_account_info(),
        magic_context.clone(),
        magic_program.clone(),
    );
    if undelegate {
        builder.commit_and_undelegate(accounts).build_and_invoke()?;
    } else {
        builder.commit(accounts).build_and_invoke()?;
    }
    Ok(())
}

fn require_book_keys(
    authority: &Pubkey,
    market: &Market,
    committee: &Committee,
    committee_key: &Pubkey,
) -> Result<()> {
    require_live_committee(committee)?;
    require!(
        *authority == market.creator || committee.is_member(authority),
        MarketError::NotResolver
    );
    require!(*committee_key == market.committee, MarketError::BadCommittee);
    Ok(())
}

fn require_live_committee(committee: &Committee) -> Result<()> {
    require!(
        committee.member_count >= 1
            && committee.m >= 1
            && committee.m <= committee.member_count,
        MarketError::BadCommittee
    );
    Ok(())
}

fn write_roster(
    committee: &mut Committee,
    authority: Pubkey,
    bump: u8,
    members: Vec<Pubkey>,
    m: u8,
) -> Result<()> {
    let n = members.len();
    require!(
        n >= 1 && n <= MAX_COMMITTEE && m >= 1 && (m as usize) <= n,
        MarketError::BadCommittee
    );
    for (i, member) in members.iter().enumerate() {
        require!(*member != Pubkey::default(), MarketError::BadCommittee);
        for prev in members.iter().take(i) {
            require!(member != prev, MarketError::BadCommittee);
        }
    }
    committee.authority = authority;
    committee.members = [Pubkey::default(); MAX_COMMITTEE];
    for (i, member) in members.iter().enumerate() {
        committee.members[i] = *member;
    }
    committee.member_count = n as u8;
    committee.m = m;
    committee.bump = bump;
    if committee.epoch == 0 {
        committee.epoch = 1;
    }
    Ok(())
}

fn settle_vault_fill<'info>(
    session: &mut Option<Box<Account<'info, ProtocolSession>>>,
    owner: AccountInfo<'info>,
    market: AccountInfo<'info>,
    board: AccountInfo<'info>,
    user_vault: AccountInfo<'info>,
    vault_program: AccountInfo<'info>,
    remaining: &[AccountInfo],
    pos: &mut Position,
    id_hash: [u8; 32],
    bump: u8,
    is_buy: bool,
    cost_usdc: u64,
    fee_usdc: u64,
    trader_key: Pubkey,
    owner_key: Pubkey,
) -> Result<()> {
    if is_buy && (cost_usdc > 0 || fee_usdc > 0) {
        if trader_key != owner_key {
            let s = session
                .as_mut()
                .ok_or_else(|| error!(MarketError::SessionUnauthorized))?;
            let need = cost_usdc.saturating_add(fee_usdc);
            require!(s.remaining_usdc >= need, MarketError::SessionUnauthorized);
            s.remaining_usdc = s.remaining_usdc.saturating_sub(need);
        }
        let need = cost_usdc.saturating_add(fee_usdc);
        require_user_vault_cover(&user_vault, &owner_key, need)?;
        if executing_on_er(remaining) {
            pos.vault_owed_cost = pos.vault_owed_cost.saturating_add(cost_usdc);
            pos.vault_owed_fee = pos.vault_owed_fee.saturating_add(fee_usdc);
        } else {
            charge_vault(
                vault_program,
                owner,
                market,
                board,
                user_vault,
                id_hash,
                bump,
                cost_usdc,
                fee_usdc,
            )?;
        }
        return Ok(());
    }
    if !is_buy && cost_usdc > 0 {
        if executing_on_er(remaining) {
            let against = pos.vault_owed_cost.min(cost_usdc);
            pos.vault_owed_cost -= against;
            pos.vault_owed_credit = pos
                .vault_owed_credit
                .saturating_add(cost_usdc.saturating_sub(against));
        } else {
            refund_vault(vault_program, owner, market, board, user_vault, id_hash, bump, cost_usdc)?;
        }
    }
    Ok(())
}

fn charge_vault<'info>(
    vault_program: AccountInfo<'info>,
    owner: AccountInfo<'info>,
    market: AccountInfo<'info>,
    board: AccountInfo<'info>,
    user: AccountInfo<'info>,
    id_hash: [u8; 32],
    bump: u8,
    cost: u64,
    fee: u64,
) -> Result<()> {
    let bump_seed = [bump];
    let seeds: &[&[u8]] = &[MARKET_SEED, id_hash.as_ref(), &bump_seed];
    credit_trade(
        CpiContext::new_with_signer(
            vault_program,
            CreditTrade {
                market,
                owner,
                board,
                user,
            },
            &[seeds],
        ),
        cost,
        fee,
    )
}

fn refund_vault<'info>(
    vault_program: AccountInfo<'info>,
    owner: AccountInfo<'info>,
    market: AccountInfo<'info>,
    board: AccountInfo<'info>,
    user: AccountInfo<'info>,
    id_hash: [u8; 32],
    bump: u8,
    cost: u64,
) -> Result<()> {
    let bump_seed = [bump];
    let seeds: &[&[u8]] = &[MARKET_SEED, id_hash.as_ref(), &bump_seed];
    refund_trade(
        CpiContext::new_with_signer(
            vault_program,
            CreditTrade {
                market,
                owner,
                board,
                user,
            },
            &[seeds],
        ),
        cost,
    )
}

fn lock_layers(common: &CreateCommon) -> Result<()> {
    require!(common.n_layers >= 1 && common.n_layers <= 8, MarketError::BadLayers);
    require!(common.d_unit > 0, MarketError::BadCapital);
    require!(common.gamma_bps <= 10_000, MarketError::BadFee);
    Ok(())
}

#[derive(Accounts)]
#[instruction(id_hash: [u8; 32], n: u16)]
pub struct CreateBoard<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(
        init,
        payer = creator,
        space = Market::SIZE,
        seeds = [MARKET_SEED, id_hash.as_ref()],
        bump
    )]
    pub market: Account<'info, Market>,
    #[account(
        init,
        payer = creator,
        space = Grid::space(Grid::shard_len(n, 0) as usize),
        seeds = [GRID_SEED, market.key().as_ref()],
        bump
    )]
    pub grid: Account<'info, Grid>,
    #[account(
        seeds = [PROTOCOL_SEED],
        bump = protocol.bump
    )]
    pub protocol: Account<'info, Protocol>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct GrowGrid<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(mut, has_one = creator)]
    pub market: Account<'info, Market>,
    /// CHECK: PDA grid. Grow resizes; write/seal parse Borsh without holding four n-vecs on the heap.
    #[account(
        mut,
        seeds = [GRID_SEED, market.key().as_ref()],
        bump,
    )]
    pub grid: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(ix: u8)]
pub struct CreateGridShard<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(mut, has_one = creator)]
    pub market: Account<'info, Market>,
    #[account(
        init,
        payer = creator,
        space = Grid::space(Grid::SHARD_CELLS as usize),
        seeds = [GRID_SEED, market.key().as_ref(), &[ix]],
        bump
    )]
    pub shard: Account<'info, Grid>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(ix: u8)]
pub struct WriteGridShard<'info> {
    pub creator: Signer<'info>,
    #[account(has_one = creator)]
    pub market: Account<'info, Market>,
    /// CHECK: extra grid shard PDA.
    #[account(
        mut,
        seeds = [GRID_SEED, market.key().as_ref(), &[ix]],
        bump
    )]
    pub shard: UncheckedAccount<'info>,
}

#[delegate]
#[derive(Accounts)]
pub struct DelegateShard<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: market key for shard seeds.
    pub market: AccountInfo<'info>,
    /// CHECK: one grid shard.
    #[account(mut, del)]
    pub shard: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct CommitShard<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: shard to commit / undelegate.
    #[account(mut)]
    pub shard: AccountInfo<'info>,
    /// CHECK: MagicBlock magic program.
    #[account(address = MAGIC_PROGRAM_ID)]
    pub magic_program: AccountInfo<'info>,
    /// CHECK: MagicBlock magic context.
    #[account(mut, address = MAGIC_CONTEXT_ID)]
    pub magic_context: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct OpenSession<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        init_if_needed,
        payer = owner,
        space = ProtocolSession::SIZE,
        seeds = [SESSION_SEED, owner.key().as_ref()],
        bump
    )]
    pub session: Account<'info, ProtocolSession>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MutSession<'info> {
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [SESSION_SEED, owner.key().as_ref()],
        bump = session.bump,
        constraint = session.owner == owner.key() @ MarketError::SessionUnauthorized
    )]
    pub session: Account<'info, ProtocolSession>,
}

#[derive(Accounts)]
#[instruction(set_hash: [u8; 32])]
pub struct OpenSeat<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    /// CHECK: position owner. Seeds bind to this key.
    pub owner: UncheckedAccount<'info>,
    /// CHECK: market key for PDA seeds. May be DLP-owned on L1 after `delegate_book`.
    pub market: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        payer = trader,
        space = Position::SIZE,
        seeds = [POS_SEED, market.key().as_ref(), owner.key().as_ref(), set_hash.as_ref()],
        bump
    )]
    pub position: Account<'info, Position>,
    #[account(
        init_if_needed,
        payer = trader,
        space = FillNonce::SIZE,
        seeds = [NONCE_SEED, owner.key().as_ref(), market.key().as_ref()],
        bump
    )]
    pub nonce_acc: Account<'info, FillNonce>,
    pub system_program: Program<'info, System>,
}

#[delegate]
#[derive(Accounts)]
#[instruction(set_hash: [u8; 32])]
pub struct DelegateSeat<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    /// CHECK: position owner.
    pub owner: UncheckedAccount<'info>,
    /// CHECK: market key for PDA seeds. After Delegate, L1 owner is DLP.
    pub market: UncheckedAccount<'info>,
    /// CHECK: position PDA delegated for ER fills.
    #[account(
        mut,
        del,
        seeds = [POS_SEED, market.key().as_ref(), owner.key().as_ref(), set_hash.as_ref()],
        bump
    )]
    pub position: AccountInfo<'info>,
    /// CHECK: fill nonce PDA delegated with the position.
    #[account(
        mut,
        del,
        seeds = [NONCE_SEED, owner.key().as_ref(), market.key().as_ref()],
        bump
    )]
    pub nonce_acc: AccountInfo<'info>,
}

#[derive(Accounts)]
#[instruction(set_hash: [u8; 32])]
pub struct CommitSeat<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    /// CHECK: position owner.
    pub owner: UncheckedAccount<'info>,
    /// CHECK: market key for PDA seeds.
    pub market: UncheckedAccount<'info>,
    /// CHECK: delegated position committed / undelegated with the nonce.
    #[account(
        mut,
        seeds = [POS_SEED, market.key().as_ref(), owner.key().as_ref(), set_hash.as_ref()],
        bump
    )]
    pub position: AccountInfo<'info>,
    /// CHECK: delegated fill nonce.
    #[account(
        mut,
        seeds = [NONCE_SEED, owner.key().as_ref(), market.key().as_ref()],
        bump
    )]
    pub nonce_acc: AccountInfo<'info>,
    /// CHECK: MagicBlock magic program.
    #[account(address = MAGIC_PROGRAM_ID)]
    pub magic_program: AccountInfo<'info>,
    /// CHECK: MagicBlock magic context.
    #[account(mut, address = MAGIC_CONTEXT_ID)]
    pub magic_context: AccountInfo<'info>,
}

#[derive(Accounts)]
#[instruction(set_hash: [u8; 32])]
pub struct SyncVault<'info> {
    pub trader: Signer<'info>,
    /// CHECK: vault / position owner.
    pub owner: UncheckedAccount<'info>,
    #[account(
        mut,
        seeds = [MARKET_SEED, market.id_hash.as_ref()],
        bump = market.bump
    )]
    pub market: Account<'info, Market>,
    #[account(
        mut,
        seeds = [POS_SEED, market.key().as_ref(), owner.key().as_ref(), set_hash.as_ref()],
        bump = position.bump
    )]
    pub position: Account<'info, Position>,
    /// CHECK: vault board. L1 client marks writable.
    #[account(
        mut,
        seeds = [vault::BOARD_SEED, market.key().as_ref()],
        bump,
        seeds::program = vault::ID
    )]
    pub board: UncheckedAccount<'info>,
    /// CHECK: user vault. L1 client marks writable.
    #[account(
        mut,
        seeds = [vault::USER_SEED, owner.key().as_ref()],
        bump,
        seeds::program = vault::ID
    )]
    pub user_vault: UncheckedAccount<'info>,
    pub vault_program: Program<'info, vault::program::Vault>,
}

#[delegate]
#[derive(Accounts)]
pub struct DelegateSession<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    /// CHECK: session PDA delegated for ER remaining_usdc writes.
    #[account(
        mut,
        del,
        seeds = [SESSION_SEED, owner.key().as_ref()],
        bump
    )]
    pub session: AccountInfo<'info>,
}

#[derive(Accounts, Session)]
#[instruction(set_mask: Vec<u8>)]
pub struct Trade<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    /// CHECK: main wallet. Position and vault seeds bind to this key.
    pub owner: UncheckedAccount<'info>,
    /// Protocol limit PDA (remaining_usdc / allowed_ix). Optional when trader == owner.
    #[account(mut)]
    pub session: Option<Box<Account<'info, ProtocolSession>>>,
    /// MagicBlock SessionTokenV2. Optional; when absent, `#[session_auth_or]` falls back to owner or protocol Session.
    #[session(signer = trader, authority = owner.key())]
    pub session_token: Option<Account<'info, SessionTokenV2>>,
    #[account(
        mut,
        seeds = [MARKET_SEED, market.id_hash.as_ref()],
        bump = market.bump
    )]
    pub market: Box<Account<'info, Market>>,
    /// CHECK: PDA grid. Write/seal/fill mutate Borsh in place; BPF bump heap is 32KiB.
    #[account(
        mut,
        seeds = [GRID_SEED, market.key().as_ref()],
        bump
    )]
    pub grid: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        payer = trader,
        space = Position::SIZE,
        seeds = [POS_SEED, market.key().as_ref(), owner.key().as_ref(), ids::set_hash(&set_mask).as_ref()],
        bump
    )]
    pub position: Box<Account<'info, Position>>,
    /// CHECK: vault board PDA. L1 client marks writable; ER client leaves readonly.
    #[account(
        seeds = [vault::BOARD_SEED, market.key().as_ref()],
        bump,
        seeds::program = vault::ID
    )]
    pub board: UncheckedAccount<'info>,
    /// CHECK: user vault PDA. Cover is checked in fill. Never Delegated.
    #[account(
        seeds = [vault::USER_SEED, owner.key().as_ref()],
        bump,
        seeds::program = vault::ID
    )]
    pub user_vault: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        payer = trader,
        space = FillNonce::SIZE,
        seeds = [NONCE_SEED, owner.key().as_ref(), market.key().as_ref()],
        bump
    )]
    pub nonce_acc: Box<Account<'info, FillNonce>>,
    pub vault_program: Program<'info, vault::program::Vault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts, Session)]
#[instruction(contract: SkellamContract)]
pub struct TradeSkellam<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    /// CHECK: main wallet. Position and vault seeds bind to this key.
    pub owner: UncheckedAccount<'info>,
    #[account(mut)]
    pub session: Option<Box<Account<'info, ProtocolSession>>>,
    #[session(signer = trader, authority = owner.key())]
    pub session_token: Option<Account<'info, SessionTokenV2>>,
    #[account(
        mut,
        seeds = [MARKET_SEED, market.id_hash.as_ref()],
        bump = market.bump
    )]
    pub market: Box<Account<'info, Market>>,
    /// CHECK: PDA grid. Write/seal/fill mutate Borsh in place; BPF bump heap is 32KiB.
    #[account(
        mut,
        seeds = [GRID_SEED, market.key().as_ref()],
        bump
    )]
    pub grid: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        payer = trader,
        space = Position::SIZE,
        seeds = [
            POS_SEED,
            market.key().as_ref(),
            owner.key().as_ref(),
            ids::skellam_ticket(contract.ticket_key().0, contract.ticket_key().1, contract.ticket_key().2).as_ref()
        ],
        bump
    )]
    pub position: Box<Account<'info, Position>>,
    /// CHECK: vault board PDA. L1 client marks writable; ER client leaves readonly.
    #[account(
        seeds = [vault::BOARD_SEED, market.key().as_ref()],
        bump,
        seeds::program = vault::ID
    )]
    pub board: UncheckedAccount<'info>,
    /// CHECK: user vault PDA. Cover is checked in fill. Never Delegated.
    #[account(
        seeds = [vault::USER_SEED, owner.key().as_ref()],
        bump,
        seeds::program = vault::ID
    )]
    pub user_vault: UncheckedAccount<'info>,
    #[account(
        init_if_needed,
        payer = trader,
        space = FillNonce::SIZE,
        seeds = [NONCE_SEED, owner.key().as_ref(), market.key().as_ref()],
        bump
    )]
    pub nonce_acc: Box<Account<'info, FillNonce>>,
    pub vault_program: Program<'info, vault::program::Vault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Halt<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [MARKET_SEED, market.id_hash.as_ref()],
        bump = market.bump
    )]
    pub market: Account<'info, Market>,
    #[account(seeds = [COMMITTEE_SEED], bump = committee.bump)]
    pub committee: Account<'info, Committee>,
}

#[delegate]
#[derive(Accounts)]
pub struct DelegateBook<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    /// CHECK: PDA checked from deserialized `Market.id_hash` before Delegation CPI.
    #[account(mut, del)]
    pub market: AccountInfo<'info>,
    /// CHECK: grid buffer PDA; delegated with the market.
    #[account(
        mut,
        del,
        seeds = [GRID_SEED, market.key().as_ref()],
        bump
    )]
    pub grid: AccountInfo<'info>,
    #[account(seeds = [COMMITTEE_SEED], bump = committee.bump)]
    pub committee: Account<'info, Committee>,
}

#[derive(Accounts)]
pub struct PrepareDelegateBuffer<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: account whose data length the buffer must match.
    pub source: AccountInfo<'info>,
    /// CHECK: MagicBlock delegate buffer PDA (`["buffer", source]`).
    #[account(
        mut,
        seeds = [ephemeral_rollups_sdk::pda::DELEGATE_BUFFER_TAG, source.key().as_ref()],
        bump
    )]
    pub buffer: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct PrepareGridDump<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: market header supplies `n` for dump length.
    pub market: AccountInfo<'info>,
    /// CHECK: market-owned copy of the ER grid (`["gdump", market]`).
    #[account(mut, seeds = [GRID_DUMP_SEED, market.key().as_ref()], bump)]
    pub dump: AccountInfo<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WriteGridDump<'info> {
    pub payer: Signer<'info>,
    /// CHECK: dump PDA grown by `prepare_grid_dump`.
    #[account(mut, seeds = [GRID_DUMP_SEED, market.key().as_ref()], bump)]
    pub dump: AccountInfo<'info>,
    /// CHECK: seeds only.
    pub market: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct CommitBook<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [MARKET_SEED, market.id_hash.as_ref()],
        bump = market.bump
    )]
    pub market: Account<'info, Market>,
    /// CHECK: grid buffer PDA committed with the market.
    #[account(
        mut,
        seeds = [GRID_SEED, market.key().as_ref()],
        bump = market.grid_bump
    )]
    pub grid: AccountInfo<'info>,
    #[account(seeds = [COMMITTEE_SEED], bump = committee.bump)]
    pub committee: Account<'info, Committee>,
    /// CHECK: MagicBlock magic program. CPI is skipped when it is not executable (L1 tests).
    #[account(address = MAGIC_PROGRAM_ID)]
    pub magic_program: AccountInfo<'info>,
    /// CHECK: MagicBlock magic context. Present on ER; empty on L1 is fine.
    #[account(mut, address = MAGIC_CONTEXT_ID)]
    pub magic_context: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct InitCommittee<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = Committee::SIZE,
        seeds = [COMMITTEE_SEED],
        bump
    )]
    pub committee: Account<'info, Committee>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitProtocol<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = Protocol::SIZE,
        seeds = [PROTOCOL_SEED],
        bump
    )]
    pub protocol: Account<'info, Protocol>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetProtocol<'info> {
    pub authority: Signer<'info>,
    #[account(mut, seeds = [PROTOCOL_SEED], bump = protocol.bump)]
    pub protocol: Account<'info, Protocol>,
}

#[derive(Accounts)]
pub struct SetRoster<'info> {
    pub authority: Signer<'info>,
    #[account(mut, seeds = [COMMITTEE_SEED], bump = committee.bump)]
    pub committee: Account<'info, Committee>,
}

#[error_code]
pub enum MarketError {
    #[msg("id_hash does not match the locked identity")]
    IdMismatch,
    #[msg("grid size or family atom count is illegal")]
    BadGrid,
    #[msg("delegate buffer is missing or smaller than the source account")]
    BufferNotReady,
    #[msg("prior parameters are illegal")]
    BadPrior,
    #[msg("beta must be > 0")]
    BadBeta,
    #[msg("platform or d_unit is illegal")]
    BadCapital,
    #[msg("fee_bps must be <= 10000")]
    BadFee,
    #[msg("close_ts / risk_lock_ts / kickoff relationship is illegal")]
    BadClock,
    #[msg("set mask is empty or the wrong length")]
    EmptySet,
    #[msg("set mask has bits outside the grid")]
    BadMask,
    #[msg("q must be > 0")]
    ZeroQty,
    #[msg("market is not TRADING")]
    NotTrading,
    #[msg("now >= close_ts")]
    Closed,
    #[msg("grid account does not belong to this market")]
    WrongGrid,
    #[msg("instruction is not valid for this distribution family")]
    WrongFamily,
    #[msg("sell exceeds inventory on this set")]
    NoInventory,
    #[msg("arithmetic overflow")]
    Overflow,
    #[msg("committee roster or M/N is illegal")]
    BadCommittee,
    #[msg("n_layers / D_unit / gamma at listing is illegal")]
    BadLayers,
    #[msg("signer is not the creator or a roster member")]
    NotResolver,
    #[msg("session is missing, expired, revoked, or not allowed for this ix")]
    SessionUnauthorized,
    #[msg("a live session already exists; revoke or wait for expiry")]
    SessionLive,
    #[msg("nonce is not the next value (or a same-nonce retry of another fill)")]
    NonceReplay,
    #[msg("grid P0 is not sealed; grow_grid / write_grid_mass / seal_grid")]
    GridNotReady,
    #[msg("market is delegated; L1 fills are ER-only")]
    Delegated,
    #[msg("market is already delegated")]
    AlreadyDelegated,
    #[msg("market is not delegated")]
    NotDelegated,
    #[msg("undelegate only after halt or close_ts")]
    StillOpen,
    #[msg("user vault account is missing or owner mismatch")]
    BadVault,
    #[msg("user vault available is below fill cost")]
    InsufficientVault,
    #[msg("another wide fill is in progress on this market")]
    WideFillBusy,
    #[msg("sells are closed; inventory stays until settlement claim")]
    SellsClosed,
    #[msg("create fields must match the official protocol account")]
    NotOfficial,
}

#[cfg(test)]
mod tests {
    use super::ids;
    use super::mask;
    use math::prior;
    use math::Q64;

    #[test]
    fn skellam_n_is_121() {
        let p = prior::independent_poisson_2d(10, Q64::from_int(1), Q64::from_int(1));
        assert_eq!(p.len(), 121);
    }

    #[test]
    fn bernoulli_id_stable() {
        let t = [9u8; 32];
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_eq!(ids::bernoulli(&t, &a), ids::bernoulli(&t, &a));
        assert_ne!(ids::bernoulli(&t, &a), ids::bernoulli(&t, &b));
    }

    #[test]
    fn mask_roundtrip_bits() {
        let m = mask::decode(&[0b0000_0011], 2).unwrap();
        assert_eq!(m, vec![true, true]);
    }

    #[test]
    fn sells_are_closed() {
        assert!(super::require_buy(true).is_ok());
        assert!(super::require_buy(false).is_err());
    }

    #[test]
    fn skellam_home_and_exact_share_cell() {
        let home = math::football::mask_home(10);
        let exact = math::football::mask_exact(10, 2, 1);
        assert!(home[math::football::cell(2, 1, 10)]);
        assert!(exact[math::football::cell(2, 1, 10)]);
        assert!(!exact[math::football::cell(1, 0, 10)]);
    }

    #[test]
    fn shared_roster_rejects_duplicates() {
        use super::{write_roster, Committee};
        use anchor_lang::prelude::Pubkey;
        let a = Pubkey::new_from_array([1u8; 32]);
        let mut c = Committee {
            authority: a,
            members: [Pubkey::default(); super::MAX_COMMITTEE],
            member_count: 0,
            m: 0,
            bump: 1,
            epoch: 0,
        };
        assert!(write_roster(&mut c, a, 1, vec![a, Pubkey::new_from_array([2u8; 32])], 2).is_ok());
        assert_eq!(c.member_count, 2);
        assert!(write_roster(&mut c, a, 1, vec![a, a], 1).is_err());
        assert!(write_roster(&mut c, a, 1, vec![a], 2).is_err());
    }

    #[test]
    fn layers_are_locked_at_create() {
        use super::{lock_layers, CreateCommon};
        use anchor_lang::prelude::Pubkey;
        let a = Pubkey::new_from_array([1u8; 32]);
        let mut common = CreateCommon {
            id_hash: [0; 32],
            n: 2,
            close_ts: 10,
            risk_lock_ts: 10,
            beta: 1,
            c_m: 1,
            fee_bps: 0,
            fee_timing: 0,
            authorized_reporter: Pubkey::default(),
            report_window_secs: 60,
            challenge_secs: 30,
            n_layers: 3,
            d_unit: 10_000,
            gamma_bps: 1_000,
            alpha_r_bps: 7_000,
            platform: a,
            report_open_ts: 10,
            committee_bond: 1,
        };
        assert!(lock_layers(&common).is_ok());
        common.n_layers = 0;
        assert!(lock_layers(&common).is_err());
        common.n_layers = 3;
        common.d_unit = 0;
        assert!(lock_layers(&common).is_err());
        common.d_unit = 10_000;
        common.gamma_bps = 10_001;
        assert!(lock_layers(&common).is_err());
    }

    #[test]
    fn report_open_cannot_precede_close() {
        use super::{lock_clocks, CreateCommon};
        use anchor_lang::prelude::Pubkey;
        let a = Pubkey::new_from_array([1u8; 32]);
        let mut common = CreateCommon {
            id_hash: [0; 32],
            n: 2,
            close_ts: 10,
            risk_lock_ts: 10,
            beta: 1,
            c_m: 1,
            fee_bps: 0,
            fee_timing: 0,
            authorized_reporter: Pubkey::default(),
            report_window_secs: 60,
            challenge_secs: 30,
            n_layers: 1,
            d_unit: 10_000,
            gamma_bps: 0,
            alpha_r_bps: 7_000,
            platform: a,
            report_open_ts: 10,
            committee_bond: 1,
        };
        assert!(lock_clocks(&common).is_ok());
        common.report_open_ts = 0;
        assert!(lock_clocks(&common).is_err());
        common.report_open_ts = 9;
        assert!(lock_clocks(&common).is_err());
    }
}
