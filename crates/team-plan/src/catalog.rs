//! Roles, pipelines and templates: the reusable description of a team.
//!
//! A catalog holds them. A template's seats name roles from the catalog, and a
//! bundle is one template with exactly the roles and pipelines it uses, which
//! is what gets exported and what a team keeps as its own copy.
//!
//! Adapted from `src/state/teams/catalog.rs` in `WhiteWh/zerus` at
//! `ca4cc4481`: reduced to roles, seats, pipelines and templates, with a
//! lead's own spec and a template bundle added.

use serde::{Deserialize, Serialize};

use crate::is_default;
use crate::model::{Acceptance, ContextMode, Settings};

/// The rounds a pipeline allows when it does not say.
pub const DEFAULT_ROUNDS: u32 = 3;

fn one() -> u32 {
    1
}

/// Which account of the role's agent a member is launched with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AccountChoiceWire", into = "AccountChoiceWire")]
pub enum AccountChoice {
    /// The signed-in account of that agent with the most room left.
    #[default]
    Freest,
    /// One account, by its ID on the member's computer.
    Named { account: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AccountMode {
    Freest,
    Named,
}

/// The JSON form of [`AccountChoice`]: `{"mode":"freest"}` or
/// `{"mode":"named","account":"..."}`. It is a strict struct because an
/// internally tagged enum accepts unknown fields on a variant without data.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountChoiceWire {
    mode: AccountMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account: Option<String>,
}

impl TryFrom<AccountChoiceWire> for AccountChoice {
    type Error = &'static str;

    fn try_from(wire: AccountChoiceWire) -> Result<Self, Self::Error> {
        match (wire.mode, wire.account) {
            (AccountMode::Freest, None) => Ok(Self::Freest),
            (AccountMode::Freest, Some(_)) => Err("the freest account choice names no account"),
            (AccountMode::Named, Some(account)) if !account.is_empty() => {
                Ok(Self::Named { account })
            }
            (AccountMode::Named, _) => Err("a named account choice needs an account"),
        }
    }
}

impl From<AccountChoice> for AccountChoiceWire {
    fn from(choice: AccountChoice) -> Self {
        match choice {
            AccountChoice::Freest => Self {
                mode: AccountMode::Freest,
                account: None,
            },
            AccountChoice::Named { account } => Self {
                mode: AccountMode::Named,
                account: Some(account),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageKind {
    Review,
    Test,
    /// Opening a pull request or merging, in a team where landing is allowed.
    Land,
}

/// What a member is for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub id: String,
    pub name: String,
    /// The role's instructions, put into every task handed to it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt: String,
    /// `claude`, `codex`, `kimi`, `dsh` or `opencode`.
    pub agent: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub account: AccountChoice,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effort: String,
    /// The pipeline of this role's tasks; empty means the template's default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pipeline: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub context: ContextMode,
}

/// Members of one role in a template's roster.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    pub role: String,
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub kind: StageKind,
    /// The role whose member does the stage.
    pub role: String,
}

/// What happens between a member's report and the end of a task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pipeline {
    pub id: String,
    pub name: String,
    /// The role that does the task itself.
    pub implementer: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<Stage>,
    /// 0 reads as [`DEFAULT_ROUNDS`].
    #[serde(default, skip_serializing_if = "is_default")]
    pub max_rounds: u32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub accept: Acceptance,
}

impl Pipeline {
    /// The number of rounds after which a returned task goes to the lead.
    pub fn rounds(&self) -> u32 {
        if self.max_rounds == 0 {
            DEFAULT_ROUNDS
        } else {
            self.max_rounds
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeadSpec {
    pub agent: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub account: AccountChoice,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effort: String,
    /// Rules added to the lead's guide.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rules: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub lead: LeadSpec,
    pub roster: Vec<Seat>,
    pub default_pipeline: String,
    #[serde(default)]
    pub settings: Settings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Catalog {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<Role>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pipelines: Vec<Pipeline>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<Template>,
    /// The planning rules given to a lead.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub planning_rules: Vec<String>,
}

/// One template with exactly the roles and pipelines it uses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateBundle {
    pub template: Template,
    pub roles: Vec<Role>,
    pub pipelines: Vec<Pipeline>,
}
