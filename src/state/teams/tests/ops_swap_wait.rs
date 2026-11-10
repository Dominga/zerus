//! Owner, 2026-11-11 (team 86821caa): a member whose session a swap took and
//! whose new one did not start.
//!
//! - Its new session waited for the replaced one, which kept its turn open
//!   until a usage limit reset. The wait was one feed event: the member read
//!   `not_created` with no reason, the lead's inbox had nothing, and the owner
//!   was not told. It is an inbox item at once and the owner's after a bound.
//! - The guides say the lead relaunches a member that did not start; `hgs team
//!   relaunch` answered "The lead changes the roster only on your word". The
//!   lead relaunches it, and the receipt says what the start still waits for.

use super::support::*;
use super::*;
use crate::state::teams::inbox;

/// The running team with dev at work in its own session.
fn team_with_dev_session() -> Team {
    let mut team = running_team();
    let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
    dev.session = Some(SessionRef { name: "codex/x/dev".into(), run_id: "r1".into(), ..Default::default() });
    dev.state = MemberState::Active;
    team
}

fn swapped() -> Team {
    let mut team = team_with_dev_session();
    apply(&mut team, "swap-member", &json!({"member":"dev","model":"gpt"}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    let dev = team.member("dev").unwrap();
    assert!(dev.session.is_none() && dev.launch_after_swap && dev.replaced.is_some(), "{dev:?}");
    team
}

#[test]
fn the_lead_relaunches_a_member_a_swap_left_without_a_session() {
    let lead = Actor::Member("lead".into());
    let none = BTreeSet::new();
    // A member at work stays the owner's to relaunch.
    let mut working = team_with_dev_session();
    assert!(!super::super::lead_plan::lead_relaunches(&working, "dev"));
    let error = apply(&mut working, "relaunch", &json!({"member":"dev"}), &lead, &none, &mut Vec::new()).unwrap_err().to_string();
    assert!(error.contains("Only you relaunch a member"), "{error}");
    // After a swap its new session has not started: that is the lead's step.
    let mut team = swapped();
    assert!(super::super::lead_plan::lead_relaunches(&team, "dev"));
    let receipt = apply(&mut team, "relaunch", &json!({"member":"dev"}), &lead, &none, &mut Vec::new()).unwrap();
    assert_eq!(receipt["status"], "relaunch");
    assert!(receipt.get("waiting").is_none(), "{receipt}");
    // While the replaced session holds its turn, the receipt says so.
    let mut waiting = swapped();
    {
        let dev = waiting.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.replaced_busy = true;
        dev.replaced_since = Some(100.0);
        dev.launch_after_swap = false;
    }
    assert!(super::super::lead_plan::lead_relaunches(&waiting, "dev"));
    let receipt = apply(&mut waiting, "relaunch", &json!({"member":"dev"}), &lead, &none, &mut Vec::new()).unwrap();
    let said = receipt["waiting"].as_str().unwrap();
    assert!(said.contains("codex/x/dev") && said.contains("usage limit") && said.contains("stop codex/x/dev in Sessions"), "{said}");
}

#[test]
fn a_start_that_waits_for_its_replaced_session_is_said_to_the_lead_and_the_owner() {
    let mut team = swapped();
    // Not waiting: no item, nothing for the owner.
    assert!(!inbox::items(&team).iter().any(|item| item.kind == "member_waits_replaced"));
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.replaced_busy = true;
        dev.replaced_since = Some(1000.0);
    }
    // The lead's inbox names it from the first tick, with the reason.
    let items = inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "member_waits_replaced").expect("an inbox item");
    assert_eq!(item.key, "member_waits_replaced:dev");
    assert!(item.summary.contains("is not started") && item.summary.contains("codex/x/dev"), "{}", item.summary);
    assert!(item.detail.contains("usage limit") && item.detail.contains("Sessions"), "{}", item.detail);
    // The owner hears once a turn would long have ended.
    let quiet = inbox::attention(&team, 1000.0 + inbox::REPLACED_WAIT_OWNER - 1.0);
    assert!(!quiet.iter().any(|item| item["kind"] == "replaced_waits"), "{quiet:?}");
    let due = inbox::attention(&team, 1000.0 + 6.0 * 60.0);
    let owner = due.iter().find(|item| item["kind"] == "replaced_waits").expect("the owner's item");
    assert_eq!((owner["member_id"].as_str(), owner["session"].as_str(), owner["minutes"].as_u64()), (Some("dev"), Some("codex/x/dev"), Some(6)));
    assert!(owner["summary"].as_str().unwrap().contains("Stop it in Sessions"), "{owner}");
    // Started: both are gone.
    {
        let dev = team.members.iter_mut().find(|member| member.id == "dev").unwrap();
        dev.session = Some(SessionRef { name: "codex/x/dev".into(), run_id: "r2".into(), ..Default::default() });
    }
    assert!(!inbox::items(&team).iter().any(|item| item.kind == "member_waits_replaced"));
    assert!(!inbox::attention(&team, 1000.0 + 6.0 * 60.0).iter().any(|item| item["kind"] == "replaced_waits"));
}
