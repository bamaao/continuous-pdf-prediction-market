use anchor_lang::prelude::*;

pub const MARKET_SEED: &[u8] = b"market";
pub const GRID_SEED: &[u8] = b"grid";
pub const GRID_DUMP_SEED: &[u8] = b"gdump";
pub const POS_SEED: &[u8] = b"pos";
pub const COMMITTEE_SEED: &[u8] = b"committee";

pub const MAX_N: u16 = 1024;
pub const MAX_COMMITTEE: usize = 16;
pub const FOOTBALL_K_MAX: u8 = 10;
pub const FOOTBALL_N: u16 = 121;
/// Charge $\phi\cdot C_S$ when the trader buys.
pub const FEE_ON_FILL: u8 = 0;
/// Charge $\phi$ of the settlement payout when the winner claims. Miss / VOID: 0.
pub const FEE_ON_CLAIM: u8 = 1;

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
    pub authorized_reporter: Pubkey,
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
    pub trading_revenue: u64,
    pub alpha_r_bps: u16,
    pub platform: Pubkey,
    pub l_max: i128,
    pub id_hash: [u8; 32],
    pub extra: FamilyExtra,
    pub fee_timing: u8,
    /// L1 fills are refused while true (FR-TRD-01). MagicBlock ER executes them off this program.
    pub delegated: bool,
    /// Journal checkpoint. Not whether a fill exists (FR-DUR-02).
    pub trades_root: [u8; 32],
    pub commit_ts: i64,
    /// Unnormalized $P_0$ sum while sealing in batches (n=1024 cannot fit every shard in one ix).
    pub p0_sum: i128,
    /// Bit i set after shard i's raw $P_0$ was added to `p0_sum`.
    pub seal_bits: u128,
    /// Wide fill (mask spans too many shards for one 64-account tx). `p0_sum` holds $\sum w_i$ while active.
    pub wide_z0: i128,
    pub wide_z: i128,
    pub wide_q: i128,
    pub wide_nonce: u64,
    pub wide_read: u128,
    pub wide_write: u128,
    pub wide_tag: u64,
    /// bit0 = active, bit1 = buy
    pub wide_flags: u8,
}

impl Market {
    pub const SIZE: usize = 8 + 576;

    pub fn fee_on_fill(&self) -> bool {
        self.fee_timing != FEE_ON_CLAIM
    }

    pub fn can_propose_as_reporter(&self, who: &Pubkey) -> bool {
        self.authorized_reporter != Pubkey::default() && self.authorized_reporter == *who
    }
}

#[account]
pub struct Committee {
    pub authority: Pubkey,
    pub members: [Pubkey; MAX_COMMITTEE],
    pub member_count: u8,
    pub m: u8,
    pub bump: u8,
    pub epoch: u32,
}

impl Committee {
    pub const SIZE: usize = 8 + 576;

    pub fn is_member(&self, who: &Pubkey) -> bool {
        self.members
            .iter()
            .take(self.member_count as usize)
            .any(|k| k == who)
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
    /// Cells stored in this shard (not the market-wide n).
    pub n: u16,
    /// First global cell index this shard covers.
    pub start: u16,
    pub bump: u8,
    /// Global LMSR z. Meaningful on the shard with `start == 0`.
    pub z: i128,
    pub p0: Vec<i128>,
    pub theta: Vec<i128>,
    pub exposure: Vec<i128>,
    pub weights: Vec<i128>,
}

impl Grid {
    /// Solana CPI `create_account` / one `realloc` may move at most 10 240 bytes.
    pub const CREATE_CAP: usize = 10_240;
    /// Unnormalized $P_0$ nodes per `write_grid_mass`. 256 exp() does not fit 1.4M CU.
    pub const PRIOR_CHUNK: usize = 64;
    /// 16 cells → 1101B. Commit uses the committor writeback buffer
    /// (`comittor_buffer` under ComtrB2). Local L1 loads a committor rebuilt
    /// with cargo-build-sbf 3.1.13 so that path can finalize.
    pub const SHARD_CELLS: u16 = 16;

    /// Borsh layout (no padding): disc(8) + market(32) + n(2) + start(2) + bump(1) + z(16)
    /// + 4 × (vec_len(4) + n × i128(16)).
    pub fn space(n: usize) -> usize {
        8 + 32 + 2 + 2 + 1 + 16 + 4 * (4 + n * 16)
    }

    pub fn shard_count(n: u16) -> u16 {
        (n + Self::SHARD_CELLS - 1) / Self::SHARD_CELLS
    }

    pub fn shard_len(n: u16, ix: u16) -> u16 {
        let start = ix.saturating_mul(Self::SHARD_CELLS);
        n.saturating_sub(start).min(Self::SHARD_CELLS)
    }

    pub fn shard_start(ix: u16) -> u16 {
        ix.saturating_mul(Self::SHARD_CELLS)
    }

    pub fn alloc_space(n: usize) -> usize {
        Self::space(n).min(Self::CREATE_CAP)
    }

    pub fn grow_steps(n: usize) -> usize {
        let need = Self::space(n);
        if need <= Self::CREATE_CAP {
            0
        } else {
            let rest = need - Self::CREATE_CAP;
            (rest + Self::CREATE_CAP - 1) / Self::CREATE_CAP
        }
    }

    pub fn mass_steps(n: usize) -> usize {
        (n + Self::PRIOR_CHUNK - 1) / Self::PRIOR_CHUNK
    }
}

#[account]
pub struct Position {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub set_hash: [u8; 32],
    pub q: i128,
    pub cost_paid: u64,
    pub claimed: bool,
    pub bump: u8,
    /// USDC cost not yet taken from `user_vault` (ER fills). Charged on L1 `sync_vault`.
    pub vault_owed_cost: u64,
    /// Fill fees not yet taken from `user_vault` (ER fills).
    pub vault_owed_fee: u64,
    /// LMSR sell proceeds not yet credited to `user_vault` (ER sells).
    pub vault_owed_credit: u64,
}

impl Position {
    pub const SIZE: usize = 8 + 32 + 32 + 32 + 16 + 8 + 1 + 1 + 8 + 8 + 8;
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
    pub fee_timing: u8,
    pub authorized_reporter: Pubkey,
    pub report_window_secs: i64,
    pub challenge_secs: i64,
    pub n_layers: u8,
    pub d_unit: u64,
    pub gamma_bps: u16,
    pub alpha_r_bps: u16,
    pub platform: Pubkey,
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

#[cfg(test)]
mod tests {
    use super::Grid;

    #[test]
    fn create_account_holds_128_not_256() {
        assert_eq!(Grid::space(16), 1_101);
        assert_eq!(Grid::space(128), 8_269);
        assert_eq!(Grid::space(256), 16_461);
        assert!(Grid::space(16) <= Grid::CREATE_CAP);
        assert!(Grid::space(128) <= Grid::CREATE_CAP);
        assert!(Grid::space(256) > Grid::CREATE_CAP);
        assert_eq!(Grid::shard_count(2), 1);
        assert_eq!(Grid::shard_count(8), 1);
        assert_eq!(Grid::shard_count(16), 1);
        assert_eq!(Grid::shard_count(121), 8);
        assert_eq!(Grid::shard_count(256), 16);
        assert_eq!(Grid::shard_count(1024), 64);
        assert_eq!(Grid::shard_len(256, 15), 16);
        assert_eq!(Grid::shard_len(121, 7), 9);
        assert_eq!(Grid::PRIOR_CHUNK, 64);
    }

    #[test]
    fn space_matches_anchor_serialize() {
        use super::Grid;
        use anchor_lang::prelude::Pubkey;
        use anchor_lang::AccountSerialize;
        for n in [2usize, 8, 128, 256, 1024] {
            let g = Grid {
                market: Pubkey::default(),
                n: n as u16,
                start: 0,
                bump: 255,
                z: 1,
                p0: vec![0; n],
                theta: vec![0; n],
                exposure: vec![0; n],
                weights: vec![0; n],
            };
            let mut buf = Vec::new();
            g.try_serialize(&mut buf).unwrap();
            assert_eq!(buf.len(), Grid::space(n), "n={n}");
            assert_eq!(u32::from_le_bytes(buf[61..65].try_into().unwrap()) as usize, n);
        }
        let empty = Grid {
            market: Pubkey::default(),
            n: 256,
            start: 0,
            bump: 1,
            z: 0,
            p0: vec![],
            theta: vec![],
            exposure: vec![],
            weights: vec![],
        };
        let mut buf = Vec::new();
        empty.try_serialize(&mut buf).unwrap();
        assert_eq!(buf.len(), 77);
        assert_eq!(u32::from_le_bytes(buf[61..65].try_into().unwrap()), 0);
    }

    #[test]
    fn market_n_is_le_u16_at_offset_12() {
        use super::{FamilyExtra, Market};
        use anchor_lang::prelude::Pubkey;
        use anchor_lang::AccountSerialize;
        let m = Market {
            family: 1,
            status: 1,
            bump: 255,
            grid_bump: 254,
            n: 256,
            fee_bps: 0,
            creator: Pubkey::default(),
            committee: Pubkey::default(),
            authorized_reporter: Pubkey::default(),
            close_ts: 0,
            risk_lock_ts: 0,
            report_window_secs: 0,
            challenge_secs: 0,
            n_layers: 1,
            gamma_bps: 1_000,
            d_unit: 1,
            beta: 1,
            c_m: 0,
            fees_accrued: 0,
            trading_revenue: 0,
            alpha_r_bps: 0,
            platform: Pubkey::default(),
            l_max: 0,
            id_hash: [0; 32],
            extra: FamilyExtra::default(),
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
        };
        let mut buf = Vec::new();
        m.try_serialize(&mut buf).unwrap();
        assert!(buf.len() <= Market::SIZE, "serialized {} > SIZE {}", buf.len(), Market::SIZE);
        assert_eq!(u16::from_le_bytes(buf[12..14].try_into().unwrap()), 256);
    }
}
