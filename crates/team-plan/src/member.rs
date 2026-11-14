//! A seat's own launch choices, separate from its shared role defaults.
use crate::catalog::AccountChoice;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberLaunch {
    /// Zero for an added seat; each explicit swap advances this generation.
    pub generation: u64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub account: AccountChoice,
}
