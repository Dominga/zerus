//! Choosing the account a member is launched with.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::catalog::AccountChoice;
use crate::problem::{code, Problem};

/// One usage window of an account, as `hgs account inspect` reports it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageWindow {
    pub used_percent: f64,
    /// When the window resets, as the caller's clock counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
}

/// An account of an agent on the member's computer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountUsage {
    pub id: String,
    pub agent: String,
    pub signed_in: bool,
    /// Empty when the account reports no usage, as an API key may not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<UsageWindow>,
}

/// The account a member of `agent` is launched with.
///
/// `Named`: the account must exist, belong to `agent` and be signed in.
///
/// `Freest`: among the signed-in accounts of `agent`, one with any window at
/// 100 percent or more is out; the most headroom (100 minus the highest
/// `used_percent`) wins, then the lower `taken` count, then the lower ID.
/// Accounts that report no windows are used only when no account with
/// windows qualifies. When nothing is left, `refs` holds the earliest time
/// an account that is out comes back: the latest reset of its full windows.
pub fn pick_account(
    agent: &str,
    choice: &AccountChoice,
    accounts: &[AccountUsage],
    taken: &BTreeMap<String, u32>,
) -> Result<String, Problem> {
    let ours = accounts
        .iter()
        .filter(|account| account.signed_in && account.agent == agent);
    match choice {
        AccountChoice::Named { account } => ours
            .clone()
            .find(|candidate| candidate.id == *account)
            .map(|found| found.id.clone())
            .ok_or_else(|| {
                Problem::new(
                    code::UNKNOWN_ACCOUNT,
                    format!("{account:?} is not a signed-in {agent} account on this computer."),
                )
                .with_refs([account.clone()])
            }),
        AccountChoice::Freest => freest(agent, ours, taken),
    }
}

fn freest<'a>(
    agent: &str,
    ours: impl Iterator<Item = &'a AccountUsage>,
    taken: &BTreeMap<String, u32>,
) -> Result<String, Problem> {
    let load = |account: &AccountUsage| taken.get(&account.id).copied().unwrap_or(0);
    let mut measured: Vec<(f64, &AccountUsage)> = Vec::new();
    let mut unmeasured: Vec<&AccountUsage> = Vec::new();
    let mut back: Option<i64> = None;
    let mut any = false;
    for account in ours {
        any = true;
        if account.windows.is_empty() {
            unmeasured.push(account);
            continue;
        }
        let full: Vec<&UsageWindow> = account
            .windows
            .iter()
            .filter(|window| used(window) >= 100.0)
            .collect();
        if full.is_empty() {
            // Folding from 0 reads a negative use as none.
            let highest = account.windows.iter().map(used).fold(0.0, f64::max);
            measured.push((100.0 - highest, account));
        } else if let Some(at) = full
            .iter()
            .map(|window| window.resets_at)
            .collect::<Option<Vec<i64>>>()
        {
            let returns = at.into_iter().max();
            back = match (back, returns) {
                (Some(earliest), Some(returns)) => Some(earliest.min(returns)),
                (earliest, returns) => earliest.or(returns),
            };
        }
    }
    measured.sort_by(|(room_a, a), (room_b, b)| {
        room_b
            .total_cmp(room_a)
            .then(load(a).cmp(&load(b)))
            .then(a.id.cmp(&b.id))
    });
    unmeasured.sort_by(|a, b| load(a).cmp(&load(b)).then(a.id.cmp(&b.id)));
    if let Some((_, account)) = measured.first() {
        return Ok(account.id.clone());
    }
    if let Some(account) = unmeasured.first() {
        return Ok(account.id.clone());
    }
    let message = if any {
        format!("Every signed-in {agent} account has used up a usage window.")
    } else {
        format!("No {agent} account is signed in on this computer.")
    };
    Err(Problem::new(code::NO_ACCOUNT_AVAILABLE, message).with_refs(back.map(|at| at.to_string())))
}

/// A window's use as a percentage; a value that is not a number counts as
/// used up.
fn used(window: &UsageWindow) -> f64 {
    if window.used_percent.is_nan() {
        100.0
    } else {
        window.used_percent
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A signed-in `claude` account with windows at the given use.
    fn account(id: &str, used: &[f64]) -> AccountUsage {
        AccountUsage {
            id: id.to_owned(),
            agent: "claude".to_owned(),
            signed_in: true,
            windows: used
                .iter()
                .map(|used_percent| UsageWindow {
                    used_percent: *used_percent,
                    resets_at: None,
                })
                .collect(),
        }
    }

    fn freest(accounts: &[AccountUsage]) -> Result<String, Problem> {
        pick_account("claude", &AccountChoice::Freest, accounts, &BTreeMap::new())
    }

    fn named(id: &str, accounts: &[AccountUsage]) -> Result<String, Problem> {
        let choice = AccountChoice::Named {
            account: id.to_owned(),
        };
        pick_account("claude", &choice, accounts, &BTreeMap::new())
    }

    fn refused(result: Result<String, Problem>, code: &str) -> Problem {
        let problem = result.expect_err("refused");
        assert_eq!(problem.code, code, "{problem}");
        problem
    }

    #[test]
    fn a_named_account_must_exist_belong_to_the_agent_and_be_signed_in() {
        let mut accounts = vec![account("work", &[10.0])];
        assert_eq!(named("work", &accounts).unwrap(), "work");
        let problem = refused(named("home", &accounts), code::UNKNOWN_ACCOUNT);
        assert_eq!(problem.refs, ["home"]);
        refused(named("", &accounts), code::UNKNOWN_ACCOUNT);
        accounts[0].agent = "codex".into();
        refused(named("work", &accounts), code::UNKNOWN_ACCOUNT);
        accounts[0].agent = "claude".into();
        accounts[0].signed_in = false;
        refused(named("work", &accounts), code::UNKNOWN_ACCOUNT);
    }

    #[test]
    fn a_named_account_is_taken_however_full_it_is() {
        // The person named it; only the freest choice weighs usage.
        assert_eq!(named("work", &[account("work", &[100.0])]).unwrap(), "work");
    }

    #[test]
    fn the_freest_choice_counts_only_signed_in_accounts_of_the_agent() {
        let mut other_agent = account("a-codex", &[0.0]);
        other_agent.agent = "codex".into();
        let mut signed_out = account("b-out", &[0.0]);
        signed_out.signed_in = false;
        let accounts = [other_agent, signed_out, account("c-busy", &[80.0])];
        assert_eq!(freest(&accounts).unwrap(), "c-busy");
    }

    #[test]
    fn an_account_with_a_full_window_is_out() {
        let accounts = [account("a", &[100.0, 0.0]), account("b", &[99.0])];
        assert_eq!(freest(&accounts).unwrap(), "b");
        let accounts = [account("a", &[120.0]), account("b", &[99.5])];
        assert_eq!(freest(&accounts).unwrap(), "b");
    }

    #[test]
    fn the_tightest_window_decides_the_headroom() {
        // a has 10 left in its tightest window, b has 50.
        let accounts = [account("a", &[5.0, 90.0]), account("b", &[50.0, 50.0])];
        assert_eq!(freest(&accounts).unwrap(), "b");
    }

    #[test]
    fn a_tie_goes_to_the_account_given_to_fewer_members_then_the_lower_id() {
        let accounts = [
            account("b", &[30.0]),
            account("a", &[30.0]),
            account("c", &[30.0]),
        ];
        assert_eq!(freest(&accounts).unwrap(), "a");
        let taken = BTreeMap::from([("a".to_owned(), 2), ("b".to_owned(), 1)]);
        let pick = pick_account("claude", &AccountChoice::Freest, &accounts, &taken);
        assert_eq!(pick.unwrap(), "c");
        // Headroom comes before the count.
        let accounts = [account("a", &[10.0]), account("b", &[30.0])];
        let taken = BTreeMap::from([("a".to_owned(), 5)]);
        let pick = pick_account("claude", &AccountChoice::Freest, &accounts, &taken);
        assert_eq!(pick.unwrap(), "a");
    }

    #[test]
    fn an_account_without_windows_is_used_only_when_no_measured_one_qualifies() {
        let accounts = [account("a-key", &[]), account("b", &[95.0])];
        assert_eq!(freest(&accounts).unwrap(), "b");
        let accounts = [
            account("a-key", &[]),
            account("b", &[100.0]),
            account("c-key", &[]),
        ];
        assert_eq!(freest(&accounts).unwrap(), "a-key");
        let taken = BTreeMap::from([("a-key".to_owned(), 1)]);
        let pick = pick_account("claude", &AccountChoice::Freest, &accounts, &taken);
        assert_eq!(pick.unwrap(), "c-key");
    }

    #[test]
    fn nothing_available_names_when_the_first_account_comes_back() {
        let window = |used_percent: f64, resets_at: Option<i64>| UsageWindow {
            used_percent,
            resets_at,
        };
        let mut a = account("a", &[]);
        // Out until both full windows reset: at 3000.
        a.windows = vec![
            window(100.0, Some(1000)),
            window(100.0, Some(3000)),
            window(10.0, Some(50)),
        ];
        let mut b = account("b", &[]);
        b.windows = vec![window(100.0, Some(2000))];
        let problem = refused(freest(&[a.clone(), b.clone()]), code::NO_ACCOUNT_AVAILABLE);
        assert_eq!(problem.refs, ["2000"]);
        // An account whose reset is unknown gives no time.
        b.windows[0].resets_at = None;
        let problem = refused(freest(&[a, b.clone()]), code::NO_ACCOUNT_AVAILABLE);
        assert_eq!(problem.refs, ["3000"]);
        let problem = refused(freest(&[b]), code::NO_ACCOUNT_AVAILABLE);
        assert!(problem.refs.is_empty());
        // One full window with an unknown reset makes the whole return unknown.
        let mut c = account("c", &[]);
        c.windows = vec![window(100.0, Some(500)), window(100.0, None)];
        let problem = refused(freest(&[c]), code::NO_ACCOUNT_AVAILABLE);
        assert!(problem.refs.is_empty());
        let problem = refused(freest(&[]), code::NO_ACCOUNT_AVAILABLE);
        assert!(problem.refs.is_empty());
    }

    #[test]
    fn a_use_that_is_not_a_number_counts_as_used_up() {
        let accounts = [account("a", &[f64::NAN]), account("b", &[70.0])];
        assert_eq!(freest(&accounts).unwrap(), "b");
        refused(
            freest(&[account("a", &[f64::NAN])]),
            code::NO_ACCOUNT_AVAILABLE,
        );
        // A negative use reads as none: a and b tie, and b is given to fewer.
        let accounts = [account("a", &[-50.0]), account("b", &[0.0])];
        let taken = BTreeMap::from([("a".to_owned(), 1)]);
        let pick = pick_account("claude", &AccountChoice::Freest, &accounts, &taken);
        assert_eq!(pick.unwrap(), "b");
    }

    #[test]
    fn usage_has_a_strict_json_form() {
        let form = json!({"id": "work", "agent": "claude", "signed_in": true,
                          "windows": [{"used_percent": 42.5, "resets_at": 1000}, {"used_percent": 1.0}]});
        let usage: AccountUsage = serde_json::from_value(form.clone()).unwrap();
        assert_eq!(serde_json::to_value(&usage).unwrap(), form);
        let bare: AccountUsage =
            serde_json::from_value(json!({"id": "key", "agent": "claude", "signed_in": true}))
                .unwrap();
        assert!(bare.windows.is_empty());
        let extra = json!({"id": "w", "agent": "claude", "signed_in": true, "plan": "max"});
        assert!(serde_json::from_value::<AccountUsage>(extra).is_err());
    }
}
