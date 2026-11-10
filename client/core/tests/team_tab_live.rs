//! Task #14 (team ede724c4) measurement: how long the Team tab waits for its
//! team view against a live hgs. Ignored unless ZERUS_LIVE_HGS names an hgs
//! and ZERUS_LIVE_TEAMS two team ids (comma separated); run the built test
//! binary with `--ignored --nocapture`.
//!
//! The Teams page follows the list and Waiting until both arrive; then the
//! first team's tab opens (list, Waiting and the team followed), then the
//! second team's at once (while the first one's inbox and feed reads run),
//! then the first one's again.

use std::sync::mpsc;
use std::time::{Duration, Instant};
use zerus_client_core::teams::{TeamsFollow, TeamsPoller};
use zerus_client_core::{Change, ComputerId};

fn wait_for(receiver: &mpsc::Receiver<Change>, key: &str, within: Duration) -> Option<Duration> {
    let started = Instant::now();
    let deadline = started + within;
    while let Ok(change) = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if let Change::Teams { key: got, update, .. } = change {
            if got == key && update.is_view() {
                return Some(started.elapsed());
            }
        }
    }
    None
}

#[test]
#[ignore]
fn live_team_tab_opens() {
    let Some(hgs) = std::env::var_os("ZERUS_LIVE_HGS") else { return };
    let teams = std::env::var("ZERUS_LIVE_TEAMS").unwrap_or_default();
    let teams: Vec<String> = teams.split(',').map(str::trim).filter(|team| !team.is_empty()).map(str::to_owned).collect();
    assert_eq!(teams.len(), 2, "ZERUS_LIVE_TEAMS names two teams");
    let (sender, receiver) = mpsc::channel();
    let poller = TeamsPoller::spawn(hgs.into(), ComputerId::local(), move |change| {
        let _ = sender.send(change);
    })
    .unwrap();
    let page = TeamsFollow { list: true, waiting: true, team: None, catalog: false };
    poller.follow(page.clone());
    let list = wait_for(&receiver, "list", Duration::from_secs(60));
    println!("Teams page: list after {list:?}");
    let tab = |team: &str| TeamsFollow { team: Some(team.to_owned()), ..page.clone() };
    let mut times = Vec::new();
    for (step, team) in [("first open", &teams[0]), ("switch", &teams[1]), ("back", &teams[0])] {
        poller.follow(tab(team));
        let took = wait_for(&receiver, &format!("team:{team}"), Duration::from_secs(120));
        println!("{step} {}: team view after {:?}", &team[..8.min(team.len())], took);
        times.push(took);
    }
    drop(poller);
    assert!(times.iter().all(Option::is_some), "every tab shows its team");
}
