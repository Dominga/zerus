//! Background session creation, against
//! `tray/src/SessionCreationController.cpp` and the placement resolver in
//! `tray/src/SessionsWindow.cpp`.
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use zerus_client_core::actions::Exit;
use zerus_client_core::creation::{self, *};
use zerus_client_core::launch::{self, Choices, Project, ProjectFolder, RepositoryChoice};
use zerus_client_core::repository::Action;

fn id(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}

struct Places {
    projects: Vec<Project>,
    placed: Vec<Placed>,
}

impl Placement for Places {
    fn resolve(&mut self, plan: &mut Value, evidence: &Value) -> Result<(), String> {
        let placed = place(plan, evidence, &self.projects)?;
        if let Some((id, name)) = &placed.create {
            self.projects.push(Project {
                id: id.clone(),
                name: name.clone(),
                folders: Vec::new(),
            });
        }
        self.placed.push(placed);
        Ok(())
    }
}

struct Driver {
    creations: Creations,
    clock: Arc<AtomicI64>,
    writes: VecDeque<(u64, Vec<u8>, Option<Remember>)>,
    calls: VecDeque<(u64, Call)>,
    launches: Vec<(String, Value)>,
    removed: Vec<String>,
    places: Places,
    journal: Vec<u8>,
}

impl Driver {
    fn new() -> Driver {
        let clock = Arc::new(AtomicI64::new(1_000));
        let next = Arc::new(AtomicU64::new(0));
        let time = Arc::clone(&clock);
        let creations = Creations::new(
            move || time.load(Ordering::SeqCst),
            move || id(next.fetch_add(1, Ordering::SeqCst) + 1),
        );
        Driver {
            creations,
            clock,
            writes: VecDeque::new(),
            calls: VecDeque::new(),
            launches: Vec::new(),
            removed: Vec::new(),
            places: Places {
                projects: vec![Project {
                    id: "p".into(),
                    name: "Project".into(),
                    folders: vec![ProjectFolder {
                        machine: String::new(),
                        path: "/srv/repo".into(),
                    }],
                }],
                placed: Vec::new(),
            },
            journal: Vec::new(),
        }
    }

    fn loaded() -> Driver {
        let mut driver = Driver::new();
        let effects = driver.creations.load(Journal::Missing);
        driver.take(effects);
        driver
    }

    fn take(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Write {
                    write,
                    bytes,
                    remember,
                } => self.writes.push_back((write, bytes, remember)),
                Effect::Call { flight, call } => self.calls.push_back((flight, call)),
                Effect::Launch { id, operation } => self.launches.push((id, operation)),
                Effect::Removed { id } => self.removed.push(id),
                Effect::Changed { .. } => {}
            }
        }
    }

    /// Acknowledges every write in order, as the ordered writer does.
    fn flush(&mut self) {
        while let Some((write, bytes, _)) = self.writes.pop_front() {
            self.journal = bytes;
            let effects = self.creations.written(write, true);
            self.take(effects);
        }
    }

    fn submit(&mut self, plan: Value) -> String {
        let (result, effects) = self.creations.submit(plan);
        self.take(effects);
        result.unwrap()
    }

    fn call(&mut self) -> (u64, Call) {
        self.calls.pop_front().expect("a call")
    }

    fn answer(&mut self, flight: u64, answer: Answer) {
        let effects = self.creations.answered(flight, answer, &mut self.places);
        self.take(effects);
    }

    fn state(&self, id: &str) -> String {
        self.op(id)["state"].as_str().unwrap_or("").to_owned()
    }

    fn op(&self, id: &str) -> Value {
        self.creations.operation(id).cloned().unwrap_or(Value::Null)
    }
}

fn ok(data: Value) -> Answer {
    Answer {
        ok: true,
        data,
        error: String::new(),
    }
}

fn fail(error: &str) -> Answer {
    Answer {
        ok: false,
        data: Value::Null,
        error: error.into(),
    }
}

const MACHINE: &str = "11111111-2222-4333-8444-555555555555";

fn identity(path: &str) -> Answer {
    ok(
        json!({"path": path, "directories": [], "creation_identity_v1": true, "computer_id": MACHINE}),
    )
}

fn choices() -> Choices {
    Choices {
        host: String::new(),
        machine: "arch".into(),
        agent: "codex".into(),
        account: "a".into(),
        name: "fix".into(),
        project: "p".into(),
        project_name: "Project".into(),
        path: "/srv/repo".into(),
        auto_preferred: false,
        branch: "work-1".into(),
        listed_folder: true,
        open_terminal: true,
        ..Choices::default()
    }
}

fn plan(choices: &Choices) -> Value {
    serde_json::to_value(launch::plan(choices)).unwrap()
}

#[test]
fn a_folder_launch_crosses_a_durable_barrier_before_every_step() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&choices()));
    assert_eq!(id, self::id(1));
    let op = driver.op(&id);
    assert_eq!(op["launch_id"], id.as_str());
    assert_eq!(op["state"], "queued");
    assert_eq!(op["creation_schema"], 1);
    // Nothing is sent before the admission is saved; its write carries the
    // choices the next form remembers.
    assert!(driver.calls.is_empty());
    let remember = driver.writes[0].2.clone().unwrap();
    assert_eq!(
        remember,
        Remember {
            host: String::new(),
            agent: "codex".into(),
            account: "a".into(),
            terminal: true
        }
    );
    driver.flush();
    assert_eq!(driver.state(&id), "checking");
    let (flight, call) = driver.call();
    assert_eq!(
        call,
        Call::CreationDirectory {
            host: String::new(),
            path: "/srv/repo".into(),
            expected_machine: String::new()
        }
    );
    driver.answer(flight, identity("/srv/repo"));
    assert_eq!(driver.op(&id)["expected_machine"], MACHINE);
    assert_eq!(driver.state(&id), "launching");
    assert!(driver.launches.is_empty(), "launch waits for its write");
    driver.flush();
    let (launched, operation) = driver.launches.pop().unwrap();
    assert_eq!(launched, id);
    assert_eq!(operation["launch_dispatched"], true);
    assert_eq!(operation["group_name"], "Project");
    let effects = driver.creations.launch_finished(&id, Ok(()));
    driver.take(effects);
    assert_eq!(driver.state(&id), "confirming");
    let effects = driver.creations.complete(&id);
    driver.take(effects);
    assert_eq!(driver.state(&id), "bound");
    assert_eq!(driver.op(&id)["show_pending"], false);
    // The journal is Qt's: schema 1 with the operations by ID.
    driver.flush();
    let journal: Value = serde_json::from_slice(&driver.journal).unwrap();
    assert_eq!(journal["schema"], 1);
    assert_eq!(journal["operations"][&id]["state"], "bound");
    let history: Vec<&str> = journal["operations"][&id]["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|step| step["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        history,
        ["queued", "checking", "launching", "confirming", "bound"]
    );
}

#[test]
fn a_failed_or_stale_write_sends_nothing() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&choices()));
    let (write, _, _) = driver.writes.pop_front().unwrap();
    let effects = driver.creations.written(write, false);
    driver.take(effects);
    assert_eq!(driver.state(&id), "storage_error");
    assert_eq!(driver.op(&id)["error"], SAVE_FAILED);
    assert!(driver.calls.is_empty());
    assert!(can_resume(&driver.op(&id)));

    // A write of an older revision continues nothing: Stop came after the
    // folder check's write was sent, so that check is never sent.
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&choices()));
    let (write, _, _) = driver.writes.pop_front().unwrap();
    let effects = driver.creations.written(write, true);
    driver.take(effects);
    assert_eq!(driver.state(&id), "checking");
    let (write, _, _) = driver.writes.pop_front().unwrap();
    let effects = driver.creations.stop(&id);
    driver.take(effects);
    let effects = driver.creations.written(write, true);
    driver.take(effects);
    driver.flush();
    assert!(driver.calls.is_empty());
    assert_eq!(driver.state(&id), "stopped");
}

#[test]
fn a_changed_computer_identity_keeps_the_original_setup() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&choices()));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    driver.flush();
    // The launch ends without a session: a new launch ID, then Continue.
    let effects = driver.creations.ended(&id, &id, "exited");
    driver.take(effects);
    let op = driver.op(&id);
    assert_eq!(op["state"], "failed");
    assert_eq!(op["previous_launches"], json!([id]));
    assert_ne!(op["launch_id"], id.as_str());
    assert_eq!(driver.creations.for_launch(&id), Some(id.clone()));
    let effects = driver.creations.resume(&id);
    driver.take(effects);
    driver.flush();
    let (flight, _) = driver.call();
    let mut other = identity("/srv/repo");
    other.data["computer_id"] = json!("99999999-2222-4333-8444-555555555555");
    driver.answer(flight, other);
    assert_eq!(driver.state(&id), "failed");
    assert_eq!(
        driver.op(&id)["error"],
        "The computer identity changed. The original setup has been kept."
    );
}

fn auto_choices() -> Choices {
    Choices {
        auto_preferred: true,
        automatic_available: true,
        automatic_path: "/srv/repo-work-1".into(),
        ..choices()
    }
}

fn catalog(root: &str) -> Value {
    json!({"state": "ok", "common_dir": "/srv/repo/.git", "selected_root": root,
           "selected_branch": "work-1", "creation_receipts_v1": true,
           "worktrees": [{"path": root, "available": true, "branch": "work-1", "kind": "linked"}]})
}

#[test]
fn a_generated_worktree_is_created_once_under_a_saved_id_then_verified() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&auto_choices()));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    assert_eq!(driver.state(&id), "verifying");
    let (flight, call) = driver.call();
    assert_eq!(
        call,
        Call::Catalog {
            host: String::new(),
            path: "/srv/repo".into(),
            expected_machine: MACHINE.into()
        }
    );
    driver.answer(flight, ok(catalog("/srv/repo")));
    let op = driver.op(&id);
    assert_eq!(op["state"], "worktree");
    assert_eq!(op["destination"], "/srv/repo-work-1");
    let worktree = op["worktree_id"].as_str().unwrap().to_owned();
    assert!(driver.calls.is_empty(), "creation waits for its saved ID");
    driver.flush();
    let (flight, call) = driver.call();
    assert_eq!(
        call,
        Call::CreateWorktree {
            host: String::new(),
            path: "/srv/repo".into(),
            common_dir: "/srv/repo/.git".into(),
            branch: "work-1".into(),
            destination: "/srv/repo-work-1".into(),
            request_id: worktree.clone(),
            receipt: true,
            expected_machine: MACHINE.into()
        }
    );
    driver.answer(
        flight,
        ok(json!({"request_id": worktree, "path": "/srv/repo-work-1", "common_dir": "/srv/repo/.git", "branch": "work-1", "status": "created"})),
    );
    assert_eq!(driver.op(&id)["worktree_done"], true);
    assert_eq!(driver.op(&id)["target"], "/srv/repo-work-1");
    driver.flush();
    let (flight, call) = driver.call();
    assert!(matches!(call, Call::CreationDirectory { ref path, .. } if path == "/srv/repo-work-1"));
    driver.answer(flight, identity("/srv/repo-work-1"));
    let (flight, _) = driver.call();
    driver.answer(flight, ok(catalog("/srv/repo-work-1")));
    // The prepared worktree's own receipt confirms it before the launch.
    let (flight, call) = driver.call();
    assert_eq!(
        call,
        Call::WorktreeReceipt {
            host: String::new(),
            action: "status".into(),
            request_id: worktree.clone(),
            expected_machine: MACHINE.into()
        }
    );
    driver.answer(
        flight,
        ok(json!({"schema": 1, "request_id": worktree, "status": "created"})),
    );
    assert_eq!(driver.state(&id), "launching");
    driver.flush();
    assert_eq!(driver.launches.len(), 1);
}

#[test]
fn a_changed_destination_or_repository_stops_before_creating() {
    let mut driver = Driver::loaded();
    let mut moved = auto_choices();
    moved.automatic_path = "/srv/elsewhere".into();
    let id = driver.submit(plan(&moved));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    let (flight, _) = driver.call();
    driver.answer(flight, ok(catalog("/srv/repo")));
    assert_eq!(driver.state(&id), "failed");
    assert_eq!(
        driver.op(&id)["error"],
        "The checkout destination changed. Review the creation choices before continuing."
    );
    assert!(driver.calls.is_empty());

    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&auto_choices()));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    let (flight, _) = driver.call();
    let mut stale = catalog("/srv/repo");
    stale["stale"] = json!(true);
    driver.answer(flight, ok(stale));
    assert_eq!(
        driver.op(&id)["error"],
        "The repository could not be confirmed. Choose the working folder again."
    );
}

#[test]
fn a_lost_worktree_is_read_again_and_never_recreated() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&auto_choices()));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    let (flight, _) = driver.call();
    driver.answer(flight, ok(catalog("/srv/repo")));
    driver.flush();
    let (flight, _) = driver.call();
    let worktree = driver.op(&id)["worktree_id"].as_str().unwrap().to_owned();
    driver.answer(flight, fail("connection lost"));
    assert_eq!(driver.state(&id), "unknown");
    assert_eq!(driver.op(&id)["failed_stage"], "worktree");
    assert!(
        !can_resume(&driver.op(&id)),
        "Continue would create it again"
    );
    let effects = driver.creations.check(&id);
    driver.take(effects);
    let (flight, call) = driver.call();
    assert!(matches!(call, Call::WorktreeReceipt { ref action, .. } if action == "status"));
    driver.answer(
        flight,
        ok(json!({"schema": 1, "request_id": worktree, "status": "created", "path": "/srv/repo-work-1", "common_dir": "/srv/repo/.git", "branch": "work-1"})),
    );
    assert_eq!(driver.state(&id), "recover");
    assert_eq!(
        driver.op(&id)["error"],
        "Worktree recovered. Continue setup to start the session."
    );
    assert!(can_resume(&driver.op(&id)));
}

#[test]
fn a_failed_worktree_without_effects_retires_its_id() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&auto_choices()));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    let (flight, _) = driver.call();
    driver.answer(flight, ok(catalog("/srv/repo")));
    driver.flush();
    let (flight, _) = driver.call();
    let worktree = driver.op(&id)["worktree_id"].clone();
    driver.answer(
        flight,
        Answer {
            ok: false,
            data: json!({"request_id": worktree, "status": "failed", "effects_started": false}),
            error: "branch exists".into(),
        },
    );
    let op = driver.op(&id);
    assert_eq!(op["state"], "failed");
    assert_eq!(op["worktree_previous"], json!([worktree]));
    assert!(op.get("worktree_id").is_none());
    assert!(can_resume(&op));
}

#[test]
fn an_older_hgs_without_receipts_cannot_confirm_a_lost_worktree() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&auto_choices()));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    let (flight, _) = driver.call();
    let mut older = catalog("/srv/repo");
    older["creation_receipts_v1"] = json!(false);
    driver.answer(flight, ok(older));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, fail("timed out"));
    let effects = driver.creations.check(&id);
    driver.take(effects);
    assert!(driver.calls.is_empty());
    assert_eq!(driver.state(&id), "unknown");
    assert_eq!(
        driver.op(&id)["error"],
        "This computer's older hgs cannot confirm the original worktree result. Inspect the retained destination before creating a separate session; this request will not be repeated."
    );
}

fn clone_choices(new_project: bool) -> Choices {
    Choices {
        path: String::new(),
        repository: Some(RepositoryChoice {
            url: "https://example.com/r.git".into(),
            parent: "/srv".into(),
            name: "r".into(),
            destination: "/srv/r".into(),
            created_path: String::new(),
            new_project,
            project_name: "New".into(),
        }),
        ..choices()
    }
}

#[test]
fn a_clone_checks_identity_and_destination_then_clones_once() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&clone_choices(true)));
    let reserved = driver.op(&id)["reserved_project"]
        .as_str()
        .unwrap()
        .to_owned();
    driver.flush();
    assert_eq!(driver.state(&id), "checking_repository");
    let (flight, call) = driver.call();
    assert!(matches!(call, Call::CreationDirectory { ref path, .. } if path == "/srv"));
    driver.answer(flight, identity("/srv"));
    assert!(driver.calls.is_empty(), "the identity is saved first");
    driver.flush();
    let (flight, call) = driver.call();
    assert_eq!(
        call,
        Call::Repository {
            host: String::new(),
            action: Action::Check,
            url: "https://example.com/r.git".into(),
            parent: "/srv".into(),
            name: "r".into(),
            request_id: String::new(),
            expected_machine: MACHINE.into()
        }
    );
    driver.answer(
        flight,
        ok(json!({"status": "ready", "path": "/srv/r", "parent": "/srv", "name": "r"})),
    );
    assert_eq!(driver.state(&id), "cloning");
    let clone = driver.op(&id)["clone_id"].as_str().unwrap().to_owned();
    assert!(driver.calls.is_empty(), "the clone ID is saved first");
    driver.flush();
    let (flight, call) = driver.call();
    assert!(
        matches!(call, Call::Repository { action: Action::Clone, ref request_id, .. } if *request_id == clone)
    );
    driver.answer(
        flight,
        ok(json!({"status": "cloned", "request_id": clone, "path": "/srv/r", "parent": "/srv", "name": "r"})),
    );
    let op = driver.op(&id);
    assert_eq!(op["clone_done"], true);
    assert_eq!(op["group"], reserved.as_str());
    assert_eq!(op["new_project"], false);
    assert_eq!(op["group_name"], "New");
    assert_eq!(
        driver.places.placed.last().unwrap(),
        &Placed {
            group: reserved.clone(),
            create: Some((reserved, "New".into())),
            add_folder: Some("/srv/r".into())
        }
    );
    driver.flush();
    assert_eq!(driver.state(&id), "checking");
}

#[test]
fn a_lost_clone_is_only_read_again() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&clone_choices(false)));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv"));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(
        flight,
        ok(json!({"status": "ready", "path": "/srv/r", "parent": "/srv", "name": "r"})),
    );
    driver.flush();
    let (flight, _) = driver.call();
    let clone = driver.op(&id)["clone_id"].as_str().unwrap().to_owned();
    driver.answer(flight, fail("connection lost"));
    assert_eq!(driver.state(&id), "unknown");
    assert!(!can_resume(&driver.op(&id)));
    let effects = driver.creations.check(&id);
    driver.take(effects);
    let (flight, call) = driver.call();
    assert!(
        matches!(call, Call::Repository { action: Action::Status, ref request_id, .. } if *request_id == clone)
    );
    driver.answer(
        flight,
        ok(json!({"status": "cloned", "request_id": clone, "path": "/srv/r", "parent": "/srv", "name": "r"})),
    );
    assert_eq!(driver.state(&id), "recover");
    assert_eq!(
        driver.op(&id)["error"],
        "Checkout recovered. Continue setup to start the session."
    );
    assert!(can_resume(&driver.op(&id)));
}

#[test]
fn another_proposed_destination_is_never_cloned() {
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&clone_choices(false)));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv"));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(
        flight,
        ok(json!({"status": "ready", "path": "/srv/r2", "parent": "/srv", "name": "r"})),
    );
    assert_eq!(
        driver.op(&id)["error"],
        "The computer proposed another destination. Review it before cloning."
    );
    assert!(driver.op(&id).get("clone_id").is_none());
}

#[test]
fn four_run_at_once_and_one_per_repository() {
    let mut driver = Driver::loaded();
    let mut ids = Vec::new();
    for index in 0..6 {
        let mut folder = choices();
        // Two creations share the first repository.
        folder.path = format!("/srv/repo{}", index.max(1));
        ids.push(driver.submit(plan(&folder)));
    }
    driver.flush();
    let running: Vec<String> = ids.iter().map(|id| driver.state(id)).collect();
    assert_eq!(
        running,
        ["checking", "queued", "checking", "checking", "checking", "queued"]
    );
    // Finishing one lets the next start.
    let (flight, _) = driver.call();
    driver.answer(flight, fail("gone"));
    driver.flush();
    assert_eq!(driver.state(&ids[1]), "checking");
}

#[test]
fn admission_is_bounded_and_safe() {
    let mut driver = Driver::loaded();
    let mut unsafe_plan = plan(&clone_choices(false));
    unsafe_plan["url"] = json!("https://user:token@example.com/r.git");
    let (result, _) = driver.creations.submit(unsafe_plan);
    assert_eq!(result, Err(UNSAFE_URL.into()));
    let (result, _) = driver.creations.submit(json!({"cmd": "codex"}));
    assert_eq!(result, Err(INCOMPLETE.into()));
    for _ in 0..UNSETTLED {
        driver.submit(plan(&choices()));
    }
    let (result, _) = driver.creations.submit(plan(&choices()));
    assert_eq!(result, Err(TOO_MANY.into()));
}

#[test]
fn settled_hidden_creations_are_evicted_oldest_first_and_their_launches_kept() {
    let mut driver = Driver::loaded();
    let mut ids = Vec::new();
    for index in 0..ADMITTED {
        driver.clock.store(1_000 + index as i64, Ordering::SeqCst);
        let id = driver.submit(plan(&choices()));
        let effects = driver.creations.stop(&id);
        driver.take(effects);
        let effects = driver.creations.hide(&id);
        driver.take(effects);
        ids.push(id);
    }
    driver.flush();
    assert_eq!(driver.creations.operations().len(), ADMITTED);
    let newest = driver.submit(plan(&choices()));
    assert_eq!(driver.removed, [ids[0].clone()]);
    assert!(driver.creations.owns(&newest));
    assert_eq!(driver.creations.for_launch(&ids[0]), Some(ids[0].clone()));
}

fn journal(operations: Value) -> Journal {
    Journal::Bytes(serde_json::to_vec(&json!({"schema": 1, "operations": operations})).unwrap())
}

fn saved(state: &str) -> Value {
    json!({"id": id(7), "creation_schema": 1, "state": state, "cmd": "codex", "tag": "fix",
           "target": "/srv/repo", "created": 5, "launch_id": id(7)})
}

#[test]
fn an_interrupted_creation_waits_in_continue_setup() {
    let mut driver = Driver::new();
    let effects = driver
        .creations
        .load(journal(json!({id(7): saved("launching")})));
    driver.take(effects);
    let op = driver.op(&id(7));
    assert_eq!(op["state"], "recover");
    assert_eq!(op["interrupted_stage"], "launching");
    assert_eq!(op["error"], INTERRUPTED);
    assert!(driver.calls.is_empty() && driver.launches.is_empty());
    // A dispatched launch is never resumed; its result is read instead.
    let mut dispatched = saved("launching");
    dispatched["launch_dispatched"] = json!(true);
    let mut driver = Driver::new();
    let effects = driver.creations.load(journal(json!({id(7): dispatched})));
    driver.take(effects);
    assert!(!can_resume(&driver.op(&id(7))));
}

#[test]
fn a_damaged_journal_keeps_requests_and_sends_nothing() {
    for (bytes, error) in [
        (Journal::Unreadable, READ_FAILED),
        (Journal::Bytes(b"{".to_vec()), UNSUPPORTED),
        (
            Journal::Bytes(br#"{"schema":2,"operations":{}}"#.to_vec()),
            UNSUPPORTED,
        ),
        (journal(json!({"x": saved("queued")})), INVALID_IDENTITY),
    ] {
        let mut driver = Driver::new();
        // A request submitted before the journal was read.
        let id = driver.submit(plan(&choices()));
        assert!(
            driver.writes.is_empty(),
            "nothing is written before the journal is read"
        );
        let effects = driver.creations.load(bytes);
        driver.take(effects);
        assert_eq!(driver.creations.storage_error(), error);
        // It fails, and with the journal unusable its failure cannot be
        // saved either.
        assert_eq!(driver.state(&id), "storage_error");
        assert_eq!(driver.op(&id)["failed_stage"], "saving");
        assert_eq!(driver.op(&id)["error"], error);
        assert!(driver.writes.is_empty() && driver.calls.is_empty());
    }
    let mut many = serde_json::Map::new();
    for index in 0..=OPERATIONS as u64 {
        let mut op = saved("failed");
        op["id"] = json!(id(1000 + index));
        many.insert(id(1000 + index), op);
    }
    let mut driver = Driver::new();
    let effects = driver.creations.load(journal(Value::Object(many)));
    driver.take(effects);
    assert_eq!(driver.creations.storage_error(), UNSUPPORTED);
}

#[test]
fn an_invalid_later_entry_does_not_partially_load_earlier_creations() {
    let mut driver = Driver::new();
    let effects = driver.creations.load(journal(
        json!({id(7):saved("launching"), "z-invalid":saved("unknown")}),
    ));
    driver.take(effects);
    assert_eq!(driver.creations.storage_error(), INVALID_IDENTITY);
    assert!(driver.creations.operations().is_empty());
    assert!(driver.writes.is_empty() && driver.calls.is_empty() && driver.launches.is_empty());
}

#[test]
fn stop_keeps_retained_files_and_reads_what_is_unconfirmed() {
    // Nothing under way: stopped at once.
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&choices()));
    let effects = driver.creations.stop(&id);
    driver.take(effects);
    assert_eq!(driver.state(&id), "stopped");
    // A call under way: stopping, then stopped when it answers.
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&auto_choices()));
    driver.flush();
    let (flight, _) = driver.call();
    let effects = driver.creations.stop(&id);
    driver.take(effects);
    assert_eq!(driver.state(&id), "stopping");
    assert_eq!(
        driver.op(&id)["error"],
        "Stopping setup. Retained files will be kept."
    );
    driver.flush();
    driver.answer(flight, identity("/srv/repo"));
    assert_eq!(driver.state(&id), "stopped");
    // A dispatched launch cannot be stopped.
    let mut driver = Driver::loaded();
    let id = driver.submit(plan(&choices()));
    driver.flush();
    let (flight, _) = driver.call();
    driver.answer(flight, identity("/srv/repo"));
    assert!(driver.creations.stop(&id).is_empty());
    // A failed launch call is unknown, never repeated.
    driver.flush();
    let effects = driver.creations.launch_finished(&id, Err("lost".into()));
    driver.take(effects);
    assert_eq!(driver.state(&id), "unknown");
    assert!(!can_resume(&driver.op(&id)));
    assert!(!driver.creations.can_edit(&id));
}

#[test]
fn placement_follows_the_resolver() {
    let projects = vec![
        Project {
            id: "a".into(),
            name: "A".into(),
            folders: vec![ProjectFolder {
                machine: String::new(),
                path: "/srv/a".into(),
            }],
        },
        Project {
            id: "b".into(),
            name: "B".into(),
            folders: vec![ProjectFolder {
                machine: "box".into(),
                path: "/srv/a".into(),
            }],
        },
    ];
    let base = json!({"group": "ungrouped", "implicit_inbox": true, "swarm": "S", "host": "", "add_folder": true});
    // Without a hint, the one project with exactly this folder here.
    let mut plan = base.clone();
    let placed = place(&mut plan, &json!({"path": "/srv/a"}), &projects).unwrap();
    assert_eq!(
        (
            placed.group.as_str(),
            &plan["group_name"],
            &plan["add_folder"]
        ),
        ("a", &json!("A"), &json!(false))
    );
    // This swarm's hint wins; a foreign one leaves Inbox.
    let mut plan = base.clone();
    place(&mut plan, &json!({"path": "/srv/a", "project_inference": {"schema": 1, "swarm_id": "S", "project_id": "b"}}), &projects).unwrap();
    assert_eq!(plan["group"], "b");
    let mut plan = base.clone();
    place(&mut plan, &json!({"path": "/srv/a", "project_inference": {"schema": 1, "swarm_id": "T", "project_id": "b"}}), &projects).unwrap();
    assert_eq!(plan["group"], "ungrouped");
    assert_eq!(plan["group_name"], "Inbox");
    // A removed project stops the creation with its files kept.
    let mut gone = json!({"group": "x", "host": ""});
    assert_eq!(
        place(&mut gone, &json!({}), &projects),
        Err(PROJECT_CHANGED.into())
    );
}

#[test]
fn diagnostics_redact_credentials() {
    assert_eq!(
        diagnostic("fatal: https://user:tok@example.com/r.git denied"),
        "fatal: https://[redacted]@example.com/r.git denied"
    );
    assert_eq!(diagnostic("HTTP://a@b"), "HTTP://[redacted]@b");
    assert_eq!(
        diagnostic("Authorization: Bearer xyz"),
        "Authorization=[redacted] xyz"
    );
    assert_eq!(diagnostic("api_key=abc next"), "api_key=[redacted] next");
    assert_eq!(diagnostic("ACCESS-TOKEN : t"), "ACCESS-TOKEN=[redacted]");
    assert_eq!(
        diagnostic("apikey:1 password=2"),
        "apikey=[redacted] password=[redacted]"
    );
    // Inside a word, or without a value, nothing changes.
    assert_eq!(diagnostic("mypassword=1"), "mypassword=1");
    assert_eq!(diagnostic("password: "), "password: ");
    assert_eq!(diagnostic(&"x".repeat(5000)).len(), 4000);
}

#[test]
fn repository_addresses_without_secrets_are_kept() {
    for (url, safe) in [
        ("git@example.com:r.git", true),
        ("https://example.com/r.git", true),
        ("ssh://git@example.com/r.git", true),
        ("https://user@example.com/r.git", false),
        ("https://user:pw@example.com/r.git", false),
        ("ssh://git:pw@example.com/r.git", false),
        ("https://example.com/r.git?token=1", false),
        ("https://example.com/r.git#x", false),
        ("https://exa mple.com/r.git", false),
    ] {
        assert_eq!(safe_repository_url(url), safe, "{url}");
    }
}

#[test]
fn operations_are_validated_as_qt_does() {
    assert!(valid_operation(&id(7), &saved("queued")));
    for (key, value) in [
        ("state", json!("other")),
        ("cmd", json!("-x")),
        ("cmd", json!("a b")),
        ("tag", json!("")),
        ("created", json!("now")),
        ("auto", json!("yes")),
        ("launch_id", json!("x")),
        ("target", json!("a\nb")),
        ("creation_schema", json!(2)),
    ] {
        let mut op = saved("queued");
        op[key] = value;
        assert!(!valid_operation(&id(7), &op), "{key}");
    }
    assert!(!valid_operation("x", &saved("queued")));
    assert!(uuid(&format!("{{{}}}", id(7))));
    assert!(!uuid("00000000-0000-0000-0000-000000000000"));
    assert!(canonical_uuid(MACHINE) && canonical_uuid("abcdef00-0000-4000-8000-00000000000a"));
    assert!(!canonical_uuid("ABCDEF00-0000-4000-8000-00000000000A"));
    assert!(!canonical_uuid(&format!("{{{MACHINE}}}")));
}

#[test]
fn continue_setup_never_repeats_what_is_unconfirmed() {
    let open = |extra: Value| {
        let mut op = json!({"state": "failed"});
        for (key, value) in extra.as_object().unwrap() {
            op[key] = value.clone();
        }
        can_resume(&op)
    };
    assert!(open(json!({})));
    assert!(!open(json!({"launch_dispatched": true})));
    assert!(!open(json!({"clone_id": id(1)})));
    assert!(open(json!({"clone_id": id(1), "clone_done": true})));
    assert!(!open(json!({"worktree_id": id(2)})));
    assert!(open(json!({"worktree_id": id(2), "worktree_done": true})));
    for state in ["recover", "stopped", "storage_error"] {
        assert!(can_resume(&json!({"state": state})), "{state}");
    }
    for state in ["unknown", "launching", "queued", "bound", "stopping"] {
        assert!(!can_resume(&json!({"state": state})), "{state}");
    }
}

#[test]
fn cards_use_qt_captions_and_details() {
    assert_eq!(
        caption("checking_repository"),
        "Checking repository access…"
    );
    assert_eq!(caption("recover"), "Continue setup");
    assert_eq!(caption("bound"), "Session created");
    let op = json!({"tag": "fix", "machine": "arch", "cmd": "opencode", "target": "/srv/repo",
        "state": "failed", "group_name": "P", "model": "m", "effort": "high", "error": "boom",
        "failed_stage": "worktree", "destination": "/srv/repo-work-1", "branch": "work-1",
        "created": 10, "updated": 10, "history": [{"state": "queued", "at": 10}]});
    let text = details(&op, 100, &|seconds, stamp| format!("{stamp:?}{seconds}"));
    assert_eq!(
        text,
        "fix\n\nProject: P\n\nComputer: arch\n\nAgent: opencode\n\nFolder: /srv/repo\n\nCreation failed\n\nModel: m\nEffort: high\n\nboom\n\nLast setup stage: Creating worktree…\n\nWorktree destination: /srv/repo-work-1\nBranch: work-1\n\nCreated: Created10\n\nStep10  Queued"
    );
    let mut slow = op.clone();
    slow["state"] = json!("cloning");
    assert!(details(&slow, 40, &|_, _| String::new()).contains("Taking longer than usual."));
    assert!(!details(&slow, 39, &|_, _| String::new()).contains("Taking longer than usual."));
}

#[test]
fn a_receipt_naming_this_creation_keeps_its_data() {
    let exit = Exit::Code {
        code: 0,
        stdout: json!({"request_id": "r", "status": "failed", "effects_started": false, "detail": "exists"})
            .to_string()
            .into_bytes(),
        stderr: String::new(),
    };
    let answer = creation::created(
        exit,
        "r",
        "work-1",
        "/srv/repo/.git",
        true,
        "/srv/repo-work-1",
    );
    assert!(!answer.ok);
    assert_eq!(answer.error, "exists");
    assert_eq!(answer.data["effects_started"], false);
}

#[cfg(unix)]
mod calls {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicUsize;

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct Fake {
        dir: PathBuf,
        hgs: PathBuf,
    }

    impl Fake {
        fn new(config: Value) -> Fake {
            let dir = std::env::temp_dir().join(format!(
                "zerus-fake-hgs-creation-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let hgs = dir.join("hgs");
            let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
            // A symlink, never a copy (see tests/actions.rs).
            std::os::unix::fs::symlink(manifest.join("tests/support/fake_hgs_creation.py"), &hgs)
                .unwrap();
            std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
            Fake { dir, hgs }
        }

        fn log(&self) -> Vec<Vec<String>> {
            std::fs::read_to_string(self.dir.join("log.ndjson"))
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const HELP: &str = "usage: hgs ... guarded-create-v1 repository-clone-v1";

    #[test]
    fn the_creation_directory_is_read_only_after_the_identity_probe() {
        let fake = Fake::new(json!({"help": HELP, "replies": {"dirs": {"stdout":
            {"path": "/srv/repo", "directories": [], "creation_identity_v1": true, "computer_id": MACHINE}}}}));
        let answer = run_call(
            &fake.hgs,
            &Call::CreationDirectory {
                host: "box".into(),
                path: "/srv/repo".into(),
                expected_machine: MACHINE.into(),
            },
        );
        assert!(answer.ok, "{}", answer.error);
        assert_eq!(answer.data["computer_id"], MACHINE);
        assert_eq!(
            fake.log(),
            [
                vec!["@box", "--help"],
                vec!["@box", "--help"],
                vec![
                    "@box",
                    "--expect-machine",
                    MACHINE,
                    "dirs",
                    "--creation",
                    "/srv/repo"
                ],
            ]
        );
        // An older CLI cannot check the expected computer: nothing is read.
        let older = Fake::new(json!({"help": "usage: hgs"}));
        let answer = run_call(
            &older.hgs,
            &Call::CreationDirectory {
                host: String::new(),
                path: "/srv/repo".into(),
                expected_machine: MACHINE.into(),
            },
        );
        assert_eq!(
            answer.error,
            "The original computer identity cannot be verified by this CLI."
        );
        assert_eq!(older.log(), [vec!["--help"]]);
        // A reply without the identity is refused.
        let bare = Fake::new(
            json!({"help": HELP, "replies": {"dirs": {"stdout": {"path": "/srv", "directories": []}}}}),
        );
        let answer = run_call(
            &bare.hgs,
            &Call::CreationDirectory {
                host: String::new(),
                path: String::new(),
                expected_machine: String::new(),
            },
        );
        assert_eq!(answer.error, "Invalid creation directory identity.");
        assert_eq!(bare.log().last().unwrap(), &vec!["dirs", "--creation", "~"]);
    }

    #[test]
    fn a_repository_call_runs_after_its_probe_and_only_once() {
        let fake = Fake::new(json!({"help": HELP, "replies": {"repository": {"stdout":
            {"status": "ready", "name": "r", "parent": "/srv", "path": "/srv/r"}}}}));
        let answer = run_call(
            &fake.hgs,
            &Call::Repository {
                host: String::new(),
                action: Action::Check,
                url: "https://example.com/r.git".into(),
                parent: "/srv".into(),
                name: "r".into(),
                request_id: String::new(),
                expected_machine: MACHINE.into(),
            },
        );
        assert!(answer.ok, "{}", answer.error);
        let log = fake.log();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0], ["--help"]);
        assert_eq!(log[1], ["--expect-machine", MACHINE, "--help"]);
        assert_eq!(
            &log[2][..4],
            ["--expect-machine", MACHINE, "repository", "check"]
        );
        let older = Fake::new(json!({"help": "usage: hgs guarded-create-v1"}));
        let answer = run_call(
            &older.hgs,
            &Call::Repository {
                host: "box".into(),
                action: Action::Clone,
                url: "u".into(),
                parent: "/srv".into(),
                name: "r".into(),
                request_id: id(1),
                expected_machine: String::new(),
            },
        );
        assert_eq!(answer.error, "Update hgs on box to clone repositories.");
        assert_eq!(older.log(), [vec!["@box", "--help"]]);
    }

    #[test]
    fn a_worktree_is_created_after_the_guard_probe_with_its_receipt() {
        let fake = Fake::new(json!({"help": HELP, "replies": {"worktrees": {"stdout":
            {"request_id": id(3), "status": "created", "path": "/srv/repo-work-1", "branch": "work-1", "common_dir": "/srv/repo/.git"}}}}));
        let answer = run_call(
            &fake.hgs,
            &Call::CreateWorktree {
                host: String::new(),
                path: "/srv/repo".into(),
                common_dir: "/srv/repo/.git".into(),
                branch: "work-1".into(),
                destination: "/srv/repo-work-1".into(),
                request_id: id(3),
                receipt: true,
                expected_machine: MACHINE.into(),
            },
        );
        assert!(answer.ok, "{}", answer.error);
        let log = fake.log();
        assert_eq!(log[0], ["--help"]);
        assert_eq!(
            &log[1][..4],
            ["--expect-machine", MACHINE, "worktrees", "create"]
        );
        assert_eq!(log[1].last().map(String::as_str), Some("--receipt"));
        // Without the guard marker nothing is created.
        let older = Fake::new(json!({"help": "usage: hgs"}));
        let answer = run_call(
            &older.hgs,
            &Call::CreateWorktree {
                host: String::new(),
                path: "/srv/repo".into(),
                common_dir: "/srv/repo/.git".into(),
                branch: "work-1".into(),
                destination: "/srv/repo-work-1".into(),
                request_id: id(3),
                receipt: true,
                expected_machine: MACHINE.into(),
            },
        );
        assert!(!answer.ok);
        assert!(answer
            .error
            .starts_with(zerus_client_core::worktrees::GUARDED_UNSUPPORTED));
        assert_eq!(older.log(), [vec!["--help"]]);
    }
}

#[test]
fn task_creation_keeps_its_reserved_launch_id_and_refuses_duplicate_admission() {
    let mut driver = Driver::loaded();
    let reserved = id(99);
    let (result, effects) = driver
        .creations
        .submit_reserved(plan(&choices()), &reserved);
    assert_eq!(result, Ok(reserved.clone()));
    driver.take(effects);
    assert_eq!(driver.op(&reserved)["launch_id"], reserved);
    assert!(driver.calls.is_empty() && driver.launches.is_empty());
    let before = driver.writes.len();
    let (result, effects) = driver
        .creations
        .submit_reserved(plan(&choices()), &reserved);
    assert!(result.is_err() && effects.is_empty());
    assert_eq!(driver.writes.len(), before);
}
