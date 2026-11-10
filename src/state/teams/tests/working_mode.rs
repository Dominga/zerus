// Task 270 (owner, 2026-11-08): the project profile selects the working mode:
// the branch pipeline for a code repository whose tests run, one shared folder
// for a huge binary or asset one, and a branch per task for code without tests.
use super::*;
use std::path::Path;
use std::process::Command;

fn git(folder: &Path, args: &[&str]) {
    let status = Command::new("git").current_dir(folder).args(args).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A repository with `files` committed on `main`.
fn repo(files: &[(&str, &[u8])]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    for (path, content) in files {
        let target = dir.path().join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(target, content).unwrap();
    }
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["-c", "user.email=t@example.com", "-c", "user.name=T", "commit", "-m", "init"]);
    dir
}

fn tests_config(providers: bool, merge_queue: bool) -> Vec<u8> {
    let mut text = String::from("version = 1\n");
    if providers {
        text += "\n[[provider]]\nname = \"rust\"\nkind = \"cargo\"\nsources = [\"src/**\"]\ntests = [\"src/**/*.rs\"]\n";
    }
    if merge_queue {
        text += "\n[merge_queue]\nenabled = true\n";
    }
    text.into_bytes()
}

fn code_repository(with_tests: bool) -> tempfile::TempDir {
    let source = b"fn main() { println!(\"hi\"); }\n".to_vec();
    repo(&[("src/main.rs", &source), (".zerus/tests.toml", &tests_config(with_tests, with_tests))])
}

/// A code repository whose tracked bytes are mostly one binary asset.
fn binary_repository() -> tempfile::TempDir {
    let source = b"int main(void) { return 0; }\n".to_vec();
    let asset = vec![0u8; 1024 * 1024];
    repo(&[("src/main.c", &source), ("assets/hero.png", &asset)])
}

#[test]
fn a_code_repository_proposes_the_branch_pipeline() {
    let dir = code_repository(true);
    let proposal = propose(dir.path()).expect("a profile for a Git repository");
    assert_eq!((proposal.preset.as_str(), proposal.pipeline.as_str()), (BRANCH_PIPELINE, "code"), "{proposal:?}");
    assert_eq!(proposal.profile.suites, 1);
    assert!(proposal.profile.merge_queue);
    assert_eq!(proposal.settings["branch_per_task"]["template"], "task/{id}");
    assert_eq!(proposal.settings["base_branch"], "main");
    assert_eq!(proposal.settings["folder_policy"], "worktree");
    let reasons = proposal.reasons.join("\n");
    assert!(reasons.contains("tracked files") && reasons.contains("test subsystem") && reasons.contains("merge queue"), "{reasons}");
    assert!(proposal.title.contains("branch per task") && proposal.title.contains("merge queue"), "{}", proposal.title);
}

#[test]
fn a_binary_heavy_repository_proposes_one_shared_folder() {
    let dir = binary_repository();
    let proposal = propose(dir.path()).expect("a profile for a Git repository");
    assert_eq!(proposal.preset, SHARED_FOLDER, "{proposal:?}");
    assert!(proposal.profile.binary_share() > 0.9, "{:?}", proposal.profile);
    assert_eq!(proposal.settings["folder_policy"], "shared");
    assert_eq!(proposal.settings["build_broker"], true);
    assert!(proposal.settings["branch_per_task"].is_null());
    let reasons = proposal.reasons.join("\n");
    assert!(reasons.contains("binary files") && reasons.contains("shared folder"), "{reasons}");
}

#[test]
fn a_code_repository_without_tests_proposes_a_branch_per_task() {
    let dir = code_repository(false);
    let proposal = propose(dir.path()).expect("a profile for a Git repository");
    assert_eq!(proposal.preset, BRANCH_PER_TASK, "{proposal:?}");
    assert_eq!(proposal.profile.suites, 0);
    assert!(proposal.pipeline.is_empty());
    assert_eq!(proposal.settings["branch_per_task"]["template"], "task/{id}");
    assert!(proposal.reasons.join(" ").contains("no test subsystem"), "{:?}", proposal.reasons);
}

/// A game or asset repository with engine markers is a shared folder even when
/// its binaries are not most of the bytes yet.
#[test]
fn an_engine_project_proposes_one_shared_folder() {
    let dir = repo(&[
        ("Game.uproject", b"{\"FileVersion\":3}\n"),
        ("Config/DefaultEngine.ini", b"[/Script/EngineSettings.GameMapsSettings]\n"),
        ("Content/Maps/Main.umap", &vec![0u8; 64 * 1024]),
    ]);
    let proposal = propose(dir.path()).expect("a profile for a Git repository");
    assert_eq!(proposal.preset, SHARED_FOLDER, "{proposal:?}");
    assert!(proposal.profile.engine.iter().any(|marker| marker.contains("uproject")), "{:?}", proposal.profile.engine);
    assert!(proposal.reasons.join(" ").contains("engine assets"), "{:?}", proposal.reasons);
}

/// Git LFS is enough on its own: its files are not cloned per member.
#[test]
fn a_repository_in_lfs_proposes_one_shared_folder() {
    let dir = repo(&[
        ("src/main.rs", b"fn main() {}\n"),
        (".gitattributes", b"*.psd filter=lfs diff=lfs merge=lfs -text\n"),
    ]);
    let proposal = propose(dir.path()).expect("a profile for a Git repository");
    assert_eq!(proposal.preset, SHARED_FOLDER, "{proposal:?}");
    assert!(proposal.profile.lfs);
}

#[test]
fn the_mode_writes_the_settings_the_preset_names() {
    let code = code_repository(true);
    let profile = profile(code.path()).unwrap();
    let mut settings = Settings::default();
    apply(&mut settings, BRANCH_PIPELINE, &profile.branch).unwrap();
    assert_eq!(settings.branch_per_task.as_ref().map(|branch| branch.template.as_str()), Some("task/{id}"));
    assert_eq!(settings.base_branch, "main");
    assert_eq!(settings.folder_policy, super::super::model::FolderPolicy::Worktree);
    apply(&mut settings, SHARED_FOLDER, "main").unwrap();
    assert!(settings.branch_per_task.is_none() && settings.base_branch.is_empty());
    assert_eq!(settings.folder_policy, super::super::model::FolderPolicy::Shared);
    assert!(settings.build_broker);
    assert!(apply(&mut settings, "a hunch", "main").is_err());
}

#[test]
fn a_team_whose_settings_disagree_with_its_project_names_what_is_missing() {
    let dir = binary_repository();
    let mut team = Team {
        id: "t".into(),
        name: "T".into(),
        lead: "lead".into(),
        folder: dir.path().display().to_string(),
        status: TeamStatus::Running,
        ..Default::default()
    };
    // A code team's settings on a binary project: branches where it needs one folder.
    team.settings.branch_per_task = Some(super::super::model::BranchPerTask { template: "task/{id}".into() });
    team.settings.base_branch = "main".into();
    let (proposal, wrong) = disagreement(&team).expect("a disagreement");
    assert_eq!(proposal.preset, SHARED_FOLDER);
    assert!(wrong.iter().any(|why| why.contains("own folders")), "{wrong:?}");
    assert!(wrong.iter().any(|why| why.contains("branch per task")), "{wrong:?}");
    // The proposed mode clears it.
    apply(&mut team.settings, SHARED_FOLDER, "main").unwrap();
    assert!(disagreement(&team).is_none());
    // A project that cannot be profiled has no proposal.
    team.folder = tempfile::tempdir().unwrap().path().display().to_string();
    assert!(proposal_of(&team).is_none());
}

fn proposal_of(team: &Team) -> Option<Proposal> {
    propose(Path::new(&team.folder))
}

#[test]
fn the_request_names_its_mode_and_an_explicit_setting_wins() {
    assert_eq!(requested_mode(&json!({})), None);
    assert_eq!(requested_mode(&json!({"working_mode":"auto"})).as_deref(), Some("auto"));
    assert_eq!(requested_mode(&json!({"working_mode":"shared-folder"})).as_deref(), Some("shared-folder"));
    let mut explicit = json!({"settings":{"base_branch":"release"}});
    assert!(super::explicit(&explicit, BRANCH_PIPELINE));
    assert!(!super::explicit(&explicit, SHARED_FOLDER), "a branch setting does not answer the shared folder");
    explicit["settings"] = json!({});
    assert!(!super::explicit(&explicit, SHARED_FOLDER));
}

// --- the flow: create writes the mode, the draft shows and applies it, a running team gets one card

use crate::state::teams::ops;
use crate::state::teams::ops::Actor;
use std::collections::BTreeSet;

fn created(folder: &Path, mode: &str) -> Team {
    let (team, _) = ops::create(
        "t270",
        &json!({"name":"Mode","folder":folder.display().to_string(),"lead":{"id":"lead","agent":"claude"},"working_mode":mode}),
    )
    .unwrap();
    team
}

#[test]
fn create_writes_the_proposed_mode_into_the_settings() {
    let code = code_repository(true);
    let team = created(code.path(), "auto");
    assert_eq!(team.working_mode, BRANCH_PIPELINE);
    assert_eq!(team.settings.branch_per_task.as_ref().map(|branch| branch.template.as_str()), Some("task/{id}"));
    assert_eq!(team.settings.base_branch, "main");
    assert_eq!(team.settings.folder_policy, super::super::model::FolderPolicy::Worktree);
    let binary = binary_repository();
    let team = created(binary.path(), "auto");
    assert_eq!(team.working_mode, SHARED_FOLDER);
    assert_eq!(team.settings.folder_policy, super::super::model::FolderPolicy::Shared);
    assert!(team.settings.build_broker);
    assert!(team.settings.branch_per_task.is_none());
}

#[test]
fn an_explicit_mode_wins_at_create() {
    let binary = binary_repository();
    let team = created(binary.path(), "branch-per-task");
    assert_eq!(team.working_mode, BRANCH_PER_TASK);
    assert!(team.settings.branch_per_task.is_some());
    assert_eq!(team.settings.folder_policy, super::super::model::FolderPolicy::Worktree);
    // A plain folder (not a repository of its own) keeps the plain flow.
    let plain = tempfile::tempdir().unwrap();
    let team = created(plain.path(), "auto");
    assert!(team.working_mode.is_empty(), "{:?}", team.working_mode);
    assert!(team.settings.branch_per_task.is_none());
}

#[test]
fn the_draft_shows_the_proposal_and_create_applies_the_accepted_mode() {
    let dir = binary_repository();
    let (mut team, _) = ops::create(
        "t270",
        &json!({"name":"Mode","folder":dir.path().display().to_string(),"lead":{"id":"lead","agent":"claude"}}),
    )
    .unwrap();
    // The project proposes the shared folder, its reasons and its settings.
    let proposal = propose(dir.path()).expect("the proposal");
    assert_eq!(proposal.preset, SHARED_FOLDER);
    assert!(proposal.value()["reasons"].as_array().is_some_and(|reasons| !reasons.is_empty()));
    // The owner picks another preset in the draft.
    ops::apply(&mut team, "plan", &json!({"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(super::super::model::Draft { revision: 1, ..Default::default() });
    ops::apply(
        &mut team,
        "draft",
        &json!({"action":"update","revision":1,"set":{"working_mode":"branch-per-task"}}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(team.working_mode, BRANCH_PER_TASK);
    // Create team writes it; the proposal's own preset is not applied over it.
    let value = ops::apply(&mut team, "draft", &json!({"action":"create"}), &Actor::Human, &BTreeSet::new(), &mut Vec::new());
    assert!(value.is_ok(), "{value:?}");
    assert_eq!(team.working_mode, BRANCH_PER_TASK);
    assert!(team.settings.branch_per_task.is_some());
    assert_ne!(team.settings.folder_policy, super::super::model::FolderPolicy::Shared);
}

#[test]
fn a_running_team_whose_settings_disagree_gets_one_card() {
    let dir = binary_repository();
    let (mut team, _) = ops::create(
        "t270",
        &json!({"name":"Mode","folder":dir.path().display().to_string(),"lead":{"id":"lead","agent":"claude"},"working_mode":"branch-per-task"}),
    )
    .unwrap();
    team.status = TeamStatus::Running;
    use crate::state::teams::dispatcher::{steps_with, Step};
    let now = crate::state::now();
    let steps = steps_with(&team, &[], now, None);
    assert!(
        steps.iter().any(|step| matches!(step, Step::WorkingMode { preset, .. } if preset == SHARED_FOLDER)),
        "{steps:?}"
    );
    // A safe point needs no open attempt: one in flight is not the moment.
    team.tasks = vec![super::super::model::Task {
        id: 1,
        title: "A".into(),
        assignee: "lead".into(),
        status: super::super::model::TaskStatus::InProgress,
        attempts: vec![super::super::model::Attempt {
            n: 1,
            id: "a1".into(),
            member: "lead".into(),
            state: super::super::model::AttemptState::Delivered,
            ..Default::default()
        }],
        ..Default::default()
    }];
    let steps = steps_with(&team, &[], now, None);
    assert!(!steps.iter().any(|step| matches!(step, Step::WorkingMode { .. })), "{steps:?}");
    team.tasks.clear();
    // The lead's card: one, with the preset and its reasons.
    let mut events = Vec::new();
    let lead = Actor::Member(team.lead.clone());
    ops::apply(
        &mut team,
        "propose",
        &json!({"kind":"working-mode","preset":SHARED_FOLDER,"reason":"the project is binary-heavy"}),
        &lead,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    let cards: Vec<&Value> = team.proposals.iter().filter(|proposal| proposal["kind"] == "working-mode").collect();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["change"]["preset"], SHARED_FOLDER);
    // One card: a second tick offers nothing while it is open.
    let steps = steps_with(&team, &[], now, None);
    assert!(!steps.iter().any(|step| matches!(step, Step::WorkingMode { .. })), "{steps:?}");
    // The owner's Allow writes the preset; the disagreement is gone.
    let id = cards[0]["id"].as_str().unwrap().to_owned();
    // U57: the owner's Allow is bound to the card's content hash.
    let hash = cards[0]["hash"].as_str().unwrap_or_default().to_owned();
    ops::apply(
        &mut team,
        "decide-proposal",
        &json!({"proposal":id,"accept":true,"reason":"yes","expected_hash":hash}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(team.working_mode, SHARED_FOLDER);
    assert_eq!(team.settings.folder_policy, super::super::model::FolderPolicy::Shared);
    assert!(team.settings.build_broker);
    assert!(disagreement(&team).is_none());
}

#[test]
fn an_unknown_mode_is_refused_at_create() {
    let dir = code_repository(true);
    // An unknown preset with a project to profile is refused, not guessed.
    let refused = ops::create(
        "t270",
        &json!({"name":"Mode","folder":dir.path().display().to_string(),"lead":{"id":"lead","agent":"claude"},"working_mode":"the old way"}),
    );
    assert!(refused.is_err(), "{:?}", refused.map(|(team, _)| team.settings));
}

// --- task 336: a code repository with tests keeps the branch pipeline, and the
// --- owner may keep the team's settings.

/// A code repository with tests whose binaries are design previews: more source
/// files than binary ones, and a binary share far above 20%.
fn preview_binary_repository() -> tempfile::TempDir {
    let mut files: Vec<(String, Vec<u8>)> = (0..12)
        .map(|index| (format!("src/module{index}.rs"), format!("fn module{index}() {{}}\n").into_bytes()))
        .collect();
    for index in 0..8 {
        files.push((format!("design/preview{index}.png"), vec![0u8; 1024 * 1024]));
    }
    files.push((".zerus/tests.toml".into(), tests_config(true, true)));
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(path, content)| (path.as_str(), content.as_slice())).collect();
    repo(&refs)
}

/// A code repository with tests whose binaries are runtime assets (not design
/// previews): more binaries than sources, and a binary share far above 20%.
fn binary_majority_repository() -> tempfile::TempDir {
    let mut files: Vec<(String, Vec<u8>)> = (0..3)
        .map(|index| (format!("src/module{index}.rs"), format!("fn module{index}() {{}}\n").into_bytes()))
        .collect();
    for index in 0..9 {
        files.push((format!("assets/texture{index}.png"), vec![0u8; 1024 * 1024]));
    }
    files.push((".zerus/tests.toml".into(), tests_config(true, true)));
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(path, content)| (path.as_str(), content.as_slice())).collect();
    repo(&refs)
}

/// Task #451 (owner, 2026-11-09): design previews, docs images and test fixtures
/// in a code repository: many of them, and three beyond 5 MiB, exactly as this
/// team's project measured (157 binary files, 38% of the tracked bytes).
fn design_and_fixture_repository() -> tempfile::TempDir {
    let mut files: Vec<(String, Vec<u8>)> = (0..40)
        .map(|index| (format!("src/module{index}.rs"), format!("fn module{index}() {{}}\n").into_bytes()))
        .collect();
    for index in 0..120 {
        files.push((format!("design/preview{index}.png"), vec![0u8; 8 * 1024]));
    }
    for index in 0..25 {
        files.push((format!("docs/shots/page{index}.jpg"), vec![0u8; 64 * 1024]));
    }
    for index in 0..9 {
        files.push((format!("tests/fixtures/capture{index}.bin"), vec![0u8; 256 * 1024]));
    }
    for index in 0..3 {
        files.push((format!("design/big{index}.psd"), vec![0u8; 6 * 1024 * 1024]));
    }
    files.push((".zerus/tests.toml".into(), tests_config(true, true)));
    let refs: Vec<(&str, &[u8])> = files.iter().map(|(path, content)| (path.as_str(), content.as_slice())).collect();
    repo(&refs)
}

#[test]
fn a_code_repository_with_tests_and_preview_binaries_stays_branch_pipeline() {
    let dir = preview_binary_repository();
    let proposal = propose(dir.path()).expect("a profile");
    assert_eq!(proposal.preset, BRANCH_PIPELINE, "{proposal:?}");
    // Task #451: design previews are not runtime assets: they count in the
    // totals, never as binaries or large assets.
    assert_eq!(proposal.profile.binary_files, 0, "{:?}", proposal.profile);
    assert_eq!(proposal.profile.preview_files, 8, "{:?}", proposal.profile);
    assert_eq!(proposal.profile.source_files, 13, "{:?}", proposal.profile);
    let reasons = proposal.reasons.join("\n");
    assert!(reasons.contains("source files are the majority"), "{reasons}");
    assert!(reasons.contains("are not runtime assets"), "{reasons}");
    assert!(!reasons.contains("one shared folder keeps"), "{reasons}");
}

#[test]
fn a_code_repository_with_design_docs_and_fixtures_never_proposes_one_shared_folder() {
    // Task #451: 157 design/docs/fixture files (38% of the bytes) and three of
    // 5 MiB or more in a code repository with its tests: the branch pipeline.
    let dir = design_and_fixture_repository();
    let proposal = propose(dir.path()).expect("a profile");
    assert_eq!(proposal.preset, BRANCH_PIPELINE, "{proposal:?}");
    assert_eq!(proposal.profile.binary_files, 0, "{:?}", proposal.profile);
    assert_eq!(proposal.profile.large_assets, 0, "{:?}", proposal.profile);
    assert_eq!(proposal.profile.preview_files, 157, "{:?}", proposal.profile);
    assert!(proposal.profile.source_files > proposal.profile.binary_files, "{:?}", proposal.profile);
    let reasons = proposal.reasons.join("\n");
    assert!(!reasons.contains("one shared folder keeps"), "{reasons}");
}

#[test]
fn this_projects_own_numbers_keep_the_branch_pipeline() {
    // Task #451 (owner, 2026-11-09), the exact profile hgs measured for this
    // team's project: 4026 tracked files, 148 MiB, 157 binary files (38% of the
    // bytes), 3 files of 5 MiB or more, 25 suites. The branch pipeline.
    let measured = ProjectProfile {
        files: 4026,
        bytes: 155_244_650,
        source_files: 3869,
        binary_files: 157,
        binary_bytes: 58_446_963,
        large_assets: 3,
        suites: 25,
        branch: "whitewh/main".into(),
        ..Default::default()
    };
    assert!(measured.binary_share() > 0.3, "{measured:?}");
    assert!(!measured.heavy(), "{measured:?}");
    assert_eq!(select(&measured).0, BRANCH_PIPELINE);
    // The same numbers with the design, docs and fixture files no longer
    // counted as assets: still the branch pipeline, and further from a floor.
    let counted = ProjectProfile { binary_files: 9, binary_bytes: 5 * 1024 * 1024, large_assets: 0, ..measured.clone() };
    assert!(!counted.heavy(), "{counted:?}");
    // Real signals do decide it: many large binaries, engine assets or LFS.
    let many = ProjectProfile { large_assets: LARGE_ASSETS, ..counted.clone() };
    assert!(many.heavy(), "{many:?}");
    let mut engine = counted.clone();
    engine.engine = vec!["a .uproject".into()];
    assert!(engine.heavy());
    let mut lfs = counted.clone();
    lfs.lfs = true;
    assert!(lfs.heavy());
}

#[test]
fn more_binaries_than_sources_still_propose_one_shared_folder() {
    let dir = binary_majority_repository();
    let proposal = propose(dir.path()).expect("a profile");
    assert_eq!(proposal.preset, SHARED_FOLDER, "{proposal:?}");
    assert!(!proposal.profile.code_first(), "{:?}", proposal.profile);
}

#[test]
fn only_huge_binaries_flip_a_code_repository_that_has_tests() {
    let code = |bytes: u64, binary_bytes: u64, binary_files: u64| ProjectProfile {
        files: 5000,
        bytes,
        source_files: 4000,
        binary_files,
        binary_bytes,
        suites: 3,
        ..Default::default()
    };
    // Design previews and fixtures: 90% of a small tree, still code first.
    assert!(!code(100 * 1024 * 1024, 90 * 1024 * 1024, 100).heavy());
    // Above the floor: one gibibyte of binaries, or two of tracked bytes.
    assert!(code(1300 * 1024 * 1024, 1200 * 1024 * 1024, 200).heavy());
    assert!(code(2500 * 1024 * 1024, 200 * 1024 * 1024, 200).heavy());
    // Engine assets and LFS keep the shared folder whatever the tests say.
    let mut engine = code(100 * 1024 * 1024, 90 * 1024 * 1024, 100);
    engine.engine = vec!["a .uproject".into()];
    assert!(engine.heavy());
    let mut lfs = code(100 * 1024 * 1024, 90 * 1024 * 1024, 100);
    lfs.lfs = true;
    assert!(lfs.heavy());
    // No tests: the old binary rules still apply.
    let mut untested = code(100 * 1024 * 1024, 90 * 1024 * 1024, 100);
    untested.suites = 0;
    assert!(untested.heavy());
}

#[test]
fn the_keep_choice_hides_the_block_until_the_profile_changes() {
    let dir = binary_majority_repository();
    let mut team = Team {
        id: "t336".into(),
        name: "Keep".into(),
        lead: "lead".into(),
        folder: dir.path().display().to_string(),
        status: TeamStatus::Running,
        settings: Settings {
            branch_per_task: Some(BranchPerTask { template: "task/{id}".into() }),
            base_branch: "main".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(disagreement(&team).is_some(), "the shared folder differs from the branch settings");
    // The owner keeps the team's settings: the block leaves.
    let at = 1_800_000_000.0;
    let record = keep(&mut team, at).expect("a keep");
    assert_eq!(record["preset"], SHARED_FOLDER);
    assert!(disagreement(&team).is_none(), "{:?}", disagreement(&team));
    assert!(kept(&team).is_some());
    // A material profile change brings it back.
    team.mode_kept = Some(json!({"preset":SHARED_FOLDER,"fingerprint":"an older profile","at":at}));
    assert!(disagreement(&team).is_some(), "the profile changed: the block returns");
    // Another preset's keep does not hide it either.
    team.mode_kept = Some(json!({"preset":BRANCH_PIPELINE,"fingerprint":"whatever","at":at}));
    assert!(disagreement(&team).is_some());
}

#[test]
fn the_cached_proposal_honors_the_keep() {
    let dir = binary_majority_repository();
    let proposal = propose(dir.path()).expect("a profile");
    let fingerprint = fingerprint(&proposal.profile);
    let mut team = Team {
        id: "t336c".into(),
        name: "Keep".into(),
        lead: "lead".into(),
        folder: dir.path().display().to_string(),
        status: TeamStatus::Running,
        settings: Settings {
            branch_per_task: Some(BranchPerTask { template: "task/{id}".into() }),
            base_branch: "main".into(),
            ..Default::default()
        },
        mode_proposal: Some(proposal.value()),
        ..Default::default()
    };
    assert!(cached_disagreement(&team).is_some());
    team.mode_kept = Some(json!({"preset":SHARED_FOLDER,"fingerprint":fingerprint,"at":1.0}));
    assert!(cached_disagreement(&team).is_none());
    // The cached profile changed: the block returns.
    team.mode_kept = Some(json!({"preset":SHARED_FOLDER,"fingerprint":"older","at":1.0}));
    assert!(cached_disagreement(&team).is_some());
}

// --- task 451 (owner, 2026-11-09): the working-mode block's buttons answer, and
// --- a code repository with design/asset binaries is never proposed one shared
// --- folder.

/// The owner's own press of Propose the switch makes the card, and Allow on that
/// card switches the team: the round trip the block offers.
#[test]
fn the_working_mode_blocks_press_proposes_a_card_and_allow_switches_the_team() {
    let dir = binary_majority_repository();
    let (mut team, _) = ops::create(
        "t451",
        &json!({"name":"Mode","folder":dir.path().display().to_string(),"lead":{"id":"lead","agent":"claude"},"working_mode":"branch-per-task"}),
    )
    .unwrap();
    team.status = TeamStatus::Running;
    assert!(disagreement(&team).is_some(), "the project is one shared folder, the settings branch per task");
    let mut events = Vec::new();
    // The owner's press, not the lead's: "auto" is the project's profile now.
    let card = ops::apply(
        &mut team,
        "propose",
        &json!({"kind":"working-mode","preset":"auto","reason":""}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(card["status"], "proposed", "{card}");
    let id = card["proposal"].as_str().unwrap().to_owned();
    let stored = team.proposals.iter().find(|proposal| proposal["id"] == id.as_str()).expect("the card");
    assert_eq!((stored["state"].as_str(), stored["kind"].as_str()), (Some("open"), Some("working-mode")), "{stored}");
    // The press carries its own reason when the block gave none.
    assert_eq!(stored["reason"], "the working-mode block's Propose the switch");
    // Allow: the settings follow the project, and the block has nothing left.
    let hash = card["hash"].as_str().unwrap_or_default().to_owned();
    ops::apply(
        &mut team,
        "decide-proposal",
        &json!({"proposal":id,"accept":true,"expected_hash":hash,"reason":"yes"}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(team.working_mode, SHARED_FOLDER);
    assert!(disagreement(&team).is_none());
    assert!(cached_disagreement(&team).is_none());
}

/// A block left over from an older profile: the press says the settings already
/// follow the project, creates no card, and the block's next read is the current
/// profile.
#[test]
fn the_blocks_press_answers_a_stale_profile_without_a_card() {
    let dir = code_repository(true);
    let (mut team, _) = ops::create(
        "t451b",
        &json!({"name":"Mode","folder":dir.path().display().to_string(),"lead":{"id":"lead","agent":"claude"},"working_mode":"branch-pipeline"}),
    )
    .unwrap();
    team.status = TeamStatus::Running;
    // The block still shows the proposal cached when the team was planned.
    team.mode_proposal = Some(json!({"preset":SHARED_FOLDER,"title":"one shared folder","reasons":["binary files"]}));
    assert!(cached_disagreement(&team).is_some(), "the stale block is showing");
    let mut events = Vec::new();
    let receipt = ops::apply(
        &mut team,
        "propose",
        &json!({"kind":"working-mode","preset":"auto","reason":""}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!((receipt["status"].as_str(), receipt["working_mode"].as_str()), (Some("already"), Some(BRANCH_PIPELINE)), "{receipt}");
    assert!(team.proposals.is_empty(), "nothing to allow: {:?}", team.proposals);
    assert_eq!(team.mode_proposal.as_ref().unwrap()["preset"], BRANCH_PIPELINE, "the block reads the current profile");
    assert!(cached_disagreement(&team).is_none(), "the block leaves");
    assert!(events.iter().any(|event| event.kind == "working_mode_current"), "the press is in the feed");
}

/// Task #451: a cached profile older than MODE_CHECK_SECONDS is read again by
/// the tick, so a block from an older profile (or an older hgs) is corrected;
/// a fresh one costs no Git read.
#[test]
fn the_tick_reads_an_old_cached_profile_again() {
    use crate::state::teams::dispatcher::{steps_with, Step};
    let dir = binary_majority_repository();
    let (mut team, _) = ops::create(
        "t451c",
        &json!({"name":"Mode","folder":dir.path().display().to_string(),"lead":{"id":"lead","agent":"claude"},"working_mode":"branch-per-task"}),
    )
    .unwrap();
    team.status = TeamStatus::Running;
    let now = crate::state::now();
    // Just read: the tick compares settings and runs no Git.
    let steps = steps_with(&team.clone(), &[], now, None);
    assert!(!steps.iter().any(|step| matches!(step, Step::ModeCheck)), "{steps:?}");
    // An old reading (here also the shape written before checked_at existed).
    team.mode_proposal = Some(json!({"preset":SHARED_FOLDER,"title":"one shared folder"}));
    let steps = steps_with(&team, &[], now, None);
    assert!(steps.iter().any(|step| matches!(step, Step::ModeCheck)), "{steps:?}");
    // An open attempt holds the folder the switch would move: no re-read yet.
    team.tasks = vec![super::super::model::Task {
        id: 1,
        title: "A".into(),
        assignee: "lead".into(),
        status: super::super::model::TaskStatus::InProgress,
        attempts: vec![super::super::model::Attempt {
            n: 1,
            id: "a1".into(),
            member: "lead".into(),
            state: super::super::model::AttemptState::Delivered,
            ..Default::default()
        }],
        ..Default::default()
    }];
    let steps = steps_with(&team, &[], now, None);
    assert!(!steps.iter().any(|step| matches!(step, Step::ModeCheck)), "{steps:?}");
}


/// Task #13 (team ede724c4, owner 2026-11-10): "Keep the settings as they
/// are" (working mode none) at Create team is the owner's Keep: the playbook
/// and the working-mode block do not ask about the settings again.
#[test]
fn create_with_working_mode_none_does_not_ask_again() {
    let dir = binary_repository();
    let (mut team, _) = ops::create(
        "t270",
        &json!({"name":"Mode","folder":dir.path().display().to_string(),"lead":{"id":"lead","agent":"claude"}}),
    )
    .unwrap();
    ops::apply(&mut team, "plan", &json!({"tasks":[{"id":1,"title":"A","assignee":"lead"}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    team.status = TeamStatus::Draft;
    team.draft = Some(super::super::model::Draft { revision: 1, ..Default::default() });
    ops::apply(&mut team, "draft", &json!({"action":"update","revision":1,"set":{"working_mode":"none"}}), &Actor::Human, &BTreeSet::new(), &mut Vec::new())
        .unwrap();
    ops::apply(&mut team, "draft", &json!({"action":"create","approve":true}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert!(team.working_mode.is_empty(), "nothing is applied: {}", team.working_mode);
    assert!(team.playbook.as_ref().is_none_or(|playbook| playbook["mode"] != "none"), "{:?}", team.playbook);
    assert_eq!(kept(&team).and_then(|kept| kept["preset"].as_str()), Some(SHARED_FOLDER));
    assert!(cached_disagreement(&team).is_none());
    let breaches = super::super::playbook::breaches(&team);
    assert!(!breaches.iter().any(|breach| breach.rule == "a"), "{:?}", breaches.iter().map(|breach| breach.title.clone()).collect::<Vec<_>>());
}
