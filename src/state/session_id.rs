//! Durable session identity (zerus-ucck P1, names-as-labels spec): selecting a
//! live or saved session by its `session_id`, and reporting a name that
//! several sessions share.
use super::*;

/// Exit status for a name several sessions share (status 3 already means
/// "unreachable").
pub(crate) const AMBIGUOUS: i32 = 5;
/// At most this many candidates are listed, newest first.
const CANDIDATES: usize = 20;

/// What a command selected: the name it resolved, and every identity its
/// caller supplied, a `--session-id` or a session reference's session,
/// conversation or run (empty when not supplied). Resolution only finds the
/// name; it never authorises the command. Every later read of a record by that
/// name, live, archived or a DeepSeek or OpenCode binding, must carry every
/// supplied identity, so a name that another session took meanwhile is refused
/// under whatever lock the command holds when it reads its target (#1355).
#[derive(Clone)]
pub(crate) struct Selection {
    pub name: String,
    pub session_id: String,
    pub conversation: String,
    pub run: String,
    /// Whether the selection was verified, when made, as the session in the
    /// ID layout under `name` (#1437 review). Its fixed `sessions/<id>.json`
    /// then stays the command's route for that name, even if the file
    /// vanishes: a read then refuses instead of reaching the label's holder.
    pub id_layout: bool,
    /// How a refusal names the selector.
    label: String,
}
impl Selection {
    /// The refusal when a record under the name is no longer this session.
    pub(crate) fn changed(&self) -> String {
        format!("{} no longer names {}; refresh and retry", self.label, self.name)
    }
    fn agrees(&self, record: &Value) -> bool {
        [(&self.session_id, "session_id"), (&self.conversation, "conversation_id"), (&self.run, "run_id")]
            .iter()
            .all(|(wanted, key)| wanted.is_empty() || string(record, key) == wanted.as_str())
    }
}
static SELECTED: std::sync::Mutex<Option<Selection>> = std::sync::Mutex::new(None);

/// A label several sessions share, named by a command whose guard arrives
/// with its request (zerus-ucck P3c): its ambiguity report, held until the
/// command's guard selects one of them ([`super::expect`]).
struct Deferred {
    name: String,
    report: (String, Value),
}
static DEFERRED: std::sync::Mutex<Option<Deferred>> = std::sync::Mutex::new(None);
/// The ambiguity report a read refused with, for the command's exit status 5.
static REFUSED: std::sync::Mutex<Option<(String, Value)>> = std::sync::Mutex::new(None);

/// Defers the ambiguity of `name` to the command's guard.
pub(crate) fn defer(name: &str, text: String, value: Value) {
    *DEFERRED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
        Some(Deferred { name: name.into(), report: (text, value) });
}

/// Takes the deferred ambiguity of `name`, when there is one.
pub(super) fn take_deferred(name: &str) -> Option<(String, Value)> {
    let mut deferred = DEFERRED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if deferred.as_ref().is_some_and(|deferred| deferred.name == name) {
        return deferred.take().map(|deferred| deferred.report);
    }
    None
}

/// Records `report` as this command's refusal and returns its text.
pub(super) fn refuse(report: (String, Value)) -> String {
    let text = report.0.clone();
    *REFUSED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(report);
    text
}

/// The ambiguity report this command refused with, if any: the CLI exits
/// with [`AMBIGUOUS`] and prints its value under `--json`.
pub(crate) fn refused() -> Option<(String, Value)> {
    REFUSED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

fn set(selection: Selection) {
    *SELECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(selection);
}

/// Records a `--session-id` selection for the rest of this command.
pub(crate) fn select(name: &str, id: &str) {
    set(Selection {
        name: name.into(),
        session_id: id.into(),
        conversation: String::new(),
        run: String::new(),
        id_layout: in_id_layout_under(name, id),
        label: format!("Session ID {id}"),
    });
}

/// Records the session that the identities a command expects selected among
/// several sharing `name` (zerus-ucck P3c): its session ID, when it has one,
/// and both supplied identities, run and conversation, kept together. Every
/// later read must still carry each of them, on the selected session's fixed
/// route (#1454 review). Without a session ID, no session carried them: every
/// record under `name` is then refused as changed, and no reader reaches the
/// label's first holder or another candidate's route.
pub(crate) fn select_guarded(name: &str, id: &str, run: &str, conversation: &str) {
    select_guarded_route(name, id, run, conversation, in_id_layout_under(name, id));
}

/// Metadata-only index routing under the state/provider transaction. The index
/// grants nothing: the caller must verify the actual path, layout, ID, label,
/// run and conversation. A missing ID route never becomes the label holder.
pub(super) fn select_guarded_route(name: &str, id: &str, run: &str, conversation: &str, id_layout: bool) {
    set(Selection {
        name: name.into(),
        session_id: id.into(),
        conversation: conversation.into(),
        run: run.into(),
        id_layout,
        label: "The expected session".into(),
    });
}

/// Whether `id` is, right now, the session in the ID layout under `name`.
fn in_id_layout_under(name: &str, id: &str) -> bool {
    canonical(id) && labels::read_id_for_label(name, id).is_ok_and(|record| string(&record, "name") == name)
}

/// Records a session reference's selection for the rest of this command: its
/// session ID when the reference carries one, and its conversation, or its
/// run when it names no conversation (P1c).
pub(crate) fn select_reference(name: &str, session_id: &str, conversation: &str, run: &str) {
    set(Selection {
        name: name.into(),
        session_id: session_id.into(),
        conversation: conversation.into(),
        run: if conversation.is_empty() { run.into() } else { String::new() },
        id_layout: in_id_layout_under(name, session_id),
        label: "The session reference".into(),
    });
}

/// Forgets the selection (tests only: a command never drops its own).
#[cfg(all(test, unix))]
pub(crate) fn clear() {
    *SELECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

/// The command's selection, when it was given one.
pub(crate) fn selected() -> Option<Selection> {
    SELECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
}

/// The run of the session selected under `name`, read under the state lock and
/// checked against every supplied identity; None without a selection for
/// `name`. A terminal action binds its target to this run before its first
/// effect (#1355 review).
pub(crate) fn selected_run(name: &str) -> Result<Option<String>> {
    if selected().is_none_or(|selection| selection.name != name) {
        return Ok(None);
    }
    let _guard = lock(None)?;
    let record = read(name)?;
    Ok(Some(string(&record, "run_id").to_owned()))
}

/// Refuses a selected name that no longer has a record: a reader must not
/// fall back to whatever untracked terminal now holds the name.
pub(super) fn verify_absent(name: &str) -> Result<()> {
    match selected() {
        Some(selection) if selection.name == name => Err(selection.changed()),
        _ => Ok(()),
    }
}

/// Refuses a record that has the selected name but not every selected
/// identity, and, while the ambiguity of its label is deferred to the
/// command's guard and nothing selected one of its sessions, any record under
/// that label (P3c): every reader of a label refuses then, never settling on
/// one of the sessions that share it.
pub(super) fn verify(record: &Value) -> Result<()> {
    let name = string(record, "name");
    let deferred = DEFERRED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .filter(|deferred| deferred.name == name)
        .map(|deferred| deferred.report.clone());
    if let Some(report) = deferred {
        if selected().is_none_or(|selection| selection.name != name) {
            return Err(refuse(report));
        }
    }
    let selected = SELECTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match selected.as_ref() {
        Some(selection) if string(record, "name") == selection.name && !selection.agrees(record) => {
            Err(selection.changed())
        }
        _ => Ok(()),
    }
}

/// A session ID as hgs writes it: a lowercase hyphenated UUID.
pub(crate) fn canonical(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.hyphenated().to_string() == id)
}

/// The current names of the live or saved sessions whose durable ID is `id`:
/// one, or none. A verified hit of the ID index (P2a) is one read; a miss or
/// a stale entry scans the bindings once, as `hgs ls` reads them, and repairs
/// the entry. Duplicate IDs, which no writer of this hgs makes, are therefore
/// reported only by that scan.
pub(crate) fn names_of(id: &str) -> Result<Vec<String>> {
    if !canonical(id) {
        return Err("A session ID is a lowercase UUID; copy it from hgs ls --json".into());
    }
    if let Some(found) = session_index::find(id) {
        return Ok(vec![found.name]);
    }
    session_index::scanned();
    use session_index::Kind;
    let found: Vec<(String, Kind)> = records()?
        .into_iter()
        .map(|record| {
            let kind = if storage::in_id_layout(&record) { Kind::IdRecord } else { Kind::Record };
            (record, kind)
        })
        .chain(
            dsh::binding_records()
                .into_iter()
                .map(|record| { let kind = provider_binding::Provider::DeepSeek.kind(&record); (record, kind) }),
        )
        .chain(
            opencode::binding_records()
                .into_iter()
                .map(|record| { let kind = provider_binding::Provider::OpenCode.kind(&record); (record, kind) }),
        )
        .filter(|(record, _)| {
            string(record, "session_id") == id && string(record, "archive_id").is_empty()
        })
        .map(|(record, kind)| (string(&record, "name").to_owned(), kind))
        .collect();
    session_index::repair(id, &found);
    let mut names: Vec<String> = found.into_iter().map(|(name, _)| name).collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// The report for a name several sessions share: the text lines and the
/// `--json` value. Candidates carry `session_id`, `run_id`,
/// `conversation_id`, `state` (live or saved), `cwd`, `created` and an
/// optional `reference`, never transcript content. Label resolution
/// (`resolve_label`, zerus-ucck P3a) reports it when a label matches more than
/// one live or saved session; until P3e no writer makes that happen.
pub(crate) fn ambiguity(name: &str, mut candidates: Vec<Value>) -> (String, Value) {
    let total = candidates.len();
    candidates.sort_by(|a, b| {
        b["created"].as_f64().unwrap_or(0.0).total_cmp(&a["created"].as_f64().unwrap_or(0.0))
    });
    candidates.truncate(CANDIDATES);
    let candidates: Vec<Value> = candidates
        .into_iter()
        .map(|c| {
            json!({"session_id":c["session_id"],"run_id":c["run_id"],"conversation_id":c["conversation_id"],
                "state":c["state"],"cwd":c["cwd"],"created":c["created"],"reference":c["reference"]})
        })
        .collect();
    let home = home();
    let mut text = format!(
        "{total} sessions on this computer are named {name}. Choose one with --session-id, or use its reference:"
    );
    for c in &candidates {
        let id: String = string(c, "session_id").chars().take(8).collect();
        let cwd = Path::new(string(c, "cwd"));
        let folder = match cwd.strip_prefix(&home) {
            Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
            _ => cwd.display().to_string(),
        };
        let started = c["created"]
            .as_f64()
            .and_then(|at| chrono::DateTime::from_timestamp(at as i64, 0))
            .map(|at| at.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default();
        text.push_str(&format!("\n  {id:<8}  {:<6}  {folder}  started {started}", string(c, "state")));
        if !string(c, "reference").is_empty() {
            text.push_str(&format!("  {}", string(c, "reference")));
        }
    }
    (
        text,
        json!({"error":"ambiguous_session","name":name,"total":total,"candidates":candidates}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    /// #1355 review: every raw reader on a selector command refuses a name
    /// the selected ID left, whether another ID's record or no record holds
    /// it. The fixture runs in a child process with a private state directory.
    #[test]
    fn raw_readers_refuse_a_name_the_selected_id_left() {
        let state = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "state::session_id::tests::raw_readers_fixture",
                "--ignored",
                "--test-threads=1",
            ])
            .env("HGS_STATE_DIR", state.path())
            .env("ZERUS_RAW_READERS_FIXTURE", "1")
            .env_remove("HGS_SESSION")
            .env_remove("HGS_SESSION_ID")
            .env_remove("HGS_RUN_ID")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("test result: ok. 1 passed"),
            "{stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    #[ignore = "run by raw_readers_refuse_a_name_the_selected_id_left"]
    fn raw_readers_fixture() {
        if std::env::var_os("ZERUS_RAW_READERS_FIXTURE").is_none() {
            return;
        }
        std::fs::create_dir_all(root()).unwrap();
        let name = "claude/example/raw";
        let fresh = || uuid::Uuid::new_v4().to_string();
        let other = json!({"version":1,"agent":"claude","name":name,"run_id":fresh(),
            "conversation_id":fresh(),"session_id":fresh(),"created":1});
        atomic(&record_path(name), &other.to_string()).unwrap();
        select(name, &fresh());
        let refused = |error: String| assert!(error.contains("no longer names"), "{error}");
        refused(super::super::notifications::current(name).unwrap_err());
        let input = json!({"request_id":fresh(),"expected_run_id":other["run_id"],
            "expected_conversation_id":other["conversation_id"]});
        refused(super::super::hook_health::session_report(name, &input).unwrap_err());
        // A session reference selects by its conversation: a record under the
        // name with another conversation is refused the same way (P1c).
        select_reference(name, "", &fresh(), &fresh());
        let refused_reference = |error: String| {
            assert!(error.starts_with("The session reference no longer names"), "{error}")
        };
        refused_reference(super::super::notifications::current(name).unwrap_err());
        refused_reference(super::super::hook_health::session_report(name, &input).unwrap_err());
        refused_reference(read(name).unwrap_err());
        // A reference without a conversation selects by its run (#1376 MX6).
        select_reference(name, "", "", &fresh());
        refused_reference(read(name).unwrap_err());
        refused_reference(super::super::notifications::current(name).unwrap_err());
        std::fs::remove_file(record_path(name)).unwrap();
        refused_reference(super::super::journal::inspection(name, 0, None, false).unwrap_err());
        select(name, &fresh());
        refused(super::super::journal::inspection(name, 0, None, false).unwrap_err());
    }
    #[test]
    fn only_a_lowercase_hyphenated_uuid_is_a_session_id() {
        assert!(canonical("3f2a9c1d-0b4e-4d6a-9c1d-2e3f4a5b6c7d"));
        for value in [
            "",
            "3F2A9C1D-0B4E-4D6A-9C1D-2E3F4A5B6C7D",
            "3f2a9c1d0b4e4d6a9c1d2e3f4a5b6c7d",
            "{3f2a9c1d-0b4e-4d6a-9c1d-2e3f4a5b6c7d}",
            "codex/project/tag",
        ] {
            assert!(!canonical(value), "{value}");
        }
    }
    #[test]
    fn an_ambiguous_name_lists_bounded_candidates_newest_first_without_content() {
        let candidate = |n: usize| {
            json!({"session_id":format!("{n:08x}-0000-4000-8000-000000000000"),"run_id":format!("run-{n}"),
                "conversation_id":format!("conversation-{n}"),"state":if n.is_multiple_of(2) {"live"} else {"saved"},
                "cwd":"/srv/example/project","created":1_800_000_000 + n as i64,"detail":"private transcript"})
        };
        let (text, value) = ambiguity("codex/project/tag", (0..25).map(candidate).collect());
        assert_eq!(value["error"], "ambiguous_session");
        assert_eq!((value["name"].as_str(), value["total"].as_u64()), (Some("codex/project/tag"), Some(25)));
        let listed = value["candidates"].as_array().unwrap();
        assert_eq!(listed.len(), 20);
        assert_eq!(listed[0]["run_id"], "run-24", "newest first");
        assert_eq!(listed[19]["run_id"], "run-5");
        assert!(!value.to_string().contains("private transcript"));
        assert!(text.starts_with("25 sessions on this computer are named codex/project/tag. Choose one with --session-id"));
        assert_eq!(text.lines().count(), 21);
        assert!(text.lines().nth(1).unwrap().starts_with("  00000018  live    /srv/example/project  started "));
    }
}
