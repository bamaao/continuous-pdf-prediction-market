//! Shared consensus numerics. Programs, Quote, and WASM must call this crate.
//! Public pricing APIs do not take or return IEEE floats.

pub mod auction;
pub mod football;
pub mod lmsr;
pub mod prior;
pub mod q64;
pub mod settle;

pub use auction::{sort_bids, unit_premium, Bid};
pub use lmsr::{buy_cost, interval_prob, lmsr_update, marginal_price, uniform_prior, LmsrState};
pub use q64::Q64;
pub use settle::{c_max, layer_loss, recovery_rate, surplus};
