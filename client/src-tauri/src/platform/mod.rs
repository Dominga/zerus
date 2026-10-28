//! Platform integrations of the client.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub mod macos_power;
// The menu bar drives the desk thread, which exists only with the live data source.
#[cfg(all(target_os = "macos", feature = "live"))]
pub mod macos_menubar;
