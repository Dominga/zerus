//! Platform integrations of the client.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(all(target_os = "linux", feature = "live"))]
pub mod linux_notify;
#[cfg(all(test, target_os = "linux"))]
pub(crate) mod linux_test_bus;
#[cfg(all(target_os = "linux", feature = "live"))]
pub mod linux_tray;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub(crate) mod macos_notification_request;
#[cfg(target_os = "macos")]
pub mod macos_power;
// The menu bar drives the desk thread, which exists only with the live data source.
#[cfg(all(target_os = "macos", feature = "live"))]
pub mod macos_menubar;
// Attention notifications come from the desk thread too.
#[cfg(all(target_os = "macos", feature = "live"))]
pub mod macos_notify;

#[cfg(all(target_os = "linux", feature = "live"))]
pub mod linux_window_layer;

#[cfg(windows)]
pub mod windows;

/// Tokio keeps its original child/pipes and runtime registration. Only its
/// synchronous OS spawn participates in the core gate; no turn reaches await.
#[cfg(feature = "live")]
pub(crate) fn spawn_async(
    command: &mut tokio::process::Command,
) -> std::io::Result<tokio::process::Child> {
    #[cfg(unix)]
    let _turn = zerus_client_core::unix_spawn::turn();
    command.spawn()
}

#[cfg(all(test, unix, feature = "live"))]
#[test]
#[ignore = "opt-in before/after synthetic Tokio spawn cost measurement"]
fn unix_spawn_cost_tokio() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        for shared in [false, true] {
            let mut samples = Vec::new();
            for n in 0..72 {
                let mut command = tokio::process::Command::new("/usr/bin/true");
                command
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .kill_on_drop(true);
                let before = std::time::Instant::now();
                let mut child = if shared {
                    spawn_async(&mut command)
                } else {
                    command.spawn()
                }
                .unwrap();
                let elapsed = before.elapsed().as_micros();
                assert!(child.wait().await.unwrap().success());
                if n >= 8 {
                    samples.push(elapsed);
                }
            }
            samples.sort_unstable();
            println!(
                "SPAWN_COST {}",
                serde_json::json!({
                    "family":"tauri-tokio", "samples":samples.len(),
                    "median_us":samples[samples.len()/2], "p95_us":samples[samples.len()*95/100],
                    "min_us":samples[0], "phase":if shared {"after"} else {"before-raw-control"}
                })
            );
        }
    });
}
