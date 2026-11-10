//! Task #35 (team ede724c4, 2026-11-10): a DeepSeek (dsh) session's sandbox
//! labels the folder it works in Low. The qa member of team 33fbafeb shared
//! D:\Tools\Zerus and labeled the owner's checkout, so every program built
//! there failed with Access denied. A DeepSeek member starts only in its own
//! Zerus-made folder (its clone or worktree), never in a shared folder and
//! never as the lead.
use super::*;

fn dsh(id: &str, folder: MemberFolder, directory: &str) -> Member {
    Member { id: id.into(), role: "QA".into(), agent: "dsh".into(), folder, directory: directory.into(), ..Default::default() }
}

#[test]
fn a_deepseek_member_starts_only_in_its_own_zerus_made_folder() {
    let dir = tempfile::tempdir().unwrap();
    set_git_ceiling(&dir.path().display().to_string());
    let repo = folder_repo(dir.path(), "project");
    let integration = folder_repo(dir.path(), "line");
    let other = dir.path().join("project-team-abcdef12-dev");
    // The team folder, the integration folder and another member's folder:
    // nothing is created, granted or started in any of them.
    for (what, directory) in [("the team folder", String::new()), ("the integration folder", integration.display().to_string()), ("another member's folder", other.display().to_string())] {
        let member = dsh("qa", MemberFolder::Shared, &directory);
        let mut team = folder_team(&repo, member.clone());
        team.settings.integration_folder = integration.display().to_string();
        let calls = std::cell::Cell::new(0);
        let run = |_: &str, args: &[&str]| {
            calls.set(calls.get() + 1);
            local_create(args)
        };
        let error = member_folder_with(&team, &member, &run).unwrap_err().to_string();
        assert!(error.contains("Low integrity label") && error.contains("may not share"), "{what}: {error}");
        assert!(error.contains("\"members_update\":[{\"id\":\"qa\",\"folder\":\"clone\"}]") && error.contains("hgs team settings --json"), "{what}: one action: {error}");
        assert_eq!(calls.get(), 0, "{what}: nothing ran");
        assert_eq!(deepseek_folder_problem(&team, &member).as_deref(), Some(error.as_str()));
    }
    // A DeepSeek lead would work in the team folder.
    let mut team = folder_team(&repo, dsh("qa", MemberFolder::Clone, ""));
    team.members[0].agent = "dsh".into();
    let lead = team.members[0].clone();
    let error = member_folder_with(&team, &lead, &|_, args| local_create(args)).unwrap_err().to_string();
    assert!(error.starts_with("lead: the lead works in the team folder") && error.contains("Choose another agent for the lead"), "{error}");
    // Its own clone starts, from the team's repository.
    let member = dsh("qa", MemberFolder::Clone, "");
    let team = folder_team(&repo, member.clone());
    assert!(deepseek_folder_problem(&team, &member).is_none());
    let folder = member_folder_with(&team, &member, &|_, args| local_create(args)).unwrap();
    assert!(folder.ends_with("project-team-abcdef12-qa"), "{folder}");
    // Another agent may share the folder; an adopted DeepSeek session runs
    // where it was adopted (nothing is launched for it); a removed member
    // starts nothing.
    let codex = Member { agent: "codex".into(), ..dsh("rev", MemberFolder::Shared, "") };
    let team = folder_team(&repo, codex.clone());
    assert!(deepseek_folder_problem(&team, &codex).is_none());
    assert_eq!(member_folder_with(&team, &codex, &|_, args| local_create(args)).unwrap(), repo.display().to_string());
    let adopted = Member { session: Some(SessionRef { name: "dsh/x/qa".into(), ..Default::default() }), ..dsh("qa", MemberFolder::Shared, &repo.display().to_string()) };
    assert!(deepseek_folder_problem(&folder_team(&repo, adopted.clone()), &adopted).is_none());
    let removed = Member { state: MemberState::Removed, ..dsh("qa", MemberFolder::Shared, "") };
    assert!(deepseek_folder_problem(&folder_team(&repo, removed.clone()), &removed).is_none());
}

#[test]
fn the_preflight_refuses_a_deepseek_member_without_its_own_folder() {
    let mut team = folder_team(std::path::Path::new("D:/synthetic/project"), dsh("qa", MemberFolder::Shared, ""));
    team.members.push(dsh("cook", MemberFolder::Clone, ""));
    let found = plan::validate(&team, &BTreeSet::new());
    let folders: Vec<&str> = found.errors.iter().filter(|finding| finding.kind == "folder").map(|finding| finding.text.as_str()).collect();
    assert_eq!(folders.len(), 1, "only qa: {folders:?}");
    assert!(folders[0].starts_with("qa: ") && folders[0].contains("its own clone"), "{folders:?}");
    // The clone it is told to take passes.
    team.members[1].folder = MemberFolder::Clone;
    assert!(plan::validate(&team, &BTreeSet::new()).errors.iter().all(|finding| finding.kind != "folder"));
}
