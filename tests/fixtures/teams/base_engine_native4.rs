//! Sealed fixture for two actual native4 source-built test images.
//! No capability removal, schema edits or native caller-proof substitutes.
use super::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    std::env::var_os("ZERUS_TEAMS_NATIVE4_PROBE")
        .map(PathBuf::from)
        .expect("private interoperability directory")
}

fn publish(root: &Path, name: &str, value: &Value) {
    let temporary = root.join(format!("{name}.tmp"));
    std::fs::write(&temporary, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    std::fs::rename(temporary, root.join(format!("{name}.json"))).unwrap();
}

fn read(root: &Path, name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(root.join(format!("{name}.json"))).unwrap()).unwrap()
}

fn wait(root: &Path, name: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    while !root.join(name).exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "missing peer step {name}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn native4(store: &Store) {
    assert_eq!(crate::teams::store::SCHEMA_VERSION, 4);
    let db = store.db.as_ref().unwrap();
    let core: u32 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(core, 4, "both actual images must use native4");
    let capability: u32 = db.query_row(
        "SELECT count(*) FROM pragma_function_list WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0",
        [], |r| r.get(0),
    ).unwrap();
    assert_eq!(capability, 1, "the actual image registers the capability");
}

// Read-only snapshots of every team-keyed table; application rows are written
// only by Harness/ops::apply below. Global schema marks may legitimately change.
fn unrelated(store: &Store) -> Value {
    let db = store.db.as_ref().unwrap();
    let mut list = db.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap();
    let tables = list
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let mut snapshot = Vec::new();
    for table in tables {
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        let mut info = db.prepare(&format!("PRAGMA table_info({quoted})")).unwrap();
        let columns = info
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let key = if table == "teams" {
            "id"
        } else if columns.iter().any(|c| c == "team") {
            "team"
        } else {
            continue;
        };
        let mut query = db
            .prepare(&format!("SELECT * FROM {quoted} WHERE {key}=?1"))
            .unwrap();
        let count = query.column_count();
        let mut records = query
            .query_map([TEAM_B], |r| {
                (0..count)
                    .map(|i| r.get_ref(i).map(|v| format!("{v:?}")))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        records.sort();
        snapshot.push((table, records));
    }
    serde_json::to_value(snapshot).unwrap()
}

fn assignment(store: &Store, item: &Value) {
    let node = item["node"].as_u64().unwrap() as u32;
    let effect = item["effect"].as_str().unwrap();
    let base = item["base"].as_str().unwrap();
    assert_eq!(store.effect_base(TEAM, effect).unwrap(), base);
    assert_eq!(
        store.attempt_base(TEAM, node, 1).unwrap().as_deref(),
        Some(base)
    );
    let team = store.team(TEAM).unwrap().unwrap();
    let member = store
        .member(TEAM, item["member"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let prompt = crate::teams::tasks::context::build_delivery(
        store,
        &team,
        &store.node(TEAM, node).unwrap().unwrap(),
        &member,
        base,
        None,
    )
    .unwrap();
    assert!(prompt.contains(&format!("Base commit: {base}")));
    let db = store.db.as_ref().unwrap();
    let effects: u32 = db
        .query_row(
            "SELECT count(*) FROM effects WHERE team=?1 AND request_id=?2",
            rusqlite::params![TEAM, effect],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(effects, 1, "each hand-out is written once");
    if let Some(request) = item["request"].as_str() {
        let receipts: u32 = db
            .query_row(
                "SELECT count(*) FROM requests WHERE id=?1",
                [request],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(receipts, 1, "one receipt for one fresh, unreplayed request");
    }
}

fn report(h: &mut Harness, node: u32, member: &str, base: &str) {
    h.observe(node, TaskStage::InReview);
    h.run(
        &service(),
        Operation::RecordReport {
            node,
            generation: h.store.generation(TEAM, node).unwrap().unwrap(),
            member: member.into(),
            text: "Source-built interoperability fixture".into(),
            commit: base.into(),
            notes: Vec::new(),
        },
    )
    .unwrap();
}

fn handout(store: &mut Store, node: u32, member: &str, base: &str) -> Value {
    let effect = uuid::Uuid::new_v4().to_string();
    let request = request(
        store.team(TEAM).unwrap().unwrap().revision,
        Operation::HandoutIntent {
            node,
            member: member.into(),
            effect_request_id: effect.clone(),
        },
    );
    crate::teams::ops::apply(store, &service(), &request, 4000).unwrap();
    let item = json!({"node":node,"member":member,"base":base,"effect":effect,"request":request.request_id});
    assignment(store, &item);
    item
}

fn preserved(store: &Store, ready: &Value) {
    native4(store);
    assert_eq!(
        store.team(TEAM).unwrap().unwrap().base_commit,
        ready["next"].as_str().unwrap()
    );
    assert_eq!(
        store.creation_base(TEAM).unwrap(),
        ready["old"].as_str().unwrap()
    );
    assert_eq!(
        store.creation_base(TEAM_B).unwrap(),
        ready["old"].as_str().unwrap()
    );
    for item in ready["assignments"].as_array().unwrap() {
        assignment(store, item);
    }
    assert_eq!(
        unrelated(store),
        ready["unrelated"],
        "unrelated team rows remain unchanged"
    );
}

#[test]
#[ignore = "source-built native4 pair; run check-teams-base-compatibility.py"]
fn native4_predecessor_cached_and_reopened_writers_remain_compatible() {
    let root = root();
    let f = super::base_advance::GitFixture::new();
    let mut h = f.harness();
    native4(&h.store);
    h.bind("lead");
    h.bind("m1");
    h.bind("m2");
    let mut assignments = Vec::new();
    for (index, member) in [(1, "m1"), (2, "m2")] {
        let node = h.add(&person(), index);
        let effect = h.submit(node, member);
        report(&mut h, node, member, &f.old);
        assignments.push(json!({"node":node,"member":member,"effect":effect,"base":f.old}));
    }
    let cached = h.add(&person(), 3);
    let reopened = h.add(&person(), 4);
    let current = h.add(&person(), 5);
    let mut other = create();
    let Operation::Create {
        base_commit,
        integration_branch,
        ..
    } = &mut other
    else {
        unreachable!()
    };
    *base_commit = f.old.clone();
    *integration_branch = "zerus/team/unrelated".into();
    h.run_for(TEAM_B, &person(), other).unwrap();
    let ready = json!({"state":h.temp.path(),"git_dir":f.repo.git_dir,"old":f.old,"next":f.next,"assignments":assignments,"cached":cached,"reopened":reopened,"current":current,"unrelated":unrelated(&h.store)});
    publish(&root, "ready", &ready);
    wait(&root, "advanced");
    preserved(&h.store, &ready);
    let cached_item = handout(&mut h.store, cached, "m1", &f.next);
    h.outcome(
        cached_item["effect"].as_str().unwrap(),
        HandoutOutcome::Submitted {
            receipt: "native4-cached".into(),
        },
    );
    h.observe(cached, TaskStage::InProgress);
    report(&mut h, cached, "m1", &f.next);
    preserved(&h.store, &ready);
    assignment(&h.store, &cached_item);
    h.store = Store::open(h.temp.path()).unwrap();
    preserved(&h.store, &ready);
    assignment(&h.store, &cached_item);
    let reopened_item = handout(&mut h.store, reopened, "m1", &f.next);
    publish(
        &root,
        "predecessor",
        &json!({"cached":cached_item,"reopened":reopened_item}),
    );
    wait(&root, "verified");
    preserved(&h.store, &ready);
    assignment(&h.store, &read(&root, "current"));
    println!("NATIVE4 predecessor: cached and reopened writes succeeded; original pins and creation origin preserved; unrelated rows unchanged");
}

#[test]
#[ignore = "source-built native4 pair; run check-teams-base-compatibility.py"]
fn native4_current_advances_without_rewriting_existing_assignments() {
    let root = root();
    let ready = read(&root, "ready");
    let mut store = Store::open(Path::new(ready["state"].as_str().unwrap())).unwrap();
    native4(&store);
    for item in ready["assignments"].as_array().unwrap() {
        assignment(&store, item);
    }
    let request = request(
        store.team(TEAM).unwrap().unwrap().revision,
        Operation::AdvanceBase {
            commit: ready["next"].as_str().unwrap().into(),
        },
    );
    let repo = crate::teams::integrate::Repo {
        git_dir: ready["git_dir"].as_str().unwrap().into(),
    };
    crate::teams::ops::apply_with_repository(&mut store, &person(), &request, 3000, &|_| {
        Ok(repo.clone())
    })
    .unwrap();
    preserved(&store, &ready);
    println!("NATIVE4 current: advanced default; original assignments retained their pins");
}

#[test]
#[ignore = "source-built native4 pair; run check-teams-base-compatibility.py"]
fn native4_current_continues_after_predecessor_cached_and_reopened_writes() {
    let root = root();
    let ready = read(&root, "ready");
    let previous = read(&root, "predecessor");
    let mut store = Store::open(Path::new(ready["state"].as_str().unwrap())).unwrap();
    preserved(&store, &ready);
    assignment(&store, &previous["cached"]);
    assignment(&store, &previous["reopened"]);
    let item = handout(
        &mut store,
        ready["current"].as_u64().unwrap() as u32,
        "m2",
        ready["next"].as_str().unwrap(),
    );
    preserved(&store, &ready);
    publish(&root, "current", &item);
    let ids: std::collections::BTreeSet<_> = [&previous["cached"], &previous["reopened"], &item]
        .into_iter()
        .map(|v| v["request"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3, "each writer used a distinct fresh request");
    println!("NATIVE4 current: later assignment uses advanced base; both directions remain readable; no request replay");
}
