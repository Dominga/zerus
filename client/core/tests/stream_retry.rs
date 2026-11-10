//! Restart schedules of the real supervisor against the fake `hgs`
//! (tests/support/fake_hgs.py): the published wait is the one that ends the
//! supervisor's wait, Reconnect now ends only that wait, and the schedule
//! reaches both the direct fleet sink and the service source channel.
mod common;

use common::{catalog, change_of, hello, status_of, FakeHgs};
use serde_json::json;
use std::process::{Child, Command};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use zerus_client_core::source::{self, Event};
use zerus_client_core::stream;
use zerus_client_core::{
    Change, ComputerId, ComputerStatus, NextAttempt, Retry, StreamEvent, Timing,
};

const WAIT: Duration = Duration::from_secs(20);

fn timing(restart: Duration) -> Timing {
    Timing {
        restart: [restart; 4].into(),
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(0.002)
    }
}

fn retry_of(event: &StreamEvent) -> Option<Retry> {
    match change_of(event)? {
        Change::Retry { computer, retry } if computer.is_local() => Some(*retry),
        _ => None,
    }
}

/// Events with the moment each one was received.
struct Timed {
    rx: Receiver<StreamEvent>,
    all: Vec<(Instant, StreamEvent)>,
}

impl Timed {
    fn new(rx: Receiver<StreamEvent>) -> Self {
        Self {
            rx,
            all: Vec::new(),
        }
    }
    fn until(&mut self, what: &str, matches: impl Fn(&StreamEvent) -> bool) -> usize {
        if let Some(index) = self.all.iter().position(|(_, event)| matches(event)) {
            return index;
        }
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .rx
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("timed out waiting for {what}: {:#?}", self.all));
            let hit = matches(&event);
            self.all.push((Instant::now(), event));
            if hit {
                return self.all.len() - 1;
            }
        }
    }
    fn drain_for(&mut self, time: Duration) {
        let deadline = Instant::now() + time;
        while let Ok(event) = self
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            self.all.push((Instant::now(), event));
        }
    }
    fn waiting(&self, index: usize) -> NextAttempt {
        match retry_of(&self.all[index].1) {
            Some(Retry::Waiting(next)) => next,
            other => panic!("not a wait: {other:?}"),
        }
    }
}

fn is_waiting(serial: u64) -> impl Fn(&StreamEvent) -> bool {
    move |event| matches!(retry_of(event), Some(Retry::Waiting(next)) if next.serial == serial)
}

fn status_is(status: ComputerStatus) -> impl Fn(&StreamEvent) -> bool {
    move |event| {
        change_of(event).and_then(|change| status_of(change, &ComputerId::local())) == Some(status)
    }
}

#[test]
fn the_published_wait_precedes_its_status_and_ends_at_its_deadline() {
    let fake = FakeHgs::new(
        "retry_published",
        json!({"": {"runs": [[{"exit": 255}],
            [{"lines": [hello("workstation"), catalog(&[("r1", "claude/sample-project/review")])]}, {"serve": true}]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Timed::new(rx);
    let handle = stream::spawn(
        &fake.path(),
        &ComputerId::local(),
        timing(Duration::from_millis(300)),
        tx,
    );
    let wait = seen.until("the first wait", is_waiting(1));
    let reconnecting = seen.until(
        "reconnecting",
        status_is(ComputerStatus::Reconnecting { attempt: 1 }),
    );
    assert!(
        wait < reconnecting,
        "the wait is published before its status"
    );
    let next = seen.waiting(wait);
    assert_eq!((next.attempt, next.delay_ms), (1, 300));
    let attempting = seen.until("the attempt", |event| {
        retry_of(event)
            == Some(Retry::Attempting {
                serial: 1,
                attempt: 1,
            })
    });
    // The attempt starts only once the published deadline passed.
    assert!(seen.all[attempting].0 >= next.deadline);
    let live = seen.until("live", status_is(ComputerStatus::Live));
    let idle = seen.until("idle", |event| retry_of(event) == Some(Retry::Idle));
    assert!(
        attempting < idle && idle < live,
        "a connected computer has no schedule"
    );
    let starts = fake.runs("", &["watch", "--json"]);
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert!(starts[1] - starts[0] >= 0.3, "{starts:?}");
    handle.stop();
}

#[test]
fn a_computer_that_comes_up_later_is_found_within_the_fast_default_schedule() {
    let fake = FakeHgs::new(
        "retry_late_host",
        json!({"": {"runs": [[{"exit": 255}], [{"exit": 255}], [{"exit": 255}],
            [{"lines": [hello("workstation"), catalog(&[])]}, {"serve": true}]]}}),
    );
    // The default schedule scaled by 0.05: 50, 100, 250, then 500 ms.
    let factor = 0.05;
    let timing = Timing {
        stop_grace: Duration::from_millis(500),
        ..Timing::scaled(factor)
    };
    let (tx, rx) = channel();
    let mut seen = Timed::new(rx);
    let began = Instant::now();
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), timing.clone(), tx);
    seen.until("live", status_is(ComputerStatus::Live));
    let found = began.elapsed();
    let starts = fake.runs("", &["watch", "--json"]);
    assert_eq!(starts.len(), 4, "{starts:?}");
    for attempt in 1..4u32 {
        let gap = starts[attempt as usize] - starts[attempt as usize - 1];
        assert!(
            gap >= timing.restart_delay(attempt).as_secs_f64(),
            "{starts:?}"
        );
    }
    // Control: the former 5, 15 and 60 s waits, scaled the same way, alone
    // take 4 s before the fourth start.
    let former: f64 = [5.0, 15.0, 60.0].iter().map(|s| s * factor).sum();
    assert!(
        found.as_secs_f64() < former,
        "found after {found:?}, the former schedule waits {former} s"
    );
    handle.stop();
}

#[test]
fn reconnect_now_ends_only_the_current_wait_once_and_keeps_the_rows() {
    let fake = FakeHgs::new(
        "retry_manual",
        json!({"": {"runs": [[{"lines": [hello("workstation"), catalog(&[("r1", "claude/sample-project/review")])]}],
            [{"exit": 255}]]}}),
    );
    let (tx, rx) = channel();
    let mut seen = Timed::new(rx);
    let handle = stream::spawn(
        &fake.path(),
        &ComputerId::local(),
        timing(Duration::from_secs(3600)),
        tx,
    );
    seen.until("the hour-long wait", is_waiting(1));
    seen.until(
        "reconnecting",
        status_is(ComputerStatus::Reconnecting { attempt: 1 }),
    );
    // Control: nothing starts by itself during the wait.
    seen.drain_for(Duration::from_millis(300));
    assert_eq!(fake.runs("", &["watch", "--json"]).len(), 1);
    assert!(
        handle.reconnect(),
        "the waiting supervisor takes the request"
    );
    assert!(!handle.reconnect(), "a repeated click coalesces");
    seen.until("the attempt", |event| {
        matches!(retry_of(event), Some(Retry::Attempting { serial: 1, .. }))
    });
    seen.until("the next wait", is_waiting(2));
    seen.drain_for(Duration::from_millis(300));
    assert_eq!(
        fake.runs("", &["watch", "--json"]).len(),
        2,
        "exactly one extra attempt"
    );
    assert_eq!(fake.runs("", &["-h"]).len(), 2);
    // The rows stay as last known throughout.
    for (_, event) in &seen.all {
        match change_of(event) {
            Some(Change::Replace { rows, .. }) => assert_eq!(rows.len(), 1),
            Some(Change::Remove { .. }) => panic!("a removal while reconnecting"),
            _ => {}
        }
    }
    handle.stop();
}

#[test]
fn a_stopped_supervisor_takes_no_reconnect_and_clears_its_wait() {
    let fake = FakeHgs::new("retry_stopped", json!({"": {"runs": [[{"exit": 255}]]}}));
    let (tx, rx) = channel();
    let mut seen = Timed::new(rx);
    let handle = stream::spawn(
        &fake.path(),
        &ComputerId::local(),
        timing(Duration::from_secs(3600)),
        tx,
    );
    seen.until("the wait", is_waiting(1));
    handle.request_stop();
    assert!(!handle.reconnect());
    let idle = seen.until("idle", |event| retry_of(event) == Some(Retry::Idle));
    let ended = seen.until("ended", status_is(ComputerStatus::Ended));
    assert!(idle < ended);
    assert_eq!(fake.runs("", &["watch", "--json"]).len(), 1);
    handle.stop();
}

#[test]
fn a_new_schedule_applies_to_the_next_wait_and_not_the_published_one() {
    let fake = FakeHgs::new("retry_changed", json!({"": {"runs": [[{"exit": 255}]]}}));
    let timing = timing(Duration::from_millis(400));
    let (tx, rx) = channel();
    let mut seen = Timed::new(rx);
    let handle = stream::spawn(&fake.path(), &ComputerId::local(), timing.clone(), tx);
    let first = seen.until("the first wait", is_waiting(1));
    assert_eq!(seen.waiting(first).delay_ms, 400);
    timing.restart.set([Duration::from_millis(100); 4]);
    let second = seen.until("the second wait", is_waiting(2));
    assert_eq!(seen.waiting(second).delay_ms, 100);
    let starts = fake.runs("", &["watch", "--json"]);
    assert!(
        starts[1] - starts[0] >= 0.4,
        "the published wait kept its deadline: {starts:?}"
    );
    handle.stop();
}

fn spawn(command: &mut Command) -> std::io::Result<Child> {
    command.spawn()
}

#[test]
fn a_source_reports_its_schedule_with_its_identity_before_its_status() {
    let fake = FakeHgs::new("retry_source", json!({"": {"runs": [[{"exit": 255}]]}}));
    let (tx, rx) = source::channel(spawn);
    let handle = tx
        .spawn(
            &fake.path(),
            &ComputerId::local(),
            timing(Duration::from_secs(3600)),
        )
        .unwrap();
    let identity = handle.identity().clone();
    let deadline = Instant::now() + WAIT;
    let mut order: Vec<&str> = Vec::new();
    while !order.contains(&"reconnecting") {
        let item = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("source event");
        match item.event() {
            Event::Retry {
                source,
                retry: Retry::Waiting(next),
                ..
            } => {
                assert_eq!(*source, identity);
                assert_eq!(next.serial, 1);
                order.push("waiting");
            }
            Event::Status {
                status: ComputerStatus::Reconnecting { .. },
                ..
            } => order.push("reconnecting"),
            _ => {}
        }
    }
    assert_eq!(order, ["waiting", "reconnecting"]);
    assert!(handle.reconnect());
    loop {
        let item = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the attempt");
        if matches!(
            item.event(),
            Event::Retry {
                retry: Retry::Attempting { serial: 1, .. },
                ..
            }
        ) {
            break;
        }
    }
    handle.stop();
}
