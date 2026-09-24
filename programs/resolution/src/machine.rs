//! Pure resolution state machine (FR-RES-02, FR-RES-03). No Session. No auto-oracle.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Open = 0,
    Proposed = 1,
    Voting = 2,
    Finalized = 3,
    Failed = 4,
    Voided = 5,
}

impl Phase {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Open),
            1 => Some(Self::Proposed),
            2 => Some(Self::Voting),
            3 => Some(Self::Finalized),
            4 => Some(Self::Failed),
            5 => Some(Self::Voided),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub phase: Phase,
    pub now: i64,
    pub close_ts: i64,
    pub report_deadline: i64,
    pub challenge_end: i64,
    pub vote_end: i64,
    pub extensions: u8,
    pub m: u8,
    pub n: u8,
    pub votes_proposal: u8,
    pub votes_challenge: u8,
    pub early_ok: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Submit,
    Challenge,
    VotePassProposal,
    VotePassChallenge,
    FinalizeQuiet,
    VoteTimeout,
    ReportTimeout,
    Void,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    BecomeProposed,
    BecomeVoting,
    FinalizeProposal,
    FinalizeChallenge,
    ExtendOnce,
    Fail,
    Void,
}

pub fn apply(case: &Case, ev: Event) -> Result<Effect, &'static str> {
    if matches!(case.phase, Phase::Finalized | Phase::Failed | Phase::Voided) {
        return Err("terminal");
    }
    match ev {
        Event::Submit => {
            if case.now < case.close_ts && !case.early_ok {
                return Err("market still open");
            }
            if case.now >= case.report_deadline {
                return Err("report window closed");
            }
            if case.phase != Phase::Open {
                return Err("cannot submit");
            }
            Ok(Effect::BecomeProposed)
        }
        Event::Challenge => {
            if case.phase != Phase::Proposed {
                return Err("no proposal");
            }
            if case.now >= case.challenge_end {
                return Err("challenge window closed");
            }
            Ok(Effect::BecomeVoting)
        }
        Event::FinalizeQuiet => {
            if case.phase != Phase::Proposed {
                return Err("not proposed");
            }
            if case.now < case.challenge_end {
                return Err("challenge window still open");
            }
            Ok(Effect::FinalizeProposal)
        }
        Event::VotePassProposal => {
            if case.phase != Phase::Voting {
                return Err("not voting");
            }
            if case.votes_proposal < case.m {
                return Err("below M");
            }
            Ok(Effect::FinalizeProposal)
        }
        Event::VotePassChallenge => {
            if case.phase != Phase::Voting {
                return Err("not voting");
            }
            if case.votes_challenge < case.m {
                return Err("below M");
            }
            Ok(Effect::FinalizeChallenge)
        }
        Event::VoteTimeout => {
            if case.phase != Phase::Voting {
                return Err("not voting");
            }
            if case.now < case.vote_end {
                return Err("vote still open");
            }
            if case.votes_proposal >= case.m {
                return Ok(Effect::FinalizeProposal);
            }
            if case.votes_challenge >= case.m {
                return Ok(Effect::FinalizeChallenge);
            }
            if case.extensions == 0 {
                Ok(Effect::ExtendOnce)
            } else {
                Ok(Effect::Fail)
            }
        }
        Event::ReportTimeout => {
            if case.phase != Phase::Open {
                return Err("not open");
            }
            if case.now < case.report_deadline {
                return Err("report window still open");
            }
            if case.extensions == 0 {
                Ok(Effect::ExtendOnce)
            } else {
                Ok(Effect::Fail)
            }
        }
        Event::Void => {
            if case.now < case.close_ts {
                return Err("void only after close");
            }
            Ok(Effect::Void)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Case {
        Case {
            phase: Phase::Open,
            now: 100,
            close_ts: 90,
            report_deadline: 200,
            challenge_end: 150,
            vote_end: 180,
            extensions: 0,
            m: 2,
            n: 3,
            votes_proposal: 0,
            votes_challenge: 0,
            early_ok: false,
        }
    }

    #[test]
    fn only_submit_result_starts_x() {
        let mut c = base();
        c.now = 80;
        assert_eq!(apply(&c, Event::Submit), Err("market still open"));
        c.now = 100;
        assert_eq!(apply(&c, Event::Submit), Ok(Effect::BecomeProposed));
    }

    #[test]
    fn quiet_finalize_after_challenge_window() {
        let mut c = base();
        c.phase = Phase::Proposed;
        c.now = 140;
        assert_eq!(apply(&c, Event::FinalizeQuiet), Err("challenge window still open"));
        c.now = 150;
        assert_eq!(apply(&c, Event::FinalizeQuiet), Ok(Effect::FinalizeProposal));
    }

    #[test]
    fn challenge_then_m_of_n() {
        let mut c = base();
        c.phase = Phase::Proposed;
        c.now = 120;
        assert_eq!(apply(&c, Event::Challenge), Ok(Effect::BecomeVoting));
        c.phase = Phase::Voting;
        c.votes_proposal = 2;
        assert_eq!(apply(&c, Event::VotePassProposal), Ok(Effect::FinalizeProposal));
    }

    #[test]
    fn failed_vote_extends_once_then_fails() {
        let mut c = base();
        c.phase = Phase::Voting;
        c.now = 180;
        c.votes_proposal = 1;
        c.votes_challenge = 1;
        assert_eq!(apply(&c, Event::VoteTimeout), Ok(Effect::ExtendOnce));
        c.extensions = 1;
        assert_eq!(apply(&c, Event::VoteTimeout), Ok(Effect::Fail));
    }

    #[test]
    fn terminal_rejects_new_submit() {
        let mut c = base();
        c.phase = Phase::Finalized;
        assert_eq!(apply(&c, Event::Submit), Err("terminal"));
        c.phase = Phase::Failed;
        assert_eq!(apply(&c, Event::Challenge), Err("terminal"));
    }

    #[test]
    fn missed_report_extends_once_then_fails() {
        let mut c = base();
        c.now = 150;
        assert_eq!(apply(&c, Event::ReportTimeout), Err("report window still open"));
        c.now = 200;
        assert_eq!(apply(&c, Event::ReportTimeout), Ok(Effect::ExtendOnce));
        c.extensions = 1;
        c.report_deadline = 260;
        c.now = 260;
        assert_eq!(apply(&c, Event::ReportTimeout), Ok(Effect::Fail));
    }

    #[test]
    fn early_resolve_can_submit_before_close() {
        let mut c = base();
        c.now = 80;
        c.early_ok = true;
        assert_eq!(apply(&c, Event::Submit), Ok(Effect::BecomeProposed));
        c.early_ok = false;
        assert_eq!(apply(&c, Event::Submit), Err("market still open"));
    }

    #[test]
    fn void_only_after_close() {
        let mut c = base();
        c.now = 80;
        assert_eq!(apply(&c, Event::Void), Err("void only after close"));
        c.now = 100;
        assert_eq!(apply(&c, Event::Void), Ok(Effect::Void));
    }
}
