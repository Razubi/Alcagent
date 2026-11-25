//! Shared primitives for the Alcagent prison simulation.
//!
//! Everything in this crate is deterministic-serializable and free of
//! simulation logic. Other crates build on these types only.

pub mod config;
pub mod ids;
pub mod policy;
pub mod rng;
pub mod simtime;

pub use ids::{AgentId, FacilityId, GangId, GuardId, PrisonerId, WardenId};
pub use policy::Policies;
pub use rng::RngStream;
pub use simtime::{SimTime, TimeSpeed};
