//! Bounded pinned role choices; no live catalog is substituted on an edit.
use super::*;
use zerus_team_plan::catalog::AccountChoice;
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MemberRole {
    pub id: String,
    pub name: String,
    pub agent: String,
    pub model: String,
    pub effort: String,
    pub account: AccountChoice,
}
pub(super) fn roles(status: &Value) -> Vec<MemberRole> {
    array(status, "member_roles")
        .iter()
        .take(MEMBERS_KEPT)
        .filter_map(|r| {
            let id = name(r, "id");
            if zerus_team_plan::id_problem(&id, "role").is_some() {
                return None;
            }
            let account = account(r.get("account")?)?;
            Some(MemberRole {
                id,
                name: text(r, "name", LINE_CHARS),
                agent: name(r, "agent"),
                model: text(r, "model", 256),
                effort: text(r, "effort", 64),
                account,
            })
        })
        .collect()
}
pub(super) fn choice(member: &Value) -> Option<AccountChoice> {
    account(member.get("account_choice")?)
}
fn account(value: &Value) -> Option<AccountChoice> {
    let choice: AccountChoice = serde_json::from_value(value.clone()).ok()?;
    if let AccountChoice::Named { account } = &choice {
        if account.trim().is_empty()
            || zerus_team_plan::name_problem(account, "account", 256).is_some()
        {
            return None;
        }
    }
    Some(choice)
}
