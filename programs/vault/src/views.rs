//! Foreign accounts. Discriminators match the defining program (`account:Name`).
//! Vault never Delegates. These types exist so settle can read $x^*$ / $E$ / $C_R$
//! without a crate cycle (market CPIs this program).

use anchor_lang::prelude::*;

pub const MARKET_ID: Pubkey = pubkey!("Market1111111111111111111111111111111111111");
pub const RESOLUTION_ID: Pubkey = pubkey!("Rso1111111111111111111111111111111111111111");
pub const RISK_ID: Pubkey = pubkey!("Rsk1111111111111111111111111111111111111111");

pub const FAMILY_SKELLAM: u8 = 0;

/// First 8 bytes are the defining program's discriminator. Owner is checked via `Owner`.
pub fn account_disc(_name: &str) -> [u8; 8] {
    [0u8; 8]
}

macro_rules! foreign_account {
    ($name:ident, $owner:expr) => {
        impl AccountSerialize for $name {
            fn try_serialize<W: std::io::Write>(&self, writer: &mut W) -> Result<()> {
                // Keep the original 8-byte discriminator; caller writes via load+store of the account.
                writer
                    .write_all(&[0u8; 8])
                    .map_err(|_| error!(ErrorCode::AccountDidNotSerialize))?;
                AnchorSerialize::serialize(self, writer)
                    .map_err(|_| error!(ErrorCode::AccountDidNotSerialize))?;
                Ok(())
            }
        }
        impl AccountDeserialize for $name {
            fn try_deserialize(buf: &mut &[u8]) -> Result<Self> {
                require!(buf.len() >= 8, ErrorCode::AccountDidNotDeserialize);
                Self::try_deserialize_unchecked(buf)
            }
            fn try_deserialize_unchecked(buf: &mut &[u8]) -> Result<Self> {
                *buf = &buf[8..];
                AnchorDeserialize::deserialize(buf)
                    .map_err(|_| error!(ErrorCode::AccountDidNotDeserialize))
            }
        }
        impl Owner for $name {
            fn owner() -> Pubkey {
                $owner
            }
        }
    };
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
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

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
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
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct Grid {
    pub market: Pubkey,
    pub n: u16,
    pub start: u16,
    pub bump: u8,
    pub z: i128,
    pub p0: Vec<i128>,
    pub theta: Vec<i128>,
    pub exposure: Vec<i128>,
    pub weights: Vec<i128>,
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct Position {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub set_hash: [u8; 32],
    pub q: i128,
    pub cost_paid: u64,
    pub claimed: bool,
    pub bump: u8,
    pub vault_owed_cost: u64,
    pub vault_owed_fee: u64,
    pub vault_owed_credit: u64,
}

#[derive(Clone, Copy, AnchorSerialize, AnchorDeserialize, Default)]
pub struct Outcome {
    pub family: u8,
    pub kind: u8,
    pub a: i128,
    pub b: i128,
    pub shares: [i128; 8],
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct Resolution {
    pub market: Pubkey,
    pub members: [Pubkey; 16],
    pub authorized_reporter: Pubkey,
    pub family: u8,
    pub phase: u8,
    pub m: u8,
    pub n: u8,
    pub extensions: u8,
    pub votes_proposal: u8,
    pub votes_challenge: u8,
    pub bump: u8,
    pub refunds_due: bool,
    pub early_resolve: bool,
    pub k_max: u8,
    pub layout: u8,
    pub n_atoms: u16,
    pub close_ts: i64,
    pub report_window_secs: i64,
    pub report_deadline: i64,
    pub challenge_secs: i64,
    pub challenge_end: i64,
    pub vote_end: i64,
    pub proposer: Pubkey,
    pub challenger: Pubkey,
    pub proposed: Outcome,
    pub challenged: Outcome,
    pub final_outcome: Outcome,
    pub evidence_hash: [u8; 32],
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
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

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
pub struct Layer {
    pub market: Pubkey,
    pub attachment: u64,
    pub thickness: u64,
    pub filled: u64,
    pub layer_id: u8,
    pub quote_count: u8,
    pub bump: u8,
}

#[derive(Clone, AnchorSerialize, AnchorDeserialize)]
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

foreign_account!(Market, MARKET_ID);
foreign_account!(Grid, MARKET_ID);
foreign_account!(Position, MARKET_ID);
foreign_account!(Resolution, RESOLUTION_ID);
foreign_account!(RiskBook, RISK_ID);
foreign_account!(Layer, RISK_ID);
foreign_account!(Quote, RISK_ID);

pub const PHASE_FINALIZED: u8 = 3;
pub const PHASE_FAILED: u8 = 4;
pub const PHASE_VOIDED: u8 = 5;
pub const STATUS_SETTLED: u8 = 3;
pub const STATUS_VOID: u8 = 4;
