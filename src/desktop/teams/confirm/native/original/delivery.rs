//! Complete production Native::deliver and shared original-child coverage.
use super::*;
use crate::{
    desktop::teams::{ledger as launch, ports as launch_ports},
    teams::ops::{self, tests as setup, Operation, Request},
};
use sha2::{Digest, Sha256};
use std::{cell::RefCell, os::unix::fs::PermissionsExt, path::Path};
thread_local! {
    pub(super) static LOCKED_FACTS:RefCell<Option<Box<dyn FnOnce()>>>=const {RefCell::new(None)};
    pub(super) static AFTER_FACTS:RefCell<Option<Box<dyn FnOnce()>>>=const {RefCell::new(None)};
    pub(super) static PROGRAM:RefCell<Option<PathBuf>>=const {RefCell::new(None)};
    pub(super) static BEFORE:RefCell<Option<Box<dyn FnOnce()>>>=const {RefCell::new(None)};
}
pub(super) fn before() {
    if let Some(f) = BEFORE.with(|slot| slot.borrow_mut().take()) {
        f();
    }
}
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        PROGRAM.with(|slot| slot.borrow_mut().take());
        BEFORE.with(|slot| slot.borrow_mut().take());
        AFTER_FACTS.with(|slot| slot.borrow_mut().take());
        LOCKED_FACTS.with(|slot| slot.borrow_mut().take());
        original_child::AFTER_FINAL_CHECK.with(|slot| slot.borrow_mut().take());
        original_child::AFTER_SPAWN.with(|slot| slot.borrow_mut().take());
    }
}
struct Fixture {
    own: super::super::ownership_launch::Fixture,
    store: Store,
    stop: Arc<AtomicBool>,
    row: Record,
    raw: Value,
    stub: PathBuf,
}
fn private_write(path: &Path, raw: &[u8]) {
    std::fs::write(path, raw).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
fn state_file(state: &Path, name: &str) -> PathBuf {
    state.join(format!("{:x}.json", Sha256::digest(name)))
}
impl Fixture {
    fn new(kind: Kind) -> Self {
        let mut own = super::super::ownership_launch::Fixture::new();
        own.b.run = "run-1".into();
        if kind != Kind::Trust {
            own.b.conversation = "conversation-1".into();
        }
        crate::platform::private_dir(&own.config.state).unwrap();
        let mut store = Store::open(&own.config.state).unwrap();
        bind(&own, &mut store);
        own.record();
        let row = prepare_launch(&own, &store);
        let (mut snapshot, mut raw) = native_record(&own.b, &kind);
        if kind == Kind::Resume {
            snapshot.archive = Some(uuid::Uuid::new_v4().to_string());
            raw["archive_id"] = json!(snapshot.archive);
            raw["state"] = json!("archived");
        }
        write_native(&own.config.state, &snapshot, &raw);
        let plan = super::super::super::engine::candidate(
            &own.b,
            &snapshot,
            kind.clone(),
            snapshot.questions.first().cloned().unwrap_or(Value::Null),
        );
        let plan = if kind == Kind::Brief {
            let mut p = plan;
            p.kind = Kind::Brief;
            p.question =
                super::super::super::engine::synthetic(&own.b, &snapshot, &p.text, "brief");
            p
        } else {
            plan
        };
        let stop = Arc::new(AtomicBool::new(false));
        let mut native = Native::new(&own.config, &own.b.machine, &stop, &mut store).unwrap();
        let mut effect = native.claim(&plan, "epoch", 100).unwrap().record;
        effect.audit = Some(native.audit_begin(&plan).unwrap());
        native.save(&effect).unwrap();
        let stub = stub(&own.config, &effect);
        drop(native);
        drop(row);
        Self {
            own,
            store,
            stop,
            row: effect,
            raw,
            stub,
        }
    }
    fn deliver(&mut self, change: impl FnOnce() + 'static) -> Outcome {
        self.deliver_on(&self.own.b.machine.clone(), change)
    }
    fn deliver_on(&mut self, machine: &str, change: impl FnOnce() + 'static) -> Outcome {
        let mut n = Native::new(&self.own.config, machine, &self.stop, &mut self.store).unwrap();
        n.capture = Some((self.row.plan.snapshot.clone(), self.raw.clone()));
        let proof = n.owned(&self.own.b).unwrap();
        super::super::super::policy::ownership(&self.own.b, &proof).unwrap();
        PROGRAM.with(|slot| *slot.borrow_mut() = Some(self.stub.clone()));
        BEFORE.with(|slot| *slot.borrow_mut() = Some(Box::new(change)));
        let _reset = Reset;
        let result = n.deliver(&self.row);
        PROGRAM.with(|slot| slot.borrow_mut().take());
        BEFORE.with(|slot| slot.borrow_mut().take());
        result
    }
    fn called(&self) -> bool {
        self.own.config.state.join("stub-called").exists()
    }
}
fn bind(own: &super::super::ownership_launch::Fixture, store: &mut Store) {
    ops::apply(
        store,
        &setup::person(),
        &setup::request(0, setup::create()),
        1,
    )
    .unwrap();
    let revision = store.team(&own.b.team).unwrap().unwrap().revision;
    let op = Operation::SetMemberSession {
        member: own.b.member.clone(),
        session: zerus_team_plan::model::SessionKey {
            machine: own.b.machine.clone(),
            session_id: format!("launch_id:{}", own.b.launch),
        },
        account: own.b.account.clone(),
        worktree: own.b.folder.clone(),
        ownership_nonce: own.b.nonce.clone(),
    };
    ops::apply(
        store,
        &setup::service(),
        &Request {
            request_id: uuid::Uuid::new_v4().to_string(),
            team: own.b.team.clone(),
            expected_revision: revision,
            operation: op,
        },
        2,
    )
    .unwrap();
}
fn prepare_launch(own: &super::super::ownership_launch::Fixture, store: &Store) -> launch::Row {
    let team = store.team(&own.b.team).unwrap().unwrap();
    let mut row = launch::Row::new(&team, &own.b.member, "epoch", launch::State::Bound);
    let tree = launch_ports::Worktree {
        path: own.b.folder.clone(),
        ownership_nonce: own.b.nonce.clone(),
    };
    row.plan = Some(own.launch_plan());
    row.worktree = Some(tree.clone());
    row.observed = Some(launch_ports::ObservedSession {
        machine: own.b.machine.clone(),
        launch_id: own.b.launch.clone(),
        agent: own.b.agent.clone(),
        account: own.b.account.clone(),
        worktree: tree,
        parent: None,
        name: "claude/example/member".into(),
        session_id: own.b.session.clone(),
        run_id: own.b.run.clone(),
        conversation_id: own.b.conversation.clone(),
    });
    launch::Ledger::open(&own.config.state)
        .unwrap()
        .insert(&row, 100)
        .unwrap();
    crate::platform::private_dir(&own.config.dir.join("swarm")).unwrap();
    private_write(&own.config.dir.join("swarm/catalog.json"),json!({"node_id":own.b.machine,"catalog":{"schema":1,"swarm_id":uuid::Uuid::new_v4().to_string(),"operations":{}}}).to_string().as_bytes());
    row
}
fn native_record(b: &Binding, kind: &Kind) -> (Snapshot, Value) {
    let (_, _, mut s) = super::super::super::policy::tests::sample();
    s.machine = b.machine.clone();
    s.launch = b.launch.clone();
    s.session = b.session.clone();
    s.run = b.run.clone();
    s.conversation = b.conversation.clone();
    s.folder = b.folder.clone();
    s.account = b.account.clone();
    s.questions[0]["questions"][0]["body"] = json!(format!("{}\n\nDisclosure", b.folder));
    if *kind != Kind::Trust {
        s.questions.clear();
        s.ready = *kind == Kind::Brief;
        s.live = *kind == Kind::Brief;
        s.ended = *kind == Kind::Resume;
        s.self_exit = *kind == Kind::Resume;
        s.exit_identity = "exit-1".into();
    }
    let mut raw = json!({"name":s.name,"agent":b.agent,"run_id":b.run,"conversation_id":b.conversation,"session_id":b.session,"launch_id":b.launch,"account_id":b.account,"cwd":b.folder,"worktree_owner":{"id":b.nonce}});
    if *kind == Kind::Resume {
        raw["completion_source"] = json!("supervisor");
        raw["completion_reason"] = json!("clean_exit");
        raw["exited_at"] = json!(1000);
        raw["exit_code"] = json!(0);
    }
    (s, raw)
}
fn write_native(state: &Path, s: &Snapshot, raw: &Value) {
    let path = if let Some(id) = &s.archive {
        crate::platform::private_dir(&state.join("archives")).unwrap();
        state.join("archives").join(format!("{id}.json"))
    } else {
        state_file(state, &s.name)
    };
    private_write(&path, raw.to_string().as_bytes());
}
fn stub(config: &Config, row: &Record) -> PathBuf {
    let (_, input, folder) = super::super::receipt::invocation(row).unwrap();
    let q: Value = serde_json::from_slice(&input).unwrap();
    let receipt = match row.plan.kind {
        Kind::Trust => {
            json!({"request_id":q["request_id"],"name":row.plan.snapshot.name,"run_id":q["expected_run_id"],"conversation_id":q["expected_conversation_id"],"question_id":q["question_id"],"question_hash":q["expected_question_hash"],"digest":format!("{:x}",Sha256::digest(&input)),"status":"answered","transport":"claude_tui"})
        }
        Kind::Resume => crate::state::lifecycle_fixture_receipt(
            &row.plan.snapshot.name,
            &input,
            "completed",
            Some("resumed-run"),
        ),
        _ => {
            json!({"request_id":q["request_id"],"name":row.plan.snapshot.name,"run_id":q["expected_run_id"],"conversation_id":q["expected_conversation_id"],"digest":format!("{:x}",Sha256::digest(&input)),"status":"submitted","transport":"terminal"})
        }
    };
    private_write(
        &config.state.join("stub-receipt"),
        receipt.to_string().as_bytes(),
    );
    let path = config.state.parent().unwrap().join("stub-child.py");
    std::fs::write(&path,format!("#!/usr/bin/env python3\nimport os,sys,json,pathlib,sqlite3\nroot=pathlib.Path(os.environ['HGS_STATE_DIR'])\nrequest=json.loads(sys.stdin.buffer.read())\nassert int(os.environ['HGS_TEAMS_CUSTODY_FD'])>2\nassert sqlite3.connect(root/'approval-audit/journal.sqlite3').execute('SELECT status FROM attempts WHERE request_id=?',(request['request_id'],)).fetchone()==('prepared',)\n(root/'stub-called').write_text('called')\nfolder=root/{folder:?}\nfolder.mkdir(exist_ok=True,mode=0o700)\np=folder/(request['request_id']+'.json')\np.write_bytes((root/'stub-receipt').read_bytes())\np.chmod(0o600)\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
#[test]
fn complete_delivery_runs_only_the_stub_and_reads_its_exact_receipt() {
    for kind in [Kind::Trust, Kind::Resume, Kind::Brief] {
        let mut f = Fixture::new(kind);
        let result = f.deliver(|| {});
        let why = match &result {
            Outcome::NotStarted(s) | Outcome::Refused(s) | Outcome::Unknown(s) => s.as_str(),
            Outcome::Retry => "retry",
            _ => "approved",
        };
        assert!(
            matches!(result, Outcome::Approved(_)),
            "Expected original stub receipt: {why}"
        );
        assert!(f.called());
    }
}
#[test]
fn delegation_revoked_after_preparation_prevents_the_original_child() {
    for kind in [Kind::Trust, Kind::Resume, Kind::Brief] {
        let mut f = Fixture::new(kind.clone());
        let state = f.own.config.state.clone();
        let result = f.deliver(move || {
            let db = team_db(&state);
            let field = if kind == Kind::Trust {
                "$.delegations.confirm_trust"
            } else {
                "$.delegations.resume_exited"
            };
            db.execute(
                "UPDATE teams SET settings=json_set(settings,?1,json('false'))",
                [field],
            )
            .unwrap();
        });
        assert!(matches!(result, Outcome::NotStarted(_)));
        assert!(!f.called());
    }
}

fn team_db(state: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(state.join("teams/teams-v1.sqlite3")).unwrap()
}
#[test]
fn current_team_member_and_preview_checks_run_in_the_spawn_path() {
    for field in [
        "team_status",
        "team_home",
        "team_owner",
        "member_state",
        "member_agent",
        "member_account",
        "member_role",
        "member_folder",
        "member_nonce",
        "member_session",
        "preview",
    ] {
        let mut f = Fixture::new(Kind::Trust);
        let state = f.own.config.state.clone();
        let result = f.deliver(move || {
            if field == "preview" {
                std::fs::remove_file(state.join("teams/preview")).unwrap();
                return;
            }
            let db = team_db(&state);
            match field {
                "team_status" => {
                    db.execute("UPDATE teams SET status='\"paused\"'", [])
                        .unwrap();
                }
                "team_home" => {
                    db.execute("UPDATE teams SET home='other-machine'", [])
                        .unwrap();
                }
                "team_owner" => {
                    db.execute("UPDATE teams SET owner='another-person'", [])
                        .unwrap();
                }
                field => {
                    let key = field.strip_prefix("member_").unwrap();
                    let key = match key {
                        "folder" => "worktree",
                        "nonce" => "ownership_nonce",
                        _ => key,
                    };
                    db.execute(
                        "UPDATE members SET data=json_set(data,?1,?2) WHERE id='m1'",
                        rusqlite::params![
                            format!("$.{key}"),
                            if key == "state" { "paused" } else { "changed" }
                        ],
                    )
                    .unwrap();
                }
            }
        });
        assert!(matches!(result, Outcome::NotStarted(_)), "{field}");
        assert!(!f.called(), "{field}");
    }
}
#[test]
fn final_store_ownership_and_machine_checks_prevent_the_original_child() {
    for field in ["anchor", "owner_file", "machine"] {
        let mut f = Fixture::new(Kind::Trust);
        let state = f.own.config.state.clone();
        let config = f.own.config.dir.clone();
        let folder = f.own.b.folder.clone();
        let result = f.deliver(move || match field {
            "anchor" => {
                let db = rusqlite::Connection::open(state.join("teams/member-launch-v1.sqlite3"))
                    .unwrap();
                db.execute("DELETE FROM launch_checkout_objects", [])
                    .unwrap();
            }
            "owner_file" => {
                let raw = std::fs::read_to_string(Path::new(&folder).join(".git")).unwrap();
                let meta = raw.trim().strip_prefix("gitdir: ").unwrap();
                std::fs::write(Path::new(meta).join("zerus-owner.json"), b"replaced").unwrap();
            }
            _ => {
                let path = config.join("swarm/catalog.json");
                let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                v["node_id"] = json!(uuid::Uuid::new_v4().to_string());
                private_write(&path, v.to_string().as_bytes());
            }
        });
        assert!(matches!(result, Outcome::NotStarted(_)), "{field}");
        assert!(!f.called(), "{field}");
    }
}
#[test]
fn final_native_record_and_pause_checks_prevent_the_original_child() {
    for field in ["missing", "worktree_owner", "paused", "pausing"] {
        let mut f = Fixture::new(Kind::Trust);
        let state = f.own.config.state.clone();
        let name = f.row.plan.snapshot.name.clone();
        let result = f.deliver(move || {
            let path = state_file(&state, &name);
            if field == "missing" {
                std::fs::remove_file(path).unwrap();
                return;
            }
            let mut v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if field == "worktree_owner" {
                v[field]["id"] = json!("another-owner");
            } else {
                v[field] = json!(true);
            }
            private_write(&path, v.to_string().as_bytes());
        });
        assert!(matches!(result, Outcome::NotStarted(_)), "{field}");
        assert!(!f.called(), "{field}");
    }
}
#[test]
fn root_replacement_with_the_same_team_files_still_refuses_a_child() {
    let mut f = Fixture::new(Kind::Resume);
    let state = f.own.config.state.clone();
    let result = f.deliver(move || {
        let old = state.with_file_name("original-state");
        std::fs::rename(&state, &old).unwrap();
        crate::platform::private_dir(&state).unwrap();
        for child in ["teams", "worktree-cache"] {
            std::fs::rename(old.join(child), state.join(child)).unwrap();
        }
        std::fs::copy(old.join("stub-receipt"), state.join("stub-receipt")).unwrap();
    });
    assert!(matches!(result, Outcome::NotStarted(_)));
    assert!(!f.called());
}
#[test]
fn missing_pins_and_nonself_exit_do_not_start_a_child() {
    let mut f = Fixture::new(Kind::Resume);
    f.raw["completion_source"] = json!("manual_archive");
    f.raw["completion_reason"] = json!("user_archived");
    write_native(&f.own.config.state, &f.row.plan.snapshot, &f.raw);
    let result = f.deliver(|| {});
    assert!(matches!(result, Outcome::NotStarted(_)));
    assert!(!f.called());
    let mut f = Fixture::new(Kind::Trust);
    let mut n = Native::new(&f.own.config, &f.own.b.machine, &f.stop, &mut f.store).unwrap();
    n.capture = Some((f.row.plan.snapshot.clone(), f.raw.clone()));
    PROGRAM.with(|slot| *slot.borrow_mut() = Some(f.stub.clone()));
    let result = n.deliver(&f.row);
    PROGRAM.with(|slot| slot.borrow_mut().take());
    drop(n);
    assert!(matches!(result, Outcome::NotStarted(_)));
    assert!(!f.called());
}

#[test]
fn a_stop_after_the_stored_read_prevents_the_original_child() {
    let mut f = Fixture::new(Kind::Trust);
    let stop = f.stop.clone();
    AFTER_FACTS.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(move || stop.store(true, Ordering::Release)))
    });
    let result = f.deliver(|| {});
    AFTER_FACTS.with(|slot| slot.borrow_mut().take());
    assert!(matches!(result, Outcome::NotStarted(_)));
    assert!(!f.called());
}

#[cfg(test)]
mod admission;

#[cfg(test)]
mod retry;
