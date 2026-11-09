// Task #26: a sub-lead with reports to read, or the member a waiting stage
// would go to, has no session; the dispatcher starts it, so its brief, its
// inbox or its stage can reach it. A paused, draining, removed or failed member
// is never started this way.
use super::super::ops::{self, Actor};
use super::support::*;
use super::*;

/// The running team with qa's "qalead" as its sub-lead: qalead has no session
/// and no task of its own.
fn sub_lead_team(folder: &str) -> Team {
    let mut team = team();
    team.folder = folder.to_owned();
    let mut events = Vec::new();
    ops::apply(
        &mut team,
        "add-member",
        &json!({"member":{"id":"qalead","role":"QA lead","agent":"codex","folder":"shared"}}),
        &Actor::Human,
        &BTreeSet::new(),
        &mut events,
    )
    .unwrap();
    ops::apply(&mut team, "sub-lead", &json!({"member":"qalead","sub":true}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut events).unwrap();
    ops::apply(&mut team, "reports-to", &json!({"member":"qa","parent":"qalead"}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    team
}

/// Task 3 handed to qa and reported: the report waits for qalead.
fn qa_reported(team: &mut Team) {
    let task = team.task_mut(3).unwrap();
    task.attempts.push(Attempt { n: 1, id: "a3".into(), member: "qa".into(), state: AttemptState::Delivered, sent_at: 10.0, delivered_at: Some(10.0), ..Default::default() });
    task.status = TaskStatus::InProgress;
    let mut events = Vec::new();
    ops::apply(team, "report", &json!({"task":3,"attempt":"a3","text":"Green"}), &Actor::Member("qa".into()), &BTreeSet::new(), &mut events).unwrap();
    assert_eq!(team.task(3).unwrap().status, TaskStatus::Submitted);
}

fn launches(planned: &[Step]) -> Vec<String> {
    planned
        .iter()
        .filter_map(|step| match step {
            Step::Launch { member } => Some(member.clone()),
            _ => None,
        })
        .collect()
}

/// Task #26: a not_created sub-lead with a report is started, and once its
/// session is there the sub-brief and the inbox reach it.
#[test]
fn a_sub_lead_with_reports_is_started_and_then_hears_its_brief_and_inbox() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = sub_lead_team(&folder.display().to_string());
        qa_reported(&mut t);
        stored(&mut store, &t);
        // The report is qalead's, not the lead's, and nothing would give it a
        // task of its own.
        let saved = store.load(&t.id).unwrap().unwrap();
        assert_eq!(crate::state::teams::inbox::items_for(&saved, "qalead").len(), 1);
        assert_eq!(saved.member("qalead").unwrap().session, None);
        let planned = steps(&saved, &[], now());
        assert_eq!(launches(&planned), ["qalead"], "only the sub-lead starts");
        assert!(!planned.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "qalead")));
        // The start runs as the dispatcher's tick runs it.
        let started = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let record = started.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], _: Option<&Value>| {
            record.borrow_mut().push(args.first().copied().unwrap_or_default().to_owned());
            Ok(fake_output(0, "", ""))
        })));
        let launch = planned.iter().find(|step| matches!(step, Step::Launch { .. })).unwrap().clone();
        run_step(&mut store, &saved, &launch, "").unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(!saved.member("qalead").unwrap().launch_id.is_empty(), "started");
        assert!(saved.member("qalead").unwrap().error.is_empty());
        assert_eq!(started.borrow()[0], "codex");
        // Its session appears: the sub-brief and the report reach it.
        let session_ref = SessionRef { name: "codex/x/qalead".into(), run_id: "r9".into(), conversation_id: "c9".into(), ..Default::default() };
        run_step(&mut store, &saved, &Step::Adopt { member: "qalead".into(), session: session_ref.clone() }, "").unwrap();
        let sent = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
        let seen_sent = sent.clone();
        set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
            if args.first().copied() == Some("send") {
                seen_sent.borrow_mut().push(input.map(crate::state::teams::doorbell::posted_text).unwrap_or_default());
            }
            Ok(fake_output(0, r#"{"status":"submitted"}"#, ""))
        })));
        let saved = store.load(&t.id).unwrap().unwrap();
        let after = steps(&saved, &[session("codex/x/qalead", "r9", true)], now());
        assert!(after.iter().any(|step| matches!(step, Step::SubBrief { member } if member == "qalead")), "{after:?}");
        assert!(after.iter().any(|step| matches!(step, Step::SubInbox { member } if member == "qalead")), "{after:?}");
        for step in after.iter().filter(|step| matches!(step, Step::SubBrief { .. } | Step::SubInbox { .. })) {
            run_step(&mut store, &saved, step, "").unwrap();
        }
        set_fake_hgs(None);
        let sent = sent.borrow();
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert!(sent[0].contains("You are now a sub-lead"), "{}", sent[0]);
        assert!(sent.iter().any(|text| text.contains("task #3")), "the report reaches it: {sent:?}");
    });
}

/// Task #26: no candidate is free for a waiting review stage and its member has
/// no session: the dispatcher starts that member; once its session is there the
/// stage goes to it.
#[test]
fn a_review_stage_starts_the_member_it_would_go_to() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = staged(&folder.display().to_string());
        // The reviewer has no session yet and no task of its own.
        let rev = t.members.iter_mut().find(|member| member.id == "rev").unwrap();
        (rev.session, rev.state) = (None, MemberState::NotCreated);
        stored(&mut store, &t);
        let saved = store.load(&t.id).unwrap().unwrap();
        let planned = steps(&saved, &free_sessions(), now());
        assert_eq!(launches(&planned), ["rev"], "{planned:?}");
        assert!(!planned.iter().any(|step| matches!(step, Step::StageOut { .. })), "the stage waits for the start: {planned:?}");
        set_fake_hgs(Some(Box::new(|_, args: &[&str], _: Option<&Value>| {
            assert_eq!(args.first().copied(), Some("claude"));
            Ok(fake_output(0, "", ""))
        })));
        let launch = planned.iter().find(|step| matches!(step, Step::Launch { .. })).unwrap().clone();
        run_step(&mut store, &saved, &launch, "").unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        assert!(!saved.member("rev").unwrap().launch_id.is_empty(), "started");
        // Its session appears: the review stage is handed to it next.
        let session_ref = SessionRef { name: "claude/x/rev".into(), run_id: "r2".into(), conversation_id: "c2".into(), ..Default::default() };
        run_step(&mut store, &saved, &Step::Adopt { member: "rev".into(), session: session_ref.clone() }, "").unwrap();
        let saved = store.load(&t.id).unwrap().unwrap();
        let after = steps(&saved, &[session("claude/x/rev", "r2", true)], now());
        set_fake_hgs(None);
        assert!(after.contains(&Step::StageOut { task: 1, member: "rev".into() }), "{after:?}");
    });
}

/// Task #26: a paused, draining, removed or failed member is not started for
/// its reports, and neither is one of a planning or draft team, which starts its
/// lead alone.
#[test]
fn a_paused_draining_removed_or_failed_member_is_not_started() {
    let dir = tempfile::tempdir().unwrap();
    crate::state::with_test_state_root(dir.path(), || {
        let folder = dir.path().join("proj");
        std::fs::create_dir_all(&folder).unwrap();
        let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
        let mut t = sub_lead_team(&folder.display().to_string());
        qa_reported(&mut t);
        stored(&mut store, &t);
        let cases = [
            ("paused", MemberState::Paused, false, ""),
            ("draining", MemberState::NotCreated, true, ""),
            ("removed", MemberState::Removed, false, ""),
            ("failed", MemberState::NotCreated, false, "hgs: the session did not start"),
        ];
        for (what, state, draining, error) in cases {
            store
                .mutate(None, &t.id, None, |team, _| {
                    let sub = team.members.iter_mut().find(|member| member.id == "qalead").unwrap();
                    (sub.state, sub.draining, sub.error) = (state, draining, error.to_owned());
                    sub.launch_id.clear();
                    Ok(Value::Null)
                })
                .unwrap();
            let saved = store.load(&t.id).unwrap().unwrap();
            // A removed sub-lead's reports are the lead's; the others keep them.
            let waiting = crate::state::teams::inbox::items_for(&saved, "qalead").len();
            assert_eq!(waiting, if state == MemberState::Removed { 0 } else { 1 }, "{what}: the report's reviewer");
            let planned = steps(&saved, &[], now());
            assert!(!launches(&planned).contains(&"qalead".to_owned()), "{what}: {planned:?}");
        }
        // A planning or draft team starts its lead alone (AGENTS.md): the
        // sub-lead is not started although it has no session.
        for status in [TeamStatus::Planning, TeamStatus::Draft] {
            let mut team = t.clone();
            (team.status, team.brief) = (status, Notice::Pending);
            let planned = steps(&team, &free_sessions(), now());
            assert!(!launches(&planned).contains(&"qalead".to_owned()), "{status:?}: {planned:?}");
        }
    });
}
