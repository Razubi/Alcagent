//! Shared primitives for the Alcagent prison simulation.
//!
//! Everything in this crate is deterministic-serializable and free of
//! simulation logic. Other crates build on these types only.

pub mod ids;
pub mod rng;
pub mod simtime;
pub mod config;
pub mod policy;

pub use ids::{AgentId, FacilityId, GangId, GuardId, PrisonerId, WardenId};
pub use rng::RngStream;
pub use simtime::{SimTime, TimeSpeed};
pub use policy::Policies;