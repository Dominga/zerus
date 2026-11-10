//! Task #35 (team ede724c4, 2026-11-10): a shared or integration folder that
//! already carries the Low label (D:\Tools\Zerus, D:\OwlCats\DCB, RogueDOD and
//! AIBalanceTools did on 2026-11-10) is named at team start and in status with
//! the owner's step; Zerus never changes a label itself. The approve of a plan
//! with a DeepSeek member in the shared folder is refused with its action.
use super::*;
use crate::state::teams::environment::{low_shared_folders, set_test_low};
use crate::state::teams::model::{FolderPolicy, MemberFolder};

fn planned(folder: &std::path::Path, members: Value) -> Team {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"T","folder":folder.display().to_string(),"lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    ops::apply(&mut team, "plan", &json!({"members":members,"tasks":[{"id":1,"title":"Docs","assignee":"qa"}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    team
}

#[test]
fn a_low_shared_folder_is_named_at_team_start_and_in_status_with_the_owners_step() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = dir.path().join("checkout");
    let line = dir.path().join("line");
    let own = dir.path().join("checkout-team-t-dev");
    for folder in [&checkout, &line, &own] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let mut team = planned(&checkout, json!([{"id":"qa","role":"QA","agent":"codex","folder":"shared"},{"id":"dev","role":"Backend","agent":"dsh","folder":"clone"}]));
    team.settings.integration_folder = line.display().to_string();
    // Nothing Low: nothing named.
    set_test_low(Some(Vec::new()));
    assert!(low_shared_folders(&team).is_empty());
    // The checkout and the line carry the label; a DeepSeek member's own clone
    // is Low by design and never named.
    set_test_low(Some(vec![checkout.clone(), line.clone(), own.clone()]));
    let found = low_shared_folders(&team);
    assert_eq!(found.iter().map(|low| low.what.as_str()).collect::<Vec<_>>(), vec!["the team folder", "the integration folder"], "{found:?}");
    let step = format!("icacls \"{}\" /setintegritylevel (OI)(CI)M /T /C /Q", checkout.display());
    assert!(found[0].text().starts_with("The team folder ") && found[0].text().contains(&step), "{}", found[0].text());
    // At team start: the preflight names each one (a warning: the owner's
    // step, never a block of the team).
    let checked = plan::validate(&team, &BTreeSet::new());
    let warnings: Vec<&str> = checked.warnings.iter().filter(|finding| finding.kind == "low_folder").map(|finding| finding.text.as_str()).collect();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings[0].contains(&step), "{warnings:?}");
    // In status, with the folder that carries the label and the step.
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open(true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
        let value = run("status", &json!({"team":team.id}), false).unwrap();
        let low = value["low_folders"].as_array().unwrap();
        assert_eq!(low.len(), 2, "{}", value["low_folders"]);
        assert_eq!(low[0]["labeled"], json!(checkout.display().to_string()));
        assert_eq!(low[0]["owner_step"], json!(step));
        assert_eq!(low[1]["what"], "the integration folder");
    });
    // The owner's step done: nothing named, in either place.
    set_test_low(Some(Vec::new()));
    assert!(low_shared_folders(&team).is_empty());
    assert!(plan::validate(&team, &BTreeSet::new()).warnings.iter().all(|finding| finding.kind != "low_folder"));
    set_test_low(None);
}

#[test]
fn a_team_with_a_deepseek_member_in_the_shared_folder_does_not_start() {
    let dir = tempfile::tempdir().unwrap();
    set_test_low(Some(Vec::new()));
    let mut team = planned(dir.path(), json!([{"id":"qa","role":"QA","agent":"dsh","folder":"shared"}]));
    let error = ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap_err().to_string();
    assert!(error.contains("qa: a DeepSeek (dsh) session's sandbox") && error.contains("\"folder\":\"clone\""), "{error}");
    assert_eq!(team.status, TeamStatus::Planning, "nothing started");
    // Its own clone, as the refusal says: the team starts.
    ops::apply(&mut team, "settings", &json!({"members_update":[{"id":"qa","folder":"clone"}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert_ne!(team.status, TeamStatus::Planning);
    set_test_low(None);
}

/// Under the default (lead) folder policy a DeepSeek member the plan or an
/// add gives no folder takes its own clone, so the happy path starts (the
/// owner's rule d2); only a named shared folder is refused.
#[test]
fn a_deepseek_member_given_no_folder_takes_its_own_clone() {
    let dir = tempfile::tempdir().unwrap();
    set_test_low(Some(Vec::new()));
    let mut team = planned(dir.path(), json!([{"id":"qa","role":"QA","agent":"dsh"},{"id":"rev","role":"Reviewer","agent":"codex","folder":"shared"}]));
    assert_eq!(team.settings.folder_policy, FolderPolicy::Lead);
    assert_eq!(team.member("qa").unwrap().folder, MemberFolder::Clone);
    assert_eq!(team.member("rev").unwrap().folder, MemberFolder::Shared, "other agents keep their choice");
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    ops::apply(&mut team, "add-member", &json!({"member":{"id":"cook","role":"Backend","agent":"dsh"}}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert_eq!(team.member("cook").unwrap().folder, MemberFolder::Clone);
    // A project that leaves no own folder keeps it shared, and says why.
    team.settings.folder_policy = FolderPolicy::Shared;
    let error = ops::apply(&mut team, "add-member", &json!({"member":{"id":"cook2","role":"Backend","agent":"dsh"}}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap_err().to_string();
    assert!(error.contains("cook2: a DeepSeek (dsh) session's sandbox"), "{error}");
    set_test_low(None);
}
