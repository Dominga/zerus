//! Recognize tool-declared generated caches for an explicit exit-cleanup choice.
//! Unknown local data stays review-only. Names, age and size alone are not proof.
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path};
use std::time::Instant;

fn file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_file())
}
fn directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir())
}
fn text(path: &Path) -> Option<String> {
    if !file(path) {
        return None;
    }
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(65537)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= 65536)
        .then(|| String::from_utf8(bytes).ok())
        .flatten()
}

pub(super) fn generated_only(tree: &Path, ignored: &[String], deadline: Instant) -> bool {
    ignored
        .iter()
        .all(|name| Instant::now() < deadline && generated(tree, Path::new(name), deadline))
}

fn generated(tree: &Path, relative: &Path, deadline: Instant) -> bool {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return false;
    }
    let path = tree.join(relative);
    if !directory(&path) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    match path.file_name().and_then(|n| n.to_str()) {
        Some("__pycache__") => {
            let Ok(entries) = fs::read_dir(&path) else {
                return false;
            };
            for (count, entry) in entries.enumerate() {
                if count >= 4096 || Instant::now() >= deadline {
                    return false;
                }
                let Ok(entry) = entry else {
                    return false;
                };
                let cache = entry.path();
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    return false;
                };
                let Some(module) = name.split('.').next().filter(|_| name.ends_with(".pyc")) else {
                    return false;
                };
                if !file(&cache) || !file(&parent.join(format!("{module}.py"))) {
                    return false;
                }
                let mut magic = [0; 4];
                if File::open(cache)
                    .and_then(|mut f| f.read_exact(&mut magic))
                    .is_err()
                    || magic[2..] != *b"\r\n"
                {
                    return false;
                }
            }
            true
        }
        Some("target") => {
            file(&parent.join("Cargo.toml"))
                && text(&path.join("CACHEDIR.TAG"))
                    .is_some_and(|s| s.starts_with("Signature: 8a477f597d28d172789f06886806bc55"))
        }
        Some("node_modules") => {
            file(&parent.join("package.json"))
                && ["package-lock.json", "yarn.lock", "pnpm-lock.yaml"]
                    .iter()
                    .any(|n| file(&parent.join(n)))
                && [".package-lock.json", ".modules.yaml"]
                    .iter()
                    .any(|n| file(&path.join(n)))
        }
        Some(".gradle") => {
            ["settings.gradle", "settings.gradle.kts"]
                .iter()
                .any(|n| file(&parent.join(n)))
                && text(&path.join("buildOutputCleanup/cache.properties"))
                    .is_some_and(|s| s.lines().any(|l| l.starts_with("gradle.version=")))
        }
        Some("build")
            if ["build.gradle", "build.gradle.kts"]
                .iter()
                .any(|n| file(&parent.join(n))) =>
        {
            ["outputs", "intermediates", "generated"]
                .iter()
                .any(|n| directory(&path.join(n)))
        }
        _ => {
            if !directory(&path.join("CMakeFiles"))
                || !["build.ninja", "Makefile"]
                    .iter()
                    .any(|n| file(&path.join(n)))
            {
                return false;
            }
            let Some(cache) = text(&path.join("CMakeCache.txt")) else {
                return false;
            };
            let Some(home) = cache
                .lines()
                .find_map(|l| l.strip_prefix("CMAKE_HOME_DIRECTORY:INTERNAL="))
            else {
                return false;
            };
            match (fs::canonicalize(tree), fs::canonicalize(home)) {
                (Ok(tree), Ok(home)) => home.starts_with(tree),
                _ => false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn tool_markers_are_required_and_unknown_data_keeps_the_folder() {
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path();
        fs::create_dir(tree.join("target")).unwrap();
        fs::write(tree.join("target/notes.txt"), "unique").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        assert!(!generated_only(tree, &["target".into()], deadline));
        fs::write(tree.join("Cargo.toml"), "[package]").unwrap();
        fs::write(
            tree.join("target/CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )
        .unwrap();
        assert!(generated_only(tree, &["target".into()], deadline));
        fs::write(tree.join("settings.local.json"), "unique").unwrap();
        assert!(!generated_only(
            tree,
            &["target".into(), "settings.local.json".into()],
            deadline
        ));
        assert!(!generated_only(tree, &["../target".into()], deadline));
        assert!(!generated_only(tree, &["target".into()], Instant::now()));
    }
    #[test]
    fn bytecode_requires_source_and_other_files_require_review() {
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path();
        fs::create_dir(tree.join("__pycache__")).unwrap();
        fs::write(
            tree.join("__pycache__/module.cpython-314.pyc"),
            b"xx\r\nbytecode",
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        assert!(!generated_only(tree, &["__pycache__".into()], deadline));
        fs::write(tree.join("module.py"), "pass\n").unwrap();
        assert!(generated_only(tree, &["__pycache__".into()], deadline));
        fs::write(tree.join("__pycache__/results.json"), "{}").unwrap();
        assert!(!generated_only(tree, &["__pycache__".into()], deadline));
    }
    #[test]
    fn cmake_requires_a_generated_build_for_this_worktree() {
        let temp = tempfile::tempdir().unwrap();
        let tree = temp.path();
        let build = tree.join("qt-output");
        fs::create_dir_all(build.join("CMakeFiles")).unwrap();
        fs::write(build.join("build.ninja"), "generated").unwrap();
        fs::write(
            build.join("CMakeCache.txt"),
            format!("CMAKE_HOME_DIRECTORY:INTERNAL={}\n", tree.display()),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        assert!(generated_only(tree, &["qt-output".into()], deadline));
        fs::write(
            build.join("CMakeCache.txt"),
            "CMAKE_HOME_DIRECTORY:INTERNAL=/unrelated\n",
        )
        .unwrap();
        assert!(!generated_only(tree, &["qt-output".into()], deadline));
    }
}
