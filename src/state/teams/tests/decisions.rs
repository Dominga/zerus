use super::*;
use crate::state::teams::model::{TaskStatus, TeamStatus};

fn team() -> Team {
    let mut team = Team { id: "t".into(), name: "Core".into(), lead: "lead".into(), status: TeamStatus::Running, ..Default::default() };
    team.tasks.push(Task { id: 1, title: "Lore embedding provider: talk to Ollama".into(), assignee: "dev".into(), status: TaskStatus::Pending, ..Default::default() });
    team.tasks.push(Task { id: 2, title: "Index format".into(), assignee: "dev".into(), status: TaskStatus::Pending,
        output: vec!["crates/lore/src/embed".into()], ..Default::default() });
    team.tasks.push(Task { id: 3, title: "Fix the tray icon colour".into(), assignee: "ui".into(), status: TaskStatus::Pending,
        output: vec!["tray/src/icon.cpp".into()], ..Default::default() });
    team
}

fn no_ollama() -> NewDecision {
    NewDecision {
        question: "Which embedding provider does lore use?".into(),
        choice: "The built-in candle embedding model, never Ollama".into(),
        why: "one binary, no service to run".into(),
        by: "owner".into(),
        spec: "docs/lore.md#2.1".into(),
        files: vec!["crates/lore/src/embed".into()],
        forbids: vec!["ollama".into()],
        ..NewDecision::default()
    }
}

#[test]
fn a_decision_reaches_later_tasks_by_output_path_and_by_meaning_only() {
    let mut team = team();
    let mut events = Vec::new();
    let id = record(&mut team, no_ollama(), 1_000.0, &mut events);
    assert_eq!(id, "d1");
    assert_eq!(events[0].kind, "decision_recorded");
    let by_meaning = binds(&team, &team.decisions[0], team.task(1).unwrap());
    assert_eq!(by_meaning, Some("same subject"), "no path in common: the subject binds it");
    let by_path = binds(&team, &team.decisions[0], team.task(2).unwrap());
    assert_eq!(by_path, Some("touches its outputs"));
    assert_eq!(binds(&team, &team.decisions[0], team.task(3).unwrap()), None, "an unrelated task is not bound");
    let text = block(&team, team.task(2).unwrap());
    assert!(text.starts_with("\nDecisions that bind this task"), "{text}");
    assert!(text.contains("- d1 (owner, 1970-01-01): Which embedding provider does lore use? -> The built-in candle embedding model, never Ollama"), "{text}");
    assert!(text.contains("Forbids: ollama") && text.contains("Amends docs/lore.md#2.1") && text.contains("[touches its outputs]"), "{text}");
    assert!(block(&team, team.task(3).unwrap()).is_empty());
    // Lore's own hits bind too (a workspace team).
    team.decision_hits.insert(3, crate::state::teams::model::DecisionHits { hash: "h".into(), ids: vec!["d1".into()] });
    assert_eq!(binds(&team, &team.decisions[0], team.task(3).unwrap()), Some("found by lore"));
}

#[test]
fn a_later_decision_on_the_same_question_supersedes_and_stops_the_old_binding() {
    let mut team = team();
    let mut events = Vec::new();
    record(&mut team, no_ollama(), 1_000.0, &mut events);
    let mut changed = no_ollama();
    changed.choice = "Ollama is allowed as an optional provider".into();
    changed.forbids.clear();
    let second = record(&mut team, changed, 2_000.0, &mut events);
    assert_eq!(team.decisions[0].superseded_by, second, "the old one stays, marked");
    assert_eq!(current(&team).map(|found| found.id.as_str()).collect::<Vec<_>>(), [second.as_str()]);
    let text = block(&team, team.task(2).unwrap());
    assert!(text.contains("- d2") && !text.contains("- d1"), "a superseded decision binds nothing: {text}");
    assert!(conflicts(&team, team.task(2).unwrap(), "ollama-rs = \"0.2\"").is_empty(), "nor forbids anything");
    // An explicit supersede names the one it replaces.
    let other = record(&mut team, NewDecision { question: "Release channel".into(), choice: "dev".into(), supersedes: "d2".into(), ..NewDecision::default() }, 3_000.0, &mut events);
    assert_eq!(team.decisions[1].superseded_by, other);
}

#[test]
fn a_contradicting_report_is_flagged_as_a_whole_word() {
    let mut team = team();
    record(&mut team, no_ollama(), 1_000.0, &mut Vec::new());
    let task = team.task(2).unwrap().clone();
    let flags = conflicts(&team, &task, "[dependencies]\nollama-rs = \"0.2\"\n");
    assert_eq!(flags, vec![json!({"decision":"d1","term":"ollama","choice":"The built-in candle embedding model, never Ollama"})]);
    assert!(conflicts(&team, &task, "struct Ollamalike;").is_empty(), "part of a word is no use of it");
    // A decision for other work is not broken by an unrelated task.
    assert!(conflicts(&team, team.task(3).unwrap(), "ollama").is_empty());
    // A decision with no paths and no tasks binds every report.
    record(&mut team, NewDecision { question: "Telemetry".into(), choice: "none".into(), forbids: vec!["sentry".into()], ..NewDecision::default() }, 2_000.0, &mut Vec::new());
    assert_eq!(conflicts(&team, team.task(3).unwrap(), "use sentry::init;")[0]["decision"], "d2");
}

#[test]
fn the_spec_section_is_generated_from_the_records() {
    let mut team = team();
    record(&mut team, no_ollama(), 1_000.0, &mut Vec::new());
    let mut changed = no_ollama();
    changed.choice = "candle only".into();
    record(&mut team, changed, 2_000.0, &mut Vec::new());
    assert_eq!(specs(&team).into_iter().collect::<Vec<_>>(), ["docs/lore.md"]);
    let section = spec_section(&team, "docs/lore.md");
    assert!(section.starts_with(BEGIN) && section.ends_with(END), "{section}");
    assert!(section.contains("- ~~d1 (owner, 1970-01-01)") && section.contains("(superseded by d2)") && section.contains("- d2 (owner"), "{section}");
    let appended = render_into("# Lore\n\nText.\n", &section);
    assert!(appended.starts_with("# Lore\n\nText.\n\n") && appended.contains(BEGIN));
    let replaced = render_into(&format!("{appended}After.\n"), "NEW");
    assert_eq!(replaced, "# Lore\n\nText.\n\nNEW\nAfter.\n");
    let hash = specs_hash(&team);
    team.decisions[1].choice = "candle, really".into();
    assert_ne!(specs_hash(&team), hash, "a changed record regenerates the section");
}

#[test]
fn a_card_answer_and_a_message_that_hands_down_a_decision() {
    let card = json!({"id":"p4","kind":"question","reason":"which provider","change":{"question":"Embedding provider?"},
        "refs":{"spec":"docs/lore.md","files":["crates/lore"],"tasks":[2],"forbids":["ollama"]},"for_tasks":[1]});
    let answered = from_card(&card, "answered", "built-in only", "built-in only");
    assert_eq!((answered.question.as_str(), answered.choice.as_str(), answered.card.as_str()), ("Embedding provider?", "built-in only", "p4"));
    assert_eq!((answered.spec.as_str(), answered.files.clone(), answered.tasks.clone(), answered.forbids.clone()),
        ("docs/lore.md", vec!["crates/lore".to_owned()], vec![2, 1], vec!["ollama".to_owned()]));
    let denied = from_card(&json!({"id":"p5","kind":"rights","change":{"summary":"widen qa"}}), "rejected", "", "");
    assert_eq!((denied.question.as_str(), denied.choice.as_str(), denied.why.as_str()), ("rights: widen qa", "denied: widen qa", "no reason given"));
    assert!(decision_like("The owner decided: no Ollama anywhere"));
    assert!(decision_like("владелец решил: без Ollama"));
    assert!(!decision_like("Please rebase on the lead branch"));
    assert!(!decision_like("The owner is away tonight"));
    // Task #36 (team ede724c4): a real decision is still one ...
    assert!(decision_like("Owner says no to Ollama"));
    assert!(decision_like("This is the owner's decision: Postgres only"));
    assert!(decision_like("A decision of the owner: no Ollama"));
    assert!(decision_like("Решение владельца: без Ollama"));
    // ... while a hand-out that quotes the owner and its decisions as context is not.
    assert!(!decision_like("Owner, 2026-11-10. Slice docs/reference.md into CodeLore records, one per heading."));
    assert!(!decision_like("Task #3: index the guides.\nDecisions that bind this task (the owner's; follow them, and hgs team ask if one seems wrong):\n- d17 (owner, 2026-11-10): extend: Add a phase of 7 tasks -> allowed"));
    assert!(!decision_like("Ask the owner before you decide on the schema"));
    assert!(refusal("t").contains("hgs team decide --json"));
}
