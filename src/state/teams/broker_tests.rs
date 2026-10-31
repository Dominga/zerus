use super::*;
use crate::state::teams::model::{Member, SessionRef, Team, TeamStatus};
use crate::state::teams::ops::{self, Actor};
use std::collections::BTreeSet;

fn run_git(folder: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "core.autocrlf=false", "-c", "commit.gpgsign=false"])
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A shared folder with sources, a big file and an asset folder, committed.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("Content").join("Maps")).unwrap();
    fs::write(root.join("src").join("a.c"), "int a(void) { return 1; }\n").unwrap();
    fs::write(root.join("src").join("b.c"), "int b(void) { return 2; }\n").unwrap();
    fs::write(root.join("big.bin"), vec![7u8; 4096]).unwrap();
    fs::write(root.join("Content").join("Maps").join("level.umap"), vec![1u8; 8192]).unwrap();
    run_git(root, &["init", "-q"]);
    run_git(root, &["add", "-A"]);
    run_git(root, &["commit", "-q", "-m", "base"]);
    dir
}

fn small_config() -> Config {
    Config { assets: vec!["Content".into()], copy_limit: 1024, ..Config::default() }
}

fn team_in(folder: &Path) -> Team {
    let session = |name: &str| Some(SessionRef { name: format!("codex/x/{name}"), run_id: "r".into(), conversation_id: "c".into(), ..Default::default() });
    let member = |id: &str, role: &str| Member { id: id.into(), role: role.into(), agent: "codex".into(), session: session(id), directory: folder.display().to_string(), ..Default::default() };
    Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        folder: folder.display().to_string(),
        status: TeamStatus::Running,
        members: vec![member("lead", "Lead"), member("dev", "Backend"), member("dev2", "Gameplay")],
        ..Default::default()
    }
}

#[test]
fn off_by_default_and_configured_by_the_tests_file() {
    let dir = project();
    let mut team = team_in(dir.path());
    // Off: no build goes through the broker, whatever the project says.
    assert!(applies(&team, "cargo-build", dir.path()).is_none());
    team.settings.build_broker = true;
    assert!(applies(&team, "cargo-build", dir.path()).is_some());
    assert!(applies(&team, "git-commit", dir.path()).is_none(), "only build templates");
    fs::create_dir_all(dir.path().join(".zerus")).unwrap();
    fs::write(dir.path().join(".zerus").join("tests.toml"),
              "[build_broker]\ntemplates = [\"make\"]\nassets = [\"Content\", \"../outside\"]\nlink = [\"ThirdParty\"]\ncopy_limit_mb = 2\n").unwrap();
    let config = config(dir.path());
    assert_eq!((config.templates.clone(), config.assets.clone(), config.link.clone(), config.copy_limit),
               (vec!["make".to_owned()], vec!["Content".to_owned()], vec!["ThirdParty".to_owned()], 2 * 1024 * 1024));
    assert!(applies(&team, "make", dir.path()).is_some() && applies(&team, "cargo-build", dir.path()).is_none());
    fs::write(dir.path().join(".zerus").join("tests.toml"), "[build_broker]\nenabled = false\n").unwrap();
    assert!(applies(&team, "cargo-build", dir.path()).is_none(), "the project's veto");
    // The setting is the owner's: the lead's settings change is refused.
    let mut events = Vec::new();
    let error = ops::apply(&mut team_in(dir.path()), "settings", &json!({"settings":{"build_broker":true}}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("build_broker"), "{error}");
}

#[test]
fn a_snapshot_has_its_own_edits_others_at_head_and_no_asset_copies() {
    let dir = project();
    let root = dir.path();
    // dev changes a.c; dev2 is half-way through b.c.
    fs::write(root.join("src").join("a.c"), "int a(void) { return 10; }\n").unwrap();
    fs::write(root.join("src").join("b.c"), "int b(void) { return 2 +").unwrap();
    let others = BTreeMap::from([("src/b.c".to_owned(), "dev2".to_owned())]);
    let snapshot = sync(root, "dev", &["src/a.c".into()], &others, &small_config()).unwrap();
    let tree = &snapshot.tree;
    assert!(tree.starts_with(root.join(".zerus-team").join("runs")));
    assert_eq!(fs::read_to_string(tree.join("src").join("a.c")).unwrap(), "int a(void) { return 10; }\n", "its own edit");
    assert_eq!(fs::read_to_string(tree.join("src").join("b.c")).unwrap(), "int b(void) { return 2; }\n", "the other member's file at HEAD");
    assert_eq!(snapshot.others.get("src/b.c").map(String::as_str), Some("dev2"));
    // The big file is a copy (never a hard link) and the asset folder one link.
    assert_eq!(fs::read(tree.join("big.bin")).unwrap().len(), 4096);
    assert_eq!(snapshot.large, 1, "named as large in the note");
    let assets = fs::symlink_metadata(tree.join("Content")).unwrap();
    assert!(assets.file_type().is_symlink() || fs::read_link(tree.join("Content")).is_ok(), "Content is a link");
    assert_eq!(fs::read(tree.join("Content").join("Maps").join("level.umap")).unwrap().len(), 8192);
    assert_eq!(snapshot.linked, 1);
    assert!(snapshot.known);
    // A write in the shared folder never reaches the snapshot's copy, nor the
    // other way round.
    fs::write(root.join("big.bin"), vec![9u8; 4096]).unwrap();
    assert_eq!(fs::read(tree.join("big.bin")).unwrap()[0], 7, "the snapshot keeps its own data");
    fs::write(root.join("big.bin"), vec![7u8; 4096]).unwrap();
    fs::write(tree.join("big.bin"), vec![3u8; 4096]).unwrap();
    assert_eq!(fs::read(root.join("big.bin")).unwrap()[0], 7, "the shared folder keeps its data");
    fs::write(tree.join("big.bin"), vec![7u8; 4096]).unwrap();
    // Nothing changed: nothing is written again and the tree hashes the same.
    let again = sync(root, "dev", &["src/a.c".into()], &others, &small_config()).unwrap();
    assert_eq!((again.copied, again.linked, again.hash.clone()), (0, 0, snapshot.hash.clone()));
    // Its own next edit: one file written, a new tree.
    fs::write(root.join("src").join("a.c"), "int a(void) { return 11; }\n").unwrap();
    let edited = sync(root, "dev", &["src/a.c".into()], &others, &small_config()).unwrap();
    assert_eq!(edited.copied, 1);
    assert_ne!(edited.hash, snapshot.hash);
    // The same size and time with other bytes is still a new tree: content
    // decides, never a size and a time.
    let before = fs::metadata(root.join("src").join("a.c")).unwrap().modified().unwrap();
    fs::write(root.join("src").join("a.c"), "int a(void) { return 12; }
").unwrap();
    fs::File::options().write(true).open(root.join("src").join("a.c")).unwrap().set_modified(before).unwrap();
    let same_stamp = sync(root, "dev", &["src/a.c".into()], &others, &small_config()).unwrap();
    assert_ne!(same_stamp.hash, edited.hash);
    assert_eq!(fs::read_to_string(same_stamp.tree.join("src").join("a.c")).unwrap(), "int a(void) { return 12; }
");
    // A changed file inside a linked folder changes the tree's key.
    fs::write(root.join("Content").join("Maps").join("level.umap"), vec![2u8; 8193]).unwrap();
    let assets_changed = sync(root, "dev", &["src/a.c".into()], &others, &small_config()).unwrap();
    assert_ne!(assets_changed.hash, same_stamp.hash);
    // Another member's snapshot of the same folder is its own folder.
    let theirs = sync(root, "dev2", &["src/b.c".into()], &BTreeMap::new(), &small_config()).unwrap();
    assert_ne!(theirs.tree, edited.tree);
    assert_eq!(fs::read_to_string(theirs.tree.join("src").join("a.c")).unwrap(), "int a(void) { return 1; }\n");
}

#[test]
fn one_build_at_a_time_per_build_folder() {
    let dir = tempfile::tempdir().unwrap();
    let held = queue(dir.path(), Duration::from_secs(1)).unwrap();
    let started = Instant::now();
    let error = queue(dir.path(), Duration::from_millis(500)).unwrap_err().to_string();
    assert!(error.contains("queue did not free") && started.elapsed() >= Duration::from_millis(400), "{error}");
    drop(held);
    assert!(queue(dir.path(), Duration::from_secs(1)).is_ok());
}

#[test]
fn a_failure_in_another_members_file_is_theirs() {
    let tree = Path::new(if cfg!(windows) { r"D:\shared\.zerus-team\runs\broker-dev\tree" } else { "/shared/.zerus-team/runs/broker-dev/tree" });
    let output = format!(
        "error[E0308]: mismatched types\n  --> src/b.rs:3:5\nsrc/c.c:4:2: error: expected ';'\n{}\\src\\d.cpp(12): error C2065: 'x'\nwarning: unused\n",
        tree.display()
    );
    let failing = failing_files(&output, tree);
    assert_eq!(failing, BTreeSet::from(["src/b.rs".to_owned(), "src/c.c".to_owned(), "src/d.cpp".to_owned()]));
    let dir = project();
    let team = team_in(dir.path());
    let others = BTreeMap::from([("src/c.c".to_owned(), "dev2".to_owned())]);
    let (line, by_member) = blame(&failing, &others, &team).unwrap();
    assert!(line.contains("dev2 (Gameplay) (src/c.c)") && line.contains("wait"), "{line}");
    // An observation, never a verdict: it may be a dependency of the asker's.
    assert!(line.contains("may be their unfinished change or a dependency of yours"), "{line}");
    assert_eq!(by_member.get("dev2"), Some(&vec!["src/c.c".to_owned()]));
    // Only its own files failing: the asker's own to fix.
    assert!(blame(&BTreeSet::from(["src/b.rs".to_owned()]), &others, &team).is_none());
}

#[test]
fn a_build_runs_in_the_snapshot_an_unchanged_tree_once_and_tells_the_other_member() {
    let dir = project();
    let root = dir.path();
    let templates = tempfile::tempdir().unwrap();
    crate::state::teams::templates::settings_override(Some(templates.path().join("templates.json")));
    let (program, flag) = if cfg!(windows) { ("cmd", "/c") } else { ("sh", "-c") };
    // A "build" that fails in dev2's file when that file is half-written there.
    // cmd gets no quotes (std would escape them for it): a half-written b.c
    // has no closing brace.
    let script = if cfg!(windows) { "findstr } src\\b.c >nul && echo built || (echo src/b.c:1:20: error: expected expression & exit 1)" }
                 else { "grep -q '}' src/b.c && echo built || { echo 'src/b.c:1:20: error: expected expression'; exit 1; }" };
    crate::state::teams::templates::set_additions(&[json!({"id":"build","program":program,"args":[flag, script]})]).unwrap();
    let mut team = team_in(root);
    team.settings.build_broker = true;
    let config = Config { templates: vec!["build".into()], ..small_config() };
    let dev = team.member("dev").unwrap().clone();
    // dev2 is half-way through b.c and says so.
    fs::write(root.join("src").join("b.c"), "int b(void) { return 2 +").unwrap();
    record_edits(root, "dev2", &["src/b.c".into()]).unwrap();
    let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(Vec<String>, Value)>::new()));
    let record = sent.clone();
    crate::state::teams::dispatcher::set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        record.borrow_mut().push((args.iter().map(|arg| (*arg).to_owned()).collect(), input.cloned().unwrap_or_default()));
        Ok(crate::state::teams::dispatcher::fake_output(0, r#"{"status":"submitted"}"#, ""))
    })));
    // dev's build sees b.c at HEAD and passes, untouched by dev2's half edit.
    let first = run(&team, &dev, root, "build", &json!({}), &config, Duration::from_secs(60)).unwrap();
    assert_eq!(first.exit, Some(0), "{first:?}");
    assert!(first.tail.contains("built") && first.tail.contains("Build broker: built build in"), "{}", first.tail);
    // The same tree again: its own outputs are in place, not built twice.
    let again = run(&team, &dev, root, "build", &json!({"files":[]}), &config, Duration::from_secs(60)).unwrap();
    assert!(again.tail.contains("already built here"), "{}", again.tail);
    // dev2 builds its own snapshot with its half-written file and fails in its
    // own file: that is its own to fix, nobody else is told.
    let dev2 = team.member("dev2").unwrap().clone();
    let own = run(&team, &dev2, root, "build", &json!({}), &config, Duration::from_secs(60)).unwrap();
    assert_eq!(own.exit, Some(1));
    assert!(!own.tail.contains("another member is changing"), "{}", own.tail);
    assert!(sent.borrow().iter().all(|(args, _)| args[0] != "send"));
    // A build of dev's whose failure names dev2's file: told as dev2's, and dev2 hears.
    crate::state::teams::templates::set_additions(&[json!({"id":"build","program":program,"args":[flag, if cfg!(windows) { "echo src/b.c:1:20: error: expected expression & exit 1" } else { "echo 'src/b.c:1:20: error: expected expression'; exit 1" }]})]).unwrap();
    fs::write(root.join("src").join("a.c"), "int a(void) { return 12; }\n").unwrap();
    let blamed = run(&team, &dev, root, "build", &json!({"files":["src/a.c"]}), &config, Duration::from_secs(60)).unwrap();
    crate::state::teams::dispatcher::set_fake_hgs(None);
    crate::state::teams::templates::settings_override(None);
    assert_eq!(blamed.error, "the build failed in files another member is changing", "{blamed:?}");
    assert!(blamed.tail.contains("dev2 (Gameplay) (src/b.c)") && blamed.tail.contains("Never edit their files"), "{}", blamed.tail);
    let told = sent.borrow().iter().find(|(args, _)| args[0] == "send").cloned().expect("dev2 is told");
    assert_eq!(told.0[1], "codex/x/dev2");
    assert!(told.1["text"].as_str().unwrap().contains("dev (Backend)'s build failed in files you are changing: src/b.c"), "{}", told.1);
    assert!(told.1["text"].as_str().unwrap().contains("an observation, not a verdict"), "{}", told.1);
    assert_eq!(edits_of(root, "dev"), vec!["src/a.c".to_owned()]);
}

#[test]
fn the_lead_proposes_the_broker_for_a_shared_folder_and_the_owners_allow_sets_it() {
    let dir = project();
    let mut team = team_in(dir.path());
    // Two members in one folder: the lead's item names the switch card.
    let items = crate::state::teams::inbox::items(&team);
    let item = items.iter().find(|item| item.kind == "build_broker").expect("the lead's item");
    assert!(item.summary.contains("dev (Backend), dev2 (Gameplay)") && item.command.contains("\"setting\":\"build_broker\""), "{} {}", item.summary, item.command);
    let mut events = Vec::new();
    let none = BTreeSet::new();
    let lead = Actor::Member("lead".into());
    // A setting outside the switch list, or no value, is refused.
    assert!(ops::apply(&mut team, "propose", &json!({"kind":"switch-setting","setting":"land_allowed","value":true,"reason":"x"}), &lead, &none, &mut events).is_err());
    assert!(ops::apply(&mut team, "propose", &json!({"kind":"switch-setting","setting":"build_broker","reason":"x"}), &lead, &none, &mut events).is_err());
    let proposed = ops::apply(&mut team, "propose", &json!({"kind":"switch-setting","setting":"build_broker","value":true,"reason":"dev and dev2 share the folder"}), &lead, &none, &mut events).unwrap();
    assert_eq!(proposed["change"]["setting"], "build_broker");
    assert_eq!(proposed["change"]["value"], true);
    // #756: the card names the members who share the folder.
    assert_eq!(proposed["change"]["shared"][0]["members"], json!(["dev", "dev2"]));
    assert!(!team.settings.build_broker, "the card sets nothing by itself");
    // Proposed once: the item goes.
    assert!(!crate::state::teams::inbox::items(&team).iter().any(|item| item.kind == "build_broker"));
    // The owner's Allow sets it.
    let id = proposed["proposal"].as_str().unwrap().to_owned();
    let hash = proposed["hash"].as_str().unwrap().to_owned();
    ops::apply(&mut team, "decide-proposal", &json!({"proposal":id,"accept":true,"expected_hash":hash}), &Actor::Human, &none, &mut events).unwrap();
    assert!(team.settings.build_broker);
    assert!(events.iter().any(|event| event.kind == "setting_switched"));
    // One member per folder: no item.
    let mut alone = team_in(dir.path());
    alone.members.retain(|member| member.id != "dev2");
    assert!(!crate::state::teams::inbox::items(&alone).iter().any(|item| item.kind == "build_broker"));
}

// #756: the Builds tab's read: the running build against its usual time, the
// queue with a request that waits for the same tree, the hit rate, the recent
// builds with a failure blamed on another member's file, the changed files
// and the file two members change, and each member's build state.
#[test]
fn the_builds_view_shows_the_running_build_the_queue_the_timeline_and_who_changes_what() {
    let dir = project();
    let team = team_in(dir.path());
    let shared = dir.path();
    let runs = shared.join(".zerus-team").join("runs");
    fs::create_dir_all(runs.join("broker-building")).unwrap();
    fs::create_dir_all(runs.join("broker-waiting")).unwrap();
    let now = 1_000_000.0;
    let ok = |ms: u64| json!({"exit":0,"tail":"","ms":ms,"timed_out":false,"error":""});
    let results = json!([
        {"key":"a","member":"dev","template":"cargo-build","at":now - 900.0,"hit":false,"blamed":[],"outcome":ok(120_000)},
        {"key":"a","member":"dev2","template":"cargo-build","at":now - 800.0,"hit":true,"by":"dev","outcome":ok(120_000)},
        {"key":"b","member":"dev2","template":"cargo-build","at":now - 600.0,"hit":false,"blamed":[{"member":"dev","files":["src/a.c"]}],
         "outcome":{"exit":1,"tail":"","ms":60_000,"timed_out":false,"error":"the build failed in another member's files"}},
        {"key":"c","member":"dev","template":"cargo-build","at":now - 300.0,"hit":false,"blamed":[],"outcome":ok(100_000)}
    ]);
    fs::write(runs.join("broker-results.json"), results.to_string()).unwrap();
    fs::write(runs.join("broker-building").join("k"), json!({"member":"dev","template":"cargo-test","started":now - 130.0}).to_string()).unwrap();
    fs::write(runs.join("broker-waiting").join("dev2.json"), json!({"member":"dev2","template":"cargo-build","since":now - 40.0,"coalesced_with":"dev"}).to_string()).unwrap();
    record_edits(shared, "dev", &["src/a.c".into(), "src/b.c".into()]).unwrap();
    fs::write(shared.join("src").join("a.c"), "int a(void) { return 3; }\n").unwrap();
    fs::write(shared.join("src").join("b.c"), "int b(void) { return 4; }\n").unwrap();
    record_edits(shared, "dev2", &["src/b.c".into()]).unwrap();

    let view = builds_view(&team, now);
    let folder = &view["folders"][0];
    assert_eq!(folder["members"], json!(["dev", "dev2"]), "{view}");
    let target = &folder["build_folders"][0];
    assert_eq!(target["name"], "target");
    assert_eq!(target["running"][0]["member"], "dev");
    assert_eq!(target["running"][0]["seconds"], 130.0);
    assert_eq!(target["running"][0]["snapshot"], true);
    assert_eq!(target["usual_ms"], 120_000.0, "the median of the green builds that ran");
    assert_eq!(target["hit_rate"], 0.25);
    assert_eq!(target["queue"][0]["member"], "dev2");
    assert_eq!(target["queue"][0]["coalesced_with"], "dev");
    assert_eq!(target["timeline"].as_array().unwrap().len(), 4);
    assert_eq!(target["timeline"][1]["blamed"][0], json!({"member":"dev","files":["src/a.c"]}), "newest first");
    assert_eq!(folder["overlaps"], json!([{"file":"src/b.c","members":["dev","dev2"]}]));
    assert_eq!(view["members"]["dev"]["state"], "building");
    assert_eq!(view["members"]["dev2"], json!({"state":"waiting","template":"cargo-build","since":now - 40.0,"ahead":1}));
}

/// 1.2: a build template's result is its outputs in one member's tree, so it
/// never serves another member; a check template's verdict does, and the key
/// names the tools.
#[test]
fn a_build_result_serves_only_its_own_tree_and_a_verdict_serves_anyone() {
    assert!(verdict("cargo-test") && verdict("qt-test") && verdict("cargo-clippy"));
    assert!(!verdict("cargo-build") && !verdict("qt-configure") && !verdict("qt-build") && !verdict("build"));
    let args = json!({"files":["src/a.c"]});
    assert_eq!(request_key("h", "cargo-test", &args, "rustc 1"), request_key("h", "cargo-test", &json!({}), "rustc 1"), "named files are not the request");
    assert_ne!(request_key("h", "cargo-test", &args, "rustc 1"), request_key("h", "cargo-test", &args, "rustc 2"), "another toolchain is another result");
    let dir = project();
    let root = dir.path();
    let templates = tempfile::tempdir().unwrap();
    crate::state::teams::templates::settings_override(Some(templates.path().join("templates.json")));
    let (program, flag) = if cfg!(windows) { ("cmd", "/c") } else { ("sh", "-c") };
    crate::state::teams::templates::set_additions(&[json!({"id":"build","program":program,"args":[flag, "echo built"]})]).unwrap();
    let mut team = team_in(root);
    team.settings.build_broker = true;
    let config = Config { templates: vec!["build".into()], ..small_config() };
    let dev = team.member("dev").unwrap().clone();
    let dev2 = team.member("dev2").unwrap().clone();
    let first = run(&team, &dev, root, "build", &json!({}), &config, Duration::from_secs(60)).unwrap();
    assert!(first.tail.contains("Build broker: built build"), "{}", first.tail);
    // dev2's tree is the same content, but dev's outputs are not in it.
    let theirs = run(&team, &dev2, root, "build", &json!({}), &config, Duration::from_secs(60)).unwrap();
    crate::state::teams::templates::settings_override(None);
    assert!(theirs.tail.contains("Build broker: built build"), "built, not served: {}", theirs.tail);
    assert!(!theirs.tail.contains("verdict"), "{}", theirs.tail);
}
