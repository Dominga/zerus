//! Real-engine compatibility test, driven by check-teams-base-engines.py.
//! The previous image owns the same store throughout these two steps.
use super::*;

fn manifest() -> serde_json::Value {
    let root = std::env::var_os("ZERUS_TEAMS_ENGINE_PROBE").expect("private probe directory");
    serde_json::from_slice(&std::fs::read(std::path::Path::new(&root).join("ready.json")).unwrap())
        .unwrap()
}

fn words(value: &serde_json::Value, name: &str) -> String {
    value[name].as_str().unwrap().to_owned()
}

fn old_assignments(store: &Store, value: &serde_json::Value) {
    for assignment in value["assignments"].as_array().unwrap() {
        let node = assignment["node"].as_u64().unwrap() as u32;
        let effect = assignment["effect"].as_str().unwrap();
        let old = words(value, "old");
        assert_eq!(store.effect_base(TEAM, effect).unwrap(), old);
        assert_eq!(
            store.attempt_base(TEAM, node, 1).unwrap(),
            Some(old.clone())
        );
        let team = store.team(TEAM).unwrap().unwrap();
        let member = store
            .member(TEAM, assignment["member"].as_str().unwrap())
            .unwrap()
            .unwrap();
        let text = crate::teams::tasks::context::build_delivery(
            store,
            &team,
            &store.node(TEAM, node).unwrap().unwrap(),
            &member,
            &old,
            None,
        )
        .unwrap();
        assert!(text.contains(&format!("Base commit: {old}")));
    }
}

#[test]
#[ignore = "requires the pinned previous-engine image; run scripts/ci/check-teams-base-engines.py"]
fn advance_base_two_engines_new_advances_and_preserves_unpinned_rows() {
    let value = manifest();
    let mut store = Store::open(std::path::Path::new(&words(&value, "state"))).unwrap();
    let pins: u32 = store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT count(*) FROM handout_bases", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pins, 0, "the actual previous engine wrote these rows");
    old_assignments(&store, &value);
    let team = store.team(TEAM).unwrap().unwrap();
    let req = request(
        team.revision,
        Operation::AdvanceBase {
            commit: words(&value, "next"),
        },
    );
    let repo = crate::teams::integrate::Repo {
        git_dir: words(&value, "git_dir").into(),
    };
    crate::teams::ops::apply_with_repository(&mut store, &person(), &req, 2000, &|_| {
        Ok(repo.clone())
    })
    .unwrap();
    old_assignments(&store, &value);
    assert_eq!(store.creation_base(TEAM).unwrap(), words(&value, "old"));
    let mark: u32 = store
        .db
        .as_ref()
        .unwrap()
        .query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        mark,
        crate::teams::store::bases::PINNED_EXTENSION,
        "the first advance must raise the private fence",
    );
}

#[test]
#[ignore = "requires the pinned previous-engine image; run scripts/ci/check-teams-base-engines.py"]
fn advance_base_two_engines_new_writer_continues_after_old_writer_refusal() {
    let value = manifest();
    let mut store = Store::open(std::path::Path::new(&words(&value, "state"))).unwrap();
    old_assignments(&store, &value);
    let team = store.team(TEAM).unwrap().unwrap();
    if team.status == zerus_team_plan::model::TeamStatus::Paused {
        crate::teams::ops::apply(
            &mut store,
            &person(),
            &request(team.revision, Operation::Resume),
            3000,
        )
        .unwrap();
    }
    let node = value["queued"].as_u64().unwrap() as u32;
    let effect = uuid::Uuid::new_v4().to_string();
    let req = request(
        store.team(TEAM).unwrap().unwrap().revision,
        Operation::HandoutIntent {
            node,
            member: "m1".into(),
            effect_request_id: effect.clone(),
        },
    );
    crate::teams::ops::apply(&mut store, &service(), &req, 4000).unwrap();
    assert_eq!(
        store.effect_base(TEAM, &effect).unwrap(),
        words(&value, "next")
    );
    assert_eq!(
        store.attempt_base(TEAM, node, 1).unwrap(),
        Some(words(&value, "next"))
    );
}
