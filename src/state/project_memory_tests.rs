// Task 126: project memory settings, status, the verified move and a team's memory.
use super::*;

const PROJECT: &str = "proj-1";

fn request(n: u32) -> String {
    format!("11111111-1111-4111-8111-{n:012}")
}

fn set_input(n: u32, revision: u64, extra: Value) -> Value {
    let mut input = json!({"by":"human","project":PROJECT,"request_id":request(n),"expected_revision":revision});
    for (key, value) in extra.as_object().unwrap() {
        input[key] = value.clone();
    }
    input
}

/// A temporary folder git treats as outside any working tree.
fn isolated() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    // Git starts at the test's folder and never goes into its parent.
    TEST_CEILING.with(|ceiling| *ceiling.borrow_mut() = dir.path().parent().map(Path::to_path_buf));
    dir
}

fn write_records(store: &Path) {
    fs::create_dir_all(store.join("lore/plans")).unwrap();
    fs::write(store.join("lore/decisions.jsonl"), "{\"id\":\"D-1\"}\n{\"id\":\"D-2\"}\n\n").unwrap();
    fs::write(store.join("lore/plans/p.jsonl"), "{\"id\":\"P-1\"}\n").unwrap();
}

/// What RustCodeLore does after a write in a private store: one commit.
fn commit_records(store: &Path) {
    git(store, &["add", "-A"]).unwrap();
    git(store, &["commit", "-q", "-m", "lore: add D-1, D-2, P-1"]).unwrap();
}

fn mode_of(store: &Path) -> String {
    let text = fs::read_to_string(store.join("lore.toml")).unwrap();
    text.lines().find(|line| line.starts_with("mode")).unwrap().split('"').nth(1).unwrap().to_owned()
}

/// A project's own repository, as a folder the owner might choose.
fn project_repo(dir: &Path) -> PathBuf {
    let repo = dir.join("project");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    git(&repo, &["config", "user.name", "T"]).unwrap();
    git(&repo, &["config", "user.email", "t@example.com"]).unwrap();
    fs::write(repo.join("README.md"), "project\n").unwrap();
    git(&repo, &["add", "-A"]).unwrap();
    git(&repo, &["commit", "-q", "-m", "init"]).unwrap();
    repo
}

#[test]
fn memory_is_off_until_the_owner_turns_it_on_and_off_keeps_the_store() {
    let dir = isolated();
    with_test_state_root(dir.path(), || {
        let view = run("get", &json!({"project":PROJECT})).unwrap();
        assert_eq!((view["enabled"].as_bool(), view["default"].as_bool(), view["revision"].as_u64()), (Some(false), Some(true), Some(0)));
        let store = default_store(PROJECT);
        assert!(store.starts_with(root()) && store.ends_with(Path::new("lore/projects/proj-1")), "{}", store.display());
        assert_eq!(view["status"]["exists"], false);
        assert!(!store.exists(), "reading creates nothing");
        // The owner only: an agent's or an unmarked write is refused.
        assert!(run("set", &json!({"project":PROJECT,"request_id":request(1),"expected_revision":0,"enabled":true})).is_err());
        // On: the default store is its own private repository (private.rs),
        // plain commits, a local identity, the marker and a first commit.
        let input = set_input(1, 0, json!({"enabled":true}));
        let result = run("set", &input).unwrap();
        assert_eq!((result["status"].as_str(), result["view"]["enabled"].as_bool(), result["view"]["revision"].as_u64()), (Some("updated"), Some(true), Some(1)));
        assert_eq!(mode_of(&store), "plain");
        assert!(store.join("lore/plans").is_dir() && store.join(".git").join(STORE_MARKER).is_file());
        assert!(same(&PathBuf::from(git(&store, &["rev-parse", "--show-toplevel"]).unwrap()), &store), "the store is the repository's top");
        assert_eq!(git(&store, &["log", "--format=%s|%an|%ae"]).unwrap(), "lore: a new store|Zerus|lore@zerus.invalid");
        assert_eq!(git(&store, &["status", "--porcelain"]).unwrap(), "", "everything Zerus wrote is committed");
        assert_eq!(git(&store, &["config", "user.email"]).unwrap(), STORE_EMAIL);
        assert_eq!(git(&store, &["remote"]).unwrap(), "", "no remote");
        // The same request replays; reused for something else, or stale, it is refused.
        assert_eq!(run("set", &input).unwrap()["view"]["revision"], 1);
        assert!(run("set", &set_input(1, 0, json!({"enabled":false}))).unwrap_err().to_string().contains("different change"));
        assert!(run("set", &set_input(2, 0, json!({"enabled":false}))).unwrap_err().to_string().contains("read them again"));
        // Status counts records (never git's files), files and bytes, and the index state.
        write_records(&store);
        commit_records(&store);
        let status = get(PROJECT).unwrap()["status"].clone();
        assert_eq!((status["records"].as_u64(), status["index"].as_str()), (Some(3), Some("none")));
        assert!(status["files"].as_u64().unwrap() > 3 && status["bytes"].as_u64().unwrap() > 0);
        fs::create_dir_all(store.join(".cache")).unwrap();
        fs::write(store.join(".cache/index.sqlite"), "index").unwrap();
        assert_eq!(get(PROJECT).unwrap()["status"]["index"], "built");
        assert_eq!(git(&store, &["status", "--porcelain"]).unwrap(), "", "the index stays out of the history");
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        fs::File::options().write(true).open(store.join(".cache/index.sqlite")).unwrap().set_modified(old).unwrap();
        assert_eq!(get(PROJECT).unwrap()["status"]["index"], "stale", "records written after the index");
        // Off keeps the store, its records and its history.
        let off = run("set", &set_input(3, 1, json!({"enabled":false}))).unwrap();
        assert_eq!((off["view"]["enabled"].as_bool(), off["view"]["status"]["records"].as_u64()), (Some(false), Some(3)));
        assert!(store.join("lore/decisions.jsonl").exists());
        assert_eq!(git(&store, &["rev-list", "--count", "HEAD"]).unwrap(), "2");
    });
}

#[test]
fn a_store_with_records_moves_verified_with_its_history_and_the_old_one_stays() {
    let dir = isolated();
    with_test_state_root(dir.path(), || {
        run("set", &set_input(1, 0, json!({"enabled":true}))).unwrap();
        let old = default_store(PROJECT);
        write_records(&old);
        commit_records(&old);
        let head = git(&old, &["rev-parse", "HEAD"]).unwrap();
        let target = dir.path().join("elsewhere").join("memory");
        // Choosing another folder for a store with records is refused: move it.
        let error = run("set", &set_input(2, 1, json!({"store":target.display().to_string()}))).unwrap_err().to_string();
        assert!(error.contains("holds 3 records") && error.contains("move it instead"), "{error}");
        // A relative folder, a non-empty one, or one inside the store is refused.
        assert!(run("move", &set_input(3, 1, json!({"to":"relative/store"}))).is_err());
        let busy = dir.path().join("busy");
        fs::create_dir_all(&busy).unwrap();
        fs::write(busy.join("other.txt"), "x").unwrap();
        assert!(run("move", &set_input(4, 1, json!({"to":busy.display().to_string()}))).unwrap_err().to_string().contains("not empty"));
        assert_eq!(fs::read_to_string(busy.join("other.txt")).unwrap(), "x", "a refused move leaves the folder alone");
        assert!(run("move", &set_input(5, 1, json!({"to":old.join("inner").display().to_string()}))).is_err());
        assert_eq!(get(PROJECT).unwrap()["revision"], 1, "nothing switched");
        // The move copies the store with its repository, verifies and switches;
        // the old store is kept.
        let moved = run("move", &set_input(6, 1, json!({"to":target.display().to_string()}))).unwrap();
        assert_eq!((moved["status"].as_str(), moved["old_kept"].as_bool()), (Some("moved"), Some(true)));
        assert!(moved["files"].as_u64().unwrap() > 3, "git's files come along");
        let view = &moved["view"];
        assert_eq!((view["store"].as_str(), view["default"].as_bool()), (Some(target.display().to_string().as_str()), Some(false)));
        assert_eq!(view["status"]["records"], 3);
        assert_eq!(view["moved_from"], json!([old.display().to_string()]));
        for file in ["lore.toml", "lore/decisions.jsonl", "lore/plans/p.jsonl"] {
            assert_eq!(fs::read(old.join(file)).unwrap(), fs::read(target.join(file)).unwrap(), "{file}");
        }
        assert_eq!(git(&target, &["rev-parse", "HEAD"]).unwrap(), head, "the history moved with it");
        assert!(is_private(&target) && mode_of(&target) == "plain");
        assert_eq!(git(&target, &["status", "--porcelain"]).unwrap(), "");
        // Back to an empty folder: allowed while that store is empty, never silently.
        assert!(run("set", &set_input(7, 2, json!({"store":""}))).unwrap_err().to_string().contains("move it instead"));
    });
}

#[test]
fn a_store_in_a_project_working_tree_is_never_initialized_and_takes_shared_tree() {
    let dir = isolated();
    with_test_state_root(dir.path(), || {
        let repo = project_repo(dir.path());
        let project_head = git(&repo, &["rev-parse", "HEAD"]).unwrap();
        // An empty store pointed into the project: no git init, shared-tree.
        run("set", &set_input(1, 0, json!({"store":repo.join("memory").display().to_string()}))).unwrap();
        run("set", &set_input(2, 1, json!({"enabled":true}))).unwrap();
        let chosen = repo.join("memory");
        assert_eq!(mode_of(&chosen), "shared-tree");
        assert!(!chosen.join(".git").exists());
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), project_head, "Zerus never commits in a project");
        // Back to the default, a private store, then moved into the project:
        // the copy leaves its repository behind and takes shared-tree.
        run("set", &set_input(3, 2, json!({"store":""}))).unwrap();
        let store = default_store(PROJECT);
        write_records(&store);
        commit_records(&store);
        let target = repo.join("docs").join("lore-store");
        let moved = run("move", &set_input(4, 3, json!({"to":target.display().to_string()}))).unwrap();
        assert_eq!(moved["view"]["status"]["records"], 3);
        assert!(!target.join(".git").exists() && mode_of(&target) == "shared-tree");
        assert!(store.join(".git").exists(), "the old store keeps its repository");
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]).unwrap(), project_head);
        // And out again, outside any tree: it becomes a private store again.
        let outside = dir.path().join("outside");
        run("move", &set_input(5, 4, json!({"to":outside.display().to_string()}))).unwrap();
        assert!(is_private(&outside) && mode_of(&outside) == "plain");
        assert_eq!(git(&outside, &["log", "--format=%s"]).unwrap(), "lore: the store moved here");
        assert_eq!(git(&outside, &["status", "--porcelain"]).unwrap(), "");
        // A non-empty folder outside any tree is never made a store.
        let busy = dir.path().join("busy");
        fs::create_dir_all(&busy).unwrap();
        fs::write(busy.join("x.txt"), "x").unwrap();
        assert!(init_store(&busy).unwrap_err().to_string().contains("not empty"));
        assert!(!busy.join(".git").exists());
    });
}

#[test]
fn a_failed_copy_removes_only_its_own_copy() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("from");
    write_records(&from);
    let to = dir.path().join("to");
    let (files, bytes) = copy_verified(&from, &to, true).unwrap();
    assert_eq!(files, 2);
    assert_eq!(bytes, fs::metadata(from.join("lore/decisions.jsonl")).unwrap().len() + fs::metadata(from.join("lore/plans/p.jsonl")).unwrap().len());
    // A target that is not empty is refused before anything is copied.
    assert!(copy_verified(&from, &to, true).is_err());
    assert!(from.join("lore/decisions.jsonl").exists());
}

fn memory(mode: &str) -> teams::model::TeamMemory {
    teams::model::TeamMemory { mode: mode.into(), ..Default::default() }
}

/// Task 127 (docs/lore.md 4.1): the five modes, their defaults and where each
/// one keeps the team's memory, with the project's store off and on.
#[test]
fn a_team_gets_the_projects_workspace_or_its_chosen_mode() {
    let dir = isolated();
    with_test_state_root(dir.path(), || {
        let team = "abcdef12-0000-4000-8000-000000000000";
        // The project is off: team (the default), plain and off are offered.
        assert_eq!(modes_for(PROJECT), (vec!["team", "plain", "off"], "team"));
        let own = team_memory(PROJECT, team, Some(&memory("team"))).unwrap().unwrap();
        assert_eq!((own.mode.as_str(), own.source.as_str()), ("team", "team"));
        assert_eq!(own.store, team_store("abcdef12").display().to_string());
        assert!(is_private(Path::new(&own.store)) && mode_of(Path::new(&own.store)) == "plain");
        // Task 126's `lore` is the default RustCodeLore mode.
        assert_eq!(team_memory(PROJECT, team, Some(&memory("lore"))).unwrap().unwrap().mode, "team");
        assert_eq!(team_memory(PROJECT, team, Some(&memory("plain"))).unwrap().unwrap(), memory("plain"));
        assert_eq!(team_memory(PROJECT, team, Some(&memory("off"))).unwrap().unwrap(), memory("off"));
        assert_eq!(team_memory(PROJECT, team, None).unwrap(), None, "without a choice: plain, as before");
        assert_eq!(team_memory("", team, None).unwrap(), None);
        for unavailable in ["project", "read-only"] {
            let error = team_memory(PROJECT, team, Some(&memory(unavailable))).unwrap_err().to_string();
            assert!(error.contains("The project's memory is off"), "{error}");
        }
        // The project is on: project is preselected and every mode offered.
        run("set", &set_input(1, 0, json!({"enabled":true}))).unwrap();
        assert_eq!(modes_for(PROJECT), (vec!["project", "team", "read-only", "plain", "off"], "project"));
        let workspace = default_store(PROJECT).join("teams").join("abcdef12");
        for (chosen, mode) in [(None, "project"), (Some(memory("lore")), "project"), (Some(memory("project")), "project"), (Some(memory("team")), "team")] {
            let found = team_memory(PROJECT, team, chosen.as_ref()).unwrap().unwrap();
            assert_eq!((found.mode.as_str(), found.source.as_str()), (mode, "project"));
            assert_eq!(found.store, default_store(PROJECT).display().to_string());
            assert_eq!(found.workspace, workspace.display().to_string());
            assert!(workspace.join("lore/plans").is_dir() && mode_of(&workspace) == "plain");
            assert!(!workspace.join(".git").exists(), "a workspace is part of the store's repository");
        }
        // Read only: the project's store and the workspace, nothing new.
        let read_only = team_memory(PROJECT, team, Some(&memory("read-only"))).unwrap().unwrap();
        assert_eq!((read_only.mode.as_str(), read_only.workspace.as_str()), ("read-only", workspace.display().to_string().as_str()));
        // The owner may turn the workspace off: plain and off are kept.
        assert_eq!(team_memory(PROJECT, team, Some(&memory("plain"))).unwrap().unwrap(), memory("plain"));
        let store = default_store(PROJECT);
        assert_eq!(git(&store, &["log", "--format=%s"]).unwrap(), "lore: a new team workspace\nlore: a new store");
        assert_eq!(git(&store, &["status", "--porcelain"]).unwrap(), "");
    });
}

#[test]
fn a_draft_created_in_a_project_with_memory_gets_its_workspace() {
    let dir = isolated();
    with_test_state_root(dir.path(), || {
        run("set", &set_input(1, 0, json!({"enabled":true}))).unwrap();
        let create = |mode: &str| {
            let (mut team, _) = teams::ops::create("abcdef12-0000-4000-8000-000000000000",
                &json!({"name":"T","lead":{"id":"lead","agent":"claude"},"project_id":PROJECT,"settings":{"memory":{"mode":mode}}})).unwrap();
            team.status = teams::model::TeamStatus::Draft;
            team.draft = Some(Default::default());
            team.tasks.push(teams::model::Task { id: 1, title: "A".into(), assignee: "lead".into(), status: teams::model::TaskStatus::Draft, phase: 1, ..Default::default() });
            team
        };
        // Opening and drafting create nothing; Create team creates the workspace.
        let mut team = create("project");
        assert!(!default_store(PROJECT).join("teams").join("abcdef12").exists(), "a draft creates no workspace");
        teams::ops::apply(&mut team, "draft", &json!({"action":"create"}), &teams::ops::Actor::Human, &Default::default(), &mut Vec::new()).unwrap();
        let memory = team.settings.memory.clone().unwrap();
        assert_eq!((memory.mode.as_str(), memory.source.as_str()), ("project", "project"));
        assert!(Path::new(&memory.workspace).join("lore.toml").exists());
        // The owner turned the workspace off: plain memory, no store.
        let mut plain = create("plain");
        teams::ops::apply(&mut plain, "draft", &json!({"action":"create"}), &teams::ops::Actor::Human, &Default::default(), &mut Vec::new()).unwrap();
        assert_eq!(plain.settings.memory.clone().unwrap().mode, "plain");
        // A mode outside the five (and task 126's lore) is refused by name.
        let error = teams::ops::create("t", &json!({"name":"T","lead":{"id":"lead","agent":"claude"},"settings":{"memory":{"mode":"vector"}}})).unwrap_err();
        assert!(format!("{error:#}").contains("plain, off, team, project, read-only"), "{error:#}");
    });
}

/// Task 127 (docs/lore.md 4.2): the owner switches a running team's memory
/// mode; the lead proposes it on a card, which stores the offer and creates
/// nothing until Allow.
#[test]
fn the_memory_mode_switches_by_the_owner_or_the_leads_card() {
    let dir = isolated();
    with_test_state_root(dir.path(), || {
        let id = "abcdef12-0000-4000-8000-000000000000";
        let (mut team, _) = teams::ops::create(id, &json!({"name":"T","lead":{"id":"lead","agent":"claude"},"project_id":PROJECT,
            "settings":{"memory":{"mode":"plain"}}})).unwrap();
        let none = Default::default();
        let mut events = Vec::new();
        let lead = teams::ops::Actor::Member("lead".into());
        // The lead cannot switch it itself; the refusal names the card.
        let error = teams::ops::apply(&mut team, "switch-memory-mode", &json!({"mode":"team"}), &lead, &none, &mut events).unwrap_err().to_string();
        assert!(error.contains("\"kind\":\"switch-memory-mode\""), "{error}");
        // The lead's card stores the offer and creates no store.
        let proposed = teams::ops::apply(&mut team, "propose", &json!({"kind":"switch-memory-mode","mode":"team","reason":"keep the faults"}), &lead, &none, &mut events).unwrap();
        assert_eq!(proposed["status"], "proposed", "{proposed}");
        assert_eq!((team.memory_mode_offer.as_str(), team.memory_mode_reason.as_str()), ("team", "keep the faults"));
        assert!(!team_store("abcdef12").exists(), "trying the card touches no store");
        // A mode the project does not allow is refused on the card already.
        let error = teams::ops::apply(&mut team, "propose", &json!({"kind":"switch-memory-mode","mode":"project","reason":"x"}), &lead, &none, &mut events).unwrap_err();
        assert!(format!("{error:#}").contains("The project's memory is off"), "{error:#}");
        // The owner switches: the team's own store is created, one event.
        let switched = teams::ops::apply(&mut team, "switch-memory-mode", &json!({"mode":"team","reason":"keep the faults"}), &teams::ops::Actor::Human, &none, &mut events).unwrap();
        assert_eq!((switched["from"].as_str(), switched["to"].as_str()), (Some("plain"), Some("team")));
        let memory = team.settings.memory.clone().unwrap();
        assert!(is_private(Path::new(&memory.workspace)));
        assert!(team.memory_mode_offer.is_empty());
        assert!(events.iter().any(|event| event.kind == "memory_mode_changed" && event.payload["to"] == "team"));
        // Back to plain keeps the workspace on disk and named.
        teams::ops::apply(&mut team, "switch-memory-mode", &json!({"mode":"plain"}), &teams::ops::Actor::Human, &none, &mut events).unwrap();
        let kept = team.settings.memory.clone().unwrap();
        assert_eq!((kept.mode.as_str(), kept.workspace.as_str()), ("plain", memory.workspace.as_str()));
        assert!(Path::new(&memory.workspace).join("lore.toml").exists());
        // A denied card clears the offer and records the reason.
        teams::ops::apply(&mut team, "propose", &json!({"kind":"switch-memory-mode","mode":"off","reason":"no digest"}), &lead, &none, &mut events).unwrap();
        let card = team.proposals.last().unwrap().clone();
        teams::ops::apply(&mut team, "decide-proposal", &json!({"proposal":card["id"],"expected_hash":card["hash"],"accept":false,"reason":"keep it"}),
            &teams::ops::Actor::Human, &none, &mut events).unwrap();
        assert!(team.memory_mode_offer.is_empty());
        assert!(events.iter().any(|event| event.kind == "memory_mode_denied"));
    });
}

/// Task #397: a hit of the project's search names the store that holds it: the
/// project itself, or the team whose workspace is `teams/<short id>`.
#[test]
fn a_search_hit_names_its_store() {
    assert_eq!(find_store_label("."), "project");
    assert_eq!(find_store_label("./"), "project");
    assert_eq!(find_store_label(""), "project");
    let team = find_store_label("teams/33fbafeb");
    assert!(team.starts_with("team ") && !team.contains('/'), "{team}");
    // A record deeper in the team's workspace belongs to the same team.
    assert_eq!(find_store_label("teams/33fbafeb/records"), team);
    // Anything else keeps its own name.
    assert_eq!(find_store_label("elsewhere"), "elsewhere");
}

/// The template a new store gets: lore's built-in multilingual embedder, so
/// Russian reads as well as English (no English-only nomic-embed-text).
#[test]
fn a_new_store_embeds_with_the_builtin_multilingual_model() {
    let text = lore_toml("plain");
    let document = text.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["embed"]["provider"].as_str(), Some("builtin"));
    assert_eq!(document["embed"]["model"].as_str(), Some("auto"));
    assert!(!text.contains("nomic") && !text.contains("ollama"), "{text}");
    assert_eq!(areas_of(&text), Some(Vec::new()));
}

/// RogueDOD: a lore.toml with real areas at the git root is the project's
/// store, found from a subfolder; the placeholder area, a subfolder's own
/// lore.toml and a private Zerus store are not.
#[test]
fn a_repository_store_is_the_git_roots_lore_toml_with_real_areas() {
    let dir = isolated();
    let repo = project_repo(dir.path());
    let sub = repo.join("Game").join("Docs");
    fs::create_dir_all(&sub).unwrap();
    assert_eq!(repo_store(&sub), None, "no lore.toml");
    fs::write(repo.join("Game").join("lore.toml"), "version = 1\nareas = [\"game\"]\n").unwrap();
    assert_eq!(repo_store(&sub), None, "a lore.toml in a subfolder is not a lore root");
    fs::write(repo.join("lore.toml"), "version = 1\nareas = [\"use-the-project-store\"]\n").unwrap();
    assert_eq!(repo_store(&sub), None, "the placeholder keeps no records");
    fs::write(repo.join("lore.toml"), "version = 1\nareas = [\n  \"game\",\n  \"docs\",\n]\n\n[commit]\nmode = \"shared-tree\"\nbranch = \"develop\"\n").unwrap();
    let found = repo_store(&sub).expect("real areas make it the store");
    assert!(same(&found, &repo), "{} is {}", found.display(), repo.display());
    assert_eq!(areas_of(&fs::read_to_string(repo.join("lore.toml")).unwrap()), Some(vec!["game".to_owned(), "docs".to_owned()]));
    assert!(repo_store_of(["", sub.to_str().unwrap()]).is_some());
    // A private store Zerus made is never a project's repository.
    let private = dir.path().join("store");
    init_store(&private).unwrap();
    fs::write(private.join("lore.toml"), "version = 1\nareas = [\"team-x\"]\n").unwrap();
    assert_eq!(repo_store(&private), None);
}

/// A repository whose own agent hooks run `lore digest` is not given a second one.
#[test]
fn a_repository_with_its_own_digest_hook_gets_no_second_digest() {
    let dir = isolated();
    let repo = project_repo(dir.path());
    fs::write(repo.join("lore.toml"), "version = 1\nareas = [\"game\"]\n").unwrap();
    assert!(!repo_has_hook(&repo, &START_HOOKS));
    fs::create_dir_all(repo.join(".claude")).unwrap();
    fs::write(repo.join(".claude/settings.json"), r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"lore digest"}]}]}}"#).unwrap();
    assert!(repo_has_hook(&repo, &START_HOOKS) && !repo_has_hook(&repo, &EDIT_HOOKS));
    assert_eq!(session_digest(&repo, "", 2000), None);
    assert!(session_store(&repo, "", &EDIT_HOOKS).is_some(), "the edit hook is still Zerus's");
}

/// With a real lore (HGS_LORE or beside hgs or on PATH), the pre-edit hook
/// names a live fault about the file once per session; without one it is
/// silent. Skipped where no lore is installed.
#[test]
fn the_pre_edit_hook_names_a_live_fault_once_per_session() {
    let Some(_) = crate::state::teams::lore_sync::lore_bin() else { return };
    let dir = isolated();
    let repo = project_repo(dir.path());
    fs::write(repo.join("lore.toml"), "version = 1\ndir = \"lore\"\nareas = [\"game\"]\n\n[commit]\nmode = \"none\"\n\n[embed]\nprovider = \"none\"\n").unwrap();
    let fault = r#"{"area":"game","title":"The import is blind to a nested name","symptom":"a nested name is lost","signals":["missing name"],"cause":"top level only","fix":"read nested names","check":"cargo test import","status":"live","refs":{"files":["README.md"]}}"#;
    run_hook_lore(&repo, &["--text-only", "--by", "T", "add", "fault", "--json"], fault.as_bytes(), 64 * 1024).expect("lore adds the fault");
    let session = format!("zerus-test-{}", std::process::id());
    let event = json!({"hook_event_name":"PreToolUse","session_id":session,"tool_name":"Edit","cwd":repo.display().to_string(),
        "tool_input":{"file_path":repo.join("README.md").display().to_string()}});
    let Some(text) = pre_edit_context(&repo, &event) else { panic!("lore said nothing about README.md") };
    assert!(text.contains("The import is blind") && text.contains("README.md"), "{text}");
    assert_eq!(pre_edit_context(&repo, &event), None, "once per file and session");
    let digest = digest_text(&repo, 2000).unwrap_or_default();
    assert!(digest.contains("The import is blind"), "{digest}");
}
