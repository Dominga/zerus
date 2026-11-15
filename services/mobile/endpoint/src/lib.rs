//! Native and connector verification share one endpoint-owned private lock.
//! Request JSON cannot choose the workspace record used by the native entry.
pub mod configuration;
mod fsguard;
mod json_limits;
pub mod native_journal;
pub mod private_state;
mod workspace;
pub use workspace::{authorize, EndpointError, Profile, VerifiedRequest};
