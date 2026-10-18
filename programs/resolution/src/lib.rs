//! L1 resolution. $x^*$ only via committee `submit_result`.
//! Never Delegates. Session is not an authority. No oracle writes x*.

use anchor_lang::prelude::*;
use market::state::{Market, Status, MAX_COMMITTEE};

pub mod machine;

use machine::{apply, Case, Effect, Event, Phase};

declare_id!("Rso1111111111111111111111111111111111111111");

pub const RES_SEED: &[u8] = b"res";
pub const VOTE_SEED: &[u8] = b"vote";
pub const VAULT_ID: Pubkey = pubkey!("VaULt11111111111111111111111111111111111111");
pub const CBOND_SEED: &[u8] = b"cbond";
pub const CHBOND_SEED: &[u8] = b"chbond";
/// Ballot seed for VOID endorsements (distinct from challenge vote ballots).
pub const VOID_SEED: &[u8] = b"vball";

pub const FAMILY_SKELLAM: u8 = 0;
pub const FAMILY_GAUSSIAN: u8 = 1;
pub const FAMILY_LOGNORMAL: u8 = 2;
pub const FAMILY_DIRICHLET: u8 = 3;
pub const FAMILY_BERNOULLI: u8 = 4;

#[program]
pub mod resolution {
    use super::*;

    pub fn open(ctx: Context<Open>) -> Result<()> {
        let market = &ctx.accounts.market;
        let signer = ctx.accounts.payer.key();
        require!(
            ctx.accounts.committee.key() == market.committee,
            ResError::CommitteeMismatch
        );
        require!(
            signer == market.creator || ctx.accounts.committee.is_member(&signer),
            ResError::NotReporter
        );
        require!(market.family <= FAMILY_BERNOULLI, ResError::BadFamily);
        require!(
            ctx.accounts.committee.member_count >= 1
                && ctx.accounts.committee.m >= 1
                && ctx.accounts.committee.m <= ctx.accounts.committee.member_count
                && (ctx.accounts.committee.m as u16) * 2
                    > ctx.accounts.committee.member_count as u16
                && market.report_window_secs > 0
                && market.challenge_secs > 0,
            ResError::BadCommittee
        );

        let rec = &mut ctx.accounts.record;
        rec.market = market.key();
        rec.members = ctx.accounts.committee.members;
        rec.authorized_reporter = market.authorized_reporter;
        rec.family = market.family;
        rec.phase = Phase::Open as u8;
        rec.m = ctx.accounts.committee.m;
        rec.n = ctx.accounts.committee.member_count;
        rec.extensions = 0;
        rec.votes_proposal = 0;
        rec.votes_challenge = 0;
        rec.refunds_due = false;
        rec.early_resolve = market.family == FAMILY_BERNOULLI && market.extra.u0 == 1;
        rec.k_max = market.extra.u2;
        rec.layout = market.extra.u0;
        rec.n_atoms = market.n;
        rec.close_ts = market.close_ts;
        rec.report_open_ts = market.report_opens_at();
        rec.report_window_secs = market.report_window_secs;
        rec.report_deadline = rec
            .report_open_ts
            .saturating_add(market.report_window_secs);
        rec.challenge_secs = market.challenge_secs;
        rec.challenge_end = 0;
        rec.vote_end = 0;
        rec.proposer = Pubkey::default();
        rec.challenger = Pubkey::default();
        rec.proposed = Outcome::default();
        rec.challenged = Outcome::default();
        rec.final_outcome = Outcome::default();
        rec.evidence_hash = [0u8; 32];
        rec.slash_due = false;
        rec.void_votes = 0;
        rec.bump = ctx.bumps.record;
        Ok(())
    }

    pub fn submit_result(
        ctx: Context<SubmitRecord>,
        outcome: Outcome,
        evidence_hash: [u8; 32],
    ) -> Result<()> {
        let rec = &mut ctx.accounts.record;
        let now = Clock::get()?.unix_timestamp;
        let reporter = ctx.accounts.reporter.key();
        require!(can_propose(rec, &reporter), ResError::NotReporter);
        // Bernoulli early occurrence: once Halted, only VOID — never settle YES.
        require!(
            !(rec.early_resolve && ctx.accounts.market.status == Status::Halted as u8),
            ResError::MustVoid
        );
        require!(
            bond_meets_min(
                &ctx.accounts.bond.to_account_info(),
                rec.market,
                ctx.accounts.market.committee_bond,
            ),
            ResError::BondNotLocked
        );
        // Proposer must be the cbond holder — blocks ExtendOnce "free ride" on another member's lock.
        require!(
            bond_holder_is(&ctx.accounts.bond.to_account_info(), &reporter),
            ResError::BondNotLocked
        );
        validate_outcome(rec.family, rec.n_atoms, &outcome)?;

        let effect = apply(&snapshot(rec, now), Event::Submit).map_err(|_| error!(ResError::BadPhase))?;
        require!(effect == Effect::BecomeProposed, ResError::BadPhase);
        rec.phase = Phase::Proposed as u8;
        rec.proposer = reporter;
        rec.proposed = outcome;
        rec.challenged = Outcome::default();
        rec.votes_proposal = 0;
        rec.votes_challenge = 0;
        // Incomplete VOID ballots are abandoned once a proposal stands (avoids single-vote DoS).
        rec.void_votes = 0;
        rec.challenge_end = now.saturating_add(rec.challenge_secs);
        rec.evidence_hash = evidence_hash;
        rec.refunds_due = false;
        Ok(())
    }

    pub fn challenge(ctx: Context<ChallengeRecord>, outcome: Outcome) -> Result<()> {
        let rec = &mut ctx.accounts.record;
        let now = Clock::get()?.unix_timestamp;
        require!(is_member(rec, &ctx.accounts.reporter.key()), ResError::NotReporter);
        require!(
            !halted_early(rec.early_resolve, ctx.accounts.market.status),
            ResError::MustVoid
        );
        require!(ctx.accounts.reporter.key() != rec.proposer, ResError::SelfChallenge);
        require!(
            bond_meets_min(
                &ctx.accounts.bond.to_account_info(),
                rec.market,
                ctx.accounts.market.committee_bond,
            ),
            ResError::BondNotLocked
        );
        // Challenger bond PDA must be locked by this reporter (not the proposer's cbond).
        require!(
            bond_holder_is(&ctx.accounts.bond.to_account_info(), &ctx.accounts.reporter.key()),
            ResError::BondNotLocked
        );
        validate_outcome(rec.family, rec.n_atoms, &outcome)?;
        require!(!outcome.same_as(&rec.proposed), ResError::SameOutcome);
        let effect = apply(&snapshot(rec, now), Event::Challenge).map_err(|_| error!(ResError::BadPhase))?;
        require!(effect == Effect::BecomeVoting, ResError::BadPhase);
        rec.phase = Phase::Voting as u8;
        rec.challenger = ctx.accounts.reporter.key();
        rec.challenged = outcome;
        rec.vote_end = now.saturating_add(rec.challenge_secs);
        rec.votes_proposal = 0;
        rec.votes_challenge = 0;
        Ok(())
    }

    pub fn vote(ctx: Context<CastVote>, for_challenge: bool) -> Result<()> {
        let rec = &mut ctx.accounts.record;
        require!(rec.phase == Phase::Voting as u8, ResError::BadPhase);
        let now = Clock::get()?.unix_timestamp;
        require!(now < rec.vote_end, ResError::BadWindow);
        require!(is_member(rec, &ctx.accounts.voter.key()), ResError::NotReporter);
        let ballot = &mut ctx.accounts.ballot;
        require!(!ballot.cast, ResError::AlreadyVoted);
        ballot.record = rec.key();
        ballot.voter = ctx.accounts.voter.key();
        ballot.for_challenge = for_challenge;
        ballot.cast = true;
        ballot.bump = ctx.bumps.ballot;
        if for_challenge {
            rec.votes_challenge = rec.votes_challenge.saturating_add(1);
        } else {
            rec.votes_proposal = rec.votes_proposal.saturating_add(1);
        }
        Ok(())
    }

    pub fn finalize(ctx: Context<Finalize>) -> Result<()> {
        require!(ctx.accounts.market.key() == ctx.accounts.record.market, ResError::CommitteeMismatch);
        let now = Clock::get()?.unix_timestamp;
        let halted = ctx.accounts.market.status;
        let sync_status = {
            let rec = &mut ctx.accounts.record;
            let snap = snapshot(rec, now);
            let ev = if rec.phase == Phase::Proposed as u8 {
                Event::FinalizeQuiet
            } else if rec.phase == Phase::Open as u8 {
                Event::ReportTimeout
            } else if rec.votes_proposal >= rec.m {
                Event::VotePassProposal
            } else if rec.votes_challenge >= rec.m {
                Event::VotePassChallenge
            } else {
                Event::VoteTimeout
            };
            let effect = apply(&snap, ev).map_err(|_| error!(ResError::BadPhase))?;
            match effect {
                Effect::FinalizeProposal => {
                    require!(
                        !halted_early(rec.early_resolve, halted),
                        ResError::MustVoid
                    );
                    rec.phase = Phase::Finalized as u8;
                    rec.final_outcome = rec.proposed;
                    rec.refunds_due = false;
                    true
                }
                Effect::FinalizeChallenge => {
                    require!(
                        !halted_early(rec.early_resolve, halted),
                        ResError::MustVoid
                    );
                    rec.phase = Phase::Finalized as u8;
                    rec.final_outcome = rec.challenged;
                    rec.refunds_due = false;
                    true
                }
                Effect::ExtendOnce => {
                    rec.phase = Phase::Open as u8;
                    rec.extensions = 1;
                    rec.report_deadline = now.saturating_add(rec.report_window_secs);
                    rec.votes_proposal = 0;
                    rec.votes_challenge = 0;
                    // Fresh VOID tally for the extended window (old vball seeds are dead).
                    rec.void_votes = 0;
                    rec.proposer = Pubkey::default();
                    rec.challenger = Pubkey::default();
                    rec.proposed = Outcome::default();
                    rec.challenged = Outcome::default();
                    rec.refunds_due = false;
                    false
                }
                Effect::Fail => {
                    rec.phase = Phase::Failed as u8;
                    rec.refunds_due = true;
                    true
                }
                Effect::AwaitAdmin => {
                    // Close the report window in place. AdminSubmit slashes; AdminVoid refunds.
                    rec.phase = Phase::Open as u8;
                    rec.extensions = rec.extensions.saturating_add(1).max(1);
                    rec.report_deadline = now;
                    rec.votes_proposal = 0;
                    rec.votes_challenge = 0;
                    rec.void_votes = 0;
                    rec.proposer = Pubkey::default();
                    rec.challenger = Pubkey::default();
                    rec.proposed = Outcome::default();
                    rec.challenged = Outcome::default();
                    rec.refunds_due = false;
                    false
                }
                _ => return err!(ResError::BadPhase),
            }
        };
        if sync_status {
            // Market reads the account bytes, not this program's in-memory copy.
            ctx.accounts.record.exit(&crate::ID)?;
            sync_market_status(
                ctx.accounts.market_program.to_account_info(),
                ctx.accounts.market.to_account_info(),
                ctx.accounts.record.to_account_info(),
            )?;
        }
        Ok(())
    }

    /// M-of-N VOID: each member endorses once; transition runs when `void_votes >= m`.
    pub fn void_resolution(ctx: Context<VoidRecord>) -> Result<()> {
        require!(ctx.accounts.market.key() == ctx.accounts.record.market, ResError::CommitteeMismatch);
        let now = Clock::get()?.unix_timestamp;
        // Before close_ts the market must already be halted so fills cannot continue.
        if now < ctx.accounts.record.close_ts {
            require!(
                ctx.accounts.market.status == Status::Halted as u8,
                ResError::BadPhase
            );
        }
        let sync = {
            let rec = &mut ctx.accounts.record;
            require!(is_member(rec, &ctx.accounts.reporter.key()), ResError::NotReporter);
            // Machine must still allow Void (Open, window, early_ok).
            apply(&snapshot(rec, now), Event::Void).map_err(|_| error!(ResError::BadPhase))?;
            let ballot = &mut ctx.accounts.ballot;
            require!(!ballot.cast, ResError::AlreadyVoted);
            ballot.record = rec.key();
            ballot.voter = ctx.accounts.reporter.key();
            ballot.for_challenge = false;
            ballot.cast = true;
            ballot.bump = ctx.bumps.ballot;
            rec.void_votes = rec.void_votes.saturating_add(1);
            if rec.void_votes < rec.m {
                return Ok(());
            }
            rec.phase = Phase::Voided as u8;
            rec.refunds_due = true;
            true
        };
        if sync {
            ctx.accounts.record.exit(&crate::ID)?;
            sync_market_status(
                ctx.accounts.market_program.to_account_info(),
                ctx.accounts.market.to_account_info(),
                ctx.accounts.record.to_account_info(),
            )?;
        }
        Ok(())
    }

    pub fn admin_submit_result(
        ctx: Context<AdminSubmit>,
        outcome: Outcome,
        evidence_hash: [u8; 32],
    ) -> Result<()> {
        require!(
            ctx.accounts.reporter.key() == ctx.accounts.market.platform,
            ResError::NotAdmin
        );
        require!(ctx.accounts.market.key() == ctx.accounts.record.market, ResError::CommitteeMismatch);
        // Same gate as submit_result: early_resolve + Halted ⇒ VOID only.
        require!(
            !(ctx.accounts.record.early_resolve
                && ctx.accounts.market.status == Status::Halted as u8),
            ResError::MustVoid
        );
        // Slash needs a locked bond; otherwise platform must `admin_void_resolution`.
        require!(
            bond_meets_min(
                &ctx.accounts.bond.to_account_info(),
                ctx.accounts.record.market,
                ctx.accounts.market.committee_bond,
            ),
            ResError::BondNotLocked
        );
        {
            let rec = &mut ctx.accounts.record;
            let now = Clock::get()?.unix_timestamp;
            validate_outcome(rec.family, rec.n_atoms, &outcome)?;
            apply(&snapshot(rec, now), Event::AdminSubmit).map_err(|_| error!(ResError::AwaitingAdmin))?;
            rec.phase = Phase::Finalized as u8;
            rec.proposer = ctx.accounts.reporter.key();
            rec.proposed = outcome;
            rec.final_outcome = outcome;
            rec.evidence_hash = evidence_hash;
            rec.slash_due = true;
            rec.refunds_due = false;
        }
        ctx.accounts.record.exit(&crate::ID)?;
        sync_market_status(
            ctx.accounts.market_program.to_account_info(),
            ctx.accounts.market.to_account_info(),
            ctx.accounts.record.to_account_info(),
        )?;
        Ok(())
    }

    pub fn admin_void_resolution(ctx: Context<Finalize>) -> Result<()> {
        require!(
            ctx.accounts.reporter.key() == ctx.accounts.market.platform,
            ResError::NotAdmin
        );
        require!(ctx.accounts.market.key() == ctx.accounts.record.market, ResError::CommitteeMismatch);
        {
            let rec = &mut ctx.accounts.record;
            let now = Clock::get()?.unix_timestamp;
            apply(&snapshot(rec, now), Event::AdminVoid).map_err(|_| error!(ResError::AwaitingAdmin))?;
            rec.phase = Phase::Voided as u8;
            rec.slash_due = false;
            rec.refunds_due = true;
        }
        ctx.accounts.record.exit(&crate::ID)?;
        sync_market_status(
            ctx.accounts.market_program.to_account_info(),
            ctx.accounts.market.to_account_info(),
            ctx.accounts.record.to_account_info(),
        )?;
        Ok(())
    }
}

fn halted_early(early_resolve: bool, status: u8) -> bool {
    early_resolve && status == Status::Halted as u8
}

fn sync_market_status<'info>(
    market_program: AccountInfo<'info>,
    market: AccountInfo<'info>,
    record: AccountInfo<'info>,
) -> Result<()> {
    market::cpi::apply_resolution_status(CpiContext::new(
        market_program,
        market::cpi::accounts::ApplyResolutionStatus { market, record },
    ))
}

fn snapshot(rec: &Resolution, now: i64) -> Case {
    Case {
        phase: Phase::from_u8(rec.phase).unwrap_or(Phase::Open),
        now,
        close_ts: rec.close_ts,
        report_open_ts: if rec.report_open_ts > 0 {
            rec.report_open_ts
        } else {
            rec.close_ts
        },
        report_deadline: rec.report_deadline,
        challenge_end: rec.challenge_end,
        vote_end: rec.vote_end,
        extensions: rec.extensions,
        m: rec.m,
        n: rec.n,
        votes_proposal: rec.votes_proposal,
        votes_challenge: rec.votes_challenge,
        early_ok: rec.early_resolve,
    }
}

fn is_member(rec: &Resolution, who: &Pubkey) -> bool {
    rec.members
        .iter()
        .take(rec.n as usize)
        .any(|m| m == who)
}

fn can_propose(rec: &Resolution, who: &Pubkey) -> bool {
    is_member(rec, who)
        || (rec.authorized_reporter != Pubkey::default() && rec.authorized_reporter == *who)
}

fn bond_meets_min(bond: &AccountInfo, market: Pubkey, min_bond: u64) -> bool {
    if *bond.owner != VAULT_ID {
        return false;
    }
    let data = match bond.try_borrow_data() {
        Ok(d) => d,
        Err(_) => return false,
    };
    if data.len() < 81 {
        return false;
    }
    let mut m = [0u8; 32];
    m.copy_from_slice(&data[8..40]);
    if Pubkey::new_from_array(m) != market {
        return false;
    }
    let amount = u64::from_le_bytes(data[72..80].try_into().unwrap_or([0; 8]));
    let slashed = data[80] != 0;
    let need = min_bond.max(1);
    amount >= need && !slashed
}

/// CommitteeBond layout: disc(8) + market(32) + holder(32) + amount(8) + slashed(1).
fn bond_holder_is(bond: &AccountInfo, who: &Pubkey) -> bool {
    let data = match bond.try_borrow_data() {
        Ok(d) => d,
        Err(_) => return false,
    };
    if data.len() < 72 {
        return false;
    }
    let mut h = [0u8; 32];
    h.copy_from_slice(&data[40..72]);
    Pubkey::new_from_array(h) == *who
}

fn validate_outcome(family: u8, n_atoms: u16, o: &Outcome) -> Result<()> {
    require!(o.family == family, ResError::FamilyMismatch);
    match family {
        FAMILY_SKELLAM => {
            require!(o.a >= 0 && o.b >= 0, ResError::BadOutcome);
            // `outcome_cell` casts to u32. Larger scores must not collapse onto a low cell.
            require!(
                o.a <= u32::MAX as i128 && o.b <= u32::MAX as i128,
                ResError::BadOutcome
            );
            require!(o.kind == 0, ResError::BadOutcome);
        }
        FAMILY_GAUSSIAN => {
            require!(o.kind == 1, ResError::BadOutcome);
        }
        FAMILY_LOGNORMAL => {
            require!(o.kind == 1, ResError::BadOutcome);
            require!(o.a > 0, ResError::BadOutcome);
        }
        FAMILY_DIRICHLET => {
            // kind=3 share-vector cannot map via outcome_cell → begin_settle would stick forever.
            require!(o.kind == 2, ResError::BadOutcome);
            require!(o.a >= 0 && o.a < n_atoms as i128, ResError::BadOutcome);
        }
        FAMILY_BERNOULLI => {
            require!(o.kind == 4, ResError::BadOutcome);
            require!(o.a == 0 || o.a == 1, ResError::BadOutcome);
        }
        _ => return err!(ResError::BadFamily),
    }
    Ok(())
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, PartialEq, Eq)]
pub struct Outcome {
    pub family: u8,
    /// 0 score pair, 1 scalar, 2 dirichlet atom, 3 share vector, 4 yes/no
    pub kind: u8,
    pub a: i128,
    pub b: i128,
    pub shares: [i128; 8],
}

impl Outcome {
    pub fn same_as(&self, other: &Self) -> bool {
        self.family == other.family
            && self.kind == other.kind
            && self.a == other.a
            && self.b == other.b
            && self.shares == other.shares
    }
}

#[account]
#[derive(Default)]
pub struct Resolution {
    pub market: Pubkey,
    pub members: [Pubkey; MAX_COMMITTEE],
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
    pub report_open_ts: i64,
    /// Set when the platform writes $x^*$ after a missed report window.
    pub slash_due: bool,
    /// Distinct member endorsements toward M-of-N VOID.
    pub void_votes: u8,
}

impl Resolution {
    pub const SIZE: usize = 8 + 1536;
}

#[account]
pub struct Ballot {
    pub record: Pubkey,
    pub voter: Pubkey,
    pub for_challenge: bool,
    pub cast: bool,
    pub bump: u8,
}

impl Ballot {
    pub const SIZE: usize = 8 + 32 + 32 + 1 + 1 + 1;
}

#[derive(Accounts)]
pub struct Open<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(owner = market::ID)]
    pub market: Box<Account<'info, Market>>,
    #[account(
        owner = market::ID,
        constraint = committee.key() == market.committee @ ResError::CommitteeMismatch
    )]
    pub committee: Box<Account<'info, market::state::Committee>>,
    #[account(
        init,
        payer = payer,
        space = Resolution::SIZE,
        seeds = [RES_SEED, market.key().as_ref()],
        bump
    )]
    pub record: Box<Account<'info, Resolution>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MutRecord<'info> {
    pub reporter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
}

#[derive(Accounts)]
pub struct SubmitRecord<'info> {
    pub reporter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
    #[account(
        owner = market::ID,
        constraint = market.key() == record.market @ ResError::CommitteeMismatch
    )]
    pub market: Box<Account<'info, Market>>,
    /// CHECK: vault `CommitteeBond` PDA for this market.
    #[account(
        seeds = [CBOND_SEED, record.market.as_ref()],
        bump,
        seeds::program = VAULT_ID
    )]
    pub bond: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct ChallengeRecord<'info> {
    pub reporter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
    #[account(
        owner = market::ID,
        constraint = market.key() == record.market @ ResError::CommitteeMismatch
    )]
    pub market: Box<Account<'info, Market>>,
    /// CHECK: vault challenger bond PDA `["chbond", market]`.
    #[account(
        seeds = [CHBOND_SEED, record.market.as_ref()],
        bump,
        seeds::program = VAULT_ID
    )]
    pub bond: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct Finalize<'info> {
    pub reporter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
    #[account(mut, constraint = market.key() == record.market @ ResError::CommitteeMismatch)]
    pub market: Box<Account<'info, Market>>,
    pub market_program: Program<'info, market::program::Market>,
}

#[derive(Accounts)]
pub struct AdminSubmit<'info> {
    pub reporter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
    #[account(mut, constraint = market.key() == record.market @ ResError::CommitteeMismatch)]
    pub market: Box<Account<'info, Market>>,
    pub market_program: Program<'info, market::program::Market>,
    /// CHECK: vault committee bond PDA — must be locked so `slash_due` has teeth.
    #[account(
        seeds = [CBOND_SEED, record.market.as_ref()],
        bump,
        seeds::program = VAULT_ID
    )]
    pub bond: UncheckedAccount<'info>,
}

#[derive(Accounts)]
pub struct VoidRecord<'info> {
    #[account(mut)]
    pub reporter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
    #[account(mut, constraint = market.key() == record.market @ ResError::CommitteeMismatch)]
    pub market: Box<Account<'info, Market>>,
    pub market_program: Program<'info, market::program::Market>,
    #[account(
        init,
        payer = reporter,
        space = Ballot::SIZE,
        seeds = [
            VOID_SEED,
            record.key().as_ref(),
            reporter.key().as_ref(),
            &[record.extensions]
        ],
        bump
    )]
    pub ballot: Account<'info, Ballot>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CastVote<'info> {
    #[account(mut)]
    pub voter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
    #[account(
        init,
        payer = voter,
        space = Ballot::SIZE,
        seeds = [
            VOTE_SEED,
            record.key().as_ref(),
            voter.key().as_ref(),
            &[record.extensions]
        ],
        bump
    )]
    pub ballot: Account<'info, Ballot>,
    pub system_program: Program<'info, System>,
}

#[error_code]
pub enum ResError {
    #[msg("committee M/N is illegal")]
    BadCommittee,
    #[msg("report or challenge window is illegal")]
    BadWindow,
    #[msg("unknown distribution family")]
    BadFamily,
    #[msg("signer is not committee or authorized reporter")]
    NotReporter,
    #[msg("signer is not the protocol platform (super-admin)")]
    NotAdmin,
    #[msg("report window closed; platform must submit or VOID")]
    AwaitingAdmin,
    #[msg("phase does not allow this instruction")]
    BadPhase,
    #[msg("outcome does not match the locked family")]
    FamilyMismatch,
    #[msg("outcome payload is illegal for this family")]
    BadOutcome,
    #[msg("challenger is the proposer")]
    SelfChallenge,
    #[msg("challenge repeats the proposal")]
    SameOutcome,
    #[msg("voter already cast a ballot")]
    AlreadyVoted,
    #[msg("roster does not include the market committee")]
    CommitteeMismatch,
    #[msg("committee bond is not locked")]
    BondNotLocked,
    #[msg("early_resolve market is Halted; only VOID is allowed")]
    MustVoid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skellam_rejects_line_name() {
        let o = Outcome {
            family: FAMILY_SKELLAM,
            kind: 4,
            a: 1,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_SKELLAM, 121, &o).is_err());
        let score = Outcome {
            family: FAMILY_SKELLAM,
            kind: 0,
            a: 2,
            b: 1,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_SKELLAM, 121, &score).is_ok());
    }

    #[test]
    fn bernoulli_is_yes_or_no() {
        let yes = Outcome {
            family: FAMILY_BERNOULLI,
            kind: 4,
            a: 1,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_BERNOULLI, 2, &yes).is_ok());
        let bad = Outcome {
            family: FAMILY_BERNOULLI,
            kind: 4,
            a: 2,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_BERNOULLI, 2, &bad).is_err());
    }

    #[test]
    fn cannot_submit_wrong_family() {
        let o = Outcome {
            family: FAMILY_BERNOULLI,
            kind: 4,
            a: 1,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_GAUSSIAN, 16, &o).is_err());
    }

    #[test]
    fn lognormal_scalar_must_be_positive() {
        let bad = Outcome {
            family: FAMILY_LOGNORMAL,
            kind: 1,
            a: 0,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_LOGNORMAL, 32, &bad).is_err());
        let ok = Outcome {
            family: FAMILY_LOGNORMAL,
            kind: 1,
            a: 1i128 << 64,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_LOGNORMAL, 32, &ok).is_ok());
    }

    #[test]
    fn dirichlet_atom_must_be_on_the_board() {
        let o = Outcome {
            family: FAMILY_DIRICHLET,
            kind: 2,
            a: 4,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_DIRICHLET, 4, &o).is_err());
        let huge = Outcome {
            family: FAMILY_SKELLAM,
            kind: 0,
            a: (u32::MAX as i128) + 1,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_SKELLAM, 121, &huge).is_err());
        let edge = Outcome {
            family: FAMILY_SKELLAM,
            kind: 0,
            a: u32::MAX as i128,
            b: u32::MAX as i128,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_SKELLAM, 121, &edge).is_ok());
        let ok = Outcome {
            family: FAMILY_DIRICHLET,
            kind: 2,
            a: 3,
            b: 0,
            shares: [0; 8],
        };
        assert!(validate_outcome(FAMILY_DIRICHLET, 4, &ok).is_ok());
    }

    #[test]
    fn roster_membership() {
        let mut rec = Resolution {
            members: [Pubkey::default(); MAX_COMMITTEE],
            n: 2,
            ..Resolution::default()
        };
        rec.members[0] = Pubkey::new_from_array([1u8; 32]);
        rec.members[1] = Pubkey::new_from_array([2u8; 32]);
        assert!(is_member(&rec, &rec.members[0]));
        assert!(!is_member(&rec, &Pubkey::new_from_array([3u8; 32])));
        rec.authorized_reporter = Pubkey::new_from_array([9u8; 32]);
        assert!(can_propose(&rec, &rec.authorized_reporter));
        assert!(!is_member(&rec, &rec.authorized_reporter));
    }

    #[test]
    fn resolution_account_fits() {
        let mut rec = Resolution::default();
        rec.phase = Phase::Finalized as u8;
        let mut data = Vec::new();
        rec.try_serialize(&mut data).unwrap();
        assert!(
            data.len() <= Resolution::SIZE,
            "serialized {} > SIZE {}",
            data.len(),
            Resolution::SIZE
        );
        // market::apply_resolution_status reads phase at disc+market+members+reporter+family.
        const PHASE_OFF: usize = 8 + 32 + 512 + 32 + 1;
        assert_eq!(data[PHASE_OFF], Phase::Finalized as u8);
    }
}
