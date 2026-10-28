// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // First, before Tauri, GTK or any thread: WebKit reads its renderer variables at start.
    zerus_client_lib::renderer::choose();
    zerus_client_lib::run();
}
