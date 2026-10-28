//! Compiles the client's metrics module standalone so its tests run without Tauri.
#[path = "../../src-tauri/src/metrics.rs"]
pub mod metrics;
