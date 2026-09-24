//! Official Circle USDC mint. The vault does not create or own this mint.
//!
//! IR-04 / CR-08: every funds instruction compares `mint == USDC_MINT`.
//! `initialize` only creates the vault's token account for that existing mint.

use anchor_lang::prelude::{pubkey, Pubkey};

/// Circle official SPL USDC on Solana mainnet-beta.
pub const CIRCLE_USDC_MAINNET: Pubkey =
    pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

/// Circle official SPL USDC on Solana devnet (Circle faucet mint).
pub const CIRCLE_USDC_DEVNET: Pubkey =
    pubkey!("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU");

/// Cluster USDC mint. Default is mainnet (fail closed). Devnet builds use `--features devnet`.
#[cfg(feature = "devnet")]
pub const USDC_MINT: Pubkey = CIRCLE_USDC_DEVNET;

#[cfg(not(feature = "devnet"))]
pub const USDC_MINT: Pubkey = CIRCLE_USDC_MAINNET;
