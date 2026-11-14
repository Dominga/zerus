//! The standing grant is narrower than ordinary automatic approvals.
use super::types::*;
use serde_json::Value;
use zerus_team_plan::model::{MemberState, TeamStatus};

fn need(yes: bool, why: &str) -> Result<(), String> {
    if yes {
        Ok(())
    } else {
        Err(why.into())
    }
}

pub(super) fn stored(b: &Binding, f: &Facts) -> Result<(), String> {
    need(f.preview, "Teams preview is off")?;
    need(
        f.team.status == TeamStatus::Running,
        "The team is not running",
    )?;
    need(f.team.id == b.team, "The team changed")?;
    need(f.team.home == b.machine, "The team's computer changed")?;
    need(f.team.owner == b.person, "The team's owner changed")?;
    need(f.member.id == b.member, "The member changed")?;
    need(
        f.member.state == MemberState::Active,
        "The member is not active",
    )?;
    need(f.member.role == b.role, "The member's role changed")?;
    need(f.member.agent == b.agent, "The member's agent changed")?;
    need(
        f.member.account == b.account,
        "The member's account changed",
    )?;
    let key = f
        .member
        .session
        .as_ref()
        .ok_or("The member has no bound session")?;
    need(key.machine == b.machine, "The member's computer changed")?;
    need(
        key.session_id == format!("launch_id:{}", b.launch),
        "The member's launch changed",
    )?;
    need(
        f.member.worktree == b.folder,
        "The member's working folder changed",
    )?;
    need(
        f.member.ownership_nonce == b.nonce,
        "The member's ownership changed",
    )?;
    need(f.binding == *b, "The member's launch provenance changed")
}

pub(super) fn session(b: &Binding, s: &Snapshot) -> Result<(), String> {
    need(s.machine == b.machine, "The session's computer changed")?;
    need(s.launch == b.launch, "The session's launch changed")?;
    need(
        !b.session.is_empty() && s.session == b.session,
        "The native session changed",
    )?;
    need(
        !s.run.is_empty() && s.run == b.run,
        "The native run changed",
    )?;
    need(
        b.conversation.is_empty() || s.conversation == b.conversation,
        "The conversation changed",
    )?;
    need(s.agent == b.agent, "The native agent changed")?;
    need(s.account == b.account, "The native account changed")?;
    need(s.folder == b.folder, "The native folder changed")
}

pub(super) fn ownership(b: &Binding, o: &Ownership) -> Result<(), String> {
    need(o.exists, "The owned checkout is missing")?;
    need(o.unchanged, "The checkout ownership was replaced")?;
    need(o.folder == b.folder, "The owned checkout path changed")?;
    need(o.common == b.common, "The owned repository changed")?;
    need(o.nonce == b.nonce, "The checkout's ownership nonce changed")?;
    need(
        o.launch == b.launch,
        "The checkout belongs to another launch",
    )
}

/// The member's pending folder-trust prompt: the only question the standing
/// confirmation considers (zerus-63rru.73). It is decided here, once, by the
/// producer's source: the folder-trust source of the member's own agent
/// ([`provider`]). Every other pending question is the person's, through the
/// session's ordinary attention, and plans nothing here: the agent's own
/// question, a permission prompt, another startup or terminal choice.
pub(super) fn trust_candidate<'a>(b: &Binding, s: &'a Snapshot) -> Option<&'a Value> {
    let (source, ..) = provider(&b.agent).ok()?;
    s.questions.iter().find(|q| q["source"] == source)
}

pub(super) fn trust(b: &Binding, s: &Snapshot) -> Result<Value, String> {
    session(b, s)?;
    need(
        s.live && !s.paused && !s.ended,
        "The member is not at a live startup",
    )?;
    need(
        s.questions.len() == 1,
        "The member does not have exactly one pending prompt",
    )?;
    let q = &s.questions[0];
    need(q["can_answer"] == true, "The prompt cannot be answered")?;
    need(
        q["run_id"] == s.run
            && crate::state::native_identity(q, "conversation_id") == Some(s.conversation.as_str()),
        "The prompt identity changed",
    )?;
    need(
        s.conversation.is_empty(),
        "The folder prompt is not a startup prompt",
    )?;
    let expected = provider(&b.agent)?;
    need(
        q["source"] == expected.0 && q["answer_transport"] == expected.1,
        "This prompt needs a person",
    )?;
    need(
        q["question_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s.len() <= 1024),
        "The prompt has no identity",
    )?;
    need(
        q["question_hash"]
            .as_str()
            .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())),
        "The prompt hash is unavailable",
    )?;
    let items = q["questions"]
        .as_array()
        .ok_or("The prompt has no question")?;
    need(items.len() == 1, "The prompt contains multiple questions")?;
    folder_item(
        &items[0],
        &b.folder,
        expected.2,
        expected.3,
        b.agent == "kimi",
        b.agent == "codex",
    )?;
    Ok(q.clone())
}

fn provider(
    agent: &str,
) -> Result<(&'static str, &'static str, &'static str, &'static str), String> {
    Ok(match agent {
        "claude" => (
            "claude_folder_trust",
            "claude_tui",
            "trust_1",
            "Yes, I trust this folder",
        ),
        "codex" => (
            "codex_folder_trust",
            "codex_tui",
            "trust_0",
            "Trust and continue",
        ),
        "kimi" => (
            "kimi_folder_trust",
            "kimi_tui",
            "trust_0",
            "Trust this folder",
        ),
        _ => return Err("This agent has no reviewed folder-trust grant".into()),
    })
}

/// The granted scope for the audit row and the texts: the repository's
/// main checkout when the folder is a linked worktree of one, else the
/// folder itself (zerus-63rru.66).
pub(super) fn scope_for(folder: &str) -> String {
    main_checkout(folder).unwrap_or_else(|| folder.to_owned())
}

/// The repository's main checkout a linked worktree belongs to, resolved
/// from Git, never from the card's text (zerus-63rru.66 round 2): the
/// common Git dir must be the main checkout's own `.git` — its file name is
/// `.git`, the repository is not bare, and the work tree's top level of the
/// common dir's parent is that parent — so a bare repository, a separate Git
/// directory and a submodule (a common dir under `.git/modules`) resolve to
/// nothing and stay the person's. A folder that is its own non-bare
/// repository resolves to itself.
pub(super) fn main_checkout(folder: &str) -> Option<String> {
    let git = |at: &str, args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(at)
            .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgSign=false"])
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).ok())
            .flatten()
    };
    let common = git(folder, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let common = common.trim();
    if common == ".git" {
        // The folder is its own repository: a main checkout only when the
        // repository is not bare.
        return non_bare(folder).then(|| folder.to_owned());
    }
    let git_dir = std::path::Path::new(common);
    if git_dir.file_name() != Some(std::ffi::OsStr::new(".git")) {
        return None;
    }
    let checkout = git_dir.parent()?.to_str()?;
    // The work tree's top level of the common dir's parent must be that
    // parent (a submodule's top level is elsewhere), and the repository is
    // not bare.
    if git(checkout, &["rev-parse", "--show-toplevel"])?.trim() != checkout {
        return None;
    }
    non_bare(checkout).then(|| checkout.to_owned())
}

fn non_bare(at: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(at)
        .args(["-c", "core.hooksPath=/dev/null", "-c", "commit.gpgSign=false"])
        .args(["rev-parse", "--is-bare-repository"])
        .output()
        .map(|o| o.status.success() && o.stdout.starts_with(b"false"))
        .unwrap_or(false)
}

/// Whether two paths name the same checkout, canonicalized: a symlink
/// spelling is accepted only when it canonicalizes to the other path
/// (zerus-63rru.66 round 2).
fn same_checkout(a: &str, b: &str) -> bool {
    let canon = |p: &str| std::fs::canonicalize(p).map(|c| c.to_string_lossy().into_owned());
    match (canon(a), canon(b)) {
        (Ok(a), Ok(b)) => a == b,
        (Err(_), Err(_)) => a == b,
        _ => false,
    }
}

fn folder_item(
    item: &Value,
    folder: &str,
    option: &str,
    label: &str,
    kimi: bool,
    repository_ok: bool,
) -> Result<(), String> {
    let repository = item["question"] == "Trust this Git repository?";
    need(
        item["id"] == "trust"
            && (item["question"] == "Trust this folder?" || repository && repository_ok),
        "The prompt grants a different scope",
    )?;
    need(
        item["multi_select"] != true && item["allow_other"] != true,
        "The prompt has other answer modes",
    )?;
    let body = item["body"].as_str().ok_or("The prompt names no folder")?;
    let mut paragraphs = body.split("\n\n");
    need(
        paragraphs
            .next()
            .is_some_and(|path| path == folder),
        "The prompt names a different folder",
    )?;
    need(
        paragraphs.next().is_some_and(|text| !text.is_empty()),
        "The prompt carries no disclosure",
    )?;
    // The repository card names the root it would grant, in the note its
    // producer writes: the member's folder itself, or the repository's main
    // checkout as Git resolves it. Anything else stays the person's.
    if repository {
        // The producer emits the note and its root on one line, the root as
        // the note's last words after its colon (state::codex_trust parse
        // and card). The root is required, and it must canonicalize to the
        // repository's main checkout as Git resolves it.
        const NOTE: &str = "Note: You\u{2019}re in a subdirectory of a Git project. Trusting will apply to the repository root:";
        let root = body
            .lines()
            .find_map(|line| {
                let line = line.trim();
                line.strip_prefix(NOTE)
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(str::to_owned)
            })
            .ok_or("The prompt grants no repository root")?;
        let checkout = main_checkout(folder).ok_or("The repository's main checkout is unknown")?;
        need(
            same_checkout(&root, &checkout),
            "The prompt grants a different repository",
        )?;
    }
    let choices = item["options"]
        .as_array()
        .ok_or("The prompt has no choices")?;
    need(
        choices.iter().filter(|o| o["id"] == option).count() == 1,
        "The grant choice is ambiguous",
    )?;
    need(choices.len() == 2, "The prompt has different choices")?;
    let count = choices
        .iter()
        .filter(|o| {
            o["id"] == option && (o["label"] == label || kimi && o["label"] == "Trust and continue")
        })
        .count();
    need(count == 1, "The reviewed folder-trust choice changed")
}

#[cfg(test)]
pub(super) mod tests;

/// What the person can do for a seat whose exited session cannot come back
/// (zerus-63rru.62), from what this build offers: Remove on the member card
/// (`archive_member`), and `hgs team reassign`, which the person or the lead
/// may run; the desktop has no Reassign button. A fresh start is not offered:
/// it ends the member's binding here.
pub(super) const SEAT: &str = "If its session cannot come back, remove the member with Remove on its member card; you or the lead can reassign its tasks to another member with hgs team reassign.";

pub(super) fn resume(s: &Snapshot) -> Result<(), String> {
    need(
        !s.live && s.ended && !s.paused,
        "The member is not an exited, unpaused session",
    )?;
    need(
        s.questions.is_empty(),
        "An exited member has a pending question",
    )?;
    need(
        s.self_exit && !s.exit_identity.is_empty(),
        "The exit has no exact supervisor evidence",
    )?;
    need(
        !s.conversation.is_empty(),
        "The exited conversation is unknown",
    )?;
    match &s.restore_refusal {
        Some(why) => Err(format!(
            "The archive of its exited session offers no restore: {why}. {SEAT}"
        )),
        None => Ok(()),
    }
}
