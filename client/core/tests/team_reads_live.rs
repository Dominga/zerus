//! Task 76 diagnostic: the Teams poller against a live hgs, with timings.
//! Ignored unless ZERUS_LIVE_HGS names an hgs; run the built test binary with
//! `--ignored --nocapture`.

use std::sync::mpsc;
use std::time::{Duration, Instant};
use zerus_client_core::teams::{TeamsFollow, TeamsPoller};
use zerus_client_core::{Change, ComputerId};

#[test]
#[ignore]
fn live_team_reads_arrive() {
    let Some(hgs) = std::env::var_os("ZERUS_LIVE_HGS") else { return };
    let team = std::env::var("ZERUS_LIVE_TEAM").ok();
    let (sender, receiver) = mpsc::channel();
    let started = Instant::now();
    let poller = TeamsPoller::spawn(hgs.into(), ComputerId::local(), move |change| {
        let _ = sender.send((started.elapsed(), change));
    })
    .unwrap();
    poller.follow(TeamsFollow { list: true, waiting: true, team: team.clone(), catalog: false });
    let deadline = Instant::now() + Duration::from_secs(40);
    while let Ok((at, change)) = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        if let Change::Teams { key, update, .. } = change {
            let text = format!("{update:?}");
            println!("{:>6.2}s {key}: {}", at.as_secs_f64(), text.chars().take(160).collect::<String>());
        }
    }
}
