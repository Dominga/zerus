// No console window next to the app in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // A launch's console window on Windows: this program started with
    // `--console-run` runs the program in it and ends with its exit code.
    #[cfg(all(windows, feature = "live"))]
    {
        use zerus_client_core::terminal_launch::console_host;
        let arguments: Vec<String> = std::env::args_os()
            .skip(1)
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect();
        if arguments.first().map(String::as_str) == Some(console_host::OPTION) {
            std::process::exit(console_host::run(&arguments));
        }
    }
    // First, before Tauri, GTK or any thread: WebKit reads its renderer variables at start.
    zerus_client_lib::renderer::choose();
    zerus_client_lib::run();
}
