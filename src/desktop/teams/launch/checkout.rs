//! Immutable directory-object provenance captured by the launch's worktree
//! verification. A missing observation grants no standing confirmation.
use super::*;
use crate::teams::store::begin_error;
use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt, time::UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::desktop::teams) struct Checkout {
    team: String,
    member: String,
    launch: String,
    path: String,
    common: String,
    nonce: String,
    device: u64,
    inode: u64,
    birth: Option<(i64, u32)>,
}
impl Checkout {
    fn capture(plan: &Plan, tree: &Worktree) -> Option<Self> {
        let (device, inode, birth) = object(Path::new(&tree.path))?;
        Some(Self {
            team: plan.team.clone(),
            member: plan.member.clone(),
            launch: plan.launch_id.clone(),
            path: tree.path.clone(),
            common: plan.placement.git_dir.clone(),
            nonce: tree.ownership_nonce.clone(),
            device,
            inode,
            birth,
        })
    }
    pub fn matches(&self, path: &str, common: &str, nonce: &str) -> bool {
        self.path == path
            && self.common == common
            && self.nonce == nonce
            && object(Path::new(path)) == Some((self.device, self.inode, self.birth))
    }
}
type Object = (u64, u64, Option<(i64, u32)>);
fn object(path: &Path) -> Option<Object> {
    if crate::platform::canonical(path).ok()?.as_path() != path {
        return None;
    }
    let file = crate::platform::no_follow(
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK),
    )
    .open(path)
    .ok()?;
    let (id, meta) = crate::platform::FileIdentity::with_metadata(&file).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let (current, _) = crate::platform::FileIdentity::at(path).ok()?;
    if (id.device, id.inode) != (current.device, current.inode) {
        return None;
    }
    let birth = meta
        .created()
        .ok()
        .and_then(|at| match at.duration_since(UNIX_EPOCH) {
            Ok(d) => i64::try_from(d.as_secs())
                .ok()
                .map(|seconds| (seconds, d.subsec_nanos())),
            Err(e) => {
                let d = e.duration();
                let seconds = i64::try_from(d.as_secs()).ok()?.checked_neg()?;
                if d.subsec_nanos() == 0 {
                    Some((seconds, 0))
                } else {
                    Some((seconds.checked_sub(1)?, 1_000_000_000 - d.subsec_nanos()))
                }
            }
        });
    Some((id.device, id.inode, birth))
}

/// One store statement of the checkout record, named for the test-only
/// injected-error seam below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Statement {
    Insert,
    Commit,
    Select,
}

/// Runs one checkout statement. Production always runs it; the test-only seam
/// may answer with an injected busy error instead.
#[cfg(not(test))]
#[inline]
fn statement<T>(_: Statement, run: impl FnOnce() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    run()
}

#[cfg(test)]
#[inline]
fn statement<T>(name: Statement, run: impl FnOnce() -> rusqlite::Result<T>) -> rusqlite::Result<T> {
    match injection::next(name) {
        Some(error) => Err(error),
        None => run(),
    }
}

pub(super) fn record(db: &mut Connection, plan: &Plan, tree: &Worktree) -> Result<bool> {
    // Absence is preserved, never backfilled by the confirmation reader.
    let Some(proof) = Checkout::capture(plan, tree) else {
        return Ok(false);
    };
    let raw = serde_json::to_string(&proof).map_err(storage)?;
    if raw.len() > 16384 {
        return Err(storage("Checkout proof exceeds its bound"));
    }
    let tx = db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(begin_error)?;
    let inserted = statement(Statement::Insert, || {
        tx.execute(
            "INSERT OR IGNORE INTO launch_checkout_objects VALUES(?1,?2,?3,?4)",
            params![plan.team, plan.member, plan.launch_id, raw],
        )
    })
    .map_err(begin_error)?;
    let prior = lookup(&tx, &plan.team, &plan.member, &plan.launch_id)?;
    if prior.as_ref() != Some(&proof) {
        return Err(storage("The launch checkout object was replaced"));
    }
    statement(Statement::Commit, move || tx.commit()).map_err(begin_error)?;
    Ok(inserted == 1)
}
pub(super) fn lookup(
    db: &Connection,
    team: &str,
    member: &str,
    launch: &str,
) -> Result<Option<Checkout>> {
    let raw: Option<Option<String>> = statement(Statement::Select, || {
        db.query_row("SELECT CASE WHEN length(CAST(data AS BLOB))<=16384 THEN data END FROM launch_checkout_objects WHERE team=?1 AND member=?2 AND launch=?3",params![team,member,launch],|r|r.get(0)).optional()
    })
    .map_err(begin_error)?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let proof: Checkout =
        serde_json::from_str(&raw.ok_or_else(|| storage("Checkout proof exceeds its bound"))?)
            .map_err(storage)?;
    if proof.team != team || proof.member != member || proof.launch != launch {
        return Err(storage("Checkout proof identity changed"));
    }
    Ok(Some(proof))
}

/// Test-only injected-error seam for the checkout statements. It fails the
/// named statement's own result with SQLITE_BUSY before the statement runs; it
/// never holds or simulates a real concurrent SQLite lock, so it cannot stand
/// in for the BEGIN contention test.
#[cfg(test)]
pub(crate) mod injection {
    use super::Statement;
    use std::cell::Cell;
    thread_local! {
        static NEXT: Cell<Option<Statement>> = const { Cell::new(None) };
    }
    pub(crate) fn fail(statement: Statement) {
        NEXT.with(|next| next.set(Some(statement)));
    }
    pub(crate) fn next(statement: Statement) -> Option<rusqlite::Error> {
        NEXT.with(|next| {
            if next.get() != Some(statement) {
                return None;
            }
            next.set(None);
            Some(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                None,
            ))
        })
    }
}
