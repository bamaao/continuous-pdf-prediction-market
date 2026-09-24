//! Market program: create by distribution family, `p0` once, L1 `buy_set`/`sell_set`.
//! Listing names (CPI, election, BTC) are metadata, not instructions.
//! Session / Delegate land later. Buys debit the L1 vault pot.

use anchor_lang::prelude::*;
use math::lmsr::{lmsr_update, LmsrState};
use math::prior;
use math::Q64;
use vault::cpi::accounts::CreditTrade;
use vault::cpi::credit_trade;

pub mod ids;
pub mod mask;
pub mod state;

use state::*;

declare_id!("Market1111111111111111111111111111111111111");

#[program]
pub mod market {
    use super::*;

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
        let alphas: Vec<Q64> = args.alpha.iter().copied().map(Q64::from_raw).collect();
        let p0 = match args.layout {
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
        };
        open_board(
            &mut ctx,
            Family::Dirichlet,
            &args.common,
            p0,
            FamilyExtra {
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

    /// L1 path used before Delegate. Session is not accepted here.
    pub fn buy_set(mut ctx: Context<Trade>, set_mask: Vec<u8>, q_raw: i128) -> Result<()> {
        fill(&mut ctx, &set_mask, q_raw, true)
    }

    pub fn sell_set(mut ctx: Context<Trade>, set_mask: Vec<u8>, q_raw: i128) -> Result<()> {
        fill(&mut ctx, &set_mask, q_raw, false)
    }

    /// 1X2 / handicap / totals / exact score on the shared Skellam grid.
    pub fn buy_skellam_set(
        mut ctx: Context<TradeSkellam>,
        contract: SkellamContract,
        q_raw: i128,
    ) -> Result<()> {
        fill_skellam(&mut ctx, contract, q_raw, true)
    }

    pub fn sell_skellam_set(
        mut ctx: Context<TradeSkellam>,
        contract: SkellamContract,
        q_raw: i128,
    ) -> Result<()> {
        fill_skellam(&mut ctx, contract, q_raw, false)
    }

    /// Creator or roster member stops fills (early YES, VOID, or after close).
    pub fn halt(ctx: Context<Halt>) -> Result<()> {
        let market = &mut ctx.accounts.market;
        require!(
            ctx.accounts.authority.key() == market.creator || market.is_member(&ctx.accounts.authority.key()),
            MarketError::NotResolver
        );
        require!(market.status == Status::Trading as u8, MarketError::NotTrading);
        market.status = Status::Halted as u8;
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
    let p0 = match family {
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
    require!(now < common.close_ts, MarketError::BadClock);
    require!(common.close_ts <= common.risk_lock_ts, MarketError::BadClock);
    lock_roster(common)?;
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
    market.creator = ctx.accounts.creator.key();
    market.committee = common.committee;
    market.members = [Pubkey::default(); MAX_COMMITTEE];
    for (i, m) in common.members.iter().enumerate() {
        market.members[i] = *m;
    }
    market.authorized_reporter = common.authorized_reporter;
    market.member_count = common.members.len() as u8;
    market.m = common.m;
    market.close_ts = common.close_ts;
    market.risk_lock_ts = common.risk_lock_ts;
    market.report_window_secs = common.report_window_secs;
    market.challenge_secs = common.challenge_secs;
    market.n_layers = common.n_layers;
    market.d_unit = common.d_unit;
    market.gamma_bps = if common.gamma_bps == 0 { 1_000 } else { common.gamma_bps };
    market.beta = common.beta;
    market.c_m = common.c_m;
    market.fees_accrued = 0;
    market.trading_revenue = 0;
    market.alpha_r_bps = common.alpha_r_bps;
    market.platform = common.platform;
    market.l_max = 0;
    market.id_hash = common.id_hash;
    market.extra = extra;

    let grid = &mut ctx.accounts.grid;
    grid.market = market.key();
    grid.n = common.n;
    grid.bump = ctx.bumps.grid;
    grid.p0 = p0.iter().map(|q| q.raw()).collect();
    grid.theta = vec![0; p0.len()];
    grid.exposure = vec![0; p0.len()];
    Ok(())
}

fn fill(ctx: &mut Context<Trade>, set_mask: &[u8], q_raw: i128, is_buy: bool) -> Result<()> {
    require!(q_raw > 0, MarketError::ZeroQty);
    require!(
        ctx.accounts.market.status == Status::Trading as u8,
        MarketError::NotTrading
    );
    let now = Clock::get()?.unix_timestamp;
    require!(now < ctx.accounts.market.close_ts, MarketError::Closed);
    require!(
        ctx.accounts.grid.market == ctx.accounts.market.key(),
        MarketError::WrongGrid
    );
    require!(ctx.accounts.grid.n == ctx.accounts.market.n, MarketError::WrongGrid);
    require!(
        ctx.accounts.grid.p0.len() == ctx.accounts.market.n as usize,
        MarketError::WrongGrid
    );

    let in_set = mask::decode(set_mask, ctx.accounts.market.n as usize)?;
    let signed = if is_buy { q_raw } else { -q_raw };
    if !is_buy {
        require!(ctx.accounts.position.q >= q_raw, MarketError::NoInventory);
    }

    let mut state = LmsrState {
        beta: Q64::from_raw(ctx.accounts.market.beta),
        p0: ctx.accounts.grid.p0.iter().copied().map(Q64::from_raw).collect(),
        theta: ctx.accounts.grid.theta.iter().copied().map(Q64::from_raw).collect(),
        exposure: ctx
            .accounts
            .grid
            .exposure
            .iter()
            .copied()
            .map(Q64::from_raw)
            .collect(),
    };
    let cost = lmsr_update(&mut state, &in_set, Q64::from_raw(signed));
    let fee_bps = ctx.accounts.market.fee_bps;
    let fee = if is_buy && fee_bps > 0 {
        cost.saturating_mul(Q64::from_int(fee_bps as i64))
            .checked_div(Q64::from_int(10_000))
            .unwrap_or(Q64::ZERO)
    } else {
        Q64::ZERO
    };
    let l_max = state.l_max().raw();
    let market_key = ctx.accounts.market.key();
    let owner_key = ctx.accounts.owner.key();

    ctx.accounts.grid.theta = state.theta.iter().map(|q| q.raw()).collect();
    ctx.accounts.grid.exposure = state.exposure.iter().map(|q| q.raw()).collect();

    ctx.accounts.market.l_max = l_max;
    let cost_usdc = (cost.raw().max(0) >> 64) as u64;
    let fee_usdc = (fee.raw().max(0) >> 64) as u64;
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
        charge_vault(
            ctx.accounts.vault_program.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.market.to_account_info(),
            ctx.accounts.board.to_account_info(),
            ctx.accounts.user_vault.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            cost_usdc,
            fee_usdc,
        )?;
    }

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
    use math::football as fb;
    Ok(match contract {
        SkellamContract::Home => vec![fb::mask_home(k_max)],
        SkellamContract::Draw => vec![fb::mask_draw(k_max)],
        SkellamContract::Away => vec![fb::mask_away(k_max)],
        SkellamContract::TotalsOver { halves } => vec![fb::mask_over(k_max, halves as i32)],
        SkellamContract::TotalsUnder { halves } => vec![fb::mask_under(k_max, halves as i32)],
        SkellamContract::BttsYes => vec![fb::mask_btts_yes(k_max)],
        SkellamContract::BttsNo => vec![fb::mask_btts_no(k_max)],
        SkellamContract::Exact { home, away } => vec![fb::mask_exact(k_max, home as u32, away as u32)],
        SkellamContract::HomeHandicap { halves } => {
            vec![fb::mask_home_handicap(k_max, halves as i32)]
        }
        SkellamContract::AwayHandicap { halves } => {
            vec![fb::mask_away_handicap(k_max, halves as i32)]
        }
        SkellamContract::HomeHandicapQuarter { quarters } => {
            require!(quarters % 2 != 0, MarketError::BadMask);
            let (a, b) = fb::quarter_to_halves(quarters as i32);
            vec![
                fb::mask_home_handicap(k_max, a),
                fb::mask_home_handicap(k_max, b),
            ]
        }
        SkellamContract::AwayHandicapQuarter { quarters } => {
            require!(quarters % 2 != 0, MarketError::BadMask);
            let (a, b) = fb::quarter_to_halves(quarters as i32);
            vec![
                fb::mask_away_handicap(k_max, a),
                fb::mask_away_handicap(k_max, b),
            ]
        }
    })
}

fn apply_lmsr(
    market: &mut Market,
    grid: &mut Grid,
    in_set: &[bool],
    signed_q: i128,
    charge_fee: bool,
) -> Result<(i128, i128)> {
    require!(in_set.len() == market.n as usize, MarketError::BadMask);
    let mut state = LmsrState {
        beta: Q64::from_raw(market.beta),
        p0: grid.p0.iter().copied().map(Q64::from_raw).collect(),
        theta: grid.theta.iter().copied().map(Q64::from_raw).collect(),
        exposure: grid.exposure.iter().copied().map(Q64::from_raw).collect(),
    };
    let cost = lmsr_update(&mut state, in_set, Q64::from_raw(signed_q));
    let fee = if charge_fee && market.fee_bps > 0 {
        cost.saturating_mul(Q64::from_int(market.fee_bps as i64))
            .checked_div(Q64::from_int(10_000))
            .unwrap_or(Q64::ZERO)
    } else {
        Q64::ZERO
    };
    grid.theta = state.theta.iter().map(|q| q.raw()).collect();
    grid.exposure = state.exposure.iter().map(|q| q.raw()).collect();
    market.l_max = state.l_max().raw();
    if charge_fee {
        let fee_usdc = (fee.raw().max(0) >> 64) as u64;
        let cost_usdc = (cost.raw().max(0) >> 64) as u64;
        market.fees_accrued = market.fees_accrued.saturating_add(fee_usdc);
        market.trading_revenue = market.trading_revenue.saturating_add(cost_usdc);
    }
    Ok((cost.raw(), fee.raw()))
}

fn fill_skellam(
    ctx: &mut Context<TradeSkellam>,
    contract: SkellamContract,
    q_raw: i128,
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
    require!(
        Clock::get()?.unix_timestamp < ctx.accounts.market.close_ts,
        MarketError::Closed
    );
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
    let mut cost_sum = 0i128;
    let mut fee_sum = 0i128;
    for mask in &masks {
        let (c, f) = apply_lmsr(
            &mut ctx.accounts.market,
            &mut ctx.accounts.grid,
            mask,
            signed,
            is_buy,
        )?;
        cost_sum = cost_sum.checked_add(c).ok_or(MarketError::Overflow)?;
        fee_sum = fee_sum.checked_add(f).ok_or(MarketError::Overflow)?;
    }
    let (kind, a, b) = contract.ticket_key();
    let market_key = ctx.accounts.market.key();
    let owner_key = ctx.accounts.owner.key();
    let l_max = ctx.accounts.market.l_max;
    let pos = &mut ctx.accounts.position;
    pos.market = market_key;
    pos.owner = owner_key;
    pos.set_hash = ids::skellam_ticket(kind, a, b);
    pos.bump = ctx.bumps.position;
    let cost_usdc = (cost_sum.max(0) >> 64) as u64;
    let fee_usdc = (fee_sum.max(0) >> 64) as u64;
    if is_buy {
        pos.q = pos.q.checked_add(q_raw).ok_or(MarketError::Overflow)?;
        pos.cost_paid = pos.cost_paid.saturating_add(cost_usdc);
    } else {
        pos.q = pos.q.checked_sub(q_raw).ok_or(MarketError::NoInventory)?;
        pos.cost_paid = pos.cost_paid.saturating_sub(cost_usdc);
    }
    if is_buy && (cost_usdc > 0 || fee_usdc > 0) {
        charge_vault(
            ctx.accounts.vault_program.to_account_info(),
            ctx.accounts.owner.to_account_info(),
            ctx.accounts.market.to_account_info(),
            ctx.accounts.board.to_account_info(),
            ctx.accounts.user_vault.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            cost_usdc,
            fee_usdc,
        )?;
    }
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

fn lock_roster(common: &CreateCommon) -> Result<()> {
    let n = common.members.len();
    require!(
        n >= 1 && n <= MAX_COMMITTEE && common.m >= 1 && (common.m as usize) <= n,
        MarketError::BadCommittee
    );
    require!(
        common.report_window_secs > 0 && common.challenge_secs > 0,
        MarketError::BadClock
    );
    require!(
        common.members.iter().any(|m| *m == common.committee),
        MarketError::BadCommittee
    );
    for (i, m) in common.members.iter().enumerate() {
        require!(*m != Pubkey::default(), MarketError::BadCommittee);
        for prev in common.members.iter().take(i) {
            require!(m != prev, MarketError::BadCommittee);
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
    system_program: AccountInfo<'info>,
    cost: u64,
    fee: u64,
) -> Result<()> {
    credit_trade(
        CpiContext::new(
            vault_program,
            CreditTrade {
                owner,
                market_key: market,
                board,
                user,
                system_program,
            },
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
        space = Grid::space(n as usize),
        seeds = [GRID_SEED, market.key().as_ref()],
        bump
    )]
    pub grid: Account<'info, Grid>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(set_mask: Vec<u8>)]
pub struct Trade<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [MARKET_SEED, market.id_hash.as_ref()],
        bump = market.bump
    )]
    pub market: Box<Account<'info, Market>>,
    #[account(
        mut,
        seeds = [GRID_SEED, market.key().as_ref()],
        bump = grid.bump
    )]
    pub grid: Box<Account<'info, Grid>>,
    #[account(
        init_if_needed,
        payer = owner,
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
    pub vault_program: Program<'info, vault::program::Vault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(contract: SkellamContract)]
pub struct TradeSkellam<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [MARKET_SEED, market.id_hash.as_ref()],
        bump = market.bump
    )]
    pub market: Box<Account<'info, Market>>,
    #[account(
        mut,
        seeds = [GRID_SEED, market.key().as_ref()],
        bump = grid.bump
    )]
    pub grid: Box<Account<'info, Grid>>,
    #[account(
        init_if_needed,
        payer = owner,
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
    #[msg("C_M must be > 0")]
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
    fn roster_is_locked_at_create() {
        use super::{lock_roster, CreateCommon};
        use anchor_lang::prelude::Pubkey;
        let a = Pubkey::new_from_array([1u8; 32]);
        let b = Pubkey::new_from_array([2u8; 32]);
        let mut common = CreateCommon {
            id_hash: [0; 32],
            n: 2,
            close_ts: 10,
            risk_lock_ts: 20,
            beta: 1,
            c_m: 1,
            fee_bps: 0,
            committee: a,
            members: vec![a, b],
            m: 2,
            authorized_reporter: Pubkey::default(),
            report_window_secs: 60,
            challenge_secs: 30,
            n_layers: 3,
            d_unit: 10_000,
            gamma_bps: 1_000,
            alpha_r_bps: 7_000,
            platform: a,
        };
        assert!(lock_roster(&common).is_ok());
        common.members = vec![b];
        assert!(lock_roster(&common).is_err());
        common.members = vec![a, a];
        common.committee = a;
        assert!(lock_roster(&common).is_err());
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
            risk_lock_ts: 20,
            beta: 1,
            c_m: 1,
            fee_bps: 0,
            committee: a,
            members: vec![a],
            m: 1,
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
