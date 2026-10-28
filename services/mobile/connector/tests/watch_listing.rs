//! The heartbeat's local listing from `hgs watch --json` (watch_listing.rs),
//! against a fixture `hgs` that streams a catalog and holds its input open.
#![cfg(unix)]
mod common;

use common::*;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;
use std::time::Duration;
use zerus_mobile_connector::runtime::Event;
use zerus_mobile_connector::watch_listing::WatchListing;

const FIXTURE: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$0.argv"
[ "$*" = "watch --json" ] || exit 2
echo '{"v":1,"seq":1,"t":"hello","host":"fixture","topics":["catalog","catalog.meta","activity"]}'
echo '{"v":1,"seq":2,"t":"snapshot","topic":"catalog","page":0,"last":true,"cursor":1,"items":[{"key":"run:b","row":{"name":"codex/zeta","run_id":"b"}},{"key":"run:a","row":{"name":"claude/Alpha","run_id":"a"}}]}'
echo '{"v":1,"seq":3,"t":"snapshot","topic":"catalog.meta","page":0,"last":true,"cursor":1,"value":{"host":"fixture","ok":true,"peers":[]}}'
echo '{"v":1,"seq":4,"t":"delta","topic":"catalog","cursor":2,"upsert":[{"key":"run:c","row":{"name":"codex/beta","run_id":"c"}}],"remove":[{"key":"run:b","reason":"archived"}]}'
# The connector keeps the input open; its end ends the stream.
cat > /dev/null
echo ended >> "$0.argv"
"#;

#[tokio::test(flavor = "current_thread")]
async fn the_listing_follows_the_stream_until_the_connector_stops() {
    let scratch = Scratch::new("watch-listing");
    let script = scratch.path("fixture-hgs");
    std::fs::write(&script, FIXTURE).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let argv = scratch.path("fixture-hgs.argv");
    tokio::task::LocalSet::new()
        .run_until(async {
            let listing = WatchListing::default();
            assert!(
                listing.listing().is_none(),
                "nothing before the stream starts"
            );
            let stopping = Rc::new(Event::default());
            let program = script.clone();
            listing.follow(
                move || tokio::process::Command::new(&program),
                stopping.clone(),
            );
            let mut names = Vec::new();
            for _ in 0..500 {
                if let Some(value) = listing.listing() {
                    names = value["sessions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| s["name"].as_str().unwrap().to_owned())
                        .collect();
                    if names.len() == 2 && value["host"] == "fixture" {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            // The snapshot, then the delta that added one row and removed another.
            assert_eq!(names, ["claude/Alpha", "codex/beta"]);
            assert_eq!(std::fs::read_to_string(&argv).unwrap(), "watch --json\n");
            // Following twice starts no second stream.
            let program = script.clone();
            listing.follow(
                move || tokio::process::Command::new(&program),
                stopping.clone(),
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(std::fs::read_to_string(&argv).unwrap(), "watch --json\n");
            // Stopping ends the stream and its listing.
            stopping.set();
            for _ in 0..500 {
                if listing.listing().is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert!(listing.listing().is_none());
        })
        .await;
}
