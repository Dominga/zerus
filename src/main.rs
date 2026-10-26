mod accounts;
mod agent_guide;
mod analytics;
mod approval_log;
mod claude_app;
mod cli;
mod config;
mod download;
mod host;
mod machines;
mod macos_session;
mod metrics;
#[cfg(unix)]
mod mobile_peers;
mod native_ui;
mod phones;
mod session_reference;
mod extensions;
mod windows_peer;
mod peer_channel;
mod platform;
mod presence;
mod screen;
mod speech;
mod status;
mod state;
mod swarm;

fn main() {
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
    // H2: `--json-file PATH` feeds the same request through stdin for every
    // `--json` command; a leading UTF-8 BOM is dropped and non-UTF-8 input is
    // refused before a command reads it.
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(index) = argv.iter().position(|arg| arg == "--json-file") {
        let Some(path) = argv.get(index + 1).cloned() else {
            eprintln!("hgs: --json-file needs a path");
            std::process::exit(2);
        };
        let failure = match std::fs::read(&path) {
            Err(error) => format!("Cannot read {path}: {error}"),
            Ok(bytes) => match state::json_request(&bytes) {
                Err(error) => error,
                Ok(value) => {
                    let temp = std::env::temp_dir().join(format!("hgs-json-{}.json", uuid::Uuid::new_v4()));
                    match std::fs::write(&temp, value.to_string()) {
                        Err(error) => format!("Cannot prepare the request: {error}"),
                        Ok(()) => match platform::redirect_stdin(&temp) {
                            Ok(()) => {
                                argv.drain(index..=index + 1);
                                // The file is this command's JSON request.
                                if !argv.iter().any(|arg| arg == "--json") {
                                    argv.insert(index, "--json".to_owned());
                                }
                                String::new()
                            }
                            Err(error) => format!("Cannot read the request: {error}"),
                        },
                    }
                }
            },
        };
        if !failure.is_empty() {
            eprintln!("hgs: {failure}");
            std::process::exit(2);
        }
    }
    let code = match cli::dispatch(argv) {
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
