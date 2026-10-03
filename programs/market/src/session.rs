//! L1 Session PDA (FR-WAL-04–06) + MagicBlock `session-keys` (CR-04).
//!
//! Two layers (architecture §2.5):
//! 1. **Protocol Session PDA** (`["session", owner]`) — expiry, `remaining_usdc`, `allowed_ix`, whitelist.
//! 2. **MagicBlock SessionTokenV2** — `#[session_auth_or]` on fills; PDA seeds under `session_keys::ID`.
//!
//! Fills debit the protocol Session. Clients create SessionTokenV2 when opening a trading session
//! (`create_session_v2` on `session_token_program_id()`).

use anchor_lang::prelude::*;
use crate::MarketError;
pub use session_keys::{session_auth_or, SessionError, SessionTokenV2};

/// On-chain program id that creates / revokes MagicBlock SessionToken accounts.
pub fn session_token_program_id() -> Pubkey {
    session_keys::ID
}

/// SessionTokenV2 PDA: `["session_token_v2", target_program, session_signer, authority]`.
pub fn session_token_v2_pda(authority: &Pubkey, session_signer: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            SessionTokenV2::SEED_PREFIX.as_bytes(),
            crate::ID.as_ref(),
            session_signer.as_ref(),
            authority.as_ref(),
        ],
        &session_keys::ID,
    )
    .0
}

pub const SESSION_SEED: &[u8] = b"session";
pub const NONCE_SEED: &[u8] = b"nonce";

pub const IX_BUY_SET: u8 = 1;
pub const IX_SELL_SET: u8 = 2;
pub const IX_BUY_SKELLAM: u8 = 4;
pub const IX_SELL_SKELLAM: u8 = 8;
pub const IX_ALL_TRADES: u8 = IX_BUY_SET | IX_SELL_SET | IX_BUY_SKELLAM | IX_SELL_SKELLAM;

pub const SESSION_LIVE: u8 = 1;
pub const SESSION_REVOKED: u8 = 2;

#[account]
pub struct Session {
    pub owner: Pubkey,
    pub authority: Pubkey,
    pub expires_ts: i64,
    pub remaining_usdc: u64,
    pub allowed_ix: u8,
    pub status: u8,
    pub bump: u8,
    pub whitelist: Pubkey,
}

impl Session {
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 8 + 1 + 1 + 1 + 32;
}

#[account]
pub struct FillNonce {
    pub last: u64,
    pub bump: u8,
}

impl FillNonce {
    pub const SIZE: usize = 8 + 8 + 1;
}

/// `true` if this nonce already filled (FR-TRD-09 retry).
pub fn is_replay(acc: &FillNonce, nonce: u64) -> bool {
    nonce > 0 && nonce == acc.last
}

pub fn require_next(acc: &FillNonce, nonce: u64) -> Result<()> {
    require!(
        nonce == acc.last.saturating_add(1),
        MarketError::NonceReplay
    );
    Ok(())
}

pub fn check_trader(
    trader: &Pubkey,
    owner: &Pubkey,
    session: Option<&Session>,
    market: &Pubkey,
    now: i64,
    ix: u8,
) -> Result<()> {
    if trader == owner {
        return Ok(());
    }
    let s = session.ok_or(error!(MarketError::SessionUnauthorized))?;
    require!(s.status == SESSION_LIVE, MarketError::SessionUnauthorized);
    require!(s.owner == *owner, MarketError::SessionUnauthorized);
    require!(s.authority == *trader, MarketError::SessionUnauthorized);
    require!(now < s.expires_ts, MarketError::SessionUnauthorized);
    require!(
        s.whitelist == Pubkey::default() || s.whitelist == *market,
        MarketError::SessionUnauthorized
    );
    require!(s.allowed_ix & ix == ix, MarketError::SessionUnauthorized);
    Ok(())
}

pub fn debit_remaining(session: &mut Option<&mut Session>, trader: &Pubkey, owner: &Pubkey, amount: u64) -> Result<()> {
    if trader == owner {
        return Ok(());
    }
    let s = session.as_deref_mut().ok_or(error!(MarketError::SessionUnauthorized))?;
    require!(s.remaining_usdc >= amount, MarketError::SessionUnauthorized);
    s.remaining_usdc = s.remaining_usdc.saturating_sub(amount);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn live(owner: Pubkey, authority: Pubkey) -> Session {
        Session {
            owner,
            authority,
            expires_ts: 100,
            remaining_usdc: 50,
            allowed_ix: IX_ALL_TRADES,
            status: SESSION_LIVE,
            bump: 1,
            whitelist: Pubkey::default(),
        }
    }

    #[test]
    fn owner_path_does_not_need_session() {
        let o = pk(1);
        assert!(check_trader(&o, &o, None, &pk(9), 1, IX_BUY_SET).is_ok());
    }

    #[test]
    fn session_must_match_authority_and_not_be_expired() {
        let o = pk(1);
        let a = pk(2);
        let s = live(o, a);
        assert!(check_trader(&a, &o, Some(&s), &pk(9), 99, IX_BUY_SET).is_ok());
        assert!(check_trader(&a, &o, Some(&s), &pk(9), 100, IX_BUY_SET).is_err());
        assert!(check_trader(&pk(3), &o, Some(&s), &pk(9), 1, IX_BUY_SET).is_err());
    }

    #[test]
    fn whitelist_and_ix_bits() {
        let o = pk(1);
        let a = pk(2);
        let mut s = live(o, a);
        s.whitelist = pk(9);
        s.allowed_ix = IX_BUY_SET;
        assert!(check_trader(&a, &o, Some(&s), &pk(9), 1, IX_BUY_SET).is_ok());
        assert!(check_trader(&a, &o, Some(&s), &pk(8), 1, IX_BUY_SET).is_err());
        assert!(check_trader(&a, &o, Some(&s), &pk(9), 1, IX_SELL_SET).is_err());
    }

    #[test]
    fn nonce_is_strictly_next_then_idempotent() {
        let acc = FillNonce { last: 2, bump: 1 };
        assert!(is_replay(&acc, 2));
        assert!(!is_replay(&acc, 3));
        assert!(require_next(&acc, 3).is_ok());
        assert!(require_next(&acc, 4).is_err());
    }
}
