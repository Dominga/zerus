fn main() {
    fork_build_identity();
    println!("cargo:rerun-if-changed=src/macos_session.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new().file("src/macos_session.c")
            .flag_if_supported("-Wno-deprecated-declarations")
            .compile("hgs_macos_session");
        for library in ["framework=Security", "framework=CoreFoundation", "bsm"] {
            println!("cargo:rustc-link-lib={library}");
        }
    }
    // hgs.exe gets the application manifest hgs-tray.exe has: UTF-8 as the ANSI
    // code page, long paths and the Windows 10 and 11 compatibility section.
    // link.exe embeds it; other targets link nothing extra.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        println!("cargo:rerun-if-changed=src/hgs.manifest");
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("src")
            .join("hgs.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    }
}


// Embed provenance, never a deployment path or an account identifier. Watching
// source inputs and Git refs prevents a cached build from claiming a newer SHA.
fn fork_build_identity() {
    use std::{path::Path, process::Command};
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).current_dir(&root).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let version = std::fs::read_to_string(Path::new(&root).join("FORK_VERSION")).expect("FORK_VERSION is required");
    let version = version.trim();
    assert!(version.len() <= 32 && version.bytes().all(|b| b.is_ascii_digit() || b".-".contains(&b)), "Invalid fork version");
    for path in ["FORK_VERSION", "build.rs", "src", "Cargo.toml", "Cargo.lock"] {
        println!("cargo:rerun-if-changed={path}");
    }
    if let Some(files) = git(&["ls-files", "-z", "--cached"]) {
        for path in files.split('\0').filter(|p| !p.is_empty()) {
            assert!(!path.contains(['\n', '\r']), "Unsupported source filename");
            println!("cargo:rerun-if-changed={path}");
        }
    }
    for path in ["HEAD", "index", "packed-refs"] {
        if let Some(file) = git(&["rev-parse", "--git-path", path]) {
            if Path::new(&root).join(&file).exists() { println!("cargo:rerun-if-changed={file}"); }
        }
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(file) = git(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={file}");
        }
    }
    let same_root = git(&["rev-parse", "--show-toplevel"]).is_some_and(|p|
        std::fs::canonicalize(p).ok() == std::fs::canonicalize(&root).ok());
    let commit = if same_root { git(&["rev-parse", "HEAD"]).unwrap_or_default() } else { String::new() };
    let valid = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let clean = valid && git(&["--no-optional-locks", "status", "--porcelain", "--untracked-files=normal"]).is_some_and(|s| s.is_empty());
    println!("cargo:rustc-env=ZERUS_FORK_VERSION={version}");
    println!("cargo:rustc-env=ZERUS_BUILD_SOURCE_COMMIT={}", if valid { commit.as_str() } else { "" });
    println!("cargo:rustc-env=ZERUS_BUILD_SOURCE_DIRTY={}", !clean);
}
