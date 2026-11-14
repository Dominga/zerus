use super::super::ports::Ports;
use super::*;
use zerus_replica::{Actor as TaskActor, Position, ProjectStore, SessionKey, Stage, Target};

fn reported_task(h: &mut Harness) -> (TaskRef, u32) {
    let store = ProjectStore::open(&h.temp.path().join("replica"), "example-project").unwrap();
    let mut doc = store
        .new_task(
            "Work",
            Stage::Todo,
            Target::Any,
            TaskActor::Person,
            &Position::End,
            "example",
            1,
        )
        .unwrap();
    let task = TaskRef {
        project: "example-project".into(),
        uuid: doc.uuid().into(),
    };
    store.save(&mut doc).unwrap();
    drop(store);
    let caller = crate::teams::tasks::Caller {
        state: h.temp.path(),
        machine: "00000000-0000-4000-8000-0000000000aa",
        actor: TaskActor::Person,
    };
    let node = crate::teams::tasks::send(&caller, &task, TEAM, None, None)
        .unwrap()
        .node;
    let store = ProjectStore::open(&h.temp.path().join("replica"), &task.project).unwrap();
    let mut doc = store.load(&task.uuid).unwrap();
    let holder = SessionKey {
        machine: "example-machine".into(),
        session_id: "launch_id:holder".into(),
        session: "codex/holder".into(),
    };
    doc.record_send(&holder, "dispatch", &TaskActor::Person, 2)
        .unwrap();
    doc.record_report(&"界".repeat(5000), &TaskActor::Agent(holder), 3)
        .unwrap();
    store.save(&mut doc).unwrap();
    (task, node.expect("bound node"))
}

#[test]
fn native_observe_uses_the_trusted_task_reader_and_records_its_current_revision() {
    let mut h = Harness::new();
    let (task, node) = reported_task(&mut h);
    let mut native = super::production::native(&h);
    native.observe(&mut h.store, &task).unwrap();
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), &task.project).unwrap();
    let doc = tasks.load(&task.uuid).unwrap();
    let observation = h.store.task_observation(TEAM, node).unwrap().unwrap();
    assert_eq!(observation.revision, doc.revision());
    assert_eq!(observation.stage, crate::teams::ops::TaskStage::InReview);
}

#[test]
fn native_report_reads_the_holders_bounded_text_without_changing_the_document() {
    let mut h = Harness::new();
    let (task, _) = reported_task(&mut h);
    let before = {
        let tasks = ProjectStore::open(&h.temp.path().join("replica"), &task.project).unwrap();
        tasks.load(&task.uuid).unwrap().revision()
    };
    let text = super::production::native(&h)
        .report(&task)
        .unwrap()
        .unwrap();
    assert_eq!(text, "界".repeat(4000));
    let tasks = ProjectStore::open(&h.temp.path().join("replica"), &task.project).unwrap();
    assert_eq!(tasks.load(&task.uuid).unwrap().revision(), before);
}

/// The standing confirmation's completed resume of `member`'s `launch`,
/// naming `session` and the run it resumed into.
pub(super) fn resumed(
    state: &std::path::Path,
    team: &str,
    member: &str,
    launch: &str,
    session: &str,
    run: &str,
) {
    let dir = state.join("teams/standing-v1");
    crate::platform::private_dir(&dir).unwrap();
    let path = dir.join("journal.sqlite3");
    crate::sqlite_file::create(&path).unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch("CREATE TABLE IF NOT EXISTS standing_bindings(team TEXT,member TEXT,launch TEXT,parent TEXT,body TEXT,receipt_ms INTEGER,PRIMARY KEY(team,member,launch))").unwrap();
    let body = json!({"team":team,"member":member,"person":"local","machine":"machine","role":"developer","agent":"codex","account":"account","launch":launch,"session":session,"run":run,"conversation":"conversation","folder":"fixture-folder","common":"fixture-repo","nonce":"nonce"});
    db.execute(
        "INSERT OR REPLACE INTO standing_bindings VALUES(?1,?2,?3,'completed',?4,100)",
        rusqlite::params![team, member, launch, body.to_string()],
    )
    .unwrap();
}

/// The session record of `member`, bound to `launch`, on run `run`: the
/// supervisor's own record, in the ID layout, where its ID finds it.
fn session_record(native: &super::super::native::Native, member: &Member, launch: &str, run: &str) {
    use std::os::unix::fs::PermissionsExt;
    let sid = super::production::fleet::session_of(&member.id);
    let mut record = crate::state::listing_fixture::launched_record(
        &member.agent,
        &format!("{}/resumed", member.agent),
        launch,
        &sid,
        run,
        &member.worktree,
        &member.account,
    );
    crate::state::listing_fixture::confirmed(&mut record, "conversation");
    record["layout"] = json!("id");
    let dir = native.config.state.join("sessions");
    crate::platform::private_dir(&dir).unwrap();
    let path = dir.join(format!("{sid}.json"));
    std::fs::write(&path, record.to_string()).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

/// A completed resume only names the session (zerus-63rru.53): the current
/// run is the one the session's record carries now, and liveness is still
/// the fleet row's.
#[test]
fn a_completed_resume_names_the_session_whose_record_gives_the_run() {
    use super::super::availability::{current, Current, Unresolved};
    let h = Harness::new();
    let native = super::production::native(&h);
    let launch = "00000000-0000-4000-8000-0000000000c1";
    let mut member = fixture()
        .members
        .into_iter()
        .find(|m| m.id == "m1")
        .unwrap();
    member.session = Some(zerus_team_plan::model::SessionKey {
        machine: native.machine.clone(),
        session_id: format!("launch_id:{launch}"),
    });
    assert_eq!(
        current(&native.config, TEAM, &member, false).unwrap(),
        Current::Unresolved(Unresolved::NoHandle)
    );
    session_record(&native, &member, launch, "record-run");
    let session = super::production::fleet::session_of("m1");
    resumed(h.temp.path(), TEAM, "m1", launch, &session, "receipt-run");
    assert_eq!(
        super::super::availability::current_run(&native.config, TEAM, &member)
            .unwrap()
            .as_deref(),
        Some("record-run")
    );
    assert!(!matches!(
        super::super::availability::free(&native.config, "epoch", TEAM, &member, false),
        Ok(true)
    ));
}

struct ResumeAtView {
    native: super::super::native::Native,
    launch: String,
    free_reads: usize,
}
impl Ports for ResumeAtView {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, _: &mut Store, _: &TaskRef) -> Result<()> {
        Ok(())
    }
    fn free(&mut self, t: &Team, m: &Member) -> Result<bool> {
        let free = self.native.free(t, m)?;
        self.free_reads += 1;
        if self.free_reads == 1 {
            // A person resumes the member: its record moves to a new run.
            session_record(&self.native, m, &self.launch, "new-run");
        }
        Ok(free)
    }
    fn deliver(
        &mut self,
        _: &Team,
        _: &Node,
        _: &Member,
        _: &str,
    ) -> super::super::ports::DeliveryResult {
        panic!("busy resumed member must not be sent a task")
    }
}
fn fleet_runs(native: &super::super::native::Native, launch: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&native.config.state, std::fs::Permissions::from_mode(0o700)).unwrap();
    crate::platform::private_dir(&native.config.dir).unwrap();
    crate::platform::private_dir(&native.config.state.join("desktop")).unwrap();
    let path = native.config.state.join("desktop/state.sqlite3");
    let existed = path.exists();
    if !existed {
        crate::sqlite_file::create(&path).unwrap();
    }
    let db = rusqlite::Connection::open(path).unwrap();
    if !existed {
        zerus_desktop_catalog::migrate(&db).unwrap();
    }
    db.execute(
        "UPDATE desktop_catalog_owner SET epoch=?1 WHERE id=1",
        [&native.epoch],
    )
    .unwrap();
    db.execute("INSERT OR REPLACE INTO desktop_catalog_sources(computer,epoch,incarnation,generation,stream,seq,visible,complete,active,hello) VALUES('',?1,1,1,'stream',1,1,1,1,?2)",rusqlite::params![native.epoch,json!({"machine_id":native.machine}).to_string()]).unwrap();
    db.execute(
        "INSERT OR REPLACE INTO desktop_catalog_generations(id,computer) VALUES(1,'')",
        [],
    )
    .unwrap();
    for (run, activity) in [("old-run", "idle"), ("new-run", "busy")] {
        let row = super::rows::seed_row(
            "codex",
            &[
                ("name", json!("codex/resumed")),
                ("activity", json!(activity)),
                ("run_id", json!(run)),
                ("launch_id", json!(launch)),
                ("conversation_id", json!("conversation")),
            ],
        )
        .to_string();
        db.execute(
            "INSERT INTO desktop_catalog_rows(generation,key,json,bytes) VALUES(1,?1,?2,?3)",
            rusqlite::params![format!("run:{run}"), row, row.len()],
        )
        .unwrap();
    }
}
#[test]
fn a_member_resumed_between_the_view_and_the_intent_is_rechecked_on_its_new_run() {
    let mut h = Harness::new();
    let launch = uuid::Uuid::new_v4().to_string();
    let mut native = super::production::native(&h);
    let sample = fixture()
        .members
        .into_iter()
        .find(|m| m.id == "m1")
        .unwrap();
    h.run(
        &service(),
        crate::teams::ops::Operation::SetMemberSession {
            member: "m1".into(),
            session: zerus_team_plan::model::SessionKey {
                machine: native.machine.clone(),
                session_id: format!("launch_id:{launch}"),
            },
            account: sample.account,
            worktree: sample.worktree,
            ownership_nonce: sample.ownership_nonce,
        },
    )
    .unwrap();
    let node = h.add(&person(), 1);
    fleet_runs(&native, &launch);
    let bound = h.store.member(TEAM, "m1").unwrap().unwrap();
    session_record(&native, &bound, &launch, "old-run");
    // The launcher records the handle at binding.
    h.run(
        &service(),
        crate::teams::ops::Operation::SetMemberHandle {
            member: "m1".into(),
            session_id: super::production::fleet::session_of("m1"),
            label_hint: String::new(),
        },
    )
    .unwrap();
    super::super::native::TEST_INVENTORY.with(|s| {
        *s.borrow_mut() = Some(json!({"node_id":native.machine,"organization":{"projects":[]}}))
    });
    let team = h.store.team(TEAM).unwrap().unwrap();
    assert!(native
        .free(&team, &h.store.member(TEAM, "m1").unwrap().unwrap())
        .unwrap());
    assert!(!crate::teams::ops::readiness(&h.store, TEAM)
        .unwrap()
        .nodes
        .is_empty());
    let mut a = super::super::Runtime::open(h.temp.path(), &native.machine).unwrap();
    let mut p = ResumeAtView {
        native,
        launch,
        free_reads: 0,
    };
    a.tick(&mut h.store, &mut p, TEAM, 100, false).unwrap();
    assert_eq!(p.free_reads, 2);
    assert!(h.node(node).assignee.is_none());
    assert!(h.store.effects(TEAM, 16, None).unwrap().rows.is_empty());
    super::super::native::TEST_INVENTORY.with(|s| s.borrow_mut().take());
}
