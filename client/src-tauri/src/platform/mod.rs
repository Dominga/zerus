//! Platform integrations of the client.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
// The menu bar drives the desk thread, which exists only with the live data source.
#[cfg(all(target_os = "macos", feature = "live"))]
pub mod macos_menubar;
// Attention notifications come from the desk thread too.
#[cfg(all(target_os = "macos", feature = "live"))]
pub mod macos_notify;
