//! Task 173: a member session working with nothing handed to it is flagged
//! to the lead at once as unassigned work, with what it runs, and cleared
//! when it stops or gets a hand-out.
use super::*;
use super::support::*;

fn busy_dev(building: bool, turn_started: Option<f64>) -> Vec<Seen> {
    let mut dev = session("codex/x/dev", "r1", false);
    dev.building = building;
    dev.build_label = if building { "cargo test --release".into() } else { String::new() };
    dev.turn_started = turn_started;
    dev.activity = "busy".into();
    vec![session("claude/x/lead", "r0", false), dev]
}

#[test]
fn a_member_building_with_no_hand_out_is_unassigned_work_at_once() {
    let team = team();
    let found = steps(&team, &busy_dev(true, Some(1_000.0)), 1_010.0);
    assert!(found.contains(&Step::Unassigned { member: "dev".into(), what: "builds cargo test --release".into() }), "{found:?}");
    // Working without a build: only past the grace.
    assert!(!steps(&team, &busy_dev(false, Some(1_000.0)), 1_060.0).iter().any(|step| matches!(step, Step::Unassigned { .. })));
    assert!(steps(&team, &busy_dev(false, Some(1_000.0)), 1_200.0).iter().any(|step| matches!(step, Step::Unassigned { member, .. } if member == "dev")));
    // With its hand-out open it is its task's work.
    let mut handed = team.clone();
    delivered(&mut handed);
    assert!(!steps(&handed, &busy_dev(true, Some(1_000.0)), 1_010.0).iter().any(|step| matches!(step, Step::Unassigned { .. })));
}

#[test]
fn the_flag_reaches_the_lead_and_look_and_clears_when_it_stops() {
    let team = team();
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111111", &json!({}), team.clone(), vec![]).unwrap();
    run_step(&mut store, &team, &Step::Unassigned { member: "dev".into(), what: "builds cargo test --release".into() }, "").unwrap();
    let flagged = store.load(&team.id).unwrap().unwrap();
    let flag = flagged.member("dev").unwrap().unassigned.clone().expect("flagged");
    assert_eq!(flag.what, "builds cargo test --release");
    let items = super::super::inbox::items(&flagged);
    let item = items.iter().find(|item| item.kind == "unassigned_work").expect("the lead's item");
    assert!(item.summary.starts_with("dev (Backend) builds cargo test --release with no task handed to it (unassigned work)"), "{}", item.summary);
    let check = MemberCheck::default();
    let look = super::super::look::snapshot(&flagged, flagged.member("dev").unwrap(), &check, 1_100.0);
    assert_eq!(look["state"], "unassigned_work", "never shown as idle");
    assert_eq!(look["unassigned"]["what"], "builds cargo test --release");
    assert!(look["next"].as_str().unwrap().contains("hands the task out"), "{look}");
    // Flagged once: the next tick asks nothing new while it works.
    assert!(!steps(&flagged, &busy_dev(true, Some(1_000.0)), 1_200.0).iter().any(|step| matches!(step, Step::Unassigned { .. })));
    // Free again: the flag ends.
    let idle = vec![session("claude/x/lead", "r0", false), session("codex/x/dev", "r1", true)];
    let found = steps(&flagged, &idle, 1_300.0);
    assert!(found.contains(&Step::UnassignedEnded { member: "dev".into() }), "{found:?}");
    run_step(&mut store, &flagged, &Step::UnassignedEnded { member: "dev".into() }, "").unwrap();
    assert!(store.load(&team.id).unwrap().unwrap().member("dev").unwrap().unassigned.is_none());
    let feed = store.events(&team.id, None, 20).unwrap().to_string();
    assert!(feed.contains("unassigned_work") && feed.contains("unassigned_work_ended"), "{feed}");
}
