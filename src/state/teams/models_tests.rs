//! U45: the dispatcher's model switch before a hand-out and the tasks for a
//! role, with the default path unchanged.
use super::super::ops::{self, Actor};
use super::*;

/// A running team: a lead, the given members and tasks.
fn team_with(members: Value, tasks: Value) -> Team {
    let (mut team, _) = ops::create(
        "t",
        &json!({"name":"Models","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead","run_id":"r0"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    ops::apply(&mut team, "plan", &json!({"members":members,"tasks":tasks}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    ops::apply(&mut team, "approve", &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    team
}

fn member(id: &str, role: &str, agent: &str, model: &str, models: Value) -> Value {
    json!({"id":id,"role":role,"agent":agent,"model":model,"models":models,
        "session":{"name":format!("{agent}/x/{id}"),"run_id":format!("run-{id}"),"conversation_id":"c"}})
}

fn on(name: &str, model: &str, effort: &str, free: bool) -> Seen {
    Seen {
        name: name.into(),
        run_id: format!("run-{}", name.rsplit('/').next().unwrap()),
        conversation_id: "c".into(),
        free,
        idle: free,
        live: true,
        model: model.into(),
        effort: effort.into(),
        ..Default::default()
    }
}

fn stored(team: &Team) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams/teams.sqlite3"), true).unwrap();
    store.create("11111111-1111-4111-8111-111111111572", &json!({}), team.clone(), vec![]).unwrap();
    (dir, store)
}

/// The team's feed kinds, oldest first.
fn kinds(store: &Store, team: &str) -> Vec<String> {
    let page = store.events(team, None, 200).unwrap();
    let mut out: Vec<String> = page["items"].as_array().unwrap().iter().map(|item| item["kind"].as_str().unwrap().to_owned()).collect();
    out.reverse();
    out
}

/// A fake `hgs` that records each call and answers `settings` with `settings`.
fn fake(settings: impl Fn() -> anyhow::Result<Output> + 'static) -> std::rc::Rc<std::cell::RefCell<Vec<(String, Value)>>> {
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let record = calls.clone();
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input: Option<&Value>| {
        record.borrow_mut().push((args.join(" "), input.cloned().unwrap_or_default()));
        match args.first().copied() {
            Some("settings") => settings(),
            _ => Ok(fake_output(0, r#"{"status":"submitted"}"#, "")),
        }
    })));
    calls
}

/// Dev may run gpt-5 at medium or gpt-5.5 at high; task #1 wants the latter,
/// task #2 names no model.
fn switching() -> Team {
    team_with(
        json!([member("dev", "Backend", "codex", "gpt-5", json!([{"model":"gpt-5","effort":"medium"},{"model":"gpt-5.5","effort":"high"}]))]),
        json!([{"id":1,"title":"API","assignee":"dev","models":["gpt-5.5@high"]},{"id":2,"title":"Docs","assignee":"dev"}]),
    )
}

fn lead_seen() -> Seen {
    on("claude/x/lead", "", "", true)
}

#[test]
fn a_team_without_the_new_fields_hands_out_reports_and_accepts_as_before() {
    let t = team_with(
        json!([member("dev", "Backend", "codex", "gpt-5", json!([])), member("qa", "Tester", "kimi", "k2", json!([]))]),
        json!([{"id":1,"title":"API","assignee":"dev","pipeline":"none"},{"id":2,"title":"Tests","assignee":"qa","deps":[{"task":1}]}]),
    );
    // Nothing new is written: an old team's JSON stays as it was.
    for task in &t.tasks {
        let value = serde_json::to_value(task).unwrap();
        for key in ["models", "role", "waits", "prepare"] {
            assert!(value.get(key).is_none(), "{key} in {value}");
        }
    }
    let dev = serde_json::to_value(t.member("dev").unwrap()).unwrap();
    assert!(dev.get("models").is_none(), "{dev}");
    // A session on another model than its launch one changes nothing.
    let seen = [lead_seen(), on("codex/x/dev", "gpt-9", "low", true), on("kimi/x/qa", "k2", "", true)];
    let found = steps(&t, &seen, 100.0);
    assert!(found.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::SwitchModel { .. } | Step::ModelUncertain { .. } | Step::TaskWaits { .. })), "{found:?}");
    let (_dir, mut store) = stored(&t);
    let calls = fake(|| panic!("no settings change without models"));
    run_step(&mut store, &t, &Step::HandOut { task: 1, member: "dev".into() }, "").unwrap();
    set_fake_hgs(None);
    assert_eq!(calls.borrow().iter().map(|(args, _)| args.as_str()).collect::<Vec<_>>(), vec!["send codex/x/dev --json"]);
    let attempt_id = store.load(&t.id).unwrap().unwrap().task(1).unwrap().attempts[0].id.clone();
    store.mutate(None, &t.id, None, |team, events| {
        ops::apply(team, "report", &json!({"task":1,"attempt":attempt_id,"text":"Done"}), &Actor::Member("dev".into()), &BTreeSet::new(), events)
    }).unwrap();
    store.mutate(None, &t.id, None, |team, events| {
        ops::apply(team, "accept", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), events)
    }).unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(saved.task(1).unwrap().status, TaskStatus::Accepted);
    assert!(saved.task(1).unwrap().attempts[0].prepare.is_empty());
    assert_eq!(kinds(&store, &t.id), vec!["handed_out", "delivered", "reported", "accepted"]);
}

#[test]
fn a_stage_returned_task_stays_with_its_implementer_on_the_models_path() {
    // #630 C2: the U45 path must not filter out the implementer after a return.
    let mut t = team_with(
        json!([
            member("dev", "Backend", "codex", "gpt-5", json!([{"model":"gpt-5","effort":"medium"}])),
            member("dev2", "Backend", "codex", "gpt-4", json!([]))
        ]),
        json!([{"id":1,"title":"API","assignee":"dev","models":["gpt-5@medium"]}]),
    );
    let task = t.task_mut(1).unwrap();
    task.attempts.push(Attempt { n: 1, member: "dev".into(), state: AttemptState::Rejected, pipeline_return: true, ..Default::default() });
    task.status = TaskStatus::Pending;
    let seen = [lead_seen(), on("codex/x/dev", "gpt-5", "medium", true), on("codex/x/dev2", "gpt-4", "", true)];
    let found = steps(&t, &seen, 100.0);
    assert!(found.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{found:?}");
    // A lead's rejection still filters dev, and dev2 cannot run the model: no hand-out.
    let mut rejected = t.clone();
    rejected.task_mut(1).unwrap().attempts[0].pipeline_return = false;
    let found = steps(&rejected, &seen, 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. })), "{found:?}");
}

#[test]
fn a_pending_switch_does_not_deliver_into_a_held_folder() {
    // #684 L2: the Ready/Switch path checks stages::held; the pending path must
    // too, or a confirmed switch delivers into a stage-held folder.
    use crate::state::teams::model::{PipelineRun, PipelineStage, StageRun, StageState};
    let mut t = switching();
    let held = t.task_mut(2).unwrap();
    held.status = TaskStatus::InTest;
    held.pipeline_run = Some(PipelineRun {
        id: "p".into(),
        name: "P".into(),
        stages: vec![PipelineStage { kind: "test".into(), role: "tester".into(), ..Default::default() }],
        max_rounds: 3,
        folder: "hold".into(),
        accept: "lead".into(),
        round: 1,
        runs: vec![StageRun { id: "s".into(), stage: 0, kind: "test".into(), round: 1, attempt: 1, state: StageState::Waiting, at: 1.0, ..Default::default() }],
    });
    held.attempts.push(Attempt { n: 1, id: "A2".into(), member: "dev".into(), state: AttemptState::Reported, ..Default::default() });
    let (_dir, mut store) = stored(&t);
    fake(|| Ok(fake_output(0, r#"{"status":"applied"}"#, "")));
    let switch = Step::SwitchModel { task: 1, member: "dev".into(), from: "gpt-5@medium".into(), model: "gpt-5.5".into(), effort: "high".into() };
    run_step(&mut store, &t, &switch, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    assert!(super::super::stages::held(&saved, "dev").is_some(), "the test stage holds dev");
    let now_on = [lead_seen(), on("codex/x/dev", "gpt-5.5", "high", true)];
    let found = steps(&saved, &now_on, 1000.0);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. })), "no delivery into the held folder: {found:?}");
    // Without the hold the confirmed switch delivers.
    let mut free = saved.clone();
    free.task_mut(2).unwrap().pipeline_run = None;
    free.task_mut(2).unwrap().status = TaskStatus::Pending;
    let found = steps(&free, &now_on, 1000.0);
    assert!(found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. })), "{found:?}");
}

#[test]
fn a_switch_is_confirmed_by_the_session_before_the_hand_out() {
    let t = switching();
    let old = [lead_seen(), on("codex/x/dev", "gpt-5", "medium", true)];
    let found = steps(&t, &old, 100.0);
    let switch = Step::SwitchModel { task: 1, member: "dev".into(), from: "gpt-5@medium".into(), model: "gpt-5.5".into(), effort: "high".into() };
    assert!(found.contains(&switch), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { .. })), "{found:?}");
    let (_dir, mut store) = stored(&t);
    let calls = fake(|| Ok(fake_output(0, r#"{"status":"applied"}"#, "")));
    run_step(&mut store, &t, &switch, "").unwrap();
    let (args, input) = calls.borrow()[0].clone();
    assert_eq!(args, "settings codex/x/dev --json");
    assert_eq!((input["model"].clone(), input["effort"].clone(), input["expected_run_id"].clone()), (json!("gpt-5.5"), json!("high"), json!("run-dev")));
    let saved = store.load(&t.id).unwrap().unwrap();
    let step = saved.task(1).unwrap().prepare[0].clone();
    assert_eq!((step.step.as_str(), step.from.as_str(), step.to.as_str(), step.result.as_str()), ("model", "gpt-5@medium", "gpt-5.5@high", "pending"));
    // Until the session shows the model nothing is handed out, and the member
    // takes no other task meanwhile; no second switch is sent.
    let waiting = steps(&saved, &old, step.at + 10.0);
    assert!(waiting.iter().all(|step| !matches!(step, Step::HandOut { .. } | Step::SwitchModel { .. } | Step::ModelUncertain { .. })), "{waiting:?}");
    let now_on = [lead_seen(), on("codex/x/dev", "gpt-5.5", "high", true)];
    let found = steps(&saved, &now_on, step.at + 10.0);
    assert!(found.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{found:?}");
    run_step(&mut store, &saved, &Step::HandOut { task: 1, member: "dev".into() }, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    let task = saved.task(1).unwrap();
    assert!(task.prepare.is_empty());
    assert_eq!(task.attempts[0].prepare[0].result, "done");
    assert_eq!(kinds(&store, &t.id), vec!["model_switch", "handed_out", "delivered"]);
    // A member already on an allowed model is handed the task with no switch.
    let mut ready = switching();
    ready.task_mut(1).unwrap().models = vec!["gpt-5".into()];
    let found = steps(&ready, &old, 100.0);
    assert!(found.contains(&Step::HandOut { task: 1, member: "dev".into() }), "{found:?}");
}

#[test]
fn a_refused_switch_leaves_the_task_waiting_with_its_reason() {
    let t = switching();
    let (_dir, mut store) = stored(&t);
    fake(|| Ok(fake_output(1, "", "hgs: unknown model gpt-5.5 for this account\n")));
    let switch = Step::SwitchModel { task: 1, member: "dev".into(), from: "gpt-5@medium".into(), model: "gpt-5.5".into(), effort: "high".into() };
    run_step(&mut store, &t, &switch, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    let task = saved.task(1).unwrap();
    assert_eq!(task.prepare[0].result, "refused");
    let wait = task.waits.clone().unwrap();
    assert_eq!((wait.waits_for.as_str(), wait.on.clone()), ("zerus", vec!["model".to_owned()]));
    assert!(wait.reason.contains("unknown model gpt-5.5"), "{}", wait.reason);
    assert_eq!(kinds(&store, &t.id), vec!["model_switch", "model_switch_refused"]);
    // Never on the wrong model, never retried on its own; the lead sees it.
    let old = [lead_seen(), on("codex/x/dev", "gpt-5", "medium", true)];
    let found = steps(&saved, &old, 1e12);
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { task: 1, .. } | Step::SwitchModel { .. })), "{found:?}");
    let items = inbox::items(&saved);
    assert!(items.iter().any(|item| item.kind == "task_waits" && item.task == Some(1) && item.command.contains("hgs team retry")), "{items:?}");
    assert_eq!(inbox::waits(&saved, task, true).unwrap()["for"], "zerus");
    // The lead's retry starts over with a new switch.
    let mut retried = saved.clone();
    ops::apply(&mut retried, "retry", &json!({"task":1}), &Actor::Member("lead".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert!(retried.task(1).unwrap().waits.is_none() && retried.task(1).unwrap().prepare.is_empty());
    assert!(steps(&retried, &old, 100.0).iter().any(|step| matches!(step, Step::SwitchModel { task: 1, .. })));
}

#[test]
fn an_unconfirmed_switch_is_uncertain_and_never_retried() {
    let t = switching();
    let (_dir, mut store) = stored(&t);
    fake(|| Ok(fake_output(0, r#"{"status":"scheduled"}"#, "")));
    let switch = Step::SwitchModel { task: 1, member: "dev".into(), from: String::new(), model: "gpt-5.5".into(), effort: "high".into() };
    run_step(&mut store, &t, &switch, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    let at = saved.task(1).unwrap().prepare[0].at;
    let old = [lead_seen(), on("codex/x/dev", "gpt-5", "medium", true)];
    assert!(!steps(&saved, &old, at + 59.0).contains(&Step::ModelUncertain { task: 1 }));
    let found = steps(&saved, &old, at + 60.0);
    assert!(found.contains(&Step::ModelUncertain { task: 1 }), "{found:?}");
    run_step(&mut store, &saved, &Step::ModelUncertain { task: 1 }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    let task = saved.task(1).unwrap();
    assert_eq!(task.prepare[0].result, "uncertain");
    assert!(task.waits.as_ref().unwrap().reason.contains("unconfirmed"), "{:?}", task.waits);
    // The session shows the model only later: still nothing without the lead.
    let late = [lead_seen(), on("codex/x/dev", "gpt-5.5", "high", true)];
    assert!(!steps(&saved, &late, at + 120.0).iter().any(|step| matches!(step, Step::HandOut { task: 1, .. } | Step::SwitchModel { .. })));
    assert_eq!(kinds(&store, &t.id), vec!["model_switch", "model_switch_uncertain"]);
    // A call whose outcome is unknown (no answer) is uncertain at once.
    let (_dir, mut store) = stored(&t);
    fake(|| anyhow::bail!("timed out after 120 s"));
    run_step(&mut store, &t, &switch, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(saved.task(1).unwrap().prepare[0].result, "uncertain");
    assert!(saved.task(1).unwrap().waits.is_some());
}

#[test]
fn a_task_for_a_role_goes_to_a_member_on_an_allowed_model_first() {
    let t = team_with(
        json!([
            member("dev", "Backend", "codex", "gpt-5", json!([{"model":"gpt-5"},{"model":"gpt-5.5"}])),
            member("dev2", "Backend", "codex", "gpt-5.5", json!([])),
            member("dev3", "Backend", "kimi", "k2", json!([])),
        ]),
        json!([{"id":1,"title":"API","role":"Backend","models":["gpt-5.5"]}]),
    );
    assert!(t.task(1).unwrap().assignee.is_empty());
    let all_free = [lead_seen(), on("codex/x/dev", "gpt-5", "", true), on("codex/x/dev2", "gpt-5.5", "", true), on("kimi/x/dev3", "k2", "", true)];
    let found = steps(&t, &all_free, 100.0);
    assert!(found.contains(&Step::Rebalance { task: 1, from: String::new(), to: "dev2".into() }), "{found:?}");
    // With dev2 busy, dev switches; dev3 (Kimi, no such model) never takes it.
    let dev2_busy = [lead_seen(), on("codex/x/dev", "gpt-5", "", true), on("codex/x/dev2", "gpt-5.5", "", false), on("kimi/x/dev3", "k2", "", true)];
    let found = steps(&t, &dev2_busy, 100.0);
    assert!(found.iter().any(|step| matches!(step, Step::SwitchModel { task: 1, member, .. } if member == "dev")), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::Rebalance { to, .. } | Step::HandOut { member: to, .. } if to == "dev3")), "{found:?}");
    // The hand-out makes the chosen member the assignee, without a rebalance.
    let (_dir, mut store) = stored(&t);
    fake(|| panic!("no switch"));
    run_step(&mut store, &t, &Step::Rebalance { task: 1, from: String::new(), to: "dev2".into() }, "").unwrap();
    set_fake_hgs(None);
    let saved = store.load(&t.id).unwrap().unwrap();
    let task = saved.task(1).unwrap();
    assert_eq!((task.assignee.as_str(), task.attempts[0].member.as_str(), task.attempts[0].rebalanced_from.as_str()), ("dev2", "dev2", ""));
    assert_eq!(kinds(&store, &t.id), vec!["handed_out", "delivered"]);
}

#[test]
fn rebalancing_never_moves_a_task_to_a_member_without_its_model() {
    let t = team_with(
        json!([
            member("dev", "Backend", "codex", "gpt-5.5", json!([])),
            member("dev2", "Backend", "kimi", "k2", json!([])),
        ]),
        json!([{"id":1,"title":"API","assignee":"dev","models":["gpt-5.5"]}]),
    );
    let busy = [lead_seen(), on("codex/x/dev", "gpt-5.5", "", false), on("kimi/x/dev2", "k2", "", true)];
    let found = steps(&t, &busy, 100.0);
    assert!(!found.iter().any(|step| matches!(step, Step::Rebalance { .. } | Step::HandOut { .. } | Step::TaskWaits { .. })), "{found:?}");
    // The same task without models goes to the free peer, as today.
    let mut plain = t.clone();
    plain.task_mut(1).unwrap().models.clear();
    assert!(steps(&plain, &busy, 100.0).contains(&Step::Rebalance { task: 1, from: "dev".into(), to: "dev2".into() }));
}

#[test]
fn a_pinned_member_that_cannot_switch_leaves_the_task_waiting() {
    let t = team_with(
        json!([member("qa", "Tester", "kimi", "k2", json!([{"model":"k2"},{"model":"k3"}]))]),
        json!([{"id":1,"title":"Tests","assignee":"qa","pinned":true,"models":["k3"]}]),
    );
    let seen = [lead_seen(), on("kimi/x/qa", "k2", "", true)];
    let found = steps(&t, &seen, 100.0);
    let reason = "Kimi cannot switch to model k3".to_owned();
    assert!(found.contains(&Step::TaskWaits { task: 1, reason: reason.clone() }), "{found:?}");
    let (_dir, mut store) = stored(&t);
    run_step(&mut store, &t, &Step::TaskWaits { task: 1, reason: reason.clone() }, "").unwrap();
    let saved = store.load(&t.id).unwrap().unwrap();
    assert_eq!(saved.task(1).unwrap().waits.as_ref().unwrap().reason, reason);
    assert!(steps(&saved, &seen, 100.0).iter().all(|step| !matches!(step, Step::TaskWaits { .. } | Step::HandOut { .. })));
}

#[test]
fn the_plan_checks_roles_and_models() {
    let (mut team, _) = ops::create("t", &json!({"name":"Models","lead":{"id":"lead","agent":"claude"}})).unwrap();
    let members = json!([member("dev", "Backend", "kimi", "k2", json!([]))]);
    let error = ops::apply(&mut team, "plan", &json!({"members":members,"tasks":[{"id":1,"title":"A","assignee":"dev","role":"Backend"}]}),
        &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap_err().to_string();
    assert!(error.contains("an assignee or a role, not both"), "{error}");
    ops::apply(&mut team, "plan", &json!({"members":members,"tasks":[
        {"id":1,"title":"A","role":"Designer"},
        {"id":2,"title":"B","role":"Backend","models":["k3"]},
        {"id":3,"title":"C","role":"Backend","models":["k2"]}]}), &Actor::Human, &BTreeSet::new(), &mut Vec::new()).unwrap();
    let found = plan::validate(&team, &BTreeSet::new());
    assert!(found.errors.iter().any(|f| f.task == Some(1) && f.text.contains("no member has the role Designer")), "{:?}", found.errors);
    assert!(!found.errors.iter().any(|f| f.task == Some(2) || f.task == Some(3)), "{:?}", found.errors);
    assert!(found.warnings.iter().any(|f| f.kind == "models" && f.task == Some(2) && f.text.contains("Kimi cannot switch to model k3")), "{:?}", found.warnings);
    assert!(!found.warnings.iter().any(|f| f.task == Some(3)), "{:?}", found.warnings);
}

/// #581 L1: a Vertex-style id with @ is one model, not a model and an effort;
/// only a known effort word after @ is split.
#[test]
fn a_model_id_with_at_is_split_only_on_a_known_effort() {
    assert_eq!(crate::state::teams::models::split("claude-sonnet-4-5@20250929"), ("claude-sonnet-4-5@20250929", ""));
    assert_eq!(crate::state::teams::models::split("claude-sonnet-4-5@high"), ("claude-sonnet-4-5", "high"));
    assert_eq!(crate::state::teams::models::split("gpt-5.5@HIGH"), ("gpt-5.5", "HIGH"));
    assert_eq!(crate::state::teams::models::split("model@"), ("model@", ""));
    let mut task = Task {
        id: 1,
        title: "A".into(),
        role: "Backend".into(),
        models: vec!["claude-sonnet-4-5@20250929".into()],
        ..Default::default()
    };
    crate::state::teams::models::check_task(&mut task).unwrap();
    assert_eq!(task.models, vec!["claude-sonnet-4-5@20250929"]);
}

/// #581 N2: dev (a reviewer who may switch to gpt-5.5) has a pending switch
/// for #1 while imp's #3 waits for review; at `at` the switch was sent.
fn switching_with_review(at: f64) -> Team {
    let mut t = team_with(
        json!([
            member("dev", "Reviewer", "codex", "gpt-5", json!([{"model":"gpt-5","effort":"medium"},{"model":"gpt-5.5","effort":"high"}])),
            member("imp", "Backend", "codex", "gpt-5", json!([])),
        ]),
        json!([{"id":1,"title":"API","assignee":"dev","models":["gpt-5.5@high"]},{"id":2,"title":"Docs","assignee":"dev"},
            {"id":3,"title":"Store","assignee":"imp"}]),
    );
    for member in &mut t.members {
        member.folder = MemberFolder::Shared;
    }
    t.task_mut(3).unwrap().pipeline = "code".into();
    super::super::pipeline::snapshot_with(&mut t, 3, &super::super::catalog::builtin());
    let task = t.task_mut(3).unwrap();
    task.attempts.push(Attempt { n: 1, id: "a3".into(), member: "imp".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    ops::apply(&mut t, "report", &json!({"task":3,"attempt":"a3","text":"Done"}), &Actor::Member("imp".into()), &BTreeSet::new(), &mut Vec::new()).unwrap();
    assert_eq!(t.task(3).unwrap().status, TaskStatus::InReview);
    super::super::models::record(
        t.task_mut(1).unwrap(),
        PrepareStep { step: "model".into(), member: "dev".into(), from: "gpt-5@medium".into(), to: "gpt-5.5@high".into(), at, result: "pending".into(), reason: String::new() },
    );
    t
}

#[test]
fn a_switching_member_takes_no_stage_and_gets_its_task_once() {
    let t = switching_with_review(100.0);
    assert!(super::super::stages::waiting(&t).iter().any(|(task, candidates)| *task == 3 && candidates.contains(&"dev".to_owned())));
    // While the switch is pending the review is not given to dev.
    let old = [lead_seen(), on("codex/x/dev", "gpt-5", "medium", true), on("codex/x/imp", "gpt-5", "", false)];
    let found = steps(&t, &old, 110.0);
    assert!(!found.iter().any(|step| matches!(step, Step::StageOut { member, .. } if member == "dev")), "{found:?}");
    assert!(!found.iter().any(|step| matches!(step, Step::HandOut { member, .. } if member == "dev")), "{found:?}");
    // Once it shows, dev gets #1 and only #1: no stage beside it.
    let now_on = [lead_seen(), on("codex/x/dev", "gpt-5.5", "high", true), on("codex/x/imp", "gpt-5", "", false)];
    let found = steps(&t, &now_on, 110.0);
    let dev: Vec<&Step> = found.iter().filter(|step| matches!(step, Step::StageOut { member, .. } | Step::HandOut { member, .. } | Step::Rebalance { to: member, .. } | Step::WorkAhead { member, .. } if member == "dev")).collect();
    assert_eq!(dev, vec![&Step::HandOut { task: 1, member: "dev".into() }], "{found:?}");
}

#[test]
fn a_confirmed_switch_hands_out_only_to_an_available_member_busy_with_nothing_else() {
    let on_new = |free: bool| [lead_seen(), on("codex/x/dev", "gpt-5.5", "high", free), on("codex/x/imp", "gpt-5", "", false)];
    let handed = |t: &Team, seen: &[Seen]| steps(t, seen, 110.0).contains(&Step::HandOut { task: 1, member: "dev".into() });
    let t = switching_with_review(100.0);
    assert!(handed(&t, &on_new(true)));
    // A session that is not free gets nothing yet.
    assert!(!handed(&t, &on_new(false)));
    // Nor one without a run identity.
    let mut no_run = on_new(true);
    no_run[1].run_id.clear();
    assert!(!handed(&t, &no_run));
    // Nor a member already busy with another open attempt.
    let mut other = t.clone();
    let task = other.task_mut(2).unwrap();
    task.attempts.push(Attempt { n: 1, id: "a2".into(), member: "dev".into(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    assert!(!handed(&other, &on_new(true)));
    // Nor a paused or draining member.
    let mut paused = t.clone();
    paused.members.iter_mut().find(|m| m.id == "dev").unwrap().state = MemberState::Paused;
    assert!(!handed(&paused, &on_new(true)));
    let mut draining = t.clone();
    draining.members.iter_mut().find(|m| m.id == "dev").unwrap().draining = true;
    assert!(!handed(&draining, &on_new(true)));
}
