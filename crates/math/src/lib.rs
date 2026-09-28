//! Shared consensus numerics. Programs, Quote, and WASM must call this crate.
//! Public pricing APIs do not take or return IEEE floats.

pub mod auction;
pub mod football;
pub mod lmsr;
pub mod outcome;
pub mod prior;
pub mod q64;
pub mod settle;

pub use auction::{sort_bids, unit_premium, Bid};
pub use lmsr::{
    buy_cost, implied_probs, interval_prob, lmsr_update, marginal_price, uniform_prior, LmsrState,
    PeakExposure,
};
pub use outcome::outcome_cell;
pub use q64::Q64;
pub use settle::{
    c_max, c_p_alloc, layer_loss, payout_floor, r_net, recovery_rate, surplus, surplus_parts, ticket_face,
    usdc, usdc_charge,
};
