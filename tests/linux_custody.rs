//! A harness-free executable: each control reexecs a fresh, single-threaded
//! owner, never the threaded Rust test harness or an installed hgs command.
#[cfg(target_os = "linux")]
#[path = "linux_custody/controls.rs"]
mod controls;
#[cfg(target_os = "linux")]
#[path = "../src/state/processes/custody.rs"]
mod custody;

fn main() {
    #[cfg(target_os = "linux")]
    controls::main();
}
