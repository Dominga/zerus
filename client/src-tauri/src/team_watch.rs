//! Task 258: the always-on Team Waiting watcher (Qt's `TeamAttentionWatch`,
//! `TrayAgent.cpp:215`). The window's own Waiting page announces while it is
//! open; this thread reads `hgs team waiting --json` every 30 s from the app
//! itself, so an owner whose window is closed (Zerus in the tray) still gets
//! the questions, permissions, owner cards, reports and phase drafts that
//! wait for them.
//!
//! The decision and the persisted id→token map live in
//! `zerus_client_core::team_notices`; this module only reads, saves and posts.
//! It starts before any window exists and does not depend on one.

use std::time::Duration;

use tauri::{AppHandle, Manager};
use zerus_client_core::team_notices::{self, Changes};
use zerus_client_core::teams::read::TeamRead;
use zerus_client_core::ComputerId;

/// The background pace Qt uses (`TeamAttentionWatch::BackgroundMs`).
pub const INTERVAL: Duration = Duration::from_secs(30);
/// Task #10: how old a Waiting reply a window's poller read may be for this
/// watcher to use it instead of reading: its interval and a little more, as
/// a hidden window's poller reads at the same pace.
pub const REUSE: Duration = Duration::from_secs(35);
/// The shown items, as Qt's `teams/attentionNotified` (an old value from the
/// window's watcher is read as it is).
pub const SETTING: &str = "teams/attentionNotified";

/// Starts the watcher: one read now, then one every [`INTERVAL`], for the
/// life of the process. A replay or a sandbox without `hgs` never reads.
pub fn start(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("zerus-team-watch".into())
        .spawn(move || loop {
            tick(&app);
            std::thread::sleep(INTERVAL);
        });
    if let Err(error) = spawned {
        eprintln!("zerus-client: the team watcher did not start: {error}");
    }
}

/// One read: decide from the stored map, save it when it changed, and post the
/// notices through the same native path as attention banners. A failed read
/// changes nothing and is retried at the next pace.
fn tick(app: &AppHandle) {
    if crate::actions::blocked() {
        return;
    }
    let Some(settings) = app.try_state::<crate::settings::Store>() else {
        return;
    };
    let read = || {
        zerus_client_core::teams::read::read(
            &zerus_client_core::default_hgs(),
            &ComputerId::local(),
            &TeamRead::Waiting { before: None },
        )
    };
    // Task #10: while a window follows Waiting its poller's reply serves;
    // only without a fresh one does this watcher read.
    let read = match app.try_state::<crate::teams::Teams>() {
        Some(teams) => teams.waiting_share().reply_or(REUSE, read),
        None => read(),
    };
    let Ok(reply) = read else {
        return;
    };
    let (changes, stored) = decide(&reply, &settings.0.text(SETTING));
    if changes.raised.is_empty() && changes.cleared.is_empty() {
        return;
    }
    // Post first: a settings write that fails must not lose a notice. The
    // native path takes the attention notifier's own notice type.
    let raised = changes
        .raised
        .into_iter()
        .map(|notice| zerus_client_core::attention::Notice {
            token: notice.token,
            title: notice.title,
            body: notice.body,
        })
        .collect();
    crate::desk::deliver_waiting(app, changes.cleared, raised);
    let _ = settings
        .0
        .set(SETTING, serde_json::Value::from(stored));
}

/// The pure half: one waiting reply and the stored map give the notices and
/// the map to store. It needs no window, so a closed Zerus still decides.
pub fn decide(reply: &serde_json::Value, stored: &str) -> (Changes, String) {
    let read = team_notices::decode(reply);
    let mut posted = team_notices::decode_posted(stored);
    let changes = team_notices::observe(&read.items, read.complete, &mut posted);
    (changes, team_notices::encode_posted(&posted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(items: serde_json::Value, more: bool) -> serde_json::Value {
        serde_json::json!({"items": items, "total": 1, "more": more})
    }

    #[test]
    fn a_new_card_is_decided_without_a_window_or_a_desk() {
        // The watcher has no window argument and starts before one exists:
        // this decision is exactly what a closed Zerus posts.
        let (changes, stored) = decide(
            &reply(
                serde_json::json!([{"id":"t/report_escalated/1","team":"t","team_name":"Orbit",
                    "kind":"report_escalated","title":"Report #1","notify":true}]),
                false,
            ),
            "{}",
        );
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(changes.raised[0].token, "team-waiting:t/report_escalated/1");
        assert_eq!(changes.raised[0].title, "Waiting for you in Orbit");
        assert_eq!(changes.raised[0].body, "Report #1");
        assert!(stored.contains("t/report_escalated/1"));
    }

    #[test]
    fn the_stored_map_stops_a_second_start_and_a_held_card_stays_quiet() {
        let held = reply(
            serde_json::json!([
                {"id":"t/a","team":"t","team_name":"Orbit","kind":"owner_word_asked","title":"A","notify":false},
                {"id":"t/b","team":"t","team_name":"Orbit","kind":"owner_word_asked","title":"B","notify":true}
            ]),
            true,
        );
        let (first, stored) = decide(&held, "{}");
        assert_eq!(first.raised.len(), 1, "the held card is not announced");
        assert_eq!(first.raised[0].token, "team-waiting:t/b");
        // A restart reads the saved map: the same page announces nothing.
        let (again, _) = decide(&held, &stored);
        assert!(again.raised.is_empty());
    }

    #[test]
    fn a_returned_batch_is_one_notice() {
        let (changes, stored) = decide(
            &reply(
                serde_json::json!([
                    {"id":"t/a","team":"t","team_name":"Orbit","kind":"owner_word_asked","title":"A","notify":true,"batch":"b1"},
                    {"id":"t/b","team":"t","team_name":"Orbit","kind":"owner_word_asked","title":"B","notify":true,"batch":"b1"}
                ]),
                true,
            ),
            "{}",
        );
        assert_eq!(changes.raised.len(), 1);
        assert_eq!(changes.raised[0].token, "team-waiting:batch:b1");
        assert_eq!(changes.raised[0].body, "A\nB");
        assert!(stored.contains("t/a") && stored.contains("t/b"));
    }

    #[test]
    fn a_card_that_no_longer_waits_withdraws_its_notice() {
        let (first, stored) = decide(
            &reply(
                serde_json::json!([{"id":"t/a","team":"t","team_name":"Orbit","kind":"owner_word_asked","title":"A","notify":true}]),
                false,
            ),
            "{}",
        );
        assert_eq!(first.raised.len(), 1);
        let (gone, _) = decide(&reply(serde_json::json!([]), false), &stored);
        assert_eq!(gone.cleared, ["team-waiting:t/a"]);
    }

    #[test]
    fn the_setting_and_pace_are_qts() {
        assert_eq!(SETTING, "teams/attentionNotified");
        assert_eq!(INTERVAL, Duration::from_secs(30));
    }
}
