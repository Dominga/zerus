//! Committed fleet and launch evidence; no native agent or desktop is started.
use super::*;
use rusqlite::params;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
fn write(path: &Path, value: Value) {
    std::fs::write(path, value.to_string()).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
pub(super) fn seed(h: &Harness, n: &native::Native) {
    std::fs::set_permissions(h.temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    for d in [
        n.config.dir.join("swarm"),
        n.config.state.join("desktop"),
        n.config.state.join("launches"),
        n.config.state.join("sessions"),
    ] {
        crate::platform::private_dir(&d).unwrap();
    }
    let path = n.config.state.join("desktop/state.sqlite3");
    if path.exists() {
        return;
    }
    write(
        &n.config.dir.join("swarm/catalog.json"),
        json!({"node_id":n.machine,"imported":true,"catalog":{"schema":1,"swarm_id":uuid::Uuid::new_v4().to_string(),"operations":{}}}),
    );
    crate::sqlite_file::create(&path).unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    zerus_desktop_catalog::migrate(&db).unwrap();
    db.execute(
        "UPDATE desktop_catalog_owner SET epoch=?1 WHERE id=1",
        [&n.epoch],
    )
    .unwrap();
    db.execute(
        "INSERT OR REPLACE INTO desktop_catalog_generations(id,computer) VALUES(1,'')",
        [],
    )
    .unwrap();
    db.execute("INSERT OR REPLACE INTO desktop_catalog_sources(computer,epoch,incarnation,generation,stream,seq,visible,complete,active,hello) VALUES('',?1,1,1,'stream',1,1,1,1,?2)",params![n.epoch,json!({"machine_id":n.machine}).to_string()]).unwrap();
    for m in h.store.members(TEAM, 200, None).unwrap().rows {
        seed_member(n, &db, &m);
    }
}
/// The seeded session ID of `member`: fixed, so a test can find its record.
pub(in crate::desktop::teams::actor) fn session_of(member: &str) -> String {
    let n = member
        .bytes()
        .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(u64::from(b)));
    format!("00000000-0000-4000-8000-{:012x}", n & 0xffff_ffff_ffff)
}
fn seed_member(n: &native::Native, db: &rusqlite::Connection, m: &Member) {
    let Some(binding) = &m.session else { return };
    let Some(launch) = binding.session_id.strip_prefix("launch_id:") else {
        return;
    };
    let sid = session_of(&m.id);
    let run = format!("fixture-{}", m.id);
    let name = format!("{}/fixture/{}", m.agent, m.id);
    // The supervisor's own record for this launch (zerus-63rru.53), placed in
    // the ID layout, where a session that reuses a label lives.
    let mut record = crate::state::listing_fixture::launched_record(
        &m.agent,
        &name,
        launch,
        &sid,
        &run,
        &m.worktree,
        &m.account,
    );
    crate::state::listing_fixture::confirmed(&mut record, &format!("conversation-{}", m.id));
    record["layout"] = json!("id");
    write(
        &n.config.state.join("sessions").join(format!("{sid}.json")),
        record,
    );
    write(
        &n.config
            .state
            .join("launches")
            .join(format!("{launch}.json")),
        json!({"launch_id":launch,"session_id":sid,"name":name,"started_at":1.0}),
    );
    // The producer's idle row of the member's agent kind, with its identity.
    let row = super::super::rows::seed_row(
        &m.agent,
        &[
            ("name", json!(name)),
            ("run_id", json!(run)),
            ("launch_id", json!(launch)),
            ("conversation_id", json!(format!("conversation-{}", m.id))),
        ],
    )
    .to_string();
    db.execute(
        "INSERT OR REPLACE INTO desktop_catalog_rows(generation,key,json,bytes) VALUES(1,?1,?2,?3)",
        params![format!("run:{run}"), row, row.len()],
    )
    .unwrap();
}

pub(super) fn change(n: &native::Native, mode: &str) {
    let db = rusqlite::Connection::open(n.config.state.join("desktop/state.sqlite3")).unwrap();
    match mode {
        "fleet unknown" => {
            db.execute("DELETE FROM desktop_catalog_rows", []).unwrap();
        }
        "fleet epoch" => {
            db.execute(
                "UPDATE desktop_catalog_owner SET epoch='other' WHERE id=1",
                [],
            )
            .unwrap();
        }
        _ => {
            let (path, value) = match mode {
                "fleet busy" => ("$.activity", json!("busy")),
                "fleet pending" => ("$.optional_questions.count", json!(1)),
                "fleet background" => ("$.process_work.count", json!(1)),
                "fleet run" => ("$.run_id", json!("other-run")),
                "fleet launch" => ("$.launch_id", json!("another-launch")),
                _ => panic!("unknown fleet mutation"),
            };
            db.execute(
                "UPDATE desktop_catalog_rows SET json=json_set(json,?1,json(?2))",
                params![path, value.to_string()],
            )
            .unwrap();
        }
    }
}
