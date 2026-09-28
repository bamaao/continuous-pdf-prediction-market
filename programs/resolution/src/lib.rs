//! L1 resolution. $x^*$ only via committee `submit_result`.
//! Never Delegates. Session is not an authority. No oracle writes x*.

use anchor_lang::prelude::*;
use market::state::{Market, Status, MAX_COMMITTEE};

pub mod machine;

use machine::{apply, Case, Effect, Event, Phase};

declare_id!("Rso1111111111111111111111111111111111111111");

pub const RES_SEED: &[u8] = b"res";
pub const VOTE_SEED: &[u8] = b"vote";

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
        rec.report_window_secs = market.report_window_secs;
        rec.report_deadline = market.close_ts.saturating_add(market.report_window_secs);
        rec.challenge_secs = market.challenge_secs;
        rec.challenge_end = 0;
        rec.vote_end = 0;
        rec.proposer = Pubkey::default();
        rec.challenger = Pubkey::default();
        rec.proposed = Outcome::default();
        rec.challenged = Outcome::default();
        rec.final_outcome = Outcome::default();
        rec.evidence_hash = [0u8; 32];
        rec.bump = ctx.bumps.record;
        Ok(())
    }

    pub fn submit_result(
        ctx: Context<MutRecord>,
        outcome: Outcome,
        evidence_hash: [u8; 32],
    ) -> Result<()> {
        let rec = &mut ctx.accounts.record;
        let now = Clock::get()?.unix_timestamp;
        let reporter = ctx.accounts.reporter.key();
        require!(can_propose(rec, &reporter), ResError::NotReporter);
        validate_outcome(rec.family, rec.n_atoms, &outcome)?;

        let effect = apply(&snapshot(rec, now), Event::Submit).map_err(|_| error!(ResError::BadPhase))?;
        require!(effect == Effect::BecomeProposed, ResError::BadPhase);
        rec.phase = Phase::Proposed as u8;
        rec.proposer = reporter;
        rec.proposed = outcome;
        rec.challenged = Outcome::default();
        rec.votes_proposal = 0;
        rec.votes_challenge = 0;
        rec.challenge_end = now.saturating_add(rec.challenge_secs);
        rec.evidence_hash = evidence_hash;
        rec.refunds_due = false;
        Ok(())
    }

    pub fn challenge(ctx: Context<MutRecord>, outcome: Outcome) -> Result<()> {
        let rec = &mut ctx.accounts.record;
        let now = Clock::get()?.unix_timestamp;
        require!(ctx.accounts.reporter.key() != rec.proposer, ResError::SelfChallenge);
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
        let rec = &mut ctx.accounts.record;
        let now = Clock::get()?.unix_timestamp;
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
                rec.phase = Phase::Finalized as u8;
                rec.final_outcome = rec.proposed;
                rec.refunds_due = false;
                ctx.accounts.market.status = Status::Settled as u8;
            }
            Effect::FinalizeChallenge => {
                rec.phase = Phase::Finalized as u8;
                rec.final_outcome = rec.challenged;
                rec.refunds_due = false;
                ctx.accounts.market.status = Status::Settled as u8;
            }
            Effect::ExtendOnce => {
                rec.phase = Phase::Open as u8;
                rec.extensions = 1;
                rec.report_deadline = now.saturating_add(rec.report_window_secs);
                rec.votes_proposal = 0;
                rec.votes_challenge = 0;
                rec.proposer = Pubkey::default();
                rec.challenger = Pubkey::default();
                rec.proposed = Outcome::default();
                rec.challenged = Outcome::default();
                rec.refunds_due = false;
            }
            Effect::Fail => {
                rec.phase = Phase::Failed as u8;
                rec.refunds_due = true;
                ctx.accounts.market.status = Status::Void as u8;
            }
            _ => return err!(ResError::BadPhase),
        }
        Ok(())
    }

    pub fn void_resolution(ctx: Context<Finalize>) -> Result<()> {
        require!(ctx.accounts.market.key() == ctx.accounts.record.market, ResError::CommitteeMismatch);
        let rec = &mut ctx.accounts.record;
        require!(is_member(rec, &ctx.accounts.reporter.key()), ResError::NotReporter);
        let now = Clock::get()?.unix_timestamp;
        apply(&snapshot(rec, now), Event::Void).map_err(|_| error!(ResError::BadPhase))?;
        rec.phase = Phase::Voided as u8;
        rec.refunds_due = true;
        ctx.accounts.market.status = Status::Void as u8;
        Ok(())
    }
}

fn snapshot(rec: &Resolution, now: i64) -> Case {
    Case {
        phase: Phase::from_u8(rec.phase).unwrap_or(Phase::Open),
        now,
        close_ts: rec.close_ts,
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

fn validate_outcome(family: u8, n_atoms: u16, o: &Outcome) -> Result<()> {
    require!(o.family == family, ResError::FamilyMismatch);
    match family {
        FAMILY_SKELLAM => {
            require!(o.a >= 0 && o.b >= 0, ResError::BadOutcome);
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
            require!(o.kind == 2 || o.kind == 3, ResError::BadOutcome);
            if o.kind == 2 {
                require!(o.a >= 0 && o.a < n_atoms as i128, ResError::BadOutcome);
            } else {
                require!(o.shares.iter().all(|&s| s >= 0), ResError::BadOutcome);
                require!(o.shares.iter().any(|&s| s > 0), ResError::BadOutcome);
            }
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
pub struct Finalize<'info> {
    pub reporter: Signer<'info>,
    #[account(mut, seeds = [RES_SEED, record.market.as_ref()], bump = record.bump)]
    pub record: Box<Account<'info, Resolution>>,
    #[account(mut, constraint = market.key() == record.market @ ResError::CommitteeMismatch)]
    pub market: Box<Account<'info, Market>>,
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
        let rec = Resolution::default();
        let mut data = Vec::new();
        rec.try_serialize(&mut data).unwrap();
        assert!(
            data.len() <= Resolution::SIZE,
            "serialized {} > SIZE {}",
            data.len(),
            Resolution::SIZE
        );
    }
}
