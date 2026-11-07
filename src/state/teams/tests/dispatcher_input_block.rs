//! Task 98 (seen 2026-11-06): a member's composer held Zerus's own pasted but
//! unsubmitted delivery, and every later hand-out was refused 26 times with
//! nobody told. A fake terminal answers the hand-outs: Zerus's own leftover
//! paste is finished once and the hand-out goes; any other draft is never
//! submitted and raises one item after three refusals.
use super::support::*;
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

const REFUSED: &str = "agent busy: terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt [trace: agent=claude cursor=(2,40) prompt_empty=false row=\"> [Pasted text #17 +62 lines]\"]";
const SCREEN: &str = "\u{1b}[2m✻ Compacted conversation\u{1b}[0m\n\n\u{1b}[33m  Update installed · Restart to apply\u{1b}[0m\n\n\u{1b}]0;title\u{7}╭──────────────╮\n│ > Hello there │\n╰──────────────╯\n";

/// What the fake terminal did: each hgs call's command and terminal action.
type Calls = Rc<RefCell<Vec<String>>>;

/// A fake terminal: the first `refusals` sends are refused for a draft, then
/// they go; `own` says whether the draft is the paste Zerus left.
fn fake_terminal(refusals: usize, own: bool) -> Calls {
    let calls: Calls = Rc::default();
    let seen = calls.clone();
    let mut sends = 0;
    set_fake_hgs(Some(Box::new(move |_, args: &[&str], input| {
        let action = input.and_then(|input| input["action"].as_str()).unwrap_or_default().to_owned();
        seen.borrow_mut().push(if args[0] == "terminal" { format!("terminal {action}") } else { args[0].to_owned() });
        Ok(match (args[0], action.as_str()) {
            ("send", _) => {
                sends += 1;
                if sends <= refusals {
                    fake_output(1, "", REFUSED)
                } else {
                    fake_output(0, r#"{"status":"submitted"}"#, "")
                }
            }
            ("terminal", "submit_own_paste") if own => fake_output(0, r#"{"status":"submitted","delivery":"d-17"}"#, ""),
            ("terminal", "submit_own_paste") => {
                fake_output(0, r#"{"status":"failed","error":"The Terminal draft is not a paste Zerus delivered; nothing was sent"}"#, "")
            }
            ("terminal", "snapshot") => fake_output(0, &json!({"status":"snapshot","screen":SCREEN}).to_string(), ""),
            other => panic!("unexpected hgs {other:?}"),
        })
    })));
    calls
}

fn hand_out_once(store: &mut Store, id: &str) -> Team {
    let team = store.load(id).unwrap().unwrap();
    run_step(store, &team, &Step::HandOut { task: 1, member: "dev".into() }, "").unwrap();
    store.load(id).unwrap().unwrap()
}

fn running() -> (tempfile::TempDir, Store, Team) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open_at(&dir.path().join("teams.sqlite3"), true).unwrap();
    let mut t = team();
    t.status = TeamStatus::Running;
    stored(&mut store, &t);
    (dir, store, t)
}

#[test]
fn zeruss_own_leftover_paste_is_finished_once_and_the_hand_out_goes() {
    let (_dir, mut store, t) = running();
    let calls = fake_terminal(1, true);
    let after = hand_out_once(&mut store, &t.id);
    // Refused for the draft; the paste Zerus left is finished with its Enter.
    assert_eq!(after.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Refused);
    assert!(after.member("dev").unwrap().input_block.is_none());
    assert!(event_kinds(&store, &t.id).contains(&"own_paste_finished".to_owned()));
    // The next hand-out goes.
    let after = hand_out_once(&mut store, &t.id);
    assert_eq!(after.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Delivered);
    assert_eq!(*calls.borrow(), ["send", "terminal submit_own_paste", "send"]);
    // Never the owner's submit_draft, and never twice.
    assert!(!calls.borrow().iter().any(|call| call.contains("submit_draft")));
    set_fake_hgs(None);
}

#[test]
fn a_foreign_draft_raises_one_item_after_three_refusals_and_is_never_submitted() {
    let (_dir, mut store, t) = running();
    let calls = fake_terminal(4, false);
    for n in 1..=2 {
        let after = hand_out_once(&mut store, &t.id);
        let block = after.member("dev").unwrap().input_block.clone().unwrap();
        assert_eq!((block.count, block.raised), (n, false));
        assert_eq!(block.reason, "agent busy: terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt");
        assert!(!super::super::inbox::items(&after).iter().any(|item| item.kind == "input_blocked"));
    }
    let mut after = hand_out_once(&mut store, &t.id);
    let block = after.member("dev").unwrap().input_block.clone().unwrap();
    assert_eq!((block.count, block.raised), (3, true));
    // The screen's last lines, without its colors and titles.
    assert_eq!(block.screen.last().map(String::as_str), Some("╰──────────────╯"));
    assert!(block.screen.contains(&"│ > Hello there │".to_owned()) && block.screen.iter().all(|line| !line.contains('\u{1b}')));
    // One item for the lead, and one for the owner, with what to do.
    let lead: Vec<_> = super::super::inbox::items(&after).into_iter().filter(|item| item.kind == "input_blocked").collect();
    assert_eq!(lead.len(), 1);
    // Task 165: one item per member, saying the terminal is busy and why.
    assert!(lead[0].summary.contains("terminal is busy: agent busy") && lead[0].detail.contains("Hello there"), "{:?}", lead[0]);
    assert_eq!(lead[0].key, "input_blocked:dev");
    assert!(lead[0].detail.contains("never submits a draft it did not paste"));
    let waiting = super::super::owner_waiting::items(std::slice::from_ref(&after), now());
    let owner: Vec<_> = waiting.iter().filter(|item| item["kind"] == "input_blocked").collect();
    assert_eq!((owner.len(), owner[0]["action"].as_str()), (1, Some("open_terminal")));
    // A fourth refusal counts, raises nothing new and reads no screen again.
    after = hand_out_once(&mut store, &t.id);
    assert_eq!(after.member("dev").unwrap().input_block.as_ref().unwrap().count, 4);
    assert_eq!(event_kinds(&store, &t.id).iter().filter(|kind| *kind == "input_blocked").count(), 1);
    let snapshots = calls.borrow().iter().filter(|call| *call == "terminal snapshot").count();
    assert_eq!(snapshots, 1);
    // Zerus only asked whether the draft was its own; it never submitted it.
    assert!(calls.borrow().iter().all(|call| matches!(call.as_str(), "send" | "terminal submit_own_paste" | "terminal snapshot")));
    // The person cleared it: the next hand-out goes and the episode ends.
    after = hand_out_once(&mut store, &t.id);
    assert_eq!(after.task(1).unwrap().attempts.last().unwrap().state, AttemptState::Delivered);
    assert!(after.member("dev").unwrap().input_block.is_none());
    assert!(event_kinds(&store, &t.id).contains(&"input_unblocked".to_owned()));
    set_fake_hgs(None);
}

#[test]
fn another_refusal_is_not_counted_as_a_draft() {
    let (_dir, mut store, t) = running();
    set_fake_hgs(Some(Box::new(|_, args: &[&str], _| {
        assert_eq!(args[0], "send", "no terminal read for another refusal");
        Ok(fake_output(1, "", "session is busy"))
    })));
    let after = hand_out_once(&mut store, &t.id);
    assert!(after.member("dev").unwrap().input_block.is_none());
    set_fake_hgs(None);
}

#[test]
fn last_lines_drop_colors_titles_and_blank_lines() {
    assert_eq!(last_lines(SCREEN, 2), ["│ > Hello there │", "╰──────────────╯"]);
    assert_eq!(last_lines("", 3), Vec::<String>::new());
    assert_eq!(last_lines(&format!("{}\n", "x".repeat(300)), 1)[0].chars().count(), 200);
}
