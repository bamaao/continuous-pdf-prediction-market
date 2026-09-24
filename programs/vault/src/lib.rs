//! L1 USDC vault (FR-WAL-03, FR-SET-06, CR-05, CR-08).
//!
//! Allowed outflows: unused-margin withdraw, later settle / LP draw / surplus / VOID.
//! There is no `admin_withdraw`. Session keys are not authority.

use anchor_lang::prelude::*;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

declare_id!("VaULt11111111111111111111111111111111111111");

pub const VAULT_SEED: &[u8] = b"vault";
pub const USER_SEED: &[u8] = b"user";

#[program]
pub mod vault {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let cfg = &mut ctx.accounts.config;
        cfg.usdc_mint = ctx.accounts.usdc_mint.key();
        cfg.token_account = ctx.accounts.vault_ata.key();
        cfg.bump = ctx.bumps.config;
        Ok(())
    }

    /// Main-wallet deposit of official USDC. Credits `available`.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::ZeroAmount);
        require_keys_eq!(
            ctx.accounts.usdc_mint.key(),
            ctx.accounts.config.usdc_mint,
            VaultError::WrongMint
        );
        require_keys_eq!(
            ctx.accounts.user_ata.mint,
            ctx.accounts.config.usdc_mint,
            VaultError::WrongMint
        );

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
        require_keys_eq!(
            ctx.accounts.user_ata.mint,
            ctx.accounts.config.usdc_mint,
            VaultError::WrongMint
        );

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
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
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
    pub usdc_mint: Account<'info, Mint>,
    #[account(
        mut,
        constraint = vault_ata.key() == config.token_account @ VaultError::WrongVaultAta,
        constraint = vault_ata.mint == config.usdc_mint @ VaultError::WrongMint
    )]
    pub vault_ata: Account<'info, TokenAccount>,
    #[account(mut, constraint = user_ata.owner == owner.key() @ VaultError::NotOwner)]
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
        constraint = vault_ata.mint == config.usdc_mint @ VaultError::WrongMint
    )]
    pub vault_ata: Account<'info, TokenAccount>,
    #[account(mut, constraint = user_ata.owner == owner.key() @ VaultError::NotOwner)]
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
}

impl UserVault {
    pub const SIZE: usize = 32 + 8 + 8 + 1;
}

#[error_code]
pub enum VaultError {
    #[msg("amount must be > 0")]
    ZeroAmount,
    #[msg("mint is not the locked USDC mint")]
    WrongMint,
    #[msg("signer is not the user vault owner")]
    NotOwner,
    #[msg("insufficient unused margin")]
    InsufficientAvailable,
    #[msg("arithmetic overflow")]
    Overflow,
    #[msg("vault token account mismatch")]
    WrongVaultAta,
}

#[cfg(test)]
mod tests {
    use super::accounting::*;

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
}
