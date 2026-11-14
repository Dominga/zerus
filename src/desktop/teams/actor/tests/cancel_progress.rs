//! Review 08x, delta 2: the cancel bound counts stale answers whose
//! observation made no progress. Progress is what the command's stale check
//! reads, not every field of the task. Production cancel admission and
//! decoder (fixture child exit) and production fact saving.
use super::super::{
    cancel::{Answer, Fact, Pending},
    native::Native,
    ports::{DeliveryResult, Ports},
    Runtime,
};
use super::*;
use crate::teams::ops::{HandoutOutcome, Operation, TaskStage};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Moves {
    Nothing,
    Fail,
    Revision,
    Title,
    Rank,
}
struct Command {
    native: Native,
    node: u32,
    state: &'static str,
    sentence: String,
    moves: Moves,
    now: i64,
    calls: Vec<i64>,
    facts: Vec<Fact>,
    inflate_first_fact: bool,
}
impl Command {
    fn new(h: &Harness, node: u32, state: &'static str, sentence: &str) -> Self {
        Self {
            native: super::production::native(h),
            node,
            state,
            sentence: sentence.into(),
            moves: Moves::Nothing,
            now: 0,
            calls: vec![],
            facts: vec![],
            inflate_first_fact: false,
        }
    }
}
/// A new projection of the node's task that moves one field only.
fn bump(store: &mut Store, node: u32, moves: Moves, now: i64) -> Result<()> {
    let mut o = store
        .task_observation(TEAM, node)?
        .ok_or_else(|| super::super::problem("fixture: no observation"))?;
    o.previous_revision = o.revision.clone();
    o.revision = uuid::Uuid::new_v4().to_string();
    match moves {
        Moves::Title => o.title = format!("renamed {}", &o.revision[..8]),
        Moves::Rank => o.rank = format!("b{}", &o.revision[..8]),
        _ => (),
    }
    let observe = Operation::ObserveTask {
        node,
        observation: o,
    };
    super::super::write(store, TEAM, observe, now).map(|_| ())
}
impl Ports for Command {
    fn changes(&mut self, _: &str, _: &str, _: u64) -> Result<zerus_replica::Changes> {
        Ok(zerus_replica::Changes {
            generation: "journal".into(),
            offset: 0,
            documents: Some(Default::default()),
        })
    }
    fn observe(&mut self, store: &mut Store, _: &TaskRef) -> Result<()> {
        match self.moves {
            Moves::Nothing => Ok(()),
            Moves::Fail => Err(super::super::problem("fixture: the task cannot be read")),
            moves => bump(store, self.node, moves, self.now),
        }
    }
    fn free(&mut self, _: &Team, _: &Member) -> Result<bool> {
        Ok(false)
    }
    fn deliver(&mut self, _: &Team, _: &Node, _: &Member, _: &str) -> DeliveryResult {
        unreachable!("no hand-out in a cancel fixture")
    }
    fn cancels(&mut self, store: &Store, team: &str, after: u32) -> Result<Vec<Pending>> {
        Native::pending_cancels(store, team, after)
    }
    fn cancel(&mut self, team: &Team, pending: &Pending, original: &str) -> Result<Answer> {
        self.calls.push(self.now);
        let body = json!({"schema":1,"team":team.id,"node":pending.node,"state":self.state,"error":self.sentence});
        crate::desktop::teams::child::FIXTURE_EXIT
            .with(|slot| *slot.borrow_mut() = Some(body.to_string().into_bytes()));
        self.native.run_cancel(team, pending, original)
    }
    fn record_cancel(
        &mut self,
        store: &mut Store,
        team: &Team,
        fact: &Fact,
        now: i64,
    ) -> Result<()> {
        self.facts.push(fact.clone());
        let mut attempted = fact.clone();
        if self.inflate_first_fact && self.facts.len() == 1 {
            // The store refuses the original fact: saved over_cap.
            attempted.reason = "x".repeat(5000);
        }
        Native::save_cancel(store, team, &attempted, now)
    }
}
fn delivered(h: &mut Harness) -> u32 {
    h.bind("m1");
    let node = h.add(&person(), 1);
    let effect = h.intent(node, "m1");
    h.outcome(
        &effect,
        HandoutOutcome::Submitted {
            receipt: "delivered".into(),
        },
    );
    node
}
fn stopped() -> (Harness, u32) {
    let mut h = Harness::new();
    let node = delivered(&mut h);
    h.run(&person(), Operation::Stop).unwrap();
    (h, node)
}
fn live() -> (Harness, u32) {
    let mut h = Harness::new();
    let node = delivered(&mut h);
    h.observe(node, TaskStage::InProgress);
    let cancel = Operation::Cancel {
        node,
        reason: "Stop this task".into(),
    };
    h.run(&person(), cancel).unwrap();
    (h, node)
}
/// One-minute production passes: tick while the team is live, the page's
/// terminal entry after a stop.
fn passes(h: &mut Harness, p: &mut Command, n: i64) {
    let mut runtime = Runtime::open(h.temp.path(), &p.native.machine).unwrap();
    for pass in 0..n {
        p.now = 1_000 + pass * 60_000;
        runtime.begin_pass();
        let team = h.store.team(TEAM).unwrap().unwrap();
        let _ = if matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
            runtime.tick(&mut h.store, p, TEAM, p.now, false)
        } else {
            runtime.page(&mut h.store, p, &[], p.now, false)
        };
    }
}
fn minutes(p: &Command) -> Vec<i64> {
    p.calls.iter().map(|t| (t - 1_000) / 60_000).collect()
}
fn outcomes(h: &Harness) -> Vec<(String, String)> {
    let db = h.store.db.as_ref().unwrap();
    let mut q = db
        .prepare("SELECT state,reason FROM cancel_outcomes ORDER BY rowid")
        .unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// E2: a live team, the command answers stale every time, and each
/// observation moves only one field for four hours. A field the stale check
/// does not read is no progress: three answers, then one refused outcome.
#[test]
fn stale_answers_count_when_only_fields_the_stale_check_does_not_read_move() {
    for moves in [
        Moves::Nothing,
        Moves::Revision,
        Moves::Title,
        Moves::Rank,
        Moves::Fail,
    ] {
        let (mut h, node) = live();
        let mut p = Command::new(&h, node, "stale", "Observe this task");
        p.moves = moves;
        passes(&mut h, &mut p, 240);
        let pending = h.store.pending_cancels(TEAM, 16, 0).unwrap();
        if moves == Moves::Fail {
            // The cached stale answer is kept while observation retries.
            assert_eq!(minutes(&p), [0], "{moves:?}");
            assert!(p.facts.is_empty(), "{moves:?}");
            assert_eq!(pending.len(), 1, "{moves:?}");
            continue;
        }
        assert_eq!(minutes(&p), [0, 1, 3], "{moves:?}");
        assert_eq!(p.facts.len(), 1, "{moves:?}");
        assert!(p.facts[0].refused, "{moves:?}");
        assert!(pending.is_empty(), "{moves:?}");
        let recorded = outcomes(&h);
        assert_eq!(recorded.len(), 1, "{moves:?}");
        assert_eq!(recorded[0].0, "refused");
        assert!(recorded[0].1.contains("made no progress"), "{moves:?}");
    }
}

/// One field of an observation, named, and how a test moves it.
type Move = (&'static str, fn(&mut crate::teams::ops::TaskObservation));
/// The fingerprint changes with each field the stale check reads and with
/// nothing else of the observation.
#[test]
fn the_progress_fingerprint_covers_exactly_what_the_stale_check_reads() {
    use crate::teams::ops::{KeyState, TaskObservation};
    let base = TaskObservation {
        revision: "r1".into(),
        key: KeyState::Ours,
        previous_revision: "r0".into(),
        stage: TaskStage::InProgress,
        current_cancelled: false,
            cancelled: false,
        rank: "a1".into(),
        title: "Task 1".into(),
        attempt: 1,
        attempt_id: "attempt-1".into(),
        attempt_conflict: false,
    };
    let print = |o: &TaskObservation| Runtime::cancel_progress(o).unwrap();
    let reads: [Move; 6] = [
        ("key", |o| o.key = KeyState::Other),
        ("stage", |o| o.stage = TaskStage::InReview),
        ("cancelled", |o| o.cancelled = true),
        ("attempt", |o| o.attempt = 2),
        ("attempt_id", |o| o.attempt_id = "attempt-2".into()),
        ("attempt_conflict", |o| o.attempt_conflict = true),
    ];
    for (field, change) in reads {
        let mut o = base.clone();
        change(&mut o);
        assert_ne!(print(&o), print(&base), "{field} is read");
    }
    let others: [Move; 4] = [
        ("revision", |o| o.revision = "r2".into()),
        ("previous_revision", |o| o.previous_revision = "r1".into()),
        ("rank", |o| o.rank = "b1".into()),
        ("title", |o| o.title = "Renamed".into()),
    ];
    for (field, change) in others {
        let mut o = base.clone();
        change(&mut o);
        assert_eq!(print(&o), print(&base), "{field} is not read");
    }
}

/// E5: the longest sentence a command can return, with whitespace runs, BEL
/// and other control characters, CJK, accents and emoji, through both exits:
/// the stale limit of a live team and a store refusal in a stopped team. The
/// recorded reasons and the lead's items carry no control character and
/// stay under two kibibytes.
#[test]
fn a_commands_sentence_reaches_the_outcome_without_control_characters() {
    let long = format!(
        "{}{}{}{}",
        "\u{754C} \t\n\u{0007}".repeat(500),
        "\u{001B}[31m\u{0000}\u{007F}\u{009B} ".repeat(20),
        "\u{00E9}. ".repeat(900),
        "\u{1F600}".repeat(400)
    );
    let clean = |text: &str, what: &str| {
        assert!(!text.chars().any(char::is_control), "{what}: {text:?}");
        assert!(text.len() <= 2048, "{what}: {} bytes", text.len());
    };
    let (mut h, node) = live();
    let mut p = Command::new(&h, node, "stale", &long);
    passes(&mut h, &mut p, 60);
    assert_eq!(minutes(&p), [0, 1, 3]);
    let recorded = outcomes(&h);
    assert_eq!(recorded.len(), 1);
    assert!(recorded[0]
        .1
        .starts_with("The cancel command answered stale: \u{754C} \u{754C} "));
    assert!(recorded[0].1.contains("made no progress"));
    let (mut s, stopped_node) = stopped();
    let mut q = Command::new(&s, stopped_node, "refused", &long);
    q.inflate_first_fact = true;
    passes(&mut s, &mut q, 30);
    assert_eq!(q.calls.len(), 1);
    assert_eq!(q.facts.len(), 2, "the original, then the exit");
    let exit = outcomes(&s);
    assert_eq!(exit.len(), 1);
    assert!(exit[0]
        .1
        .starts_with("The cancel command answered refused: "));
    assert!(exit[0].1.contains("The team store refused this outcome"));
    for (h, label) in [(&h, "live"), (&s, "stopped")] {
        for (_, reason) in outcomes(h) {
            clean(&reason, label);
        }
        for item in h.store.inbox(TEAM, 200, None).unwrap().rows {
            clean(&item.text, label);
        }
    }
    for fact in p.facts.iter().chain(&q.facts[1..]) {
        clean(&fact.reason, "fact");
    }
}

/// Review 44: a cutting_over reply's sentence becomes the row's error and,
/// after three failures, the retry card's text. It is cleaned as the
/// command's other sentences are: no control character, whitespace
/// normalized, cut.
#[test]
fn a_cutting_over_sentence_reaches_the_failure_card_clean() {
    let hostile = format!(
        "{}{}",
        "busy\u{0007} \t\n\u{001B}\u{0000}\u{009B}".repeat(200),
        "x".repeat(600)
    );
    let (mut h, node) = live();
    let mut p = Command::new(&h, node, "cutting_over", &hostile);
    passes(&mut h, &mut p, 10);
    let cards = h.store.cards(TEAM, 200, None).unwrap().rows;
    let card = cards
        .iter()
        .find(|c| c.kind == "team_actor_blocked")
        .expect("the retry card after three failures");
    assert!(!card.text.chars().any(char::is_control), "{:?}", card.text);
    assert!(card.text.contains("busy busy busy"), "{}", card.text);
    assert!(p.facts.is_empty(), "a held project records no outcome");
    assert_eq!(h.store.pending_cancels(TEAM, 16, 0).unwrap().len(), 1);
}
