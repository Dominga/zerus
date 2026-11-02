mod accounts;
mod agent_guide;
mod analytics;
mod approval_log;
mod cli;
mod config;
mod desktop;
mod download;
mod host;
mod machines;
mod macos_session;
mod metrics;
#[cfg(unix)]
mod mobile_peers;
mod native_ui;
mod phones;
mod platform;
mod presence;
mod screen;
mod session_reference;
mod speech;
mod state;
mod status;
mod swarm;
mod tasks;
mod watch;

fn main() {
    // COM cold activation cannot load config, create jobs or acquire ownership.
    #[cfg(windows)]
    if let Some(code) = desktop::windows_notification_activation(std::env::args().nth(1).as_deref())
    {
        std::process::exit(code);
    }
    // Fixed native implementation worker: bounded frame, no config/owner load.
    #[cfg(windows)]
    if let Some(code) =
        desktop::windows_registration_entry(&std::env::args().skip(1).collect::<Vec<_>>())
    {
        std::process::exit(code);
    }
    if std::env::args().nth(1).as_deref() == Some("--macos-session-service") {
        std::process::exit(macos_session::serve());
    }
    // The Windows session host and its tools, before `cli::dispatch` puts
    // hgs.exe into its own kill-on-close job (spec D1, D4).
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() == Some("__host") {
        let args: Vec<String> = std::env::args().skip(2).collect();
        std::process::exit(host::run(&args));
    }
    let code = match cli::dispatch(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(err) => {
            // One write (eprintln! writes each piece on its own), so that no
            // frame a pseudoconsole paints shows the prefix without the message.
            let line = format!("hgs: {}\n", err.message);
            eprint!("{line}");
            err.code
        }
    };
    std::process::exit(code);
}
