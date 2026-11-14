//! Private preview fixtures made by the product writer, then read by the
//! source-built CLI and desktop core. No person-only command is invoked.
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;
use zerus_client_core::teams::{TeamsFollow, TeamsPoller};
use zerus_client_core::{Batch, Coalescer};

fn quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn private_cli(root: &Path, binary: &Path, on: bool) -> std::path::PathBuf {
    for dir in ["home", "state", "config", "tmux"] {
        crate::platform::private_dir(&root.join(dir)).unwrap();
    }
    if on {
        // The same writer used by preview on; fixture setup only, inside the
        // test-owned directory. Caller proof remains on every real command.
        super::turn_on(&root.join("state")).unwrap();
    }
    let hgs = root.join("hgs-private");
    let script = format!(
        "#!/bin/sh\nexec env -i PATH=\"$PATH\" HOME={} HGS_STATE_DIR={} HGS_CONFIG_DIR={} TMUX_TMPDIR={} HGS_SELF=example-local HGS_PEERS='' {} \"$@\"\n",
        quote(&root.join("home")), quote(&root.join("state")), quote(&root.join("config")),
        quote(&root.join("tmux")), quote(binary),
    );
    std::fs::write(&hgs, script).unwrap();
    std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o700)).unwrap();
    hgs
}

fn list_batch(receiver: &mpsc::Receiver<zerus_client_core::Change>) -> Batch {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let change = receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap();
        let mut coalescer = Coalescer::default();
        coalescer.push(change);
        let batch = coalescer.take();
        if batch.teams.iter().any(|entry| entry.key == "list") {
            return batch;
        }
    }
}

#[test]
#[ignore = "requires HGS_TEST_BIN built from this source; private real CLI contract"]
fn the_real_cli_preview_off_and_on_reach_the_core_and_retry() {
    let binary =
        std::path::PathBuf::from(std::env::var_os("HGS_TEST_BIN").expect("source-built CLI"));
    assert!(binary.is_absolute() && binary.is_file());
    let fixture_dir =
        std::env::var_os("HGS_TEST_TEAMS_FIXTURE_DIR").expect("private fixture parent");
    let root = tempfile::tempdir_in(fixture_dir).unwrap();
    let mut replies = Vec::new();
    let mut scripts = Vec::new();
    for on in [false, true] {
        let dir = root.path().join(if on { "on" } else { "off" });
        let hgs = private_cli(&dir, &binary, on);
        let output = std::process::Command::new(&hgs)
            .args(["team", "preview", "status", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status["preview"], if on { "on" } else { "off" });
        let (sender, receiver) = mpsc::channel();
        let poller = TeamsPoller::spawn(hgs.clone(), move |change| {
            sender.send(change).unwrap();
        })
        .unwrap();
        poller.follow(TeamsFollow {
            list: true,
            ..TeamsFollow::default()
        });
        let first = list_batch(&receiver);
        let entry = &first.teams[0];
        assert_eq!(entry.kind, if on { "list" } else { "failed" });
        if !on {
            assert_eq!(entry.payload["code"], "teams_inactive");
        }
        println!(
            "REAL_PREVIEW_BATCH={}",
            serde_json::to_string(&first).unwrap()
        );
        poller.recheck();
        let again = list_batch(&receiver);
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&again).unwrap()
        );
        replies.push(first);
        scripts.push(hgs);
    }
    if let Some(path) = std::env::var_os("HGS_TEST_TEAMS_BATCHES") {
        std::fs::write(path, serde_json::to_vec(&replies).unwrap()).unwrap();
    }
    // The native test consumes these same product-written fixtures while
    // their TempDir is alive; it launches no app, window or desktop service.
    if let Some(test) = std::env::var_os("HGS_TEST_TEAMS_TAURI_TEST") {
        let status = std::process::Command::new(test)
            .args([
                "teams::real_reads::the_real_cli_off_and_on_reach_the_tauri_pump",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HGS_TEST_TEAMS_OFF", &scripts[0])
            .env("HGS_TEST_TEAMS_ON", &scripts[1])
            .status()
            .unwrap();
        assert!(status.success(), "Tauri real-CLI contract");
    }
}
