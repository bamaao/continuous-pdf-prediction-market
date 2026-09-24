use anchor_lang::prelude::*;

pub const MARKET_SEED: &[u8] = b"market";
pub const GRID_SEED: &[u8] = b"grid";
pub const POS_SEED: &[u8] = b"pos";

pub const MAX_N: u16 = 1024;
pub const MAX_COMMITTEE: usize = 8;
pub const FOOTBALL_K_MAX: u8 = 10;
pub const FOOTBALL_N: u16 = 121;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Skellam = 0,
    Gaussian = 1,
    Lognormal = 2,
    Dirichlet = 3,
    Bernoulli = 4,
}

pub const DIRICHLET_ATOMS: u8 = 0;
pub const DIRICHLET_TOP_N: u8 = 1;
pub const DIRICHLET_SIMPLEX: u8 = 2;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Trading = 1,
    Halted = 2,
    Settled = 3,
    Void = 4,
}

#[account]
pub struct Market {
    pub family: u8,
    pub status: u8,
    pub bump: u8,
    pub grid_bump: u8,
    pub n: u16,
    pub fee_bps: u16,
    pub creator: Pubkey,
    pub committee: Pubkey,
    pub members: [Pubkey; MAX_COMMITTEE],
    pub authorized_reporter: Pubkey,
    pub member_count: u8,
    pub m: u8,
    pub close_ts: i64,
    pub risk_lock_ts: i64,
    pub report_window_secs: i64,
    pub challenge_secs: i64,
    pub n_layers: u8,
    pub gamma_bps: u16,
    pub d_unit: u64,
    pub beta: i128,
    pub c_m: u64,
    pub fees_accrued: u64,
    pub l_max: i128,
    pub id_hash: [u8; 32],
    pub extra: FamilyExtra,
}

impl Market {
    pub const SIZE: usize = 8 + 640;

    pub fn is_member(&self, who: &Pubkey) -> bool {
        self.members
            .iter()
            .take(self.member_count as usize)
            .any(|k| k == who)
    }

    pub fn can_propose(&self, who: &Pubkey) -> bool {
        self.is_member(who)
            || (self.authorized_reporter != Pubkey::default() && self.authorized_reporter == *who)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default)]
pub struct FamilyExtra {
    pub a: i128,
    pub b: i128,
    pub c: i128,
    pub d: i128,
    pub e: i64,
    pub f: i64,
    pub u0: u8,
    pub u1: u8,
    pub u2: u8,
    pub u3: u8,
}

#[account]
pub struct Grid {
    pub market: Pubkey,
    pub n: u16,
    pub bump: u8,
    pub p0: Vec<i128>,
    pub theta: Vec<i128>,
    pub exposure: Vec<i128>,
}

impl Grid {
    pub fn space(n: usize) -> usize {
        8 + 32 + 2 + 1 + 3 * (4 + n * 16)
    }
}

#[account]
pub struct Position {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub set_hash: [u8; 32],
    pub q: i128,
    pub bump: u8,
}

impl Position {
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 16 + 1;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct CreateCommon {
    pub id_hash: [u8; 32],
    pub n: u16,
    pub close_ts: i64,
    pub risk_lock_ts: i64,
    pub beta: i128,
    pub c_m: u64,
    pub fee_bps: u16,
    pub committee: Pubkey,
    pub members: Vec<Pubkey>,
    pub m: u8,
    pub authorized_reporter: Pubkey,
    pub report_window_secs: i64,
    pub challenge_secs: i64,
    pub n_layers: u8,
    pub d_unit: u64,
    pub gamma_bps: u16,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct SkellamArgs {
    pub common: CreateCommon,
    pub topic: [u8; 32],
    pub score_scope: u8,
    pub kickoff_ts: i64,
    pub prior_kind: u8,
    pub lambda_home: i128,
    pub lambda_away: i128,
    pub dc_rho: i128,
}

/// 1-D continuous board: Gaussian or lognormal. `topic`/`tag` are listing keys only.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct IntervalArgs {
    pub common: CreateCommon,
    pub topic: [u8; 32],
    pub tag: [u8; 32],
    pub x_min: i128,
    pub x_max: i128,
    pub mu: i128,
    pub sigma: i128,
}

/// Dirichlet board. `layout` is atoms / top-n combinations / simplex — not a product type.
#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct DirichletArgs {
    pub common: CreateCommon,
    pub topic: [u8; 32],
    pub layout: u8,
    pub top_n: u8,
    pub bins: u16,
    pub alpha: Vec<i128>,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct BernoulliArgs {
    pub common: CreateCommon,
    pub topic: [u8; 32],
    pub tag: [u8; 32],
    pub deadline_ts: i64,
    pub early_resolve: bool,
    pub alpha_yes: i128,
    pub alpha_no: i128,
}

/// Typed football projection. Custom unions still go through `buy_set` + bitmask.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub enum SkellamContract {
    Home,
    Draw,
    Away,
    TotalsOver { halves: i16 },
    TotalsUnder { halves: i16 },
    BttsYes,
    BttsNo,
    Exact { home: u8, away: u8 },
    HomeHandicap { halves: i16 },
    AwayHandicap { halves: i16 },
    HomeHandicapQuarter { quarters: i16 },
    AwayHandicapQuarter { quarters: i16 },
}

impl SkellamContract {
    pub fn ticket_key(self) -> (u8, i16, i16) {
        match self {
            Self::Home => (0, 0, 0),
            Self::Draw => (1, 0, 0),
            Self::Away => (2, 0, 0),
            Self::TotalsOver { halves } => (3, halves, 0),
            Self::TotalsUnder { halves } => (4, halves, 0),
            Self::BttsYes => (5, 0, 0),
            Self::BttsNo => (6, 0, 0),
            Self::Exact { home, away } => (7, home as i16, away as i16),
            Self::HomeHandicap { halves } => (8, halves, 0),
            Self::AwayHandicap { halves } => (9, halves, 0),
            Self::HomeHandicapQuarter { quarters } => (10, quarters, 0),
            Self::AwayHandicapQuarter { quarters } => (11, quarters, 0),
        }
    }
}

#[event]
pub struct FillEvent {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub buy: bool,
    pub q: i128,
    pub cost: i128,
    pub fee: i128,
    pub l_max: i128,
}
