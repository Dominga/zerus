use super::super::ops::{self, Actor};
use super::*;
use super::support::*;

const SESSIONS: &str = "Repeat the matrix: launch scratch sessions, read each with hgs terminal and start a team worker.";

fn dev_mut(team: &mut Team) -> &mut Member {
    team.members.iter_mut().find(|member| member.id == "dev").unwrap()
}

fn launch_of(agent: &str, profile: &str) -> Vec<String> {
    let t = team();
    let member = Member { id: "m".into(), agent: agent.into(), permissions_profile: profile.into(), launch_id: "l1".into(), ..Default::default() };
    launch_args(&t, &member, "D:/work")
}

fn permissions(args: &[String]) -> Option<&str> {
    args.iter().position(|arg| arg == "--permissions").map(|at| args[at + 1].as_str())
}

/// Task 154 (1): the launch carries each agent's real mode for the profile.
#[test]
fn a_member_launches_with_its_agents_mode_for_its_profile() {
    assert_eq!(permissions(&launch_of("dsh", "operate")), Some("native:{\"preset\":\"danger-full-access\"}"));
    assert_eq!(permissions(&launch_of("dsh", "read")), Some("native:{\"preset\":\"read-only\"}"));
    assert_eq!(permissions(&launch_of("dsh", "write")), Some("native:{\"preset\":\"workspace-write\"}"));
    assert_eq!(permissions(&launch_of("codex", "read")), Some("native:{\"approval\":\"on-request\",\"sandbox\":\"read-only\"}"));
    assert_eq!(permissions(&launch_of("codex", "operate")), Some("native:{\"approval\":\"on-request\",\"sandbox\":\"danger-full-access\"}"));
    assert_eq!(permissions(&launch_of("kimi", "read")), Some("native:{\"plan\":\"on\"}"));
    // Claude keeps its mode as an agent flag after `--`.
    let claude = launch_of("claude", "operate");
    assert_eq!(permissions(&claude), None);
    let dashes = claude.iter().position(|arg| arg == "--").unwrap();
    assert_eq!(&claude[dashes + 1..dashes + 3], ["--permission-mode", "acceptEdits"]);
    // No profile: the account's own mode, as before.
    for agent in ["claude", "codex", "kimi", "dsh"] {
        let args = launch_of(agent, "");
        assert_eq!(permissions(&args), None, "{agent}: {args:?}");
        assert!(!args.iter().any(|arg| arg == "--permission-mode"), "{agent}");
    }
    // An explicit selection goes as it is.
    let t = team();
    let member = Member { id: "m".into(), agent: "dsh".into(), permissions: "native:{\"preset\":\"team-qa\"}".into(), permissions_profile: "read".into(), ..Default::default() };
    assert_eq!(permissions(&launch_args(&t, &member, "D:/work")), Some("native:{\"preset\":\"team-qa\"}"));
}

/// Task 154 (3): the hand-out never gives a task to a member whose launch
/// cannot do it; the task waits with who can and which card fixes it.
#[test]
fn a_task_never_goes_to_a_member_that_cannot_do_it() {
    let mut t = team();
    t.task_mut(1).unwrap().description = SESSIONS.into();
    let found = steps(&t, &free_sessions(), 20.0);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. })), "{found:?}");
    let reason = found
        .iter()
        .find_map(|step| match step {
            Step::RightsWait { task: 1, reason } => Some(reason.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no rights wait: {found:?}"));
    assert!(reason.contains("needs sessions, terminals, worker"), "{reason}");
    assert!(reason.contains("dev (Backend) (codex"), "{reason}");
    assert!(reason.contains("qa (Tester)"), "kimi runs unsandboxed and can: {reason}");
    assert!(reason.contains("\"kind\":\"rights\"") && reason.contains("\"permissions_profile\":\"operate\""), "{reason}");
    // The other task of the same member still goes.
    assert!(!found.iter().any(|step| matches!(step, Step::RightsWait { task: 2, .. })), "{found:?}");
    // With rights that do it, it goes.
    dev_mut(&mut t).permissions_profile = "operate".into();
    let found = steps(&t, &free_sessions(), 20.0);
    assert!(!found.iter().any(|step| matches!(step, Step::RightsWait { .. })), "{found:?}");
    assert!(found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. } | Step::Settings { .. })), "{found:?}");
    // An explicit empty `needs` says the words ask nothing.
    dev_mut(&mut t).permissions_profile.clear();
    t.task_mut(1).unwrap().needs = Some(vec![]);
    assert!(!steps(&t, &free_sessions(), 20.0).iter().any(|step| matches!(step, Step::RightsWait { .. })));
}

/// The waiting step records the reason once, for the lead.
#[test]
fn a_rights_wait_is_recorded_once_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = team();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let step = Step::RightsWait { task: 1, reason: "#1 needs sessions".into() };
        run_step(&mut store, &t, &step, "").unwrap();
        run_step(&mut store, &t, &step, "").unwrap();
        let after = store.load(&t.id).unwrap().unwrap();
        let wait = after.task(1).unwrap().waits.clone().unwrap();
        assert_eq!((wait.on.as_slice(), wait.reason.as_str()), (["rights".to_owned()].as_slice(), "#1 needs sessions"));
        assert_eq!(event_kinds(&store, &t.id).iter().filter(|kind| *kind == "task_waits").count(), 1);
    });
}

/// Task 154 (3): plan validation refuses the mismatch, naming the fix.
#[test]
fn plan_validation_refuses_a_task_its_member_cannot_do() {
    let mut t = team();
    t.task_mut(1).unwrap().description = SESSIONS.into();
    let validation = plan::validate(&t, &BTreeSet::new());
    let finding = validation.errors.iter().find(|finding| finding.kind == "rights").expect("a rights error");
    assert_eq!((finding.task, finding.member.as_deref()), (Some(1), Some("dev")));
    assert!(finding.text.contains("hgs team propose"), "{}", finding.text);
    dev_mut(&mut t).permissions_profile = "operate".into();
    assert!(!plan::validate(&t, &BTreeSet::new()).errors.iter().any(|finding| finding.kind == "rights"));
    // A finished task is history.
    dev_mut(&mut t).permissions_profile.clear();
    t.task_mut(1).unwrap().status = TaskStatus::Accepted;
    assert!(!plan::validate(&t, &BTreeSet::new()).errors.iter().any(|finding| finding.kind == "rights"));
}

/// Task 154 (2): the lead narrows a member's rights, the owner widens them;
/// a running member is relaunched in its conversation once it is idle.
#[test]
fn the_owner_widens_and_the_lead_narrows_a_running_members_rights() {
    let mut t = team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    let lead = Actor::Member("lead".into());
    let error = ops::apply(&mut t, "rights", &json!({"member":"dev","permissions_profile":"operate"}), &lead, &none, &mut events)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Wider rights are the owner's") && error.contains("\"kind\":\"rights\""), "{error}");
    let error = ops::apply(&mut t, "rights", &json!({"member":"dev","permissions":"auto"}), &Actor::Member("dev".into()), &none, &mut events)
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Only the lead or you change a member's rights");
    let changed = ops::apply(&mut t, "rights", &json!({"member":"dev","permissions_profile":"operate"}), &Actor::Human, &none, &mut events).unwrap();
    assert_eq!((changed["status"].as_str(), changed["relaunch"].as_bool()), (Some("changed"), Some(true)));
    assert_eq!(changed["rights"]["selection"], "native:{\"approval\":\"on-request\",\"sandbox\":\"danger-full-access\"}");
    let dev = t.member("dev").unwrap();
    assert!(dev.rights_relaunch && dev.permissions_profile == "operate");
    // The lead narrows on its own.
    ops::apply(&mut t, "rights", &json!({"member":"dev","permissions_profile":"read"}), &lead, &none, &mut events).unwrap();
    assert_eq!(t.member("dev").unwrap().permissions_profile, "read");
    // DeepSeek's auto is refused as at a solo launch; a member without a
    // session takes the rights at its launch.
    t.members.push(Member { id: "ds".into(), role: "QA".into(), agent: "dsh".into(), state: MemberState::NotCreated, ..Default::default() });
    let error = ops::apply(&mut t, "rights", &json!({"member":"ds","permissions":"auto"}), &Actor::Human, &none, &mut events).unwrap_err();
    assert!(format!("{error:#}").contains("DeepSeek has no auto mode"), "{error:#}");
    let changed = ops::apply(&mut t, "rights", &json!({"member":"ds","permissions":"native:{\"preset\":\"danger-full-access\"}"}), &Actor::Human, &none, &mut events)
        .unwrap();
    assert_eq!(changed["relaunch"], false);
    assert!(events.iter().any(|event| event.kind == "member_rights"));
}

/// The card: the lead proposes wider rights; Allow runs the owner's op.
#[test]
fn the_lead_proposes_wider_rights_as_a_card() {
    let mut t = team();
    let mut events = Vec::new();
    let proposed = ops::apply(
        &mut t,
        "propose",
        &json!({"kind":"rights","member":"dev","permissions_profile":"operate","reason":"#140 launches scratch sessions"}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(proposed["status"], "proposed", "{proposed}");
    assert_eq!(proposed["change"]["op"], "rights");
    assert!(proposed["change"]["summary"].as_str().unwrap().contains("operate"), "{proposed}");
    // Proposing changed nothing yet.
    assert_eq!(t.member("dev").unwrap().permissions_profile, "");
}

/// Task 154 (2): an idle member with new rights is relaunched: paused, then
/// launched under its name without --new, which resumes its conversation.
#[test]
fn new_rights_relaunch_an_idle_member_in_its_conversation() {
    let mut t = team();
    dev_mut(&mut t).rights_relaunch = true;
    dev_mut(&mut t).permissions_profile = "operate".into();
    dev_mut(&mut t).directory = "D:/work/dev".into();
    // Busy: it waits, and takes no new task meanwhile.
    let mut busy = free_sessions();
    busy[1].free = false;
    let found = steps(&t, &busy, 20.0);
    assert!(!found.iter().any(|step| matches!(step, Step::RelaunchRights { .. } | Step::HandOut { .. })), "{found:?}");
    let found = steps(&t, &free_sessions(), 20.0);
    assert!(found.contains(&Step::RelaunchRights { member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "dev")), "{found:?}");
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Vec<String>>::new()));
        let record = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            record.borrow_mut().push(args.iter().map(|arg| arg.to_string()).collect());
            Ok(fake_output(0, "{}", ""))
        })));
        run_step(&mut store, &t, &Step::RelaunchRights { member: "dev".into() }, "").unwrap();
        set_fake_hgs(None);
        let calls = calls.borrow();
        assert_eq!(calls[0], ["pause", "codex/x/dev", "--json"]);
        assert_eq!(
            calls[1],
            ["codex", "D:/work/dev", "-n", "dev", "-d", "--permissions", "native:{\"approval\":\"on-request\",\"sandbox\":\"danger-full-access\"}"]
        );
        assert!(!calls[1].iter().any(|arg| arg == "--new"), "the same conversation");
        let after = store.load(&t.id).unwrap().unwrap();
        assert!(!after.member("dev").unwrap().rights_relaunch);
        assert!(event_kinds(&store, &t.id).contains(&"member_relaunched".to_owned()));
    });
}

/// A session whose name a launch cannot resume is not replaced by a new one:
/// the lead hears it.
#[test]
fn a_relaunch_that_cannot_resume_tells_the_lead() {
    let mut t = team();
    dev_mut(&mut t).rights_relaunch = true;
    dev_mut(&mut t).session.as_mut().unwrap().name = "codex-dev".into();
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        store.create("11111111-1111-4111-8111-111111111111", &json!({}), t.clone(), vec![]).unwrap();
        set_fake_hgs(Some(Box::new(|_, args: &[&str], _: Option<&Value>| panic!("no launch: {args:?}"))));
        run_step(&mut store, &t, &Step::RelaunchRights { member: "dev".into() }, "").unwrap();
        set_fake_hgs(None);
        let after = store.load(&t.id).unwrap().unwrap();
        assert!(!after.member("dev").unwrap().rights_relaunch);
        assert!(event_kinds(&store, &t.id).contains(&"member_rights_failed".to_owned()));
    });
}

/// Task 154 (4): a QA member gets operate and a merger commit by default,
/// within the role's own rights (no extra owner confirmation).
#[test]
fn role_defaults_reach_the_members_without_widening() {
    let (mut t, _) = ops::create("t", &json!({"name":"Roles","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let mut events = Vec::new();
    ops::apply(
        &mut t,
        "plan",
        &json!({"members":[{"id":"qa","role":"QA tester","agent":"dsh"},{"id":"merger","role":"Reviewer & Merger","agent":"claude"},
            {"id":"dev","role":"Backend","agent":"dsh"}],
            "tasks":[{"id":1,"title":"Matrix","assignee":"qa","description":SESSIONS}]}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(t.member("qa").unwrap().permissions_profile, "operate");
    assert_eq!(t.member("merger").unwrap().permissions_profile, "commit");
    assert_eq!(t.member("dev").unwrap().permissions_profile, "");
    assert!(t.profile_widening.is_empty(), "{:?}", t.profile_widening);
    assert!(!plan::validate(&t, &BTreeSet::new()).errors.iter().any(|finding| finding.kind == "rights"));
    assert_eq!(super::super::rights::view(t.member("qa").unwrap())["selection"], "native:{\"preset\":\"danger-full-access\"}");
    // Wider than its role still waits for the owner.
    ops::apply(
        &mut t,
        "plan",
        &json!({"members":[{"id":"dev","role":"Backend","agent":"dsh","permissions_profile":"operate"}],
            "tasks":[{"id":1,"title":"Matrix","assignee":"dev"}]}),
        &Actor::Member("lead".into()),
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    assert_eq!(t.profile_widening.len(), 1, "{:?}", t.profile_widening);
}
