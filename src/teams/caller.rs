//! Interim caller proof shared by Teams and task intake. Both callers pass one
//! namespace admission first; then a marked caller must descend from its live
//! saved agent, and an unmarked caller is the person only after a complete,
//! bounded inspection finds no verified marked agent.
//!
//! The admission opens `/proc` and resolves `self/ns/user` from that
//! descriptor with `openat2` under `RESOLVE_NO_XDEV`: the lookup crosses no
//! mount and does not follow the last link, so a bind mount over the
//! process's own `ns` directory cannot answer for the real namespace. Any
//! failure refuses, including a kernel or sandbox without `openat2`; there is
//! no path fallback. A command in a sandbox with its own user namespace is
//! therefore refused for both marked and unmarked callers, so a lead or a
//! member whose shell runs in such a sandbox cannot use `hgs team`; those
//! team commands must run outside it. macOS has no namespaces and therefore
//! no admission. It withholds the environment of some programs, the system
//! shells among them; a tail with no byte other than NUL after the arguments
//! is read as withheld, never as empty, and the caller's own markers come
//! from the process itself. A person's command below a program that hides its
//! environment is refused with a sentence that names the Zerus window; a
//! marked command reads no ancestor's environment and is not affected.
//! `context` states what macOS withholds, which shapes pass, the limit at the
//! top and what a macOS sandbox can change. Other platforms refuse both
//! callers.
//!
//! This is not verified Person authority. A command whose ancestry does not
//! pass through readable same-user processes to its agent is outside this
//! rule: reparenting and credential-changing helpers can sever that chain. A
//! command that leaves its host's or agent's process tree (a double fork,
//! setsid, a background job whose shell exits) is adopted by the user's
//! service manager and passes as a person; with its own markers kept it is
//! refused plainly. This is the reparenting limit and it holds for every agent
//! kind.
//! Agents started without session markers are also undetectable, and the walk
//! refuses parent zero. A caller that replaces or instruments `hgs` itself or
//! writes the team store directly is outside this interim proof. Other
//! process boundaries still need the future verified Person authority.
//! Discovery uses ancestor environments, never a saved-session inventory;
//! only the few recorded resident hosts are listed, within their bound.
//! The top is established from process-table owners and parentage, never a
//! program name. A same-user parent of zero refuses. Only a verified foreign
//! parent ends the walk. Its top may have an unreadable environment; every node below
//! it must be inspected. An unreadable or credential-changing helper in the
//! middle still refuses; the message explains that process boundary without
//! assuming the person has left a terminal.
//!
//! Resident agent hosts serve several sessions from one long-lived process:
//! DeepSeek's host and OpenCode's shared service. No hosted session's own
//! agent is an ancestor of its shell, and a DeepSeek host passes the markers
//! of whichever session started it to every shell below it. So the person
//! path also refuses when an ancestor up to the top is a recorded live
//! resident host. A DeepSeek host is this computer's own record, matched by
//! process ID and birth like a recorded agent and never by a program name;
//! at most 64 host folders are read, and on macOS, where launchd adopts the
//! host as the top, no environment needs to be read. A host spawned by an
//! attempt that ended unconfirmed, before its running record, is also
//! recognized by its own exact host arguments beneath this state directory.
//! An unrecorded host with a different argument shape remains outside the check. OpenCode registers its
//! service only in its own file, read as untrusted data: the process it
//! names counts when that ancestor is not younger than the file, so the file
//! can only add a refusal. The check does not see a service that is not
//! registered there, a command it detaches, or an agent that changes its own
//! environment or the service file: one that points its state directory
//! elsewhere, or backdates, deletes or rewrites its own service file. OpenCode
//! is not covered against an agent that means to pass as a person. A host
//! record whose birth differs is not that host, and a record from another boot
//! names no process of this one. A missing state folder records no host. A
//! recorded ID without a birth, an unreadable or corrupt host record and more
//! host folders than the bound refuse.
//!
//! A marked DeepSeek claim (`dsh/...`) is proven by the hosted caller proof
//! and by nothing else (zerus-63rru.28): the host's per-call token names the
//! conversation whose tool call runs this command, the host's socket peer is
//! the recorded host and the host is an ancestor (`session_graph::
//! hosted_caller`). A proven claim is that session's key, `launch_id:` or
//! `run_id:` like any other agent's; a refusal carries the proof's own
//! sentence; an archived binding refuses; the ancestry proof is never tried
//! after it. A DeepSeek child agent's command is proven as its top-level
//! session and acts with that session's standing, as an in-process Claude or
//! Codex subagent does; a DeepSeek fork is its own session. Only a host that
//! advertises per-call tokens can prove anything; a host started before them
//! is never restarted or reloaded for it. A marked caller whose binding says
//! OpenCode's shared service serves its session is refused plainly: such a
//! session cannot be a team lead or member in this milestone, and
//! `host_served_agent` lets team creation's bundle validation (and catalog
//! export or save) refuse it too; no member-launch code calls it, because an
//! agent kind cannot change after creation.
//! Any other marked claim below a recorded resident agent host is refused
//! with a sentence of its own: a command there can only be a session that
//! host serves, and only a DeepSeek claim's token proves one. The person path
//! and such a claim ask one check, [`hosted_ancestor`], which calls the shared
//! `state::dsh::hosted_ancestor`. It reads the bounded records, then the process
//! table alone and each ancestor's own argument vector, never `HGS_AGENT` or
//! another caller-set marker. Without records it still recognizes a live
//! host by its own exact arguments, but does not adopt walk refusals. With
//! records, an unreadable record or birth and a walk past its bound refuse
//! with the shared check's exact cause and recovery text. Deleting a host's
//! same-user records does not conceal an ancestor whose own arguments still
//! identify it; reparenting or a host with neither record nor matching argv
//! remains outside this interim proof. OpenCode keeps its separate service
//! file check described above.
//! [`limits`] is the one text that states every limit of these proofs.
//!
//! With a session-ID hint, ids/<ID>.json selects one verified record.
//! Without an index entry, fixed ID-layout and legacy-label paths are both
//! probed; disagreement refuses. Without a hint only the exact legacy name
//! route is used. One rename shadow is allowed; launch_label proves the old
//! name of a renamed ID record. No missing route is an absence proof: when no
//! record can be reached, or any identity contradicts the claim, admission
//! refuses. Only an established stale/archived/nonmatching process continues
//! the walk. No inventory scan, repair or compatibility fallback is used.
//! The walk is capped at 64 processes; environments and bindings at 1 MiB,
//! index entries at 4 KiB. No process environment is changed.
mod ancestry;
mod context;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub(crate) mod hosted_fixture;
#[cfg(test)]
mod test_sources;
use ancestry::ancestors;

use crate::config::Config;
use serde_json::Value;
use zerus_team_plan::model::SessionKey;
use zerus_team_plan::problem::code;
use zerus_team_plan::Problem;

pub(crate) const INTERIM_NOTE: &str = "This interim rule does not detect commands whose ancestry does not pass through readable processes of the same user to their agent, for example after reparenting or credential-changing helpers, nor a caller that replaces or instruments hgs itself or writes the team store directly. Agents started without session markers are also not detected. A command in a sandbox with its own user namespace is refused, so team commands must run outside such a sandbox.";

/// What the hosted DeepSeek proof adds to the interim rule, between it and
/// the proof's own limit.
const HOSTED_NOTE: &str = "A DeepSeek session's command is proven by its host's per-call token instead of by ancestry, and only under a host that advertises those tokens; a host started before them is never restarted for it. A DeepSeek child agent's command is proven as its top-level session and acts with that session's standing, as an in-process Claude or Codex subagent does; a DeepSeek fork is its own session.";

/// The limits of this milestone's caller proofs as one text: the interim
/// ancestry rule's, then the hosted DeepSeek proof's. The preview reply and a
/// person's actor view state it.
pub(crate) fn limits() -> String {
    format!(
        "{INTERIM_NOTE} {HOSTED_NOTE} {}",
        crate::state::session_graph::HOSTED_LIMIT
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Caller {
    Person,
    Session(SessionKey),
    Refused(Problem),
}

pub(crate) fn caller(config: &Config) -> Caller {
    #[cfg(test)]
    test_sources::called();
    if let Err(problem) = context::available() {
        return Caller::Refused(problem);
    }
    // The admission runs before any marker is read into a proof and before
    // anything reads the process table. Its refusal never falls through.
    let admission = context::admission();
    let session = std::env::var_os("HGS_SESSION");
    let run = std::env::var_os("HGS_RUN_ID");
    let hint = std::env::var_os("HGS_SESSION_ID");
    let marked = session.is_some() || run.is_some() || hint.is_some();
    if let Err(problem) = admission {
        return Caller::Refused(if marked { sandboxed() } else { problem });
    }
    if marked {
        let pair = session
            .as_ref()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .zip(
                run.as_ref()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty()),
            );
        let Some((name, run)) = pair else {
            return Caller::Refused(not_allowed());
        };
        let id = match hint.as_ref() {
            Some(value) => match value.to_str().filter(|s| !s.is_empty()) {
                Some(id) => Some(id),
                None => return Caller::Refused(not_allowed()),
            },
            None => None,
        };
        if crate::state::session_graph::hosted_claim(name) {
            return hosted_session(config, name, run, id);
        }
        let hosted = crate::state::session_graph::caller_hosted_backend(name, run, id);
        if hosted.as_deref().is_some_and(host_served_agent) {
            return Caller::Refused(host_served());
        }
        match hosted_ancestor() {
            Ok(None) => (),
            Ok(Some(_)) => return Caller::Refused(claim_inside_host()),
            Err(refusal) => return Caller::Refused(host_refusal(refusal)),
        }
        return match prove(config, name, run, id) {
            Ok(Some(key)) => Caller::Session(key),
            Ok(None) => Caller::Refused(not_allowed()),
            Err(problem) => Caller::Refused(problem),
        };
    }
    match agent_above() {
        Ok(None) => Caller::Person,
        Ok(Some(refusal)) | Err(refusal) => Caller::Refused(refusal),
    }
}

/// A DeepSeek claim's caller: the hosted proof's session, or its refusal in
/// the proof's own words. Nothing falls back to the ancestry proof.
fn hosted_session(config: &Config, name: &str, run: &str, session: Option<&str>) -> Caller {
    match crate::state::session_graph::hosted_caller(name, run, session) {
        Ok(Some(record)) => match session_key(config, &record) {
            Ok(key) => Caller::Session(key),
            Err(problem) => Caller::Refused(problem),
        },
        Ok(None) => Caller::Refused(not_allowed()),
        Err(reason) => Caller::Refused(Problem::new(code::NOT_ALLOWED, reason)),
    }
}

/// Whether sessions of this agent kind are served by a resident agent host
/// (DeepSeek's host, OpenCode's shared service) rather than by an agent
/// process of their own, and so cannot hold a team seat in this milestone.
/// OpenCode's commands cannot be bound to their own session at all. A
/// DeepSeek session's commands are proven by its host's per-call token;
/// its seat also requires witnessed inputs and hosted lifecycle observations.
/// Team creation's roster validation and the catalog's export/save call this;
/// no member-launch code does, because an agent kind cannot change after
/// creation.
pub(crate) fn host_served_agent(agent: &str) -> bool {
    matches!(agent, "dsh" | "opencode")
}

fn host_served() -> Problem {
    Problem::new(code::NOT_ALLOWED,
        "This session is served by OpenCode's shared service; in this milestone such a session cannot be a team lead or member, because its commands cannot be bound to its own session by process ancestry.")
}

fn inside_agent() -> Problem {
    Problem::new(code::NOT_ALLOWED,
        "A live agent session runs this command; person-only operations require a caller outside its process tree.")
}

fn inside_host() -> Problem {
    Problem::new(code::NOT_ALLOWED,
        "This command runs inside an agent host's process tree; person-only operations need a caller outside it.")
}

/// A marked claim of another session below a recorded resident agent host.
fn claim_inside_host() -> Problem {
    Problem::new(code::NOT_ALLOWED,
        "This command runs inside an agent host's process tree, where a command can only be a session that host serves; session markers that name another session cannot be proven here.")
}

/// A marked command carries the lead's or the member's shell environment, so
/// its refusal must name the sandbox rather than the person proof.
fn sandboxed() -> Problem {
    Problem::new(code::NOT_ALLOWED,
        "The command runs in a sandbox with its own user namespace. Team commands of a lead or a member must run outside it.")
}

fn not_allowed() -> Problem {
    Problem::new(code::NOT_ALLOWED,
        "The session markers cannot verify the claimed agent's live process, birth and ancestry. Run session commands from the agent's own shell with its original launch markers.")
}

fn unreadable() -> Problem {
    Problem::new(code::INVALID_STATE,
        "Cannot complete the process ancestry check. A process between this command and the session start changed its credentials or cannot be read, or the walk exceeded its bound.")
}

fn marked_unreadable() -> Problem {
    Problem::new(code::INVALID_STATE,
        "Cannot read or verify the claimed agent's process ownership, birth or ancestry, saved binding or computer identity. Run session commands from the agent's own shell after its saved session state is available.")
}

/// A withheld environment below the top: a program whose environment macOS
/// hides, usually a system shell, stands between the person's command and the
/// top of the user's tree. It names the cause and the one way out that is
/// known to pass, and promises no terminal. Distinct from the marked path's
/// sandbox sentence.
fn withheld_below_top() -> Problem {
    Problem::new(code::NOT_ALLOWED,
        "On macOS a person's team command cannot be checked below a program that hides its environment, usually a system shell; run it from the Zerus window.")
}

fn binding_unreadable() -> Problem {
    Problem::new(code::INVALID_STATE,
        "Cannot verify an ancestor's saved session binding; this interim check cannot establish a person as caller.")
}

/// A saved binding's immutable identity: its launch ID, else its run ID.
pub(crate) fn identity(record: &Value) -> String {
    for key in ["launch_id", "run_id"] {
        if let Some(id) = record[key].as_str().filter(|id| !id.is_empty()) {
            return format!("{key}:{id}");
        }
    }
    String::new()
}

fn session_key(config: &Config, record: &Value) -> Result<SessionKey, Problem> {
    let session_id = identity(record);
    if session_id.is_empty() {
        return Err(marked_unreadable());
    }
    let machine =
        crate::state::session_graph::caller_machine(config).map_err(|_| marked_unreadable())?;
    Ok(SessionKey {
        machine,
        session_id,
    })
}

fn prove(
    config: &Config,
    name: &str,
    run: &str,
    session: Option<&str>,
) -> Result<Option<SessionKey>, Problem> {
    prove_read(
        config,
        name,
        run,
        session,
        crate::state::session_graph::caller_record_for,
    )
}

fn prove_read(
    config: &Config,
    name: &str,
    run: &str,
    session: Option<&str>,
    read: impl FnOnce(&str, &str, Option<&str>) -> std::result::Result<Option<Value>, &'static str>,
) -> Result<Option<SessionKey>, Problem> {
    read(name, run, session)
        .map_err(|_| marked_unreadable())?
        .map(|record| session_key(config, &record))
        .transpose()
}

/// The refusal when a verified live agent or a recorded resident agent host
/// is above this command, `None` when neither is.
fn agent_above() -> Result<Option<Problem>, Problem> {
    let chain = ancestors()?;
    for ancestor in &chain {
        if let Some(claim) = &ancestor.claim {
            if crate::state::session_graph::caller_agent_for(
                &claim.name,
                &claim.run,
                claim.session.as_deref(),
                ancestor.pid,
                ancestor.identity.birth.as_deref().ok_or_else(unreadable)?,
            )
            .map_err(|_| binding_unreadable())?
            .is_some()
            {
                return Ok(Some(inside_agent()));
            }
        }
    }
    if hosted_ancestor().map_err(host_refusal)?.is_some() {
        return Ok(Some(inside_host()));
    }
    // T2: no deterministic test pins this post-proof race window; changing a
    // real process exactly after the binding read needs a timing/injection seam.
    // Recheck original edges and births before accepting a negative proof.
    ancestry::unchanged(&chain)?;
    Ok(None)
}

/// A recorded resident agent host that is an ancestor of this command.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Host {
    pid: u32,
    birth: String,
}

/// Why [`hosted_ancestor`] cannot tell whether a recorded host is above this
/// command. Each cause is its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The shared DeepSeek check retains its exact cause and recovery text.
    #[cfg(unix)]
    DeepSeek(crate::state::dsh::Refusal),
    /// A recorded process ID is an ancestor's, but the birth that would tell
    /// the two apart cannot be read on one side.
    Birth,
    /// The walk did not reach the top of the user's tree within its bound, or
    /// could not read a process on the way.
    Walk,
}

/// DeepSeek uses the same recorded-host and live-argv proof as send, tasks
/// and question routing. OpenCode retains its separate, refusal-only service
/// registration check; it never proves a hosted session.
fn hosted_ancestor() -> Result<Option<Host>, Refusal> {
    #[cfg(unix)]
    if let Some(host) = crate::state::dsh::hosted_ancestor().map_err(Refusal::DeepSeek)? {
        return Ok(Some(Host { pid: host.pid, birth: host.birth }));
    }
    opencode_ancestor_read(
        crate::state::session_graph::caller_opencode_host,
        ancestry::processes,
        native_start,
    )
}

fn opencode_ancestor_read(
    service: impl FnOnce() -> Option<crate::state::session_graph::Registered>,
    walk: impl FnOnce() -> Result<Vec<ancestry::Ancestor>, Problem>,
    native: impl Fn(u32) -> Option<(String, f64)>,
) -> Result<Option<Host>, Refusal> {
    let Some(service) = service() else { return Ok(None); };
    let chain = walk().map_err(|_| Refusal::Walk)?;
    hosted(&chain, &service, native)
}

/// A check that cannot tell refuses, retaining the shared check's cause.
pub(crate) fn host_refusal(refusal: Refusal) -> Problem {
    match refusal {
        #[cfg(unix)]
        Refusal::DeepSeek(cause) => Problem::new(code::INVALID_STATE, cause.sentence()),
        Refusal::Birth => host_unreadable(),
        Refusal::Walk => unreadable(),
    }
}

fn host_unreadable() -> Problem {
    Problem::new(code::INVALID_STATE,
        "Cannot verify a recorded agent host of this computer; this interim check cannot establish a person as caller.")
}

/// A process's birth and start, as [`hosted`] matches OpenCode's service.
fn native_start(pid: u32) -> Option<(String, f64)> {
    crate::platform::process_identity(pid).map(|native| (native.birth, native.started))
}

/// OpenCode's service file is untrusted and names no birth: its ID counts
/// when that ancestor is not younger than the file, with one second for clock
/// granularity. A reused ID can only add a refusal. Unreadable birth refuses.
fn hosted(
    chain: &[ancestry::Ancestor],
    service: &crate::state::session_graph::Registered,
    native: impl Fn(u32) -> Option<(String, f64)>,
) -> Result<Option<Host>, Refusal> {
    for ancestor in chain.iter().filter(|ancestor| ancestor.pid == service.pid) {
        match native(ancestor.pid) {
            Some((birth, started)) if Some(&birth) == ancestor.identity.birth.as_ref() => {
                if started <= service.written + 1.0 {
                    return Ok(Some(Host { pid: ancestor.pid, birth }));
                }
            }
            _ => return Err(Refusal::Birth),
        }
    }
    Ok(None)
}

/// Claims are only discovery hints. The explicit saved-record proof establishes
/// ancestry and liveness; neither environment labels nor a missing file do.
fn markers(bytes: &[u8]) -> Option<(String, String)> {
    let mut session = None;
    let mut run = None;
    for entry in bytes.split(|byte| *byte == 0) {
        if let Some(value) = entry.strip_prefix(b"HGS_SESSION=") {
            session = text(value);
        } else if let Some(value) = entry.strip_prefix(b"HGS_RUN_ID=") {
            run = text(value);
        }
    }
    session.zip(run)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Claim {
    name: String,
    run: String,
    session: Option<String>,
}

/// The three session markers, each as it begins an environment entry.
const MARKERS: [&[u8]; 3] = [b"HGS_SESSION=", b"HGS_RUN_ID=", b"HGS_SESSION_ID="];

/// A marker named twice in one environment is unreadable on every platform:
/// the agent's own `getenv` takes the first entry and a reader of the block
/// could take another, so neither is trusted.
fn duplicated(bytes: &[u8]) -> bool {
    MARKERS.iter().any(|prefix| {
        bytes
            .split(|byte| *byte == 0)
            .filter(|entry| entry.starts_with(prefix))
            .nth(1)
            .is_some()
    })
}

fn claim(bytes: &[u8]) -> Result<Option<Claim>, Problem> {
    if duplicated(bytes) {
        return Err(unreadable());
    }
    let has_marker = bytes
        .split(|byte| *byte == 0)
        .any(|entry| MARKERS.iter().any(|prefix| entry.starts_with(prefix)));
    match markers(bytes) {
        Some((name, run)) => {
            let session = bytes
                .split(|byte| *byte == 0)
                .find_map(|entry| entry.strip_prefix(b"HGS_SESSION_ID="))
                .map(|value| text(value).ok_or_else(unreadable))
                .transpose()?;
            Ok(Some(Claim { name, run, session }))
        }
        None if has_marker => Err(unreadable()),
        None => Ok(None),
    }
}

fn text(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    (!text.is_empty()).then(|| text.to_owned())
}

/// One process's environment, bounded. The agent a caller descends from runs
/// as this user, so only this user's processes are read.
#[cfg(target_os = "linux")]
pub(crate) fn environment(pid: u32) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(format!("/proc/{pid}/environ"))?
        .take((1 << 20) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1 << 20 {
        return Err(std::io::Error::other("environment is too large"));
    }
    Ok(bytes)
}

/// This process's own environment, from the process itself. The walk takes
/// the caller's markers from here and never from the system's read of the
/// caller, which macOS may withhold for a signed release build of `hgs`.
fn own_environment() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (key, value) in std::env::vars_os() {
        bytes.extend_from_slice(key.as_encoded_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(value.as_encoded_bytes());
        bytes.push(0);
    }
    bytes
}

/// What the macOS read passes to `KERN_PROCARGS2`, as the Linux read's bound.
#[cfg(any(target_os = "macos", test))]
const PROCARGS_BUFFER: usize = 1 << 20;

/// The same read on macOS, through `KERN_PROCARGS2`.
#[cfg(target_os = "macos")]
pub(crate) fn environment(pid: u32) -> std::io::Result<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as i32];
    let mut bytes = vec![0u8; PROCARGS_BUFFER];
    let mut size = bytes.len();
    // SAFETY: the buffer and the size are valid for the call.
    let code = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if code != 0 {
        return Err(std::io::Error::other("environment is unavailable"));
    }
    bytes.truncate(size);
    procargs_environment(&bytes, PROCARGS_BUFFER)
}

/// The environment part of a `KERN_PROCARGS2` answer, which holds the
/// argument count, the executable and its padding, the arguments and then
/// the environment, read into a buffer of `capacity` bytes.
///
/// macOS withholds the environment of some programs from a reader of the
/// same user by ending the answer right after the arguments, without an
/// error. A process whose environment is really empty still leaves the
/// kernel's own `key=value` strings there. So a tail with no byte other than
/// NUL is a withheld read, never an empty environment. Three other shapes are
/// unreadable: an answer that fills the buffer, which may be cut; fewer
/// arguments than the count; and an argument that begins with a marker name,
/// which means the count has run into the environment (an empty first
/// argument, or arguments the process rewrote with fewer separators).
#[cfg(any(target_os = "macos", test))]
fn procargs_environment(bytes: &[u8], capacity: usize) -> std::io::Result<Vec<u8>> {
    let invalid = || std::io::Error::other("environment is unavailable");
    if bytes.len() >= capacity {
        return Err(invalid());
    }
    let count = bytes.get(..4).ok_or_else(invalid)?;
    let argc = i32::from_ne_bytes(count.try_into().map_err(|_| invalid())?);
    if !(1..=4096).contains(&argc) {
        return Err(invalid());
    }
    let after = |rest: &[u8]| -> std::io::Result<usize> {
        Ok(rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(invalid)?
            + 1)
    };
    let mut rest = &bytes[4..];
    rest = &rest[after(rest)?..];
    while rest.first() == Some(&0) {
        rest = &rest[1..];
    }
    for _ in 0..argc {
        let end = after(rest)?;
        if MARKERS.iter().any(|prefix| rest[..end].starts_with(prefix)) {
            return Err(invalid());
        }
        rest = &rest[end..];
    }
    if rest.iter().all(|byte| *byte == 0) {
        return Err(withheld());
    }
    Ok(rest.to_vec())
}

/// The system answered for a process but left its environment out.
#[derive(Debug)]
struct Withheld;

impl std::fmt::Display for Withheld {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the system withholds this process's environment")
    }
}

impl std::error::Error for Withheld {}

#[cfg(any(target_os = "macos", test))]
fn withheld() -> std::io::Error {
    std::io::Error::other(Withheld)
}

fn is_withheld(error: &std::io::Error) -> bool {
    error.get_ref().is_some_and(|inner| inner.is::<Withheld>())
}

/// Unsupported platforms refuse before this read is reachable. This fallback
/// stays an error; it can never establish an unmarked person's authority.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn environment(_pid: u32) -> std::io::Result<Vec<u8>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "this computer does not expose another process's environment",
    ))
}

#[cfg(test)]
mod tests;
