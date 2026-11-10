// Task 22: a DeepSeek member whose start fails only because the DeepSeek host
// is still starting starts again by itself once the host is up; a host that
// never comes up fails the member once, with its reason, after a bounded
// number of starts.
use super::*;
use super::support::*;
use crate::state::teams::model::{Member, MemberFolder};

/// The exact failure team 33fbafeb saw for its `hgs` member on 2026-11-05.
const STILL_STARTING: &str = "hgs: DeepSeek host is still starting; retry opening the session";

/// Task #35: a DeepSeek member starts only in its own clone (as hgs and ui2
/// had in team 33fbafeb); the computer's catalog says it is there, on the
/// member's branch.
fn own_clone(args: &[&str]) -> Option<std::process::Output> {
    if args.first() != Some(&"worktrees") {
        return None;
    }
    let path = args[args.iter().position(|arg| *arg == "--path")? + 1];
    let name = Path::new(path).file_name()?.to_str()?;
    let (short, member) = name.rsplit_once("-team-")?.1.split_once('-')?;
    let catalog = json!({"state":"ok","selected_root":path,"worktrees":[{"path":path,"branch":format!("team/{short}/{member}")}]});
    Some(fake_output(0, &catalog.to_string(), ""))
}

/// A team with two DeepSeek members on one computer (one host), each with a
/// ready task, as hgs and ui2 were in team 33fbafeb.
fn deepseek_team() -> Team {
    let mut team = team();
    team.folder = if cfg!(windows) { r"D:\work\shop".into() } else { "/work/shop".into() };
    for (id, task) in [("hgs", 1), ("ui2", 3)] {
        team.members.push(Member {
            id: id.into(),
            role: format!("DeepSeek {id}"),
            agent: "dsh".into(),
            folder: MemberFolder::Clone,
            computer: "devbox".into(),
            ..Default::default()
        });
        team.tasks.iter_mut().find(|found| found.id == task).unwrap().assignee = id.into();
    }
    team.tasks.iter_mut().find(|found| found.id == 3).unwrap().deps.clear();
    team
}

fn member(store: &Store, team: &str, id: &str) -> Member {
    store.load(team).unwrap().unwrap().member(id).unwrap().clone()
}

/// Starts `id` as the dispatcher does, with the team as stored.
fn start(store: &mut Store, team: &str, id: &str) {
    let saved = store.load(team).unwrap().unwrap();
    run_step(store, &saved, &Step::Launch { member: id.into() }, "").unwrap();
}

fn launches(store: &Store, team: &str) -> Vec<String> {
    let saved = store.load(team).unwrap().unwrap();
    steps(&saved, &[], now())
        .into_iter()
        .filter_map(|step| match step {
            Step::Launch { member } => Some(member),
            _ => None,
        })
        .collect()
}

#[test]
fn a_member_whose_deepseek_host_was_still_starting_starts_once_it_is_up() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = deepseek_team();
        stored(&mut store, &t);
        assert_eq!(launches(&store, &t.id), ["hgs", "ui2"], "both members are started");
        // The first start on the host launches it and gives up waiting; the
        // second, a moment later, finds it up (team 33fbafeb).
        let host_up = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let (up, record) = (host_up.clone(), seen.clone());
        set_fake_hgs(Some(Box::new(move |computer, args: &[&str], _| {
            if let Some(output) = own_clone(args) {
                return Ok(output);
            }
            assert_eq!((computer, args[0]), ("devbox", "dsh"));
            let name = args[args.iter().position(|arg| *arg == "-n").unwrap() + 1].to_owned();
            record.borrow_mut().push(name);
            Ok(if up.replace(true) { fake_output(0, "", "") } else { fake_output(1, "", STILL_STARTING) })
        })));
        start(&mut store, &t.id, "hgs");
        start(&mut store, &t.id, "ui2");
        let hgs = member(&store, &t.id, "hgs");
        // Not a failure: no error, so no inbox item; the next tick starts it again.
        assert_eq!((hgs.error.as_str(), hgs.launch_id.as_str(), hgs.host_waits), ("", "", 1), "{hgs:?}");
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(!crate::state::teams::inbox::items(&saved).iter().any(|item| item.kind == "member_error"));
        assert!(!member(&store, &t.id, "ui2").launch_id.is_empty(), "ui2 started");
        assert_eq!(launches(&store, &t.id), ["hgs"], "hgs is started again without a relaunch");
        start(&mut store, &t.id, "hgs");
        let hgs = member(&store, &t.id, "hgs");
        assert!(hgs.error.is_empty() && !hgs.launch_id.is_empty() && hgs.host_waits == 0, "{hgs:?}");
        assert_eq!(seen.borrow().len(), 3);
        let kinds = event_kinds(&store, &t.id);
        assert_eq!(kinds.iter().filter(|kind| *kind == "launch_waiting").count(), 1, "{kinds:?}");
        assert!(!kinds.iter().any(|kind| kind == "launch_failed"), "{kinds:?}");
        set_fake_hgs(None);
    });
}

#[test]
fn a_deepseek_host_that_never_comes_up_fails_the_member_once() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = deepseek_team();
        stored(&mut store, &t);
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let count = calls.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _| {
            if let Some(output) = own_clone(args) {
                return Ok(output);
            }
            count.set(count.get() + 1);
            Ok(fake_output(1, "", STILL_STARTING))
        })));
        // The dispatcher starts it while it plans a start, and no more.
        let mut starts = 0;
        while launches(&store, &t.id).contains(&"hgs".to_owned()) {
            start(&mut store, &t.id, "hgs");
            starts += 1;
            assert!(starts <= 10, "unbounded");
        }
        assert_eq!((starts, calls.get()), (HOST_WAITS + 1, HOST_WAITS + 1));
        let hgs = member(&store, &t.id, "hgs");
        assert!(hgs.error.contains("DeepSeek host is still starting"), "{}", hgs.error);
        assert!(hgs.error.contains("started 4 times while the DeepSeek host was starting"), "{}", hgs.error);
        let kinds = event_kinds(&store, &t.id);
        assert_eq!(kinds.iter().filter(|kind| *kind == "launch_failed").count(), 1, "{kinds:?}");
        assert_eq!(kinds.iter().filter(|kind| *kind == "launch_waiting").count(), HOST_WAITS as usize, "{kinds:?}");
        let saved = store.load(&t.id).unwrap().unwrap();
        let errors = crate::state::teams::inbox::items(&saved).into_iter().filter(|item| item.kind == "member_error").count();
        assert_eq!(errors, 1);
        // A relaunch starts the count again.
        store
            .mutate(Some(("44444444-4444-4444-8444-444444444444", &json!({}))), &t.id, None, |team, events| {
                super::super::ops::apply(team, "relaunch", &json!({"member":"hgs"}), &super::super::ops::Actor::Human, &BTreeSet::new(), events)?;
                Ok(Value::Null)
            })
            .unwrap();
        assert_eq!(member(&store, &t.id, "hgs").host_waits, 0);
        set_fake_hgs(None);
    });
}

#[test]
fn any_other_start_failure_reaches_the_inbox_at_once() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let t = deepseek_team();
        stored(&mut store, &t);
        set_fake_hgs(Some(Box::new(|_, args: &[&str], _| {
            if let Some(output) = own_clone(args) {
                return Ok(output);
            }
            Ok(fake_output(1, "", "hgs: Official DeepSeek host failed to start. Private log: host.log"))
        })));
        start(&mut store, &t.id, "hgs");
        let hgs = member(&store, &t.id, "hgs");
        assert!(hgs.error.contains("failed to start") && hgs.host_waits == 0, "{hgs:?}");
        assert!(!launches(&store, &t.id).contains(&"hgs".to_owned()));
        assert_eq!(event_kinds(&store, &t.id).iter().filter(|kind| *kind == "launch_failed").count(), 1);
        set_fake_hgs(None);
    });
}
