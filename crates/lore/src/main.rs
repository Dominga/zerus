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
        caller: caller(std::env::var("LORE_CALLER").ok().as_deref()),
        agent_session: ["HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())),
        finder: Some(&BUILTIN),
        roots: roots(),
        history: history_source(),
        history_scope: history_scope(),
        ..Env::default()
    };
    let references: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = cli::run(&references, &env);
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    std::process::exit(output.code);
}

/// Task 186: `LORE_CALLER` is `member:<team>:<name>`, `lead:<team>:<name>`
/// or `owner`, set by the embedding for the session it runs lore for.
fn caller(value: Option<&str>) -> Option<zerus_lore::working::Viewer> {
    use zerus_lore::working::{Role, Viewer};
    let value = value?.trim();
    if value == "owner" {
        return Some(Viewer::owner());
    }
    let mut parts = value.splitn(3, ':');
    let role = match parts.next()? {
        "member" => Role::Member,
        "lead" => Role::Lead,
        _ => return None,
    };
    let team = parts.next().filter(|team| !team.is_empty())?;
    let name = parts.next().filter(|name| !name.is_empty())?;
    Some(Viewer { team: Some(team.to_owned()), name: name.to_owned(), role })
}

/// docs/lore.md 2.4: a Zerus store's reference roots, the folders a cited
/// commit or file may be in, as `LORE_ROOTS` (the platform's path list); Zerus
/// sets it for the stores it writes. Unset: CodeLore's one repository.
fn roots() -> Option<zerus_lore::refs::Roots> {
    roots_from(std::env::var_os("LORE_ROOTS"))
}

fn roots_from(value: Option<std::ffi::OsString>) -> Option<zerus_lore::refs::Roots> {
    let value = value.filter(|value| !value.is_empty())?;
    let folders: Vec<std::path::PathBuf> = std::env::split_paths(&value).filter(|path| !path.as_os_str().is_empty()).collect();
    (!folders.is_empty()).then(|| zerus_lore::refs::Roots::new(folders, "this computer"))
}

/// docs/lore.md 11.7: Zerus registers its content-search index by naming its
/// hgs in `LORE_HISTORY_HGS`; standalone lore has no history source.
fn history_source() -> Option<std::sync::Arc<dyn zerus_lore::history::HistorySource>> {
    let hgs = std::env::var_os("LORE_HISTORY_HGS").filter(|value| !value.is_empty())?;
    Some(std::sync::Arc::new(zerus_lore::history::HgsSource { hgs: hgs.into() }))
}

/// Whose sessions the history channel returns: `LORE_HISTORY_SESSIONS` (a
/// member's own, the lead's team's; unset: the owner's, any) and
/// `LORE_HISTORY_EXCLUDE` (sessions whose memory_history is off).
fn history_scope() -> zerus_lore::history::Scope {
    let list = |name: &str| -> Option<Vec<String>> {
        let value = std::env::var(name).ok()?;
        Some(value.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect())
    };
    zerus_lore::history::Scope { sessions: list("LORE_HISTORY_SESSIONS"), excluded: list("LORE_HISTORY_EXCLUDE").unwrap_or_default() }
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

#[cfg(test)]
mod tests {
    use super::roots_from;
    use std::path::PathBuf;

    /// Task 128: Zerus names a store's reference roots in `LORE_ROOTS`, the
    /// platform's path list, the store first; unset or empty is CodeLore's
    /// one repository.
    #[test]
    fn lore_roots_name_the_reference_folders_in_order() {
        let folders = [PathBuf::from("store"), PathBuf::from("team"), PathBuf::from("member")];
        let joined = std::env::join_paths(&folders).unwrap();
        let roots = roots_from(Some(joined)).expect("roots");
        assert_eq!(roots.folders, folders);
        assert_eq!(roots.machine, "this computer");
        assert_eq!(roots_from(None), None);
        assert_eq!(roots_from(Some("".into())), None);
    }
}
