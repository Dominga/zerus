//! WhiteWh fork: stage delivery (U36, #532). The dispatcher gives a task's
//! open stage to the first free member with its role (or its pinned member),
//! in the task's own folder: the implementer's worktree or clone, or the
//! shared folder. A review reads there; a test also builds there through the
//! command templates, writing only build output and its own temporary folder,
//! and only while the stage is open. A test with the hold rule holds the
//! folder: the implementer's next task waits. With the checkout rule Zerus
//! tests a detached checkout of the attempt's commits and removes it after.
//! A folder without Git works too: Git is never initialized.
use super::super::root;
use super::dispatcher::{git_at, git_command, member_folder_path};
use super::model::*;
use super::pipeline;
use anyhow::{bail, ensure, Context};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// At most this many changed files are listed and hashed for a stage.
const MAX_FILES: usize = 200;
/// A file larger than this is listed with its size only.
const MAX_HASH_BYTES: u64 = 64 * 1024 * 1024;
/// The command templates a test stage may run in the task's folder: builds and
/// tests, which write only build output. Merges and commits change the task.
const TEST_TEMPLATES: [&str; 6] = ["cargo-build", "cargo-test", "cargo-clippy", "qt-configure", "qt-build", "qt-test"];

/// The implementer's attempt the run checks.
fn checked_attempt<'a>(task: &'a Task, run: &StageRun) -> Option<&'a Attempt> {
    task.attempts.iter().find(|attempt| attempt.n == run.attempt)
}

/// The task's own folder: where its implementer works.
pub(crate) fn task_folder(team: &Team, task: &Task, run: &StageRun) -> anyhow::Result<String> {
    let implementer = checked_attempt(task, run).map(|attempt| attempt.member.clone()).unwrap_or_else(|| task.assignee.clone());
    let member = team.member(&implementer).context("The task's implementer is not in the team")?;
    member_folder_path(team, member)
}

/// Whether `folder` is inside a Git work tree. Nothing is initialized.
pub(crate) fn is_git(folder: &str) -> bool {
    git_at(folder, &["rev-parse", "--is-inside-work-tree"]).as_deref() == Some("true")
}

/// The files the report says changed: its structured `files`, else its file
/// evidence. Relative paths, at most MAX_FILES.
fn reported_files(attempt: &Attempt) -> Vec<String> {
    let Some(structured) = attempt.report.as_ref().and_then(|report| report.structured.as_ref()) else { return Vec::new() };
    let mut files: Vec<String> = structured["files"].as_array().into_iter().flatten().filter_map(|file| file.as_str().map(str::to_owned)).collect();
    if files.is_empty() {
        files = structured["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["kind"] == "file")
            .filter_map(|item| item["path"].as_str().map(str::to_owned))
            .collect();
    }
    files.retain(|file| !file.trim().is_empty());
    files.truncate(MAX_FILES);
    files
}

/// One file's state: its size and the start of its SHA-256, or that it is gone.
fn file_state(folder: &Path, file: &str) -> Value {
    let path = if Path::new(file).is_absolute() { PathBuf::from(file) } else { folder.join(file) };
    let Ok(metadata) = std::fs::metadata(&path) else { return json!({"path":file,"deleted":true}) };
    if !metadata.is_file() {
        return json!({"path":file,"directory":metadata.is_dir()});
    }
    let mut state = json!({"path":file,"size":metadata.len()});
    if metadata.len() <= MAX_HASH_BYTES {
        if let Ok(bytes) = std::fs::read(&path) {
            state["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes))[..16].to_owned());
        }
    }
    state
}

/// What changed in the checked attempt: its commits (B4) in a Git checkout,
/// otherwise the report's changed files with their state now.
pub(crate) fn changed(task: &Task, run: &StageRun, folder: &str, git: bool) -> Vec<Value> {
    let Some(attempt) = checked_attempt(task, run) else { return Vec::new() };
    if git && !attempt.commits.is_empty() {
        return attempt.commits.iter().map(|commit| json!({"commit":commit.sha,"files":commit.files})).collect();
    }
    reported_files(attempt).iter().map(|file| file_state(Path::new(folder), file)).collect()
}

/// Where a stage works, prepared before its hand-off.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Prepared {
    pub folder: String,
    pub checkout: String,
    pub checkout_repo: String,
    pub holds: bool,
    pub changed: Vec<Value>,
    pub temp: String,
    /// Why a checkout rule fell back to holding the folder.
    pub note: String,
}

fn stage_dir(team: &Team, run: &StageRun, suffix: &str) -> PathBuf {
    let short = |id: &str| id.chars().take(8).collect::<String>();
    root().join("teams").join("stages").join(format!("{}-{}{suffix}", short(&team.id), short(&run.id)))
}

/// Prepares the open stage of `task_id`: its folder, a detached checkout
/// for a test with the checkout rule, what changed, and a test's own temporary
/// folder.
pub(crate) fn prepare(team: &Team, task_id: u32) -> anyhow::Result<Prepared> {
    let task = team.task(task_id).context("Unknown task")?;
    let pipeline = task.pipeline_run.as_ref().context("The task has no pipeline")?;
    let run = pipeline.open_run().context("The task is not in a stage")?;
    let folder = task_folder(team, task, run)?;
    ensure!(Path::new(&folder).is_dir(), "The task's folder {folder} is not there");
    let git = is_git(&folder);
    // Each stage of one round reads the files as the first stage found them.
    let earlier = pipeline.runs.iter().find(|other| other.round == run.round && other.id != run.id && !other.changed.is_empty());
    let changed = earlier.map(|other| other.changed.clone()).unwrap_or_else(|| changed(task, run, &folder, git));
    let mut prepared = Prepared { folder: folder.clone(), changed, ..Default::default() };
    if run.kind != "test" {
        return Ok(prepared);
    }
    let temp = stage_dir(team, run, "-tmp");
    std::fs::create_dir_all(&temp).with_context(|| format!("Could not create the stage's temporary folder {}", temp.display()))?;
    prepared.temp = temp.display().to_string();
    let sha = checked_attempt(task, run).and_then(|attempt| attempt.commits.last()).map(|commit| commit.sha.clone());
    match (pipeline.folder.as_str(), git, sha) {
        ("checkout", true, Some(sha)) => {
            let path = stage_dir(team, run, "");
            let output = git_command(&folder)
                .args(["worktree", "add", "--detach"])
                .arg(&path)
                .arg(&sha)
                .output()
                .context("Could not run git")?;
            ensure!(output.status.success(), "git worktree add failed: {}", String::from_utf8_lossy(&output.stderr).trim());
            prepared.folder = path.display().to_string();
            prepared.checkout = prepared.folder.clone();
            prepared.checkout_repo = folder;
        }
        ("checkout", true, None) => {
            prepared.holds = true;
            prepared.note = "The attempt made no commits, so the test works in the task's folder, which is held.".into();
        }
        ("checkout", false, _) => {
            prepared.holds = true;
            prepared.note = "The task's folder has no Git, so the test works in it, and it is held.".into();
        }
        _ => prepared.holds = true,
    }
    Ok(prepared)
}

/// Removes a stage's detached checkout and its temporary folder.
pub(crate) fn clean_up(run: &StageRun) -> anyhow::Result<()> {
    if !run.checkout.is_empty() && Path::new(&run.checkout).exists() {
        let output = git_command(&run.checkout_repo).args(["worktree", "remove", "--force"]).arg(&run.checkout).output().context("Could not run git")?;
        if !output.status.success() && Path::new(&run.checkout).exists() {
            bail!("git worktree remove failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        }
        let _ = git_command(&run.checkout_repo).args(["worktree", "prune"]).output();
    }
    if !run.temp.is_empty() && Path::new(&run.temp).exists() {
        std::fs::remove_dir_all(&run.temp).with_context(|| format!("Could not remove {}", run.temp))?;
    }
    Ok(())
}

/// Records where the given stage works, once its member has it.
pub(crate) fn record(team: &mut Team, task_id: u32, run_id: &str, prepared: &Prepared) {
    let Some(run) = team.task_mut(task_id).and_then(|task| task.pipeline_run.as_mut()).and_then(|run| run.runs.iter_mut().find(|run| run.id == run_id)) else {
        return;
    };
    run.folder = prepared.folder.clone();
    run.checkout = prepared.checkout.clone();
    run.checkout_repo = prepared.checkout_repo.clone();
    run.holds = prepared.holds;
    run.changed = prepared.changed.clone();
    run.temp = prepared.temp.clone();
}

/// The task whose test stage holds `member`'s folder: the implementer's next
/// task there waits until the test ends. A test waiting for its member already
/// holds it under the hold rule.
pub(crate) fn held(team: &Team, member: &str) -> Option<u32> {
    team.tasks.iter().find_map(|task| {
        let pipeline = task.pipeline_run.as_ref()?;
        let run = pipeline.open_run()?;
        let holds = run.kind == "test" && if run.state == StageState::Waiting { pipeline.folder != "checkout" } else { run.holds };
        let implementer = checked_attempt(task, run).map(|attempt| attempt.member.as_str()).unwrap_or(task.assignee.as_str());
        (holds && implementer == member).then_some(task.id)
    })
}

/// The members busy with a stage they were given.
pub(crate) fn busy(team: &Team) -> Vec<&str> {
    team.tasks
        .iter()
        .filter_map(|task| task.pipeline_run.as_ref()?.open_run())
        .filter(|run| run.state == StageState::Delivered && !run.member.is_empty())
        .map(|run| run.member.as_str())
        .collect()
}

/// The stages that wait for a member, oldest first: (task, candidates).
pub(crate) fn waiting(team: &Team) -> Vec<(u32, Vec<String>)> {
    let mut found: Vec<(f64, u32, Vec<String>)> = team
        .tasks
        .iter()
        .filter(|task| task.status.in_stage())
        .filter_map(|task| {
            let run = task.pipeline_run.as_ref()?.open_run()?;
            (run.state == StageState::Waiting).then(|| (run.at, task.id, pipeline::candidates(team, task.id)))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    found.into_iter().map(|(_, task, candidates)| (task, candidates)).collect()
}

/// The stages that ended with a checkout or a temporary folder left to remove.
pub(crate) fn to_clean(team: &Team) -> Vec<(u32, String)> {
    team.tasks
        .iter()
        .filter_map(|task| task.pipeline_run.as_ref().map(|run| (task.id, run)))
        .flat_map(|(id, pipeline)| {
            pipeline.runs.iter().filter(|run| !run.state.open() && (!run.checkout.is_empty() || !run.temp.is_empty())).map(move |run| (id, run.id.clone()))
        })
        .collect()
}

/// A member's stage queue for status: the stage it has, then the waiting
/// stages it may take.
pub(crate) fn queue(team: &Team, member: &str) -> Vec<Value> {
    let mut queue = Vec::new();
    for task in &team.tasks {
        let Some(pipeline) = task.pipeline_run.as_ref() else { continue };
        let Some(run) = pipeline.open_run() else { continue };
        let entry = json!({"task":task.id,"stage":run.kind,"round":run.round,"state":run.state,"since":run.delivered_at.unwrap_or(run.at)});
        if run.state == StageState::Delivered && run.member == member {
            queue.insert(0, entry);
        } else if run.state == StageState::Waiting && pipeline::candidates(team, task.id).iter().any(|found| found == member) {
            queue.push(entry);
        }
    }
    queue
}

/// U21 for a stage: the folder a template run with this attempt id works in.
/// None when the id is no open stage of this member (an ordinary attempt).
/// A review runs nothing; a test runs builds and tests only, in the stage's
/// folder, and only while the stage is open.
pub(crate) fn run_folder(team: &Team, member: &str, attempt: &str, template: &str) -> anyhow::Result<Option<(PathBuf, String)>> {
    let Some(task) = pipeline::open_run_task(team, attempt, member) else { return Ok(None) };
    let run = team.task(task).and_then(|task| task.pipeline_run.as_ref()).and_then(PipelineRun::open_run).context("The stage ended")?;
    ensure!(run.kind == "test", "A review stage reads the task's files; it runs no command");
    ensure!(TEST_TEMPLATES.contains(&template), "A test stage runs builds and tests only ({}), not {template}", TEST_TEMPLATES.join(", "));
    ensure!(!run.folder.is_empty() && Path::new(&run.folder).is_dir(), "The stage's folder is not there");
    Ok(Some((PathBuf::from(&run.folder), run.temp.clone())))
}

/// The stage's hand-off: where, what changed, the criteria and the report,
/// and how to report the verdict.
pub(crate) fn hand_off_text(team: &Team, task: &Task, run: &StageRun, prepared: &Prepared, report_host: &str) -> String {
    let pipeline = task.pipeline_run.as_ref();
    let max_rounds = pipeline.map(|run| run.max_rounds).unwrap_or_default();
    let implementer = checked_attempt(task, run).map(|attempt| team.role_of(&attempt.member)).unwrap_or_default();
    let mut text = format!(
        "[Zerus team \u{b7} from: Zerus \u{b7} task: #{} \u{b7} {} stage \u{b7} round {} of {max_rounds}]\nTeam: {}. Your role: {}.\n",
        task.id,
        run.kind,
        run.round,
        team.name,
        team.role_of(&run.member)
    );
    text += &if run.kind == "test" {
        format!("\nTest task #{}: {} by {implementer} (attempt {}). Run only the suites of the files it changed and take the criteria's shots.\n", task.id, task.title, run.attempt)
    } else {
        format!("\nReview task #{}: {} by {implementer} (attempt {}) against its criteria.\n", task.id, task.title, run.attempt)
    };
    if prepared.checkout.is_empty() {
        text += &format!("Folder: {} (the task's own folder; never change the task's files).\n", prepared.folder);
    } else {
        text += &format!("Folder: {} (a detached checkout of the attempt's commits for this test; Zerus removes it after the stage).\n", prepared.folder);
    }
    if prepared.holds {
        text += "The implementer's next task in this folder waits until your verdict.\n";
    }
    if !prepared.note.is_empty() {
        text += &format!("{}\n", prepared.note);
    }
    if run.kind == "test" {
        text += &format!(
            "Build and test there with hgs team run, naming this stage as the attempt ({}); the runs write only build output and your temporary folder {}. They end with the stage.\n",
            TEST_TEMPLATES.join(", "),
            prepared.temp
        );
    } else {
        text += "Read only: a review runs no command and changes nothing.\n";
    }
    if !prepared.changed.is_empty() {
        text += "\nWhat changed:\n";
        for item in &prepared.changed {
            text += &if let Some(sha) = item["commit"].as_str() {
                format!("- commit {} ({} files)\n", &sha[..sha.len().min(12)], item["files"])
            } else if item["deleted"] == true {
                format!("- {} (deleted)\n", item["path"].as_str().unwrap_or_default())
            } else {
                format!(
                    "- {} ({} bytes{})\n",
                    item["path"].as_str().unwrap_or_default(),
                    item["size"],
                    item["sha256"].as_str().map(|sha| format!(", sha256 {sha}")).unwrap_or_default()
                )
            };
        }
    }
    if !task.criteria.is_empty() {
        text += "\nAcceptance criteria:\n";
        for criterion in &task.criteria {
            text += &format!("- {criterion}\n");
        }
    }
    if let Some(report) = checked_attempt(task, run).and_then(|attempt| attempt.report.as_ref()) {
        let body: String = report.text.chars().take(4000).collect();
        text += &format!("\n{implementer}'s report:\n{body}\n");
    }
    let earlier = pipeline::rounds_text(task);
    if !earlier.is_empty() {
        text += &format!("\nEarlier rounds:\n{earlier}");
    }
    let at = if report_host.is_empty() { String::new() } else { format!("@{report_host} ") };
    let verdict = json!({"request_id":uuid::Uuid::new_v4().to_string(),"team":team.id,"task":task.id,"attempt":run.id,
        "verdict":"pass","text":"...","findings":[]});
    text += &format!(
        "\nReport once with your verdict: pass, or return with each finding in \"findings\" (it goes back to {implementer} as a new attempt):\n{verdict} | hgs {at}team report --json\n"
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::teams::catalog::{builtin, Catalog};
    use crate::state::teams::dispatcher::set_git_ceiling;
    use crate::state::teams::ops::{apply, create, Actor};
    use std::collections::BTreeSet;
    use std::process::Command;

    fn git(folder: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "user.name=Zerus Test", "-c", "user.email=test@example.com", "-c", "core.hooksPath=", "-c", "commit.gpgsign=false"])
            .arg("-C")
            .arg(folder)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn write(folder: &Path, file: &str, text: &str) {
        let path = folder.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The project: a Git repository with one commit, or a plain folder.
    fn project(dir: &Path, with_git: bool) -> PathBuf {
        let folder = dir.join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        write(&folder, "src/lib.rs", "pub fn one() {}\n");
        if with_git {
            git(&folder, &["init", "-q", "-b", "main"]);
            git(&folder, &["add", "-A"]);
            git(&folder, &["commit", "-q", "-m", "start"]);
        }
        folder
    }

    /// A running team in `folder`: dev implements #1 (and has #2 next), rev
    /// reviews, qa tests; #1 takes the preset `pipeline` of `catalog`. dev's
    /// folder is made as Zerus lays it out: a worktree or a clone of the
    /// project, or the project folder itself.
    fn team_in(folder: &Path, dev_folder: MemberFolder, catalog: &Catalog, pipeline: &str) -> (Team, PathBuf) {
        let (mut team, _) = create("5ea7f00d-0000-4000-8000-000000000532", &json!({"name":"T","lead":{"id":"lead","agent":"claude"}})).unwrap();
        let mut events = Vec::new();
        apply(
            &mut team,
            "plan",
            &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"},{"id":"rev","role":"Reviewer","agent":"claude"},
                    {"id":"qa","role":"Tester","agent":"kimi"}],
                "tasks":[{"id":1,"title":"API","assignee":"dev","criteria":["Paged"]},{"id":2,"title":"More","assignee":"dev"}]}),
            &Actor::Human,
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
        team.folder = folder.display().to_string();
        for member in &mut team.members {
            member.folder = if member.id == "dev" { dev_folder } else { MemberFolder::Shared };
        }
        team.task_mut(1).unwrap().pipeline = pipeline.into();
        pipeline::snapshot_with(&mut team, 1, catalog);
        let path = PathBuf::from(member_folder_path(&team, team.member("dev").unwrap()).unwrap());
        match dev_folder {
            MemberFolder::Worktree => {
                git(folder, &["worktree", "add", "-q", "-b", "dev", path.to_str().unwrap()]);
            }
            MemberFolder::Clone => {
                let output = Command::new("git").args(["clone", "-q"]).arg(folder).arg(&path).output().unwrap();
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            }
            MemberFolder::Shared => assert_eq!(path, folder),
        }
        (team, path)
    }

    /// dev reports #1 with its changed files and, when asked, a commit Zerus
    /// made for the attempt (B4).
    fn report(team: &mut Team, folder: &Path, commit: bool) {
        write(folder, "src/lib.rs", "pub fn one() {}\npub fn two() {}\n");
        write(folder, "docs/api.md", "# API\n");
        let mut commits = Vec::new();
        if commit {
            git(folder, &["add", "-A"]);
            git(folder, &["commit", "-q", "-m", "API"]);
            commits.push(CommitRef { sha: git(folder, &["rev-parse", "HEAD"]), at: 1.0, by: "Zerus".into(), files: 2 });
        }
        let task = team.task_mut(1).unwrap();
        task.attempts.push(Attempt { n: 1, id: "a1".into(), member: "dev".into(), state: AttemptState::Delivered, commits, ..Default::default() });
        task.status = TaskStatus::InProgress;
        let mut events = Vec::new();
        let value = apply(
            team,
            "report",
            &json!({"task":1,"attempt":"a1","text":"Done: two() added",
                "report":{"summary":"Done","files":["src/lib.rs","docs/api.md","old.txt"]}}),
            &Actor::Member("dev".into()),
            &BTreeSet::new(),
            &mut events,
        )
        .unwrap();
        assert_eq!(value["status"], "in_review", "{value}");
    }

    fn verdict(team: &mut Team, member: &str, run: &str) {
        let mut events = Vec::new();
        apply(team, "report", &json!({"task":1,"attempt":run,"verdict":"pass"}), &Actor::Member(member.into()), &BTreeSet::new(), &mut events).unwrap();
    }

    /// Prepares the open stage and gives it to `member`, as the dispatcher does.
    fn give(team: &mut Team, member: &str) -> (String, Prepared) {
        let prepared = prepare(team, 1).unwrap();
        let run = pipeline::give_stage(team, 1, member).unwrap();
        record(team, 1, &run, &prepared);
        (run, prepared)
    }

    fn run_of(team: &Team, id: &str) -> StageRun {
        team.task(1).unwrap().pipeline_run.as_ref().unwrap().runs.iter().find(|run| run.id == id).unwrap().clone()
    }

    fn isolated<T>(body: impl FnOnce(&Path) -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        let path = dunce(dir.path());
        set_git_ceiling(&path.display().to_string());
        let result = crate::state::with_test_state_root(&path.join("state"), || body(&path));
        drop(dir);
        result
    }

    /// The temp dir without Windows' \\?\ prefix, so Git takes it.
    fn dunce(path: &Path) -> PathBuf {
        let path = path.canonicalize().unwrap();
        PathBuf::from(path.display().to_string().trim_start_matches(r"\\?\"))
    }

    fn checkout_catalog() -> Catalog {
        let mut catalog = builtin();
        catalog.pipelines[0].folder = "checkout".into();
        catalog
    }

    #[test]
    fn a_worktree_task_is_reviewed_by_its_commits_and_its_test_holds_the_folder() {
        isolated(|dir| {
            let project = project(dir, true);
            let (mut team, worktree) = team_in(&project, MemberFolder::Worktree, &builtin(), "code");
            report(&mut team, &worktree, true);
            let sha = team.task(1).unwrap().attempts[0].commits[0].sha.clone();
            // The review reads the implementer's worktree and its commit.
            let (review, prepared) = give(&mut team, "rev");
            assert_eq!(PathBuf::from(&prepared.folder), worktree);
            assert_eq!(prepared.changed, [json!({"commit":sha,"files":2})]);
            assert!(!prepared.holds && prepared.checkout.is_empty() && prepared.temp.is_empty(), "a review holds nothing");
            let task = team.task(1).unwrap();
            let text = hand_off_text(&team, task, &run_of(&team, &review), &prepared, "");
            assert!(text.contains(&format!("Folder: {} (the task's own folder", worktree.display())), "{text}");
            assert!(text.contains(&format!("- commit {} (2 files)", &sha[..12])), "{text}");
            assert!(text.contains("Acceptance criteria:\n- Paged") && text.contains("Backend's report:\nDone: two() added"), "{text}");
            assert!(text.contains(&format!("\"attempt\":\"{review}\"")) && text.contains("Read only: a review runs no command"), "{text}");
            assert_eq!(held(&team, "dev"), None, "a review does not hold the folder");
            verdict(&mut team, "rev", &review);
            // The test holds the worktree from the moment it waits: dev's #2
            // waits and says so.
            assert_eq!(held(&team, "dev"), Some(1));
            let waits = crate::state::teams::inbox::waits(&team, team.task(2).unwrap(), true).unwrap();
            assert_eq!(waits["unblock"], "folder held by the test of #1", "{waits}");
            let (test, prepared) = give(&mut team, "qa");
            assert!(prepared.holds && Path::new(&prepared.folder) == worktree);
            assert!(Path::new(&prepared.temp).is_dir(), "the test's own temporary folder");
            assert_eq!(prepared.changed, [json!({"commit":sha,"files":2})], "the round's first stage found these");
            assert_eq!(held(&team, "dev"), Some(1));
            verdict(&mut team, "qa", &test);
            assert_eq!(held(&team, "dev"), None, "the hold ends with the test");
            // The ended test's temporary folder goes.
            let ended = run_of(&team, &test);
            assert_eq!(to_clean(&team), [(1, test.clone())]);
            clean_up(&ended).unwrap();
            assert!(!Path::new(&ended.temp).exists());
        });
    }

    #[test]
    fn a_clone_task_is_tested_on_a_detached_checkout_that_goes_after_the_stage() {
        isolated(|dir| {
            let project = project(dir, true);
            let (mut team, clone) = team_in(&project, MemberFolder::Clone, &checkout_catalog(), "code");
            report(&mut team, &clone, true);
            let sha = team.task(1).unwrap().attempts[0].commits[0].sha.clone();
            let (review, _) = give(&mut team, "rev");
            verdict(&mut team, "rev", &review);
            // The checkout rule: no hold, and dev's next task goes ahead.
            assert_eq!(held(&team, "dev"), None);
            let (test, prepared) = give(&mut team, "qa");
            let checkout = PathBuf::from(&prepared.checkout);
            assert!(checkout.join("src/lib.rs").is_file(), "{prepared:?}");
            assert_eq!(git(&checkout, &["rev-parse", "HEAD"]), sha);
            assert_eq!(PathBuf::from(&prepared.folder), checkout);
            assert_eq!(PathBuf::from(&prepared.checkout_repo), clone);
            assert!(!prepared.holds && held(&team, "dev").is_none());
            let text = hand_off_text(&team, team.task(1).unwrap(), &run_of(&team, &test), &prepared, "");
            assert!(text.contains("a detached checkout of the attempt's commits"), "{text}");
            verdict(&mut team, "qa", &test);
            let ended = run_of(&team, &test);
            clean_up(&ended).unwrap();
            assert!(!checkout.exists(), "the checkout is removed after the stage");
            assert!(!git(&clone, &["worktree", "list"]).contains(&checkout.display().to_string().replace('\\', "/")));
        });
    }

    #[test]
    fn a_shared_folder_task_lists_its_files_and_a_checkout_without_commits_holds() {
        isolated(|dir| {
            let project = project(dir, true);
            let (mut team, folder) = team_in(&project, MemberFolder::Shared, &checkout_catalog(), "code");
            assert_eq!(folder, project, "the members work in the shared folder");
            // No commit for the attempt: what changed is the reported files.
            report(&mut team, &folder, false);
            let (review, prepared) = give(&mut team, "rev");
            assert_eq!(PathBuf::from(&prepared.folder), project);
            let digest = |text: &str| format!("{:x}", Sha256::digest(text.as_bytes()))[..16].to_owned();
            assert_eq!(
                prepared.changed,
                [
                    json!({"path":"src/lib.rs","size":32,"sha256":digest("pub fn one() {}\npub fn two() {}\n")}),
                    json!({"path":"docs/api.md","size":6,"sha256":digest("# API\n")}),
                    json!({"path":"old.txt","deleted":true}),
                ]
            );
            let text = hand_off_text(&team, team.task(1).unwrap(), &run_of(&team, &review), &prepared, "");
            assert!(text.contains("- src/lib.rs (32 bytes, sha256 ") && text.contains("- old.txt (deleted)"), "{text}");
            verdict(&mut team, "rev", &review);
            let (_, prepared) = give(&mut team, "qa");
            assert!(prepared.holds && prepared.checkout.is_empty());
            assert!(prepared.note.contains("made no commits"), "{prepared:?}");
            assert_eq!(held(&team, "dev"), Some(1), "the implementer's next task in the shared folder waits");
            assert_eq!(held(&team, "rev"), None, "other members keep working there");
        });
    }

    #[test]
    fn a_folder_without_git_works_and_git_is_never_initialized() {
        isolated(|dir| {
            let project = project(dir, false);
            let (mut team, folder) = team_in(&project, MemberFolder::Shared, &checkout_catalog(), "code");
            assert!(!is_git(&folder.display().to_string()));
            report(&mut team, &folder, false);
            let (review, prepared) = give(&mut team, "rev");
            assert_eq!(prepared.changed.len(), 3);
            assert_eq!(prepared.changed[1]["path"], "docs/api.md");
            verdict(&mut team, "rev", &review);
            let (test, prepared) = give(&mut team, "qa");
            assert!(prepared.holds && prepared.note.contains("has no Git"), "{prepared:?}");
            verdict(&mut team, "qa", &test);
            clean_up(&run_of(&team, &test)).unwrap();
            assert!(!folder.join(".git").exists(), "Zerus never initializes Git");
        });
    }

    #[test]
    fn stage_permissions_follow_the_stage_and_end_with_it() {
        isolated(|dir| {
            let project = project(dir, false);
            let (mut team, folder) = team_in(&project, MemberFolder::Shared, &builtin(), "code");
            report(&mut team, &folder, false);
            let (review, _) = give(&mut team, "rev");
            // A review reads: it runs nothing.
            let error = run_folder(&team, "rev", &review, "cargo-test").unwrap_err().to_string();
            assert!(error.contains("A review stage reads"), "{error}");
            verdict(&mut team, "rev", &review);
            let (test, prepared) = give(&mut team, "qa");
            // A test builds and tests in the task's folder with its own temp
            // folder; it never merges or commits there.
            let (at, temp) = run_folder(&team, "qa", &test, "cargo-test").unwrap().unwrap();
            assert_eq!((at, temp.clone()), (folder.clone(), prepared.temp.clone()));
            for template in ["git-commit", "git-merge-lead", "project-script"] {
                assert!(run_folder(&team, "qa", &test, template).is_err(), "{template}");
            }
            // Only its own member, and the implementer never.
            assert_eq!(run_folder(&team, "dev", &test, "cargo-test").unwrap(), None);
            assert_eq!(run_folder(&team, "rev", &test, "cargo-test").unwrap(), None);
            // The stage ends: the same request is no longer a stage's.
            verdict(&mut team, "qa", &test);
            assert_eq!(run_folder(&team, "qa", &test, "cargo-test").unwrap(), None);
            assert_eq!(pipeline::open_run_task(&team, &test, "qa"), None);
            clean_up(&run_of(&team, &test)).unwrap();
            assert!(!Path::new(&temp).exists(), "its temporary folder goes with it");
        });
    }

    #[test]
    fn the_stage_queue_lists_the_given_stage_then_the_waiting_ones() {
        isolated(|dir| {
            let project = project(dir, false);
            let (mut team, folder) = team_in(&project, MemberFolder::Shared, &builtin(), "code");
            report(&mut team, &folder, false);
            assert_eq!(queue(&team, "rev").len(), 1, "a waiting review it may take");
            assert!(queue(&team, "dev").is_empty() && queue(&team, "qa").is_empty());
            assert_eq!(waiting(&team), [(1, vec!["rev".to_owned()])]);
            assert!(busy(&team).is_empty());
            give(&mut team, "rev");
            let queue = queue(&team, "rev");
            assert_eq!((queue[0]["task"].clone(), queue[0]["stage"].clone(), queue[0]["state"].clone()), (json!(1), json!("review"), json!("delivered")));
            assert_eq!(busy(&team), ["rev"]);
            assert!(waiting(&team).is_empty());
        });
    }
}
