use super::*;

pub(crate) fn running_team() -> Team {
    let (mut team, _) = create(
        "t",
        &json!({"name":"Checkout","lead":{"id":"lead","agent":"claude","session":{"name":"claude/x/lead"}}}),
    )
    .unwrap();
    let mut events = Vec::new();
    plan(
        &mut team,
        &json!({"members":[{"id":"dev","role":"Backend","agent":"codex"},{"id":"qa","role":"Tester","agent":"kimi"}],
            "tasks":[{"id":1,"title":"API","assignee":"dev"},{"id":2,"title":"Tests","assignee":"qa","deps":[{"task":1,"kind":"soft"}]}]}),
        &Actor::Member("lead".into()),
        &mut events,
    )
    .unwrap();
    approve(&mut team, &json!({}), &Actor::Human, &BTreeSet::new(), &mut events).unwrap();
    team
}

/// What the dispatcher records for a delivered hand-off.
pub(crate) fn hand_out(team: &mut Team, id: u32) -> String {
    let task = team.task_mut(id).unwrap();
    let n = task.attempts.len() as u32 + 1;
    let attempt = format!("00000000-0000-4000-8000-00000000000{n}");
    task.attempts.push(Attempt { n, id: attempt.clone(), member: task.assignee.clone(), state: AttemptState::Delivered, ..Default::default() });
    task.status = TaskStatus::InProgress;
    attempt
}

/// The running team with "qalead" as the Tester's sub-lead (spec 5.6).
pub(crate) fn team_with_sub_lead() -> Team {
    let mut team = running_team();
    let none = BTreeSet::new();
    let mut events = Vec::new();
    apply(&mut team, "add-member", &json!({"member":{"id":"qalead","role":"QA lead","agent":"codex"}}), &Actor::Human, &none, &mut events)
        .unwrap();
    apply(&mut team, "sub-lead", &json!({"member":"qalead","sub":true}), &Actor::Member("lead".into()), &none, &mut events).unwrap();
    apply(&mut team, "reports-to", &json!({"member":"qa","parent":"qalead"}), &Actor::Human, &none, &mut events).unwrap();
    team
}

pub(crate) fn kinds(events: &[Event]) -> Vec<String> {
    events.iter().map(|event| event.kind.to_owned()).collect()
}
