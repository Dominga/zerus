//! Opening session files, against `tray/src/ExternalFile.cpp`.
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zerus_client_core::external_file::*;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Folder(PathBuf);

impl Folder {
    fn new() -> Folder {
        let path = std::env::temp_dir().join(format!(
            "zerus-external-file-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Folder(path)
    }

    fn file(&self, name: &str, bytes: &[u8], mode: u32) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn program_types_run_programs_by_their_suffix() {
    let folder = Folder::new();
    for name in [
        "setup.exe",
        "run.SH",
        "tool.py",
        "Installer.pkg",
        "launch.desktop",
        "disk.dmg",
        "trailing.exe. .",
        "Notes.app",
    ] {
        assert!(runs_programs(&folder.file(name, b"text", 0o644)), "{name}");
    }
    for name in [
        "report.pdf",
        "data.json",
        "notes.md",
        "image.png",
        "archive.tar.gz",
    ] {
        assert!(
            !runs_programs(&folder.file(name, b"plain text", 0o644)),
            "{name}"
        );
    }
}

#[test]
fn programs_are_known_by_their_content_and_executable_bit() {
    let folder = Folder::new();
    // An executable file without a type runs; a document type wins.
    assert!(runs_programs(&folder.file("tool", b"text", 0o755)));
    assert!(!runs_programs(&folder.file("README", b"text", 0o644)));
    assert!(!runs_programs(&folder.file("notes.txt", b"text", 0o755)));
    // ELF, Mach-O and a shebang naming an interpreter path.
    assert!(runs_programs(&folder.file(
        "a.dat",
        b"\x7fELF\x02\x01",
        0o644
    )));
    assert!(runs_programs(&folder.file(
        "b.dat",
        &[0xcf, 0xfa, 0xed, 0xfe, 7],
        0o644
    )));
    assert!(runs_programs(&folder.file(
        "c.txt",
        b"#!/bin/sh\necho hi\n",
        0o644
    )));
    assert!(runs_programs(&folder.file(
        "d.txt",
        b"#! \t/usr/bin/env python3\n",
        0o644
    )));
    // Rust's attribute is not a shebang.
    assert!(!runs_programs(&folder.file(
        "lib.rs.txt",
        b"#![no_std]\nfn main() {}\n",
        0o644
    )));
    // A PE image points to its "PE\0\0" header; text starting with MZ does not.
    let mut pe = vec![0u8; 0x90];
    pe[..2].copy_from_slice(b"MZ");
    pe[60] = 0x80;
    pe[0x80..0x84].copy_from_slice(b"PE\0\0");
    assert!(runs_programs(&folder.file("e.dat", &pe, 0o644)));
    let mut text =
        b"MZ is a nice prefix for a note that is long enough to have a header field.".to_vec();
    text.resize(80, b' ');
    assert!(!runs_programs(&folder.file("f.txt", &text, 0o644)));
}

#[test]
fn folders_and_links_are_judged_by_what_opens() {
    let folder = Folder::new();
    let documents = folder.0.join("Documents");
    std::fs::create_dir(&documents).unwrap();
    assert!(!runs_programs(&documents));
    // A link named like a document reaches a script.
    let script = folder.file("deploy.sh", b"echo", 0o644);
    let link = folder.0.join("Report.pdf");
    std::os::unix::fs::symlink(&script, &link).unwrap();
    assert!(runs_programs(&link));
    // "x/." is x.
    let dotted = PathBuf::from(format!("{}/.", script.display()));
    assert!(runs_programs(&dotted));
}

#[test]
fn opening_reveals_programs_and_opens_documents() {
    let folder = Folder::new();
    let document = folder.file("report.pdf", b"%PDF", 0o644);
    let program = folder.file("install.sh", b"echo", 0o644);
    let opened = |path: &Path| path.ends_with("report.pdf");
    assert_eq!(
        open(&document, Origin::OtherMachine, opened, |_| unreachable!()),
        Outcome::Opened
    );
    assert_eq!(
        open(
            &document,
            Origin::ThisComputer,
            |_| false,
            |_| unreachable!()
        ),
        Outcome::OpenFailed
    );
    assert_eq!(
        open(&program, Origin::OtherMachine, |_| unreachable!(), |_| true),
        Outcome::Revealed
    );
    assert_eq!(
        open(
            &program,
            Origin::ThisComputer,
            |_| unreachable!(),
            |_| false
        ),
        Outcome::RevealFailed
    );
}

#[test]
fn linux_has_no_download_marker() {
    let folder = Folder::new();
    let document = folder.file("report.pdf", b"%PDF", 0o644);
    if cfg!(target_os = "linux") {
        assert!(!mark_downloaded(&document));
        assert!(download_marker(&document).is_empty());
    }
    assert!(!mark_downloaded(&folder.0.join("missing.pdf")));
}

#[test]
fn notices_name_the_file() {
    let path = Path::new("/srv/out/install.sh");
    assert_eq!(
        revealed_notice(path),
        "install.sh can run programs, so it is shown in the file manager instead of being opened."
    );
    assert_eq!(
        reveal_failed_notice(path),
        "install.sh can run programs, and the file manager could not show it. Its copy is at /srv/out/install.sh."
    );
}
