//! Market program: create by distribution family, `p0` once, L1 `buy_set`/`sell_set`.
//! Listing names (CPI, election, BTC) are metadata, not instructions.
//! Session PDA authorizes in-board fills (FR-WAL-04–06). After `delegate_book`, L1 fills return `Delegated`.

use anchor_lang::prelude::*;
use math::prior;
use math::settle::{usdc, usdc_charge};
use math::Q64;
use vault::cpi::accounts::CreditTrade;
use vault::cpi::credit_trade;

pub mod ids;
pub mod mask;
pub mod session;
pub mod state;

use session::{
    check_trader, is_replay, require_next, FillNonce, Session, IX_BUY_SET, IX_BUY_SKELLAM, IX_SELL_SET,
    IX_SELL_SKELLAM, IX_ALL_TRADES, NONCE_SEED, SESSION_LIVE, SESSION_REVOKED, SESSION_SEED,
};
use state::*;

declare_id!("Market1111111111111111111111111111111111111");

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

    /// L1 path used before Delegate. Trader may be the owner or a live Session.
    pub fn buy_set(mut ctx: Context<Trade>, set_mask: Vec<u8>, q_raw: i128, nonce: u64) -> Result<()> {
        fill(&mut ctx, &set_mask, q_raw, nonce, true)
    }

    pub fn sell_set(mut ctx: Context<Trade>, set_mask: Vec<u8>, q_raw: i128, nonce: u64) -> Result<()> {
        fill(&mut ctx, &set_mask, q_raw, nonce, false)
    }

    /// 1X2 / handicap / totals / exact score on the shared Skellam grid.
    pub fn buy_skellam_set(
        mut ctx: Context<TradeSkellam>,
        contract: SkellamContract,
        q_raw: i128,
        nonce: u64,
    ) -> Result<()> {
        fill_skellam(&mut ctx, contract, q_raw, nonce, true)
    }

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

    /// Creator or roster member stops fills (early YES, VOID, or after close).
    pub fn halt(ctx: Context<Halt>) -> Result<()> {
        require_book_authority(&ctx)?;
        let market = &mut ctx.accounts.market;
        require!(market.status == Status::Trading as u8, MarketError::NotTrading);
        market.status = Status::Halted as u8;
        Ok(())
    }

    /// Marks the board delegated. Further L1 `buy_set` / `sell_set` fail until undelegate.
    pub fn delegate_book(ctx: Context<Halt>) -> Result<()> {
        require_book_authority(&ctx)?;
        let market = &mut ctx.accounts.market;
        require!(market.status == Status::Trading as u8, MarketError::NotTrading);
        require!(!market.delegated, MarketError::AlreadyDelegated);
        market.delegated = true;
        Ok(())
    }

    /// Writes a journal checkpoint. Does not move Vault USDC (FR-DUR-02).
    pub fn commit_book(ctx: Context<Halt>, trades_root: [u8; 32]) -> Result<()> {
        require_book_authority(&ctx)?;
        let market = &mut ctx.accounts.market;
        require!(market.delegated, MarketError::NotDelegated);
        market.trades_root = trades_root;
        market.commit_ts = Clock::get()?.unix_timestamp;
        Ok(())
    }

    /// Clears `delegated` after halt or `close_ts` so L1 can settle.
    pub fn undelegate_book(ctx: Context<Halt>) -> Result<()> {
        require_book_authority(&ctx)?;
        let market = &mut ctx.accounts.market;
        require!(market.delegated, MarketError::NotDelegated);
        let now = Clock::get()?.unix_timestamp;
        require!(
            market.status == Status::Halted as u8 || now >= market.close_ts,
            MarketError::StillOpen
        );
        market.delegated = false;
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
    require!(common.fee_bps <= 10_000, MarketError::BadFee);
    require!(
        common.fee_timing == crate::state::FEE_ON_FILL || common.fee_timing == crate::state::FEE_ON_CLAIM,
        MarketError::BadFee
    );
    require!(now < common.close_ts, MarketError::BadClock);
    require!(common.risk_lock_ts <= common.close_ts, MarketError::BadClock);
    lock_clocks(common)?;
    lock_layers(common)?;
    require!(common.alpha_r_bps <= 10_000, MarketError::BadFee);
    require!(common.platform != Pubkey::default(), MarketError::BadCapital);

    let market = &mut ctx.accounts.market;
    market.family = family as u8;
    market.status = Status::Trading as u8;
    market.bump = ctx.bumps.market;
    market.grid_bump = ctx.bumps.grid;
    market.n = common.n;
    market.fee_bps = common.fee_bps;
    market.fee_timing = common.fee_timing;
    market.creator = ctx.accounts.creator.key();
    market.committee = Pubkey::find_program_address(&[COMMITTEE_SEED], &crate::ID).0;
    market.authorized_reporter = common.authorized_reporter;
    market.close_ts = common.close_ts;
    market.risk_lock_ts = common.risk_lock_ts;
    market.report_window_secs = common.report_window_secs;
    market.challenge_secs = common.challenge_secs;
    market.n_layers = common.n_layers;
    market.d_unit = common.d_unit;
    market.gamma_bps = if common.gamma_bps == 0 { 1_000 } else { common.gamma_bps };
    market.beta = common.beta;
    market.c_m = 0;
    market.fees_accrued = 0;
    market.trading_revenue = 0;
    market.alpha_r_bps = common.alpha_r_bps;
    market.platform = common.platform;
    market.l_max = 0;
    market.id_hash = common.id_hash;
    market.extra = extra;
    market.delegated = false;
    market.trades_root = [0u8; 32];
    market.commit_ts = 0;

    let grid = &mut ctx.accounts.grid;
    grid.market = market.key();
    grid.n = common.n;
    grid.bump = ctx.bumps.grid;
    if Grid::space(p0.len()) <= Grid::CREATE_CAP {
        write_grid_prior(grid, &p0);
    } else {
        require!(
            family == Family::Gaussian || family == Family::Lognormal || family == Family::Dirichlet,
            MarketError::BadGrid
        );
        grid.p0 = Vec::new();
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

const GRID_HDR: usize = 8 + 32 + 2 + 1 + 16;

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

fn grid_header(data: &[u8]) -> Result<(Pubkey, u16, u8, i128)> {
    require!(data.len() >= GRID_HDR + 4, MarketError::BadGrid);
    require!(&data[..8] == Grid::DISCRIMINATOR, MarketError::BadGrid);
    let market_bytes: [u8; 32] = data[8..40].try_into().map_err(|_| error!(MarketError::BadGrid))?;
    let n_bytes: [u8; 2] = data[40..42].try_into().map_err(|_| error!(MarketError::BadGrid))?;
    Ok((
        Pubkey::from(market_bytes),
        u16::from_le_bytes(n_bytes),
        data[42],
        grid_i128(data, 43)?,
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
    let (g_market, g_n, _, z) = grid_header(data)?;
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
    let z0 = Q64::from_raw(grid_i128(data, 43)?);
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
    set_grid_i128(data, 43, z_now.raw())?;
    Ok(cost)
}

fn expand_grid(ctx: Context<GrowGrid>) -> Result<()> {
    let n = ctx.accounts.market.n as usize;
    require!((2..=MAX_N).contains(&(n as u16)), MarketError::BadGrid);
    let info = ctx.accounts.grid.to_account_info();
    {
        let data = info.try_borrow_data()?;
        let (g_market, g_n, _, _) = grid_header(&data)?;
        require!(g_market == ctx.accounts.market.key(), MarketError::WrongGrid);
        require!(g_n == ctx.accounts.market.n, MarketError::WrongGrid);
    }
    let need = Grid::space(n);
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

fn append_interval_mass(ctx: Context<GrowGrid>) -> Result<()> {
    let n = ctx.accounts.market.n as usize;
    require!((2..=MAX_N).contains(&(n as u16)), MarketError::BadGrid);
    let info = ctx.accounts.grid.to_account_info();
    require!(info.data_len() >= Grid::space(n), MarketError::GridNotReady);
    let family = ctx.accounts.market.family;
    let kn = if family == Family::Dirichlet as u8 {
        None
    } else {
        let (lo, hi) = interval_log_bounds(&ctx.accounts.market)?;
        let mu = Q64::from_raw(ctx.accounts.market.extra.c);
        let sigma = Q64::from_raw(ctx.accounts.market.extra.d);
        Some(prior::TruncatedNormal::new(n, lo, hi, mu, sigma))
    };
    let mut data = info.try_borrow_mut_data()?;
    let (g_market, g_n, _, z) = grid_header(&data)?;
    require!(g_market == ctx.accounts.market.key(), MarketError::WrongGrid);
    require!(g_n == ctx.accounts.market.n, MarketError::WrongGrid);
    let (p0_len, theta_len, exp_len, w_len) = grid_vec_lens(&data)?;
    if p0_len == n && w_len == n && z != 0 {
        return Ok(());
    }
    require!(
        z == 0 && w_len == 0 && theta_len == 0 && exp_len == 0,
        MarketError::BadGrid
    );
    if p0_len >= n {
        return Ok(());
    }
    let start = p0_len;
    let end = (start + Grid::PRIOR_CHUNK).min(n);
    let old_tail = GRID_HDR + 4 + start * 16;
    let new_tail = GRID_HDR + 4 + end * 16;
    require!(data.len() >= new_tail + 12, MarketError::BadGrid);
    data.copy_within(old_tail..old_tail + 12, new_tail);
    data[GRID_HDR..GRID_HDR + 4].copy_from_slice(&(end as u32).to_le_bytes());
    let p0_off = GRID_HDR + 4;
    if family == Family::Dirichlet as u8 {
        match ctx.accounts.market.extra.u0 {
            DIRICHLET_SIMPLEX => {
                let alphas = dirichlet_alphas(&ctx.accounts.market)?;
                let bins = ctx.accounts.market.extra.e as usize;
                let chunk = prior::simplex_chunk_raw(alphas.len(), bins, &alphas, start, end);
                require!(chunk.len() == end - start, MarketError::BadGrid);
                for (i, raw) in chunk.iter().enumerate() {
                    set_grid_i128(&mut data, p0_off + (start + i) * 16, *raw)?;
                }
            }
            DIRICHLET_TOP_N => {
                for i in start..end {
                    set_grid_i128(&mut data, p0_off + i * 16, Q64::ONE.raw())?;
                }
            }
            _ => return err!(MarketError::BadGrid),
        }
        return Ok(());
    }
    let kn = kn.ok_or_else(|| error!(MarketError::BadGrid))?;
    for i in start..end {
        set_grid_i128(&mut data, p0_off + i * 16, kn.weight_at(i).raw())?;
    }
    Ok(())
}

fn seal_interval_p0(ctx: Context<GrowGrid>) -> Result<()> {
    let n = ctx.accounts.market.n as usize;
    require!((2..=MAX_N).contains(&(n as u16)), MarketError::BadGrid);
    let info = ctx.accounts.grid.to_account_info();
    require!(info.data_len() >= Grid::space(n), MarketError::GridNotReady);
    let mut data = info.try_borrow_mut_data()?;
    let (g_market, g_n, _, z) = grid_header(&data)?;
    require!(g_market == ctx.accounts.market.key(), MarketError::WrongGrid);
    require!(g_n == ctx.accounts.market.n, MarketError::WrongGrid);
    let (p0_len, theta_len, exp_len, w_len) = grid_vec_lens(&data)?;
    if p0_len == n && w_len == n && z != 0 {
        return Ok(());
    }
    require!(
        p0_len == n && z == 0 && w_len == 0 && theta_len == 0 && exp_len == 0,
        MarketError::GridNotReady
    );
    let p0_off = GRID_HDR + 4;
    let mut p0 = vec![0i128; n];
    for (i, slot) in p0.iter_mut().enumerate() {
        let off = p0_off + i * 16;
        let bytes: [u8; 16] = data[off..off + 16]
            .try_into()
            .map_err(|_| error!(MarketError::BadGrid))?;
        *slot = i128::from_le_bytes(bytes);
    }
    prior::normalize_i128(&mut p0);
    let z_sum = p0
        .iter()
        .fold(Q64::ZERO, |a, p| a.saturating_add(Q64::from_raw(*p)))
        .raw();
    data[43..59].copy_from_slice(&z_sum.to_le_bytes());
    for (i, v) in p0.iter().enumerate() {
        let off = p0_off + i * 16;
        data[off..off + 16].copy_from_slice(&v.to_le_bytes());
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
        let at = off + i * 16;
        data[at..at + 16].copy_from_slice(&v.to_le_bytes());
    }
    Ok(())
}

fn fill(ctx: &mut Context<Trade>, set_mask: &[u8], q_raw: i128, nonce: u64, is_buy: bool) -> Result<()> {
    require!(q_raw > 0, MarketError::ZeroQty);
    require!(
        ctx.accounts.market.status == Status::Trading as u8,
        MarketError::NotTrading
    );
    require!(!ctx.accounts.market.delegated, MarketError::Delegated);
    let now = Clock::get()?.unix_timestamp;
    require!(now < ctx.accounts.market.close_ts, MarketError::Closed);
    let owner_key = ctx.accounts.owner.key();
    let trader_key = ctx.accounts.trader.key();
    let market_key = ctx.accounts.market.key();
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
    let n = ctx.accounts.market.n as usize;
    mask::check(set_mask, n)?;
    let signed = if is_buy { q_raw } else { -q_raw };
    if !is_buy {
        require!(ctx.accounts.position.q >= q_raw, MarketError::NoInventory);
    }
    let grid_info = ctx.accounts.grid.to_account_info();
    let mut data = grid_info.try_borrow_mut_data()?;
    let (theta_off, exp_off, w_off) =
        require_sealed_bytes(&data, ctx.accounts.market.key(), ctx.accounts.market.n)?;
    let cost = apply_cached_on_bytes(
        &mut data,
        n,
        theta_off,
        exp_off,
        w_off,
        Q64::from_raw(ctx.accounts.market.beta),
        |i| mask::bit(set_mask, i),
        signed,
    )?;
    let l_max = grid_l_max_bytes(&data, exp_off, n)?;
    drop(data);

    ctx.accounts.market.l_max = l_max;
    let fee_bps = ctx.accounts.market.fee_bps;
    let fee = if is_buy && ctx.accounts.market.fee_on_fill() && fee_bps > 0 {
        cost.saturating_mul(Q64::from_int(fee_bps as i64))
            .checked_div(Q64::from_int(10_000))
            .unwrap_or(Q64::ZERO)
    } else {
        Q64::ZERO
    };
    let cost_usdc = if is_buy { usdc_charge(cost) } else { usdc(cost) };
    let fee_usdc = if is_buy { usdc_charge(fee) } else { usdc(fee) };
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

    if is_buy && (cost_usdc > 0 || fee_usdc > 0) {
        if trader_key != owner_key {
            let s = ctx
                .accounts
                .session
                .as_mut()
                .ok_or_else(|| error!(MarketError::SessionUnauthorized))?;
            let need = cost_usdc.saturating_add(fee_usdc);
            require!(s.remaining_usdc >= need, MarketError::SessionUnauthorized);
            s.remaining_usdc = s.remaining_usdc.saturating_sub(need);
        }
        charge_vault(
            ctx.accounts.vault_program.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.market.to_account_info(),
            ctx.accounts.board.to_account_info(),
            ctx.accounts.user_vault.to_account_info(),
            ctx.accounts.market.id_hash,
            ctx.accounts.market.bump,
            cost_usdc,
            fee_usdc,
        )?;
    }
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
    require!(q_raw > 0, MarketError::ZeroQty);
    require!(
        ctx.accounts.market.family == Family::Skellam as u8,
        MarketError::WrongFamily
    );
    require!(
        ctx.accounts.market.status == Status::Trading as u8,
        MarketError::NotTrading
    );
    require!(!ctx.accounts.market.delegated, MarketError::Delegated);
    let now = Clock::get()?.unix_timestamp;
    require!(now < ctx.accounts.market.close_ts, MarketError::Closed);
    let owner_key = ctx.accounts.owner.key();
    let trader_key = ctx.accounts.trader.key();
    let market_key = ctx.accounts.market.key();
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
    let n = ctx.accounts.market.n as usize;
    let beta = Q64::from_raw(ctx.accounts.market.beta);
    let fee_bps = ctx.accounts.market.fee_bps;
    let charge_fee = is_buy && ctx.accounts.market.fee_on_fill() && fee_bps > 0;
    let grid_info = ctx.accounts.grid.to_account_info();
    let mut data = grid_info.try_borrow_mut_data()?;
    let (theta_off, exp_off, w_off) =
        require_sealed_bytes(&data, ctx.accounts.market.key(), ctx.accounts.market.n)?;
    let mut cost_sum = 0i128;
    let mut fee_sum = 0i128;
    for m in &masks {
        require!(m.len() == n, MarketError::BadMask);
        let cost = apply_cached_on_bytes(
            &mut data,
            n,
            theta_off,
            exp_off,
            w_off,
            beta,
            |i| m[i],
            signed,
        )?;
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
        }
        cost_sum = cost_sum.checked_add(cost.raw()).ok_or(MarketError::Overflow)?;
        fee_sum = fee_sum.checked_add(fee.raw()).ok_or(MarketError::Overflow)?;
    }
    let l_max = grid_l_max_bytes(&data, exp_off, n)?;
    drop(data);
    ctx.accounts.market.l_max = l_max;
    let (kind, a, b) = contract.ticket_key();
    let pos = &mut ctx.accounts.position;
    pos.market = market_key;
    pos.owner = owner_key;
    pos.set_hash = ids::skellam_ticket(kind, a, b);
    pos.bump = ctx.bumps.position;
    let cost_q = Q64::from_raw(cost_sum);
    let fee_q = Q64::from_raw(fee_sum);
    let cost_usdc = if is_buy { usdc_charge(cost_q) } else { usdc(cost_q) };
    let fee_usdc = if is_buy { usdc_charge(fee_q) } else { usdc(fee_q) };
    if is_buy {
        pos.q = pos.q.checked_add(q_raw).ok_or(MarketError::Overflow)?;
        pos.cost_paid = pos.cost_paid.saturating_add(cost_usdc);
    } else {
        pos.q = pos.q.checked_sub(q_raw).ok_or(MarketError::NoInventory)?;
        pos.cost_paid = pos.cost_paid.saturating_sub(cost_usdc);
    }
    if is_buy && (cost_usdc > 0 || fee_usdc > 0) {
        if trader_key != owner_key {
            let s = ctx
                .accounts
                .session
                .as_mut()
                .ok_or_else(|| error!(MarketError::SessionUnauthorized))?;
            let need = cost_usdc.saturating_add(fee_usdc);
            require!(s.remaining_usdc >= need, MarketError::SessionUnauthorized);
            s.remaining_usdc = s.remaining_usdc.saturating_sub(need);
        }
        charge_vault(
            ctx.accounts.vault_program.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.market.to_account_info(),
            ctx.accounts.board.to_account_info(),
            ctx.accounts.user_vault.to_account_info(),
            ctx.accounts.market.id_hash,
            ctx.accounts.market.bump,
            cost_usdc,
            fee_usdc,
        )?;
    }
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
        common.report_window_secs > 0 && common.challenge_secs > 0,
        MarketError::BadClock
    );
    Ok(())
}

fn require_book_authority(ctx: &Context<Halt>) -> Result<()> {
    require_live_committee(&ctx.accounts.committee)?;
    require!(
        ctx.accounts.authority.key() == ctx.accounts.market.creator
            || ctx.accounts.committee.is_member(&ctx.accounts.authority.key()),
        MarketError::NotResolver
    );
    require!(
        ctx.accounts.committee.key() == ctx.accounts.market.committee,
        MarketError::BadCommittee
    );
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
        space = Grid::alloc_space(n as usize),
        seeds = [GRID_SEED, market.key().as_ref()],
        bump
    )]
    pub grid: Account<'info, Grid>,
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
pub struct OpenSession<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        init_if_needed,
        payer = owner,
        space = Session::SIZE,
        seeds = [SESSION_SEED, owner.key().as_ref()],
        bump
    )]
    pub session: Account<'info, Session>,
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
    pub session: Account<'info, Session>,
}

#[derive(Accounts)]
#[instruction(set_mask: Vec<u8>)]
pub struct Trade<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    /// CHECK: main wallet. Position and vault seeds bind to this key.
    pub owner: UncheckedAccount<'info>,
    #[account(mut)]
    pub session: Option<Box<Account<'info, Session>>>,
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
    #[account(
        mut,
        seeds = [vault::BOARD_SEED, market.key().as_ref()],
        bump = board.bump,
        seeds::program = vault::ID
    )]
    pub board: Box<Account<'info, vault::Board>>,
    #[account(
        mut,
        seeds = [vault::USER_SEED, owner.key().as_ref()],
        bump = user_vault.bump,
        seeds::program = vault::ID,
        constraint = user_vault.owner == owner.key()
    )]
    pub user_vault: Box<Account<'info, vault::UserVault>>,
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
#[instruction(contract: SkellamContract)]
pub struct TradeSkellam<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,
    /// CHECK: main wallet. Position and vault seeds bind to this key.
    pub owner: UncheckedAccount<'info>,
    #[account(mut)]
    pub session: Option<Box<Account<'info, Session>>>,
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
    #[account(
        mut,
        seeds = [vault::BOARD_SEED, market.key().as_ref()],
        bump = board.bump,
        seeds::program = vault::ID
    )]
    pub board: Box<Account<'info, vault::Board>>,
    #[account(
        mut,
        seeds = [vault::USER_SEED, owner.key().as_ref()],
        bump = user_vault.bump,
        seeds::program = vault::ID,
        constraint = user_vault.owner == owner.key()
    )]
    pub user_vault: Box<Account<'info, vault::UserVault>>,
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
}
