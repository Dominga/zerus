//! The `lore` binary: CodeLore's command line over `cli::run`, and the
//! built-in model's `lore model fetch`/`status`.
use std::io::Read;
use zerus_lore::builtin::BUILTIN;
use zerus_lore::cli::{self, Env};
use zerus_lore::embed::{self, ModelSource};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("model") {
        std::process::exit(model(&args[1..]));
    }
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let env = Env {
        cwd: std::env::current_dir().unwrap_or_default(),
        stdin,
        lore_by: std::env::var("LORE_BY").ok(),
        finder: Some(&BUILTIN),
        roots: None,
    };
    let references: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = cli::run(&references, &env);
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    std::process::exit(output.code);
}

/// `lore model fetch [--local <dir>] [--json]` and `lore model status [--json]`.
fn model(args: &[String]) -> i32 {
    let json = args.iter().any(|arg| arg == "--json");
    let local = args
        .iter()
        .position(|arg| arg == "--local")
        .and_then(|position| args.get(position + 1))
        .map(std::path::PathBuf::from);
    let unknown: Vec<&String> = args
        .iter()
        .filter(|arg| {
            !matches!(arg.as_str(), "--json" | "--local" | "fetch" | "status")
                && local.as_ref().is_none_or(|path| path.as_os_str() != arg.as_str())
        })
        .collect();
    let command = args.iter().find(|arg| matches!(arg.as_str(), "fetch" | "status")).map(String::as_str);
    if !unknown.is_empty() || command.is_none() {
        eprintln!("usage: lore model fetch [--local <dir>] [--json] | lore model status [--json]");
        return 2;
    }
    if command == Some("status") {
        let status = embed::status();
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "model": embed::MODEL,
                    "dir": status.dir.display().to_string(),
                    "available": status.available,
                    "reason": status.reason,
                })
            );
        } else if status.available {
            println!("{} is installed at {}", embed::MODEL, status.dir.display());
        } else {
            println!("{} is not installed: {} ({}); run `lore model fetch`", embed::MODEL, status.reason, status.dir.display());
        }
        return if status.available { 0 } else { 1 };
    }
    let source = match &local {
        Some(path) => ModelSource::Local(path),
        None => ModelSource::Cache,
    };
    let mut progress = |name: &str, done: u64, total: u64| {
        if !json && (done == total || done % (64 * 1024 * 1024) < 1024 * 1024) {
            eprintln!("{name}: {done}/{total}");
        }
    };
    match embed::fetch_model(source, &mut progress) {
        Ok(dir) => {
            if json {
                let files: Vec<serde_json::Value> = embed::FILES
                    .iter()
                    .map(|file| {
                        serde_json::json!({"name": file.name, "bytes": file.bytes, "sha256": file.sha256})
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({"model": embed::MODEL, "path": dir.display().to_string(), "files": files})
                );
            } else {
                println!("{} is installed at {}", embed::MODEL, dir.display());
            }
            0
        }
        Err(error) => {
            if json {
                println!("{}", serde_json::json!({"error": error.to_string()}));
            } else {
                eprintln!("lore: {error}");
            }
            1
        }
    }
}
