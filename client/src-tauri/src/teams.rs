//! The Teams views' reads and actions (M1): one Teams poller of this computer
//! feeding the pump, the intent table of the actions, and the guarded
//! `hgs team <op>` calls of the Team tab.
//!
//! The webview never runs or parses an `hgs team` read: it follows the views it
//! shows (`teams_follow`), tells the core when the tab is hidden
//! (`teams_hidden`) and when the followed project's tasks topic changed
//! (`teams_tasks_changed`), asks for a later page (`teams_page`), and takes the
//! views from the per-frame batches (`batch.teams`, keyed `list`, `waiting` and
//! `team:<id>`). One team is followed at a time; following another ends the
//! first. One reader serves the life of the application, so two readers never
//! run beside each other.
//!
//! An action carries the request id the person's action made once
//! ([`TeamCall`]). The intent table remembers each id: a second call while it
//! is pending sends nothing, a settled intent returns its recorded outcome,
//! and an uncertain owner-card decision is checked by its original receipt
//! only. Other interim actions retain their explicit `team_action_again`.
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::Value;
use tauri::State;
use zerus_client_core::actions::{Exit, Invocation};
use zerus_client_core::run_limited;
use zerus_client_core::teams::action::{Admission, Intents, Pending, TeamCall, TeamOutcome};
use zerus_client_core::teams::read::valid_team;
use zerus_client_core::teams::{TeamsFollow, TeamsPoller};

use crate::commands::AppState;

pub mod observed;
mod settings;

/// The one Teams reader of this computer and the intents whose actions it
/// sent.
#[derive(Default)]
pub struct Teams {
    poller: Mutex<Option<TeamsPoller>>,
    intents: Mutex<Intents>,
    decisions: std::sync::Arc<crate::desktop_service::team_person::Decisions>,
    actions: std::sync::Arc<crate::desktop_service::team_person_actions::Actions>,
}

impl Teams {
    pub fn new() -> Self {
        Self::default()
    }
    pub(crate) fn native_actions(
        &self,
    ) -> std::sync::Arc<crate::desktop_service::team_person_actions::Actions> {
        self.actions.clone()
    }

    fn poller(&self) -> MutexGuard<'_, Option<TeamsPoller>> {
        self.poller.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn intents(&self) -> MutexGuard<'_, Intents> {
        self.intents.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads every followed view again after an action of our own.
    fn refresh(&self) {
        if let Some(poller) = self.poller().as_ref() {
            poller.refresh();
        }
    }

    fn follow(
        &self,
        hgs: std::path::PathBuf,
        pump: crate::pump::Pump,
        follow: TeamsFollow,
        refresh: bool,
    ) -> Result<(), String> {
        let mut poller = self.poller();
        if poller.is_none() {
            let reader = TeamsPoller::spawn(hgs, move |change| pump.push(change))
                .map_err(|error| format!("The Teams reader did not start: {error}"))?;
            *poller = Some(reader);
        }
        if let Some(poller) = poller.as_ref() {
            poller.follow(follow);
            if refresh {
                poller.recheck();
            }
        }
        Ok(())
    }
}

/// Follows the Teams views this window shows: the team list, what waits for
/// the person and at most one team. Every flag false and no team ends the
/// views but keeps the one reader.
#[tauri::command(async)]
pub fn teams_follow(
    state: State<'_, AppState>,
    teams: State<'_, Teams>,
    list: bool,
    waiting: bool,
    team: Option<String>,
    refresh: Option<bool>,
) -> Result<(), String> {
    let team = team.filter(|team| !team.is_empty());
    if team.as_deref().is_some_and(|team| !valid_team(team)) {
        return Err("Not a team id.".into());
    }
    let follow = TeamsFollow {
        list,
        waiting,
        team,
    };
    teams.follow(
        zerus_client_core::default_hgs(),
        state.pump(),
        follow,
        refresh.unwrap_or(false),
    )
}

/// The Team tab was hidden (`true`) or shown again. A hidden tab reads nothing;
/// shown again, every followed view is read at once.
#[tauri::command]
pub fn teams_hidden(teams: State<'_, Teams>, hidden: bool) -> Result<(), String> {
    if let Some(poller) = teams.poller().as_ref() {
        poller.hidden(hidden);
    }
    Ok(())
}

/// The followed project's tasks topic changed: the watched team's status and
/// queue are read again (a report moves a node's state).
#[tauri::command]
pub fn teams_tasks_changed(teams: State<'_, Teams>) -> Result<(), String> {
    if let Some(poller) = teams.poller().as_ref() {
        poller.tasks_changed();
    }
    Ok(())
}

/// Reads one page of a followed view by its `next` cursor; the page replaces
/// the shown one and stays selected for every later read. No cursor selects
/// the first page again. The key is `list`, `waiting` or `team:<id>`.
#[tauri::command]
pub fn teams_page(
    teams: State<'_, Teams>,
    key: String,
    cursor: Option<String>,
) -> Result<(), String> {
    let poller = teams.poller();
    let Some(poller) = poller.as_ref() else {
        return Err("No Teams view is followed.".into());
    };
    if poller.page(&key, cursor.as_deref()) {
        Ok(())
    } else {
        Err("No such page for this view.".into())
    }
}

/// Selects the optional phase pane on the existing watched-team reader.
/// Its epoch lets the window refuse a late pane from an earlier selection.
#[tauri::command]
pub fn teams_phases(
    teams: State<'_, Teams>,
    team: String,
    phase: Option<u32>,
    enabled: bool,
    expected_epoch: Option<u64>,
) -> Result<u64, String> {
    if !valid_team(&team) {
        return Err("Not a team id.".into());
    }
    teams
        .poller()
        .as_ref()
        .and_then(|poller| poller.phases(&team, phase, enabled, expected_epoch))
        .ok_or_else(|| "This team is not followed.".into())
}

/// Runs one action off the main thread with its original request ID. Owner
/// cards use native confirmation and admission; interim actions use the CLI.
/// The followed view is read again after it.
#[tauri::command]
pub async fn team_action(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    teams: State<'_, Teams>,
    call: Value,
) -> Result<TeamOutcome, String> {
    let call = match settings::checked_call(call) {
        Ok(call) => call,
        Err(outcome) => return Ok(outcome),
    };
    let admission = teams.intents().admit(&call);
    run(app, state.desktop_service(), teams, admission, call, false).await
}

/// Checks an uncertain owner-card decision read-only. Other interim actions
/// keep their existing same-request CLI recovery path.
#[tauri::command]
pub async fn team_action_again(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    teams: State<'_, Teams>,
    call: Value,
) -> Result<TeamOutcome, String> {
    let call = match settings::checked_call(call) {
        Ok(call) => call,
        Err(outcome) => return Ok(outcome),
    };
    let admission = teams.intents().admit_again(&call);
    run(app, state.desktop_service(), teams, admission, call, true).await
}

async fn run(
    app: tauri::AppHandle,
    service: Option<std::sync::Arc<crate::desktop_service::Service>>,
    teams: State<'_, Teams>,
    admission: Admission,
    call: TeamCall,
    check_only: bool,
) -> Result<TeamOutcome, String> {
    match admission {
        Admission::InFlight => Ok(TeamOutcome::Pending {
            request_id: call.request_id.clone(),
        }),
        Admission::Recorded(outcome) | Admission::Refused(outcome) => Ok(outcome),
        Admission::Send => {
            // The guard settles this intent on every way out, including a
            // panicking or failed worker: an unknown outcome, never a stray
            // Pending entry.
            let pending = Pending::new(&teams.intents, &call);
            let sent = call.clone();
            let actions = teams.actions.clone();
            let decisions = teams.decisions.clone();
            let outcome = match tauri::async_runtime::spawn_blocking(move || {
                if matches!(
                    sent.action,
                    zerus_client_core::teams::action::TeamAction::Decide { .. }
                ) {
                    crate::desktop_service::team_person::run(
                        app, service, decisions, sent, check_only,
                    )
                } else {
                    crate::desktop_service::team_person_actions::run(
                        app, service, actions, sent, check_only,
                    )
                }
            })
            .await
            {
                Ok(outcome) => outcome,
                Err(error) => TeamOutcome::Uncertain {
                    detail: format!(
                        "The action's worker failed: {error}. Read the team before checking it."
                    ),
                    request_id: call.request_id.clone(),
                },
            };
            let recorded = pending.settle(outcome);
            teams.refresh();
            Ok(recorded)
        }
    }
}

/// The person dismisses an uncertain intent: it settles as abandoned and
/// nothing is sent. Only an uncertain intent can be dismissed.
#[tauri::command]
pub fn team_intent_dismiss(teams: State<'_, Teams>, call: TeamCall) -> Result<(), String> {
    if teams.intents().dismiss(&call) {
        teams.decisions.dismiss(&call.request_id);
        teams.actions.dismiss(&call.request_id);
        Ok(())
    } else {
        Err("Only an uncertain action can be dismissed.".into())
    }
}

/// The person's Teams catalog: `hgs team catalog show --json` answers
/// `{"ok": true, "catalog": {...}}` (roles, pipelines, templates and the
/// lead's planning rules). The reply is read into the command line's own
/// catalog type, so a malformed entry refuses the catalog before the form
/// dereferences it, and every field the form reads is present.
#[tauri::command]
pub async fn teams_catalog() -> Result<Value, String> {
    let reply = read_team_json(&["team", "catalog", "show", "--json"], None).await?;
    catalog_shape(&reply)
}

/// Saves one template to the person's own catalog: `hgs team template save`
/// reads the template on standard input. The request is bounded before it is
/// serialized into a child's stdin.
#[tauri::command]
pub async fn teams_template_save(template: Value) -> Result<Value, String> {
    let size = serde_json::to_vec(&template)
        .map_err(|error| error.to_string())?
        .len();
    if size > TEMPLATE_INPUT_MAX {
        return Err(format!(
            "The template is {size} bytes; the limit is {TEMPLATE_INPUT_MAX}."
        ));
    }
    let sent = template
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or("The template has no id. Nothing was saved.")?
        .to_owned();
    let reply = read_team_json(&["team", "template", "save", "--json"], Some(template)).await?;
    zerus_client_core::teams::catalog::saved_receipt(&reply, &sent)
}

/// One on-demand, bounded read of this team's current base and local main refs.
#[tauri::command]
pub async fn teams_base(team: String) -> Result<Value, String> {
    if !valid_team(&team) {
        return Err("Not a team id.".into());
    }
    let reply = read_team_json(&["team", "base", "--team", &team, "--json"], None).await?;
    base_shape(reply, &team)
}

fn base_shape(reply: Value, team: &str) -> Result<Value, String> {
    if reply.get("ok").and_then(Value::as_bool) != Some(true)
        || reply.get("team").and_then(Value::as_str) != Some(team)
        || reply.get("revision").and_then(Value::as_u64).is_none()
        || reply.get("base_commit").and_then(Value::as_str).is_none()
        || !reply
            .get("refs")
            .and_then(Value::as_array)
            .is_some_and(|refs| {
                !refs.is_empty()
                    && refs.len() <= 2
                    && refs.iter().all(|r| {
                        r.get("commit").and_then(Value::as_str).is_some()
                            && matches!(
                                r.get("reference").and_then(Value::as_str),
                                Some("refs/heads/main" | "refs/remotes/origin/main")
                            )
                    })
            })
    {
        return Err("The team base answer is invalid.".into());
    }
    Ok(reply)
}

/// A project folder's existing checkout facts through the bounded CLI read.
#[tauri::command]
pub async fn teams_project(path: String) -> Result<Value, String> {
    if path.len() > PATH_INPUT_MAX {
        return Err(format!(
            "The folder path is {} bytes; the limit is {PATH_INPUT_MAX}.",
            path.len()
        ));
    }
    let reply = read_team_json(&["git-status", "--path", &path, "--json"], None).await?;
    if !reply.is_object() {
        return Err("The folder check's answer is not an object.".into());
    }
    for key in ["state", "head", "branch"] {
        if let Some(value) = reply.get(key) {
            if !value.is_string() {
                return Err(format!("The folder check's {key} is not a string."));
            }
        }
    }
    Ok(reply)
}

/// Turns the person's Teams preview on or off: `hgs team preview on|off` with
/// a fixed word, no renderer-supplied text. The answer must name the state that
/// was asked for.
#[tauri::command]
pub async fn teams_preview(on: bool) -> Result<Value, String> {
    let reply = read_team_json(preview_args(on), None).await?;
    preview_shape(&reply, on)
}

/// The fixed words of the preview command, never a renderer's.
fn preview_args(on: bool) -> &'static [&'static str] {
    if on {
        &["team", "preview", "on"]
    } else {
        &["team", "preview", "off"]
    }
}

/// `{ok:true, preview:"on"|"off"}`; the state must be the one asked for.
fn preview_shape(reply: &Value, on: bool) -> Result<Value, String> {
    let wanted = if on { "on" } else { "off" };
    if reply.get("ok").and_then(Value::as_bool) != Some(true)
        || reply.get("preview").and_then(Value::as_str) != Some(wanted)
    {
        return Err(format!("The preview answer did not name {wanted}."));
    }
    Ok(reply.clone())
}

/// The catalog's own shape: the command line's catalog type, given back with
/// every field the form reads.
fn catalog_shape(reply: &Value) -> Result<Value, String> {
    zerus_client_core::teams::catalog::form_catalog(reply)
}

/// The CLI's own request bound for one write (262,144 bytes).
const TEMPLATE_INPUT_MAX: usize = 262_144;
/// The longest folder path accepted before any child runs (4 KiB).
const PATH_INPUT_MAX: usize = 4_096;

/// One person-only `hgs` read through the client's bounded runner. These
/// commands start nothing: a refusal keeps its code and words.
async fn read_team_json(args: &[&str], stdin: Option<Value>) -> Result<Value, String> {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let stdin = stdin.map(|value| value.to_string().into_bytes());
    let invocation = Invocation {
        args,
        stdin,
        timeout: std::time::Duration::from_secs(15),
        request_id: None,
    };
    let hgs = zerus_client_core::default_hgs();
    let exit = tauri::async_runtime::spawn_blocking(move || {
        run_limited(&hgs, &invocation, &[], zerus_client_core::reads::READ_LIMIT)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|too_large| format!("hgs answer passed {} bytes", too_large.cap))?;
    let (code, stdout, stderr) = match exit {
        Exit::Code {
            code,
            stdout,
            stderr,
        } => (code, stdout, stderr),
        Exit::Abnormal { stderr } => (1, Vec::new(), stderr),
        Exit::TimedOut => return Err("hgs did not answer in time".into()),
        Exit::StartFailed(message) => return Err(format!("hgs did not start: {message}")),
        Exit::WriteFailed => return Err("hgs did not take the request".into()),
    };
    decode_team_reply(code, &stdout, stderr.as_bytes())
}

/// Decode one helper's answer: a structured refusal on stdout is read even on a
/// nonzero exit, so its reason reaches the person; otherwise a nonzero exit
/// falls back to stderr, and a success is the JSON object it answered.
fn decode_team_reply(code: i32, stdout: &[u8], stderr: &[u8]) -> Result<Value, String> {
    if let Ok(reply) = serde_json::from_slice::<Value>(stdout) {
        if reply.get("ok").and_then(Value::as_bool) == Some(false) {
            let refusal = reply["error"]["code"].as_str().unwrap_or_default();
            let message = reply["error"]["message"].as_str().unwrap_or_default();
            return Err(if message.is_empty() {
                format!("hgs refused: {refusal}")
            } else {
                format!("{refusal}: {message}")
            });
        }
    }
    if code != 0 {
        let text = String::from_utf8_lossy(stderr);
        let text = text.trim();
        return Err(if text.is_empty() {
            "hgs refused the read".into()
        } else {
            text.to_owned()
        });
    }
    serde_json::from_slice(stdout).map_err(|error| error.to_string())
}

// Runs a source-built command line through a Unix executable fixture.
#[cfg(all(test, unix))]
mod real_create;
#[cfg(all(test, target_os = "linux"))]
mod real_preview;
#[cfg(all(test, unix))]
mod real_reads;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Handwritten wire-shape controls; operation rows are tested in hgs.
    #[test]
    fn handwritten_base_reply_is_bounded_and_scoped_to_the_selected_team() {
        let team = "00000000-0000-4000-8000-000000000165";
        let commit = "0123456789abcdef0123456789abcdef01234567";
        let reply = json!({"ok":true,"team":team,"revision":7,"base_commit":commit,
            "refs":[{"reference":"refs/heads/main","commit":commit}]});
        assert!(base_shape(reply.clone(), team).is_ok());
        assert!(base_shape(reply.clone(), "other").is_err());
        for refs in [
            json!([]),
            json!([{}, {}, {}]),
            json!([{"reference":"HEAD","commit":commit}]),
        ] {
            let mut invalid = reply.clone();
            invalid["refs"] = refs;
            assert!(base_shape(invalid, team).is_err());
        }
    }

    #[test]
    fn a_structured_refusal_is_decoded_on_a_nonzero_exit() {
        let refusal = json!({"ok": false, "error": {"code": "permission_denied", "message": "Only the person may save a template."}});
        let error = decode_team_reply(1, refusal.to_string().as_bytes(), b"").unwrap_err();
        assert!(
            error.contains("Only the person may save a template."),
            "{error}"
        );
        assert!(error.contains("permission_denied"), "{error}");
    }

    /// The store's codes on a person-only read or save (the catalog, a template
    /// save): a busy store and a storage failure keep their code and the store's
    /// sentence; a problem's detail is not part of it.
    #[test]
    fn a_busy_store_and_a_problems_detail_on_a_read() {
        let busy: Value = serde_json::from_str(include_str!(
            "../../../crates/team-plan/fixtures/problem-store-busy.json"
        ))
        .unwrap();
        let mut error = busy.clone();
        error["detail"] = json!("lock held by another writer");
        let reply = json!({"ok": false, "error": error});
        assert_eq!(
            decode_team_reply(1, reply.to_string().as_bytes(), b"").unwrap_err(),
            "store_busy: The team store is busy. Nothing was written; try again."
        );
        let storage = include_str!("../../../crates/team-plan/fixtures/cli-storage-failed.json");
        assert_eq!(
            decode_team_reply(1, storage.as_bytes(), b"").unwrap_err(),
            "storage_failed: The result is not known; repeat the same request with the same request ID."
        );
    }

    #[test]
    fn a_nonzero_exit_without_a_reply_keeps_stderr_or_a_generic_reason() {
        assert_eq!(
            decode_team_reply(1, b"", b"hgs: the catalog is unreadable\n").unwrap_err(),
            "hgs: the catalog is unreadable"
        );
        assert_eq!(
            decode_team_reply(1, b"", b"").unwrap_err(),
            "hgs refused the read"
        );
    }

    #[test]
    fn a_malformed_success_is_refused_by_the_catalog_shape() {
        assert!(catalog_shape(&json!({})).is_err());
        assert!(catalog_shape(&json!({"ok": true})).is_err());
        assert!(catalog_shape(&json!({"ok": true, "catalog": {"roles": [], "pipelines": [], "templates": [{"name": "No id"}]}})).is_err());
        // Second review, finding 7: an entry the form dereferences must be whole.
        let broken = json!({"ok": true, "catalog": {"roles": [], "pipelines": [], "templates": [{"id": "broken", "name": "Broken"}]}});
        assert!(
            catalog_shape(&broken).is_err(),
            "a template without roster or settings"
        );
        assert!(
            catalog_shape(&json!({"ok": true, "catalog": {"roles": [{"id": "r", "name": "R"}]}}))
                .is_err(),
            "a role without an agent"
        );
        let builtin: Value = serde_json::from_str(include_str!(
            "../../../crates/team-plan/fixtures/catalog-builtin.json"
        ))
        .unwrap();
        let shaped = catalog_shape(&json!({"ok": true, "catalog": builtin})).unwrap();
        let template = &shaped["catalog"]["templates"][0];
        assert!(template["roster"].is_array() && template["settings"]["integration"].is_string());
        // An empty catalog is three empty arrays, never a missing key.
        assert_eq!(
            catalog_shape(&json!({"ok": true, "catalog": {}})).unwrap()["catalog"]["templates"],
            json!([])
        );
    }

    #[test]
    fn a_folder_check_object_needs_no_ok_field() {
        assert!(decode_team_reply(
            0,
            br#"{"state":"ok","head":"0123456789abcdef0123456789abcdef01234567","branch":"main"}"#,
            b""
        )
        .is_ok());
    }

    #[test]
    fn the_preview_command_takes_only_its_own_fixed_words() {
        assert_eq!(preview_args(true), ["team", "preview", "on"]);
        assert_eq!(preview_args(false), ["team", "preview", "off"]);
    }

    #[test]
    fn the_preview_answer_must_name_the_state_that_was_asked_for() {
        assert!(preview_shape(&json!({"ok": true, "preview": "on"}), true).is_ok());
        assert!(preview_shape(&json!({"ok": true, "preview": "off"}), false).is_ok());
        assert!(preview_shape(&json!({"ok": true, "preview": "off"}), true).is_err());
        assert!(preview_shape(&json!({"ok": true}), false).is_err());
        assert!(preview_shape(&json!({}), true).is_err());
    }

    #[test]
    fn the_input_bounds_refuse_before_a_child_runs() {
        const { assert!(TEMPLATE_INPUT_MAX == 262_144 && PATH_INPUT_MAX == 4_096) };
        assert!(
            serde_json::to_vec(&json!({"x": "a".repeat(TEMPLATE_INPUT_MAX)}))
                .unwrap()
                .len()
                > TEMPLATE_INPUT_MAX
        );
    }
}
