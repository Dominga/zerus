//! Durable session identity (zerus-ucck P1, names-as-labels spec): selecting a
//! live or saved session by its `session_id`, and reporting a name that
//! several sessions share.
use super::*;

/// Exit status for a name several sessions share (status 3 already means
/// "unreachable").
pub(crate) const AMBIGUOUS: i32 = 5;
/// At most this many candidates are listed, newest first.
const CANDIDATES: usize = 20;

/// A session ID as hgs writes it: a lowercase hyphenated UUID.
pub(crate) fn canonical(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.hyphenated().to_string() == id)
}

/// The current names of the live or saved sessions whose durable ID is `id`:
/// one, or none. One scan of the bindings, as `hgs ls` reads them, for one
/// explicit CLI call; P2's `sessions/<id>` layout makes it a lookup. Never put
/// it on a polling, status or watch path before that.
pub(crate) fn names_of(id: &str) -> Result<Vec<String>> {
    if !canonical(id) {
        return Err("A session ID is a lowercase UUID; copy it from hgs ls --json".into());
    }
    let mut names: Vec<String> = records()?
        .into_iter()
        .chain(dsh::binding_records())
        .chain(opencode::binding_records())
        .filter(|record| string(record, "session_id") == id && string(record, "archive_id").is_empty())
        .map(|record| string(&record, "name").to_owned())
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// The report for a name several sessions share: the text lines and the
/// `--json` value. Candidates carry `session_id`, `run_id`,
/// `conversation_id`, `state` (live or saved), `cwd`, `created` and an
/// optional `reference`, never transcript content. P2's label index calls it
/// when a name matches more than one session; until P3 names stay unique.
#[cfg_attr(not(test), allow(dead_code))]
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
