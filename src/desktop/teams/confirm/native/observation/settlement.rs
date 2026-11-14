//! Real state/archive, standing ledger, audit and card writers in private children.
use super::*;
use crate::desktop::teams::confirm::{engine, ledger::Ledger, ports::Ports};
use crate::state::resume_settlement_fixture::Fixture;
use crate::teams::{
    ops::{self, tests as setup, Operation, Request},
    store::Store,
};
use std::{
    path::Path,
    process::Command,
    sync::{atomic::AtomicBool, Arc},
};

struct WriterPorts {
    exit: Fixture,
    config: crate::config::Config,
    store: Store,
    ledger: Ledger,
    binding: Binding,
    stop: Arc<AtomicBool>,
    originals: usize,
    admitted: usize,
    cards: usize,
}
impl WriterPorts {
    fn new() -> Self {
        let exit = Fixture::new();
        let config = crate::config::Config::load().unwrap();
        // Unit context, as in ops::tests::Harness. Rows below use store writers.
        crate::platform::private_dir(&config.state.join("teams")).unwrap();
        std::fs::write(config.state.join("teams/preview"), "on\n").unwrap();
        let mut store = Store::open(&config.state).unwrap();
        ops::apply(
            &mut store,
            &setup::person(),
            &setup::request(0, setup::create()),
            1,
        )
        .unwrap();
        let (mut b, _, _) = super::super::super::policy::tests::sample();
        for (target, key) in [
            (&mut b.run, "run_id"),
            (&mut b.conversation, "conversation_id"),
            (&mut b.session, "session_id"),
            (&mut b.launch, "launch_id"),
            (&mut b.folder, "cwd"),
            (&mut b.account, "account_id"),
        ] {
            *target = text(&exit.raw, key);
        }
        let revision = store.team(&b.team).unwrap().unwrap().revision;
        ops::apply(
            &mut store,
            &setup::service(),
            &Request {
                request_id: uuid::Uuid::new_v4().to_string(),
                team: b.team.clone(),
                expected_revision: revision,
                operation: Operation::SetMemberSession {
                    member: b.member.clone(),
                    session: zerus_team_plan::model::SessionKey {
                        machine: b.machine.clone(),
                        session_id: format!("launch_id:{}", b.launch),
                    },
                    account: b.account.clone(),
                    worktree: b.folder.clone(),
                    ownership_nonce: b.nonce.clone(),
                },
            },
            2,
        )
        .unwrap();
        assert!(
            store
                .team(&b.team)
                .unwrap()
                .unwrap()
                .settings
                .delegations
                .resume_exited
        );
        let ledger = Ledger::open(&config.state).unwrap();
        Self {
            exit,
            config,
            store,
            ledger,
            binding: b,
            stop: Arc::new(AtomicBool::new(false)),
            originals: 0,
            admitted: 0,
            cards: 0,
        }
    }
    fn run(&mut self, now: i64) {
        engine::step(self, &self.binding.clone(), "settle-fixture", now).unwrap();
    }
    fn counts(&self) -> (i64, i64, usize) {
        let effects =
            Connection::open(self.config.state.join("teams/standing-v1/journal.sqlite3")).unwrap();
        let audit = crate::approval_log::open(&self.config.state.join("approval-audit")).unwrap();
        (
            effects
                .query_row("SELECT count(*) FROM standing_effects", [], |r| r.get(0))
                .unwrap(),
            audit
                .query_row("SELECT count(*) FROM attempts", [], |r| r.get(0))
                .unwrap(),
            self.store
                .cards(&self.binding.team, 200, None)
                .unwrap()
                .rows
                .len(),
        )
    }
    fn prior_reservation(&mut self, n: usize, now: i64) {
        // Synthetic prior-run identities; the quota/effect/audit/card rows themselves
        // are written only by production claim, audit and team-card operations.
        let mut b = self.binding.clone();
        b.run = format!("prior-{n}");
        let mut s = self.observe(&self.binding.clone()).unwrap();
        s.run = b.run.clone();
        s.resume_wait = None;
        let p = engine::candidate(&b, &s, Kind::Resume, Value::Null);
        let mut row = self.claim(&p, "prior", now).unwrap().record;
        row.audit = Some(self.audit_begin(&p).unwrap());
        self.save(&row).unwrap();
        row.state = "failed".into();
        row.detail = "prior definite refusal".into();
        self.audit_finish(&row).unwrap();
        row.audit_done = true;
        self.card(&row).unwrap();
        row.card_done = true;
        self.save(&row).unwrap();
    }
}
impl Ports for WriterPorts {
    fn facts(&mut self, b: &Binding) -> Result<Facts> {
        Ok(Facts {
            team: self.store.team(&b.team).unwrap().unwrap(),
            member: self.store.member(&b.team, &b.member).unwrap().unwrap(),
            binding: b.clone(),
            preview: true,
        })
    }
    fn observe(&mut self, b: &Binding) -> Result<Snapshot> {
        settled_view(
            &b.machine,
            b,
            &self.exit.view(),
            &self.exit.raw,
            Some(self.exit.id.clone()),
        )
    }
    fn ownership(&mut self, b: &Binding) -> Result<Ownership> {
        // Ownership is test context, not the property under test. Native CI proves it.
        Ok(Ownership {
            folder: b.folder.clone(),
            common: b.common.clone(),
            nonce: b.nonce.clone(),
            launch: b.launch.clone(),
            exists: Path::new(&b.folder).is_dir(),
            unchanged: true,
        })
    }
    fn unsettled(&mut self, b: &Binding) -> Result<Option<Record>> {
        self.ledger.unsettled(b)
    }
    fn pending_brief(&mut self, b: &Binding) -> Result<Option<Plan>> {
        self.ledger.pending_brief(b)
    }
    fn claim(&mut self, p: &Plan, e: &str, n: i64) -> Result<Claim> {
        self.ledger.claim(p, e, n)
    }
    fn save(&mut self, r: &Record) -> Result<()> {
        self.ledger.save(r)
    }
    fn audit_begin(&mut self, p: &Plan) -> Result<Audit> {
        Native::new(
            &self.config,
            &p.binding.machine,
            &self.stop,
            &mut self.store,
        )?
        .audit_begin(p)
    }
    fn audit_finish(&mut self, r: &Record) -> Result<()> {
        Native::new(
            &self.config,
            &r.plan.binding.machine,
            &self.stop,
            &mut self.store,
        )?
        .audit_finish(r)
    }
    fn card(&mut self, r: &Record) -> Result<()> {
        self.cards += 1;
        Native::new(
            &self.config,
            &r.plan.binding.machine,
            &self.stop,
            &mut self.store,
        )?
        .card(r)
    }
    fn original(&mut self, _r: &Record, check: &mut dyn FnMut(&mut Self) -> Result<()>) -> Outcome {
        if let Err(e) = check(self) {
            return Outcome::NotStarted(e);
        }
        self.originals += 1;
        match self.exit.restore_admitted() {
            Ok(0) => {
                self.admitted += 1;
                Outcome::NotStarted("Test stops after the real Restore dry admission".into())
            }
            Ok(_) => panic!("unexpected admission result"),
            Err(e) => Outcome::Refused(e),
        }
    }
}
fn child(case: &str) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for name in [
        "home", "state", "cfg", "tmux", "tmp", "runtime", "data", "cache",
    ] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    let exe = std::env::current_exe().unwrap();
    let test = format!("{}::fixture", module_path!().split_once("::").unwrap().1);
    let output = Command::new(&exe)
        .args([
            "--exact",
            &test,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HGS_SETTLEMENT_FIXTURE", case)
        .env("HOME", root.join("home"))
        .env("HGS_STATE_DIR", root.join("state"))
        .env("HGS_CONFIG_DIR", root.join("cfg"))
        .env("TMUX_TMPDIR", root.join("tmux"))
        .env("TMPDIR", root.join("tmp"))
        .env("XDG_RUNTIME_DIR", root.join("runtime"))
        .env("XDG_CONFIG_HOME", root.join("cfg"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("HGS_PEERS", "")
        .env("HGS_EXECUTABLE", &exe)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("HGS_SESSION")
        .env_remove("HGS_RUN_ID")
        .env_remove("HGS_SESSION_ID")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&format!("SETTLEMENT_WRITERS_EXECUTED={case}")),
        "child fixture did not execute: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
#[test]
fn an_archived_exit_waits_for_its_pane_before_restore_admission() {
    child("pane");
}
#[test]
fn an_archived_exit_waits_for_its_saved_label_before_restore_admission() {
    child("label");
}
#[test]
fn an_unsettled_exit_expires_to_one_card_and_never_resumes() {
    child("expiry");
}
#[test]
fn a_deferral_reserves_no_hourly_resume() {
    child("quota");
}
#[test]
#[ignore = "run in a private child by the four settlement controls"]
fn fixture() {
    let case = std::env::var("HGS_SETTLEMENT_FIXTURE").unwrap();
    let mut p = WriterPorts::new();
    if case == "label" {
        p.exit.hold_label();
    } else {
        p.exit.hold_pane();
    }
    let s = p.observe(&p.binding.clone()).unwrap();
    let wait = s.resume_wait.unwrap();
    assert!(
        engine::RESUME_SETTLE_MS >= crate::state::LEFTOVER_DELETE_TIME.as_millis() as i64,
        "standing resume must cover the teardown delete budget"
    );
    assert_eq!(wait.reason, "waiting for the original run to settle");
    assert!(p
        .exit
        .restore_admitted()
        .unwrap_err()
        .contains("already uses this name"));
    if case == "quota" {
        p.prior_reservation(1, wait.until - 4000);
        p.prior_reservation(2, wait.until - 3000);
    }
    let before = p.counts();
    let cards = p.cards;
    for now in [wait.until - 2000, wait.until - 1000] {
        p.run(now);
        assert_eq!(p.counts(), before);
        assert_eq!(p.originals, 0);
        assert_eq!(p.cards, cards);
    }
    if case == "expiry" {
        for n in 0..3 {
            p.run(wait.until + n);
            assert_eq!(p.counts(), (1, 0, 1));
            assert_eq!(p.cards, 1);
            assert_eq!(p.originals, 0);
        }
        p.exit.release();
        p.run(wait.until + 4);
        assert_eq!(p.counts(), (1, 0, 1));
        assert_eq!(p.cards, 1);
        assert_eq!(p.originals, 0);
    } else {
        p.exit.release();
        p.run(wait.until - 500);
        assert_eq!(p.originals, 1);
        assert_eq!(
            p.admitted, 1,
            "actual Restore checks must admit the released name"
        );
        assert_eq!(p.counts().0, before.0 + 1);
        if case == "quota" {
            let mut b = p.binding.clone();
            b.run = "fourth".into();
            let mut s = p.observe(&p.binding.clone()).unwrap();
            s.run = b.run.clone();
            let plan = engine::candidate(&b, &s, Kind::Resume, Value::Null);
            assert!(p.ledger.claim(&plan, "fourth", wait.until).unwrap().limited);
        }
    }
    println!("SETTLEMENT_WRITERS_EXECUTED={case}");
}
