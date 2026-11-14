//! Per-member originals, stored separately from task hand-outs. Unique keys
//! and compare-and-swap transitions prevent a second service writer acquiring
//! an existing original. Reading one key never scans a team's journal.
use super::ports::{ObservedSession, Placement, Plan, Result, Worktree};
use crate::teams::{
    ops::Request,
    store::{storage, Guard},
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
use zerus_team_plan::{model::Team, problem::code, Problem};

pub(super) const BRANCH: &str = "@branch";
pub(super) const SCHEMA: u32 = 4;
pub(super) const BUDGET: &str = "@budget";
pub(super) const FILE: &str = "member-launch-v1.sqlite3";

pub(super) mod names;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum State {
    Retry,
    LaunchUnknown,
    BranchReady,
    BranchIntent,
    BranchDone,
    WorktreeReady,
    WorktreeIntent,
    WorktreeDone,
    WorktreeVerify,
    LaunchReady,
    LaunchIntent,
    Submitted,
    Bound,
    ParentIntent,
    Done,
    Blocked,
}

impl State {
    pub fn waits_on_backoff(&self) -> bool {
        matches!(
            self,
            Self::Retry
                | Self::BranchReady
                | Self::WorktreeReady
                | Self::WorktreeVerify
                | Self::LaunchReady
                | Self::Submitted
                | Self::LaunchUnknown
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BranchPlan {
    pub team: String,
    pub project: String,
    pub machine: String,
    pub branch: String,
    pub base: String,
    pub placement: Placement,
}
impl BranchPlan {
    pub fn of(team: &Team, placement: Placement) -> Self {
        Self {
            team: team.id.clone(),
            placement,
            project: team.project.clone(),
            machine: team.home.clone(),
            branch: team.integration_branch.clone(),
            base: team.base_commit.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Row {
    pub team: String,
    pub member: String,
    pub epoch: String,
    pub person: String,
    pub version: u64,
    pub original: String,
    pub observed: Option<ObservedSession>,
    pub parent_result: Option<String>,
    pub state: State,
    pub plan: Option<Plan>,
    pub branch: Option<BranchPlan>,
    pub worktree: Option<Worktree>,
    pub problem: Option<Problem>,
    pub card_request: Option<Request>,
    /// One request ID per intended write, kept across every retry and a
    /// restart so a lost answer is replayed, never repeated (zerus-63rru.91).
    /// Skipped when none: a row with no pending reservation keeps the
    /// previous build's layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_request: Option<Request>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_request: Option<Request>,
    pub card_done: bool,
    #[serde(default)]
    pub card_id: Option<u64>,
    #[serde(default)]
    pub retry_at: i64,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub retry_seen: bool,
    #[serde(default)]
    pub parent_tries: u32,
    #[serde(default)]
    pub unknown_until: i64,
    #[serde(default)]
    pub late_after_stop: bool,
    #[serde(default)]
    pub placement: Option<Placement>,
}
impl Row {
    pub fn new(team: &Team, member: &str, epoch: &str, state: State) -> Self {
        Self {
            team: team.id.clone(),
            member: member.into(),
            epoch: epoch.into(),
            person: team.owner.clone(),
            version: 0,
            original: uuid::Uuid::new_v4().to_string(),
            observed: None,
            parent_result: None,
            state,
            plan: None,
            branch: None,
            worktree: None,
            problem: None,
            card_request: None,
            session_request: None,
            close_request: None,
            card_done: false,
            card_id: None,
            retry_at: 0,
            failures: 0,
            retry_seen: false,
            parent_tries: 0,
            unknown_until: 0,
            late_after_stop: false,
            placement: None,
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "launch/checkout.rs"]
pub(super) mod checkout;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use checkout::Checkout;
pub(super) struct Ledger(Connection);
impl Ledger {
    pub fn open(state: &Path) -> Result<Self> {
        if !crate::teams::enabled(state) {
            return Err(Problem::new(code::TEAMS_INACTIVE, "Teams preview is off."));
        }
        let state = crate::platform::canonical(state).map_err(storage)?;
        let dir = state.join("teams");
        if !crate::platform::verify_private(&dir, crate::platform::Private::Directory)
            .map_err(storage)?
        {
            return Err(Guard::LaunchDirectory.problem());
        }
        let path = dir.join(FILE);
        for suffix in ["", "-wal", "-shm", "-journal"] {
            match crate::platform::verify_private(
                &dir.join(format!("{FILE}{suffix}")),
                crate::platform::Private::File,
            ) {
                Ok(true) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Ok(false) => return Err(Guard::LaunchPrivacy.problem()),
                Err(error) => return Err(storage(error)),
            }
        }
        #[cfg(unix)]
        let identity = {
            crate::sqlite_file::create(&path).map_err(storage)?;
            crate::sqlite_file::private(&path).map_err(storage)?
        };
        #[cfg(not(unix))]
        {
            let file = crate::platform::no_follow(crate::platform::private_open(
                std::fs::OpenOptions::new()
                    .create(true)
                    .truncate(false)
                    .read(true)
                    .write(true),
            ))
            .open(&path)
            .map_err(storage)?;
            if !crate::platform::verify_private_file(&file).map_err(storage)? {
                return Err(Guard::LaunchPrivacy.problem());
            }
        }
        let mut db = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(storage)?;
        #[cfg(unix)]
        crate::sqlite_file::bind_private(&db, &path, identity).map_err(storage)?;
        db.busy_timeout(Duration::from_secs(2)).map_err(storage)?;
        let tx = db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        let version: u32 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        match version {
            0 => {
                tx.execute_batch(include_str!("schema.sql"))
                    .map_err(storage)?;
                tx.execute_batch(include_str!("bounds.sql"))
                    .map_err(storage)?;
            }
            1 => {
                tx.execute_batch("UPDATE originals SET data=json_remove(data,'$.plan.instructions','$.plan.parent_session'); PRAGMA user_version=2;").map_err(storage)?;
                tx.execute_batch(include_str!("bounds.sql"))
                    .map_err(storage)?;
            }
            2..=4 => (),
            _ => return Err(Guard::LaunchSchema.problem()),
        }
        if version < 3 {
            tx.execute_batch(include_str!("notices.sql"))
                .map_err(storage)?;
            tx.execute_batch("PRAGMA user_version=3").map_err(storage)?;
        }
        if version < 4 {
            tx.execute_batch(include_str!("health.sql"))
                .map_err(storage)?;
            tx.execute_batch("PRAGMA user_version=4").map_err(storage)?;
        }
        tx.execute_batch("CREATE TABLE IF NOT EXISTS launch_checkout_objects (
            team TEXT NOT NULL, member TEXT NOT NULL, launch TEXT NOT NULL,
            data TEXT NOT NULL CHECK(length(CAST(data AS BLOB))<=16384), PRIMARY KEY(team,member,launch));").map_err(storage)?;
        tx.execute_batch(include_str!("launch-names.sql")).map_err(storage)?;
        tx.commit().map_err(storage)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")
            .map_err(storage)?;
        Ok(Self(db))
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn record_checkout(&mut self, plan: &Plan, tree: &Worktree) -> Result<bool> {
        checkout::record(&mut self.0, plan, tree)
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn checkout(&self, team: &str, member: &str, launch: &str) -> Result<Option<Checkout>> {
        checkout::lookup(&self.0, team, member, launch)
    }
    /// The timers keep their doorbell reservations in this private file.
    pub fn connection(&self) -> &Connection {
        &self.0
    }
    pub fn write_health(&mut self, code: Option<i64>, now: i64) -> Result<()> {
        let at = code.map(|_| now.max(0));
        if self
            .0
            .execute(
                "UPDATE launch_health SET code=?1,at_ms=?2 WHERE id=1",
                params![code, at],
            )
            .map_err(storage)?
            != 1
        {
            return Err(Guard::LaunchHealth.problem());
        }
        Ok(())
    }
    pub fn get(&self, team: &str, member: &str) -> Result<Option<Row>> {
        let data: Option<String> = self
            .0
            .query_row(
                &format!(
                    "SELECT data FROM {} WHERE team=?1 AND member=?2",
                    table(member)
                ),
                params![team, member],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        let row: Option<Row> = data
            .map(|s| serde_json::from_str(&s).map_err(storage))
            .transpose()?;
        if row
            .as_ref()
            .is_some_and(|r| r.team != team || r.member != member)
        {
            return Err(Guard::LaunchKey.problem());
        }
        Ok(row)
    }
    pub fn rows_page(&self, team: &str, after: &str) -> Result<Vec<Row>> {
        let mut q = self
            .0
            .prepare(
                "SELECT data FROM originals WHERE team=?1 AND member>?2 ORDER BY member LIMIT 200",
            )
            .map_err(storage)?;
        let values = q
            .query_map([team, after], |r| r.get::<_, String>(0))
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        values
            .into_iter()
            .map(|s| serde_json::from_str(&s).map_err(storage))
            .collect()
    }
    pub fn prune(&mut self, store: &crate::teams::store::Store, after: &mut String) -> Result<()> {
        let teams = {
            let mut q = self
                .0
                .prepare(
                    "SELECT team FROM originals WHERE team>?1 UNION SELECT team FROM budget_alerts WHERE team>?1 ORDER BY team LIMIT 16",
                )
                .map_err(storage)?;
            let values = q
                .query_map([after.as_str()], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(storage)?;
            values
        };
        for team in &teams {
            let status = store.team(team)?.map(|t| t.status);
            if matches!(
                status,
                Some(
                    zerus_team_plan::model::TeamStatus::Stopped
                        | zerus_team_plan::model::TeamStatus::Archived
                )
            ) {
                let mut member_after = String::new();
                for _ in 0..2 {
                    let page = self.rows_page(team, &member_after)?;
                    member_after = page.last().map(|r| r.member.clone()).unwrap_or_default();
                    let more = page.len() == 200;
                    for mut row in page {
                        if !row.late_after_stop
                            && matches!(
                                row.state,
                                State::LaunchIntent | State::Submitted | State::LaunchUnknown
                            )
                        {
                            row.late_after_stop = true;
                            self.save(&mut row, chrono::Utc::now().timestamp_millis())?;
                        }
                    }
                    if !more {
                        break;
                    }
                }
            }
            if matches!(
                status,
                Some(
                    zerus_team_plan::model::TeamStatus::Stopped
                        | zerus_team_plan::model::TeamStatus::Archived
                )
            ) {
                self.0.execute("DELETE FROM originals WHERE rowid IN (SELECT rowid FROM originals WHERE team=?1 AND json_extract(data,'$.state') IN ('done','branch_done') LIMIT 202)",[team]).map_err(storage)?;
                self.0
                    .execute("DELETE FROM budget_alerts WHERE team=?1", [team])
                    .map_err(storage)?;
            }
        }
        *after = if teams.len() == 16 {
            teams.last().cloned().unwrap_or_default()
        } else {
            String::new()
        };
        Ok(())
    }
    pub fn insert(&mut self, row: &Row, now_ms: i64) -> Result<bool> {
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        let data = encoded(row)?;
        let inserted = tx
            .execute(
                &format!(
                    "INSERT OR IGNORE INTO {}(team,member,version,data) VALUES(?1,?2,0,?3)",
                    table(&row.member)
                ),
                params![row.team, row.member, data],
            )
            .map_err(ledger_error)?
            == 1;
        if inserted {
            journal(&tx, row, now_ms)?;
        }
        tx.commit().map_err(storage)?;
        Ok(inserted)
    }
    pub fn save(&mut self, row: &mut Row, now_ms: i64) -> Result<()> {
        let old = row.version;
        let mut next = row.clone();
        next.version = old
            .checked_add(1)
            .ok_or_else(|| Guard::LaunchRevision.problem())?;
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        let changed = tx
            .execute(
                &format!(
                    "UPDATE {} SET version=?3,data=?4 WHERE team=?1 AND member=?2 AND version=?5",
                    table(&row.member)
                ),
                params![row.team, row.member, next.version, encoded(&next)?, old],
            )
            .map_err(ledger_error)?;
        if changed != 1 {
            return Err(Guard::LaunchOriginal.problem());
        }
        journal(&tx, &next, now_ms)?;
        tx.commit().map_err(storage)?;
        *row = next;
        Ok(())
    }
}
fn table(member: &str) -> &'static str {
    if member == BUDGET {
        "budget_alerts"
    } else {
        "originals"
    }
}
fn ledger_error(error: rusqlite::Error) -> Problem {
    match &error {
        rusqlite::Error::SqliteFailure(_,Some(message)) if message == "member launch budget exceeded" => Problem::new(code::OVER_CAP,"Team member creation is temporarily at capacity. Zerus will collect completed launch records from stopped or archived teams before trying again.").with_refs(["member_launch_ledger"]),
        rusqlite::Error::SqliteFailure(_,Some(message)) if message == "launch notification budget exceeded" => Problem::new(code::OVER_CAP,"Team launch notifications are at capacity.").with_refs(["launch_notification_outbox"]),
        _=>storage(error),
    }
}
pub(super) fn is_budget(problem: &Problem) -> bool {
    problem.code == code::OVER_CAP
        && problem
            .refs
            .first()
            .is_some_and(|s| s == "member_launch_ledger")
}
pub(super) fn is_notice_budget(problem: &Problem) -> bool {
    problem.code == code::OVER_CAP
        && problem
            .refs
            .first()
            .is_some_and(|s| s == "launch_notification_outbox")
}
fn encoded(row: &Row) -> Result<String> {
    let data = serde_json::to_string(row).map_err(storage)?;
    if data.len() > 262_144 {
        return Err(Guard::LaunchRecord.problem());
    }
    Ok(data)
}
fn journal(tx: &rusqlite::Transaction<'_>, row: &Row, now_ms: i64) -> Result<()> {
    let actor = serde_json::json!({"kind":"service","person":row.person,"team":row.team});
    tx.execute(
        "INSERT INTO journal(team,member,version,at_ms,actor,state) VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            row.team,
            row.member,
            row.version,
            now_ms,
            actor.to_string(),
            serde_json::to_string(&row.state).map_err(storage)?
        ],
    )
    .map_err(storage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teams::ops::tests::{Harness, TEAM};
    #[cfg(unix)]
    #[test]
    fn state_aliases_open_the_ledgers_and_their_readers_but_database_aliases_are_refused() {
        use std::{fs, os::unix::fs::symlink};
        let h = Harness::new();
        let alias = h.temp.path().join("alias");
        symlink(h.temp.path(), &alias).unwrap();
        let mut ledger = Ledger::open(&alias).unwrap();
        ledger.write_health(Some(1), 7).unwrap();
        assert_eq!(super::super::health(&alias).unwrap().at_ms, 7);
        drop(ledger);

        let actor = super::super::actor::test_ledger(&alias);
        super::super::items::put(
            &actor,
            &super::super::items::Failure {
                team: TEAM,
                kind: "cancel_retry",
                subject: "7",
                node: Some(3),
                reason: "Retry",
                failures: 1,
                now: 7,
                next_ms: Some(60_007),
            },
        )
        .unwrap();
        assert_eq!(
            super::super::items::read(&alias, TEAM, Some(true), 7)
                .unwrap()
                .items
                .len(),
            1
        );
        drop(actor);

        for name in [FILE, "actor-v1.sqlite3"] {
            let path = h.temp.path().join("teams").join(name);
            let saved = path.with_extension("saved");
            fs::rename(&path, &saved).unwrap();
            symlink(&saved, &path).unwrap();
        }
        assert!(Ledger::open(&alias).is_err());
        assert!(super::super::health(&alias).is_none());
        assert!(super::super::items::read(&alias, TEAM, Some(true), 7).is_none());
    }
    #[test]
    fn a_stale_writer_is_refused_even_after_its_journal_was_pruned() {
        let h = Harness::new();
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut l = Ledger::open(h.temp.path()).unwrap();
        let mut row = Row::new(&t, "lead", "epoch", State::WorktreeReady);
        assert!(l.insert(&row, 1).unwrap());
        let mut stale = row.clone();
        row.state = State::WorktreeDone;
        l.save(&mut row, 2).unwrap();
        // Retention can remove the earlier version's journal uniqueness guard.
        l.0.execute("DELETE FROM journal", []).unwrap();
        stale.state = State::LaunchReady;
        assert!(l.save(&mut stale, 3).is_err());
        assert_eq!(l.get(TEAM, "lead").unwrap().unwrap(), row);
    }
    #[test]
    fn reserving_the_same_key_never_replaces_its_original() {
        let h = Harness::new();
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut l = Ledger::open(h.temp.path()).unwrap();
        let row = Row::new(&t, "lead", "epoch", State::WorktreeReady);
        assert!(l.insert(&row, 1).unwrap());
        let replacement = Row::new(&t, "lead", "another-epoch", State::LaunchReady);
        assert!(!l.insert(&replacement, 2).unwrap());
        assert_eq!(l.get(TEAM, "lead").unwrap().unwrap(), row);
    }
    #[test]
    fn health_is_a_fixed_singleton_even_when_both_outboxes_are_full() {
        let h = Harness::new();
        let mut ledger = Ledger::open(h.temp.path()).unwrap();
        ledger.0.execute_batch("UPDATE launch_budget SET rows=16384 WHERE id=1; UPDATE notice_budget SET rows=16384 WHERE id=1;").unwrap();
        ledger.write_health(Some(1), 7).unwrap();
        let health = crate::desktop::teams::health(h.temp.path()).unwrap();
        assert_eq!(health.code, "launch_notification_overflow");
        assert_eq!(health.at_ms, 7);
        assert!(ledger
            .0
            .execute("INSERT INTO launch_health VALUES(2,1,8)", [])
            .is_err());
        assert!(ledger.0.execute("DELETE FROM launch_health", []).is_err());
        assert!(ledger
            .0
            .execute("UPDATE launch_health SET code=3", [])
            .is_err());
        assert!(ledger
            .0
            .execute("UPDATE launch_health SET code='unbounded text'", [])
            .is_err());
        for at in 8..100 {
            ledger.write_health(Some(2), at).unwrap();
        }
        let count: u32 = ledger
            .0
            .query_row("SELECT COUNT(*) FROM launch_health", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            crate::desktop::teams::health(h.temp.path()).unwrap().at_ms,
            99
        );
        ledger.write_health(None, 0).unwrap();
        assert_eq!(crate::desktop::teams::health(h.temp.path()), None);
    }
    #[test]
    fn health_read_is_lazy_and_v3_migration_preserves_pending_work() {
        let h = Harness::new();
        assert_eq!(crate::desktop::teams::health(h.temp.path()), None);
        assert!(!h.temp.path().join("teams").join(FILE).exists());
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut ledger = Ledger::open(h.temp.path()).unwrap();
        let row = Row::new(&t, "lead", "epoch", State::WorktreeVerify);
        ledger.insert(&row, 1).unwrap();
        ledger
            .0
            .execute_batch("DROP TABLE launch_health; PRAGMA user_version=3;")
            .unwrap();
        assert_eq!(crate::desktop::teams::health(h.temp.path()), None);
        let version: u32 = ledger
            .0
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 3, "the health read must not migrate or write");
        drop(ledger);
        let ledger = Ledger::open(h.temp.path()).unwrap();
        assert_eq!(ledger.get(TEAM, "lead").unwrap(), Some(row));
        assert_eq!(crate::desktop::teams::health(h.temp.path()), None);
    }
    #[test]
    fn a_v2_ledger_adds_the_notification_outbox_without_changing_originals() {
        let h = Harness::new();
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut ledger = Ledger::open(h.temp.path()).unwrap();
        let saved = Row::new(&t, "lead", "epoch", State::WorktreeDone);
        ledger.insert(&saved, 1).unwrap();
        ledger
            .0
            .execute_batch(
                "DROP TABLE budget_alerts; DROP TABLE notice_budget; DROP TABLE launch_health; PRAGMA user_version=2;",
            )
            .unwrap();
        drop(ledger);
        let mut ledger = Ledger::open(h.temp.path()).unwrap();
        assert_eq!(ledger.get(TEAM, "lead").unwrap(), Some(saved));
        assert!(ledger
            .insert(&Row::new(&t, BUDGET, "epoch", State::Retry), 2)
            .unwrap());
        let version: u32 = ledger
            .0
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA);
    }
    #[test]
    fn budget_notifications_have_a_separate_bounded_outbox() {
        let h = Harness::new();
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut l = Ledger::open(h.temp.path()).unwrap();
        l.0.execute("UPDATE launch_budget SET rows=16384 WHERE id=1", [])
            .unwrap();
        let mut notice = Row::new(&t, BUDGET, "epoch", State::Retry);
        assert!(l.insert(&notice, 1).unwrap());
        l.0.execute("UPDATE notice_budget SET bytes=67108864 WHERE id=1", [])
            .unwrap();
        notice.problem = Some(Problem::new(code::OVER_CAP, "bounded notification"));
        assert!(l.save(&mut notice, 2).is_err());
        assert_eq!(l.get(TEAM, BUDGET).unwrap().unwrap().version, 0);
        let mut another = Row::new(&t, BUDGET, "epoch", State::Retry);
        another.team = crate::teams::ops::tests::TEAM_B.into();
        assert!(l.insert(&another, 3).is_err());
    }
    #[test]
    fn stopped_teams_release_settled_rows_but_preserve_pending_launches() {
        let mut h = Harness::new();
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut ledger = Ledger::open(h.temp.path()).unwrap();
        for (member, state) in [
            ("lead", State::Done),
            ("@branch", State::BranchDone),
            ("m1", State::Submitted),
        ] {
            ledger
                .insert(&Row::new(&t, member, "epoch", state), 1)
                .unwrap();
        }
        h.run(
            &crate::teams::ops::tests::person(),
            crate::teams::ops::Operation::Stop,
        )
        .unwrap();
        ledger.prune(&h.store, &mut String::new()).unwrap();
        assert!(ledger.get(TEAM, "lead").unwrap().is_none());
        assert!(ledger.get(TEAM, "@branch").unwrap().is_none());
        assert!(ledger.get(TEAM, "m1").unwrap().unwrap().late_after_stop);
    }
    #[test]
    fn ledger_budgets_refuse_growth_and_archived_settled_rows_are_collected() {
        let mut h = Harness::new();
        let t = h.store.team(TEAM).unwrap().unwrap();
        let mut l = Ledger::open(h.temp.path()).unwrap();
        let row = Row::new(&t, "lead", "epoch", State::Done);
        l.insert(&row, 1).unwrap();
        l.0.execute("UPDATE launch_budget SET bytes=67108864 WHERE id=1", [])
            .unwrap();
        assert!(l
            .insert(&Row::new(&t, "m1", "epoch", State::WorktreeReady), 2)
            .is_err());
        l.0.execute("UPDATE launch_budget SET bytes=length((SELECT data FROM originals LIMIT 1)) WHERE id=1",[]).unwrap();
        h.run(
            &crate::teams::ops::tests::person(),
            crate::teams::ops::Operation::Stop,
        )
        .unwrap();
        h.run(
            &crate::teams::ops::tests::person(),
            crate::teams::ops::Operation::Archive,
        )
        .unwrap();
        l.prune(&h.store, &mut String::new()).unwrap();
        assert!(l.get(TEAM, "lead").unwrap().is_none());
    }
}
