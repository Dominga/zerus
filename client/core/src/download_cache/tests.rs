// DownloadCache's behaviour (tray/src/DownloadCache.cpp), on folders under
// the system temporary folder that are removed even when a test fails.
use super::*;

struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Folder {
        let path = std::env::temp_dir().join(format!(
            "zerus-download-cache-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        Folder(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_800_000_000_000;

fn source(path: &str) -> Source {
    Source {
        host: "mac".into(),
        path: path.into(),
    }
}

/// A copy saved with these bytes under `name`, `age_ms` before NOW.
fn save(root: &Path, name: &str, bytes: &[u8], source: Option<&Source>, age_ms: i64) -> PathBuf {
    let part = reserve(root).unwrap();
    fs::write(part.path(), bytes).unwrap();
    let done = complete(part, name, source, NOW - age_ms).unwrap();
    assert_eq!(done.warning, None);
    done.path
}

fn index(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join(INDEX)).unwrap()).unwrap()
}

#[test]
fn the_folder_follows_the_setting_then_the_downloads_folder() {
    let home = Path::new("/srv/home/example");
    assert_eq!(
        folder("", Some(Path::new("/srv/home/example/Загрузки")), home),
        PathBuf::from("/srv/home/example/Загрузки/Zerus Files")
    );
    assert_eq!(
        folder("", None, home),
        PathBuf::from("/srv/home/example/Downloads/Zerus Files")
    );
    assert_eq!(
        folder("/srv/shared/../files/./cache", None, home),
        PathBuf::from("/srv/files/cache")
    );
}

#[test]
fn names_are_made_safe_as_qt_makes_them() {
    assert_eq!(filename("/srv/projects/report.pdf"), "report.pdf");
    assert_eq!(filename("bad<>:\"|?*name.txt"), "bad_______name.txt");
    assert_eq!(filename("tab\tname"), "tab_name");
    assert_eq!(filename("trailing. . "), "trailing");
    assert_eq!(filename(""), "download");
    assert_eq!(filename(".."), "download");
    assert_eq!(filename("con.txt"), "download-con.txt");
    assert_eq!(filename("LPT¹"), "download-LPT¹");
    assert_eq!(filename("COM10.txt"), "COM10.txt");
    assert_eq!(filename(".zerus-files.json"), "download-.zerus-files.json");
    let long = format!("{}.tar.gz", "ж".repeat(200));
    let made = filename(&long);
    assert!(made.ends_with(".gz") && made.len() <= 180, "{made}");
    assert!(
        !safe_name(".Zerus-part-1")
            && !safe_name("a/b")
            && !safe_name("c:")
            && safe_name("report.pdf")
    );
}

#[test]
fn source_digests_are_qts() {
    // Produced by Qt 6.12: QDataStream (Qt_6_0) << host << path, then SHA-256.
    assert_eq!(
        source_digest(&source("/srv/projects/report.pdf")),
        "ba716e2d4f5b8e750832e7051d4aecf26c57f2580c96b004e15edb9c08f940c0"
    );
    assert_eq!(
        source_digest(&Source {
            host: "speech-host".into(),
            path: "/srv/ünïcode/日本語 😀.txt".into()
        }),
        "5db65d5d72cc0707b91657d3d920b3507dc3b681b0a9801300d5e06338c783db"
    );
}

#[test]
fn a_saved_copy_is_found_again_until_it_changes() {
    let folder = Folder::new("lookup");
    let root = &folder.0;
    let report = source("/srv/projects/report.pdf");
    assert_eq!(lookup(root, &report, NOW).unwrap(), None);
    let saved = save(root, "report.pdf", b"first", Some(&report), 0);
    assert_eq!(saved, root.join("report.pdf"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    let written = index(root);
    assert_eq!(written["version"], 1);
    let entry = &written["files"][0];
    assert_eq!(entry["bytes"], 5);
    assert_eq!(entry["saved"], NOW);
    assert_eq!(
        entry["source"],
        json!({"host": "mac", "path": "/srv/projects/report.pdf"})
    );
    assert_eq!(entry["source_sha256"], source_digest(&report));
    assert_eq!(entry["sha256"], hex(&Sha256::digest(b"first")));

    // A hit renews the opening grace.
    assert_eq!(lookup(root, &report, NOW + 5).unwrap(), Some(saved.clone()));
    assert_eq!(index(root)["files"][0]["saved"], NOW + 5);

    // A second copy takes " (1)" and the attribution; the first stays.
    let newer = save(root, "report.pdf", b"second", Some(&report), 0);
    assert_eq!(newer, root.join("report (1).pdf"));
    let files = index(root)["files"].as_array().unwrap().clone();
    assert_eq!(files.len(), 2);
    let named = |name: &str| {
        files
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap()
            .clone()
    };
    assert!(named("report.pdf").get("source").is_none());
    assert!(named("report (1).pdf").get("source").is_some());
    assert_eq!(lookup(root, &report, NOW).unwrap(), Some(newer.clone()));

    // Edited: no longer found, and its attribution goes; the file stays.
    fs::write(&newer, b"edited by hand").unwrap();
    assert_eq!(lookup(root, &report, NOW).unwrap(), None);
    assert!(newer.exists());
    assert!(index(root)["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry.get("source").is_none()));
    assert_eq!(lookup(root, &report, NOW).unwrap(), None);
}

#[test]
fn a_trim_removes_the_oldest_unchanged_copies_past_the_grace() {
    let folder = Folder::new("trim");
    let root = &folder.0;
    let old = save(root, "old.bin", &[1; 400], None, 10 * 60_000);
    let changed = save(root, "changed.bin", &[2; 400], None, 9 * 60_000);
    let middle = save(root, "middle.bin", &[3; 400], None, 8 * 60_000);
    let recent = save(root, "recent.bin", &[4; 400], None, 30_000);
    assert_eq!(
        usage(root),
        Usage {
            bytes: 1600,
            files: 4,
            error: None
        }
    );
    // Same size, other content: kept, and no longer counted or indexed.
    fs::write(&changed, [9; 400]).unwrap();
    let after = trim(root, 500, NOW);
    assert!(!old.exists() && !middle.exists());
    assert!(changed.exists() && recent.exists());
    assert_eq!(
        after,
        Usage {
            bytes: 400,
            files: 1,
            error: None
        }
    );
    let names: Vec<String> = index(root)["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["recent.bin"]);
    // Never clear: nothing goes.
    assert_eq!(
        trim(root, 0, NOW + 3_600_000),
        Usage {
            bytes: 400,
            files: 1,
            error: None
        }
    );
    assert!(recent.exists());
}

#[test]
fn an_index_qt_could_not_trust_is_never_rewritten() {
    let folder = Folder::new("invalid");
    let root = &folder.0;
    let copy = save(root, "keep.bin", b"keep", None, 10 * 60_000);
    for bad in [
        &b"{not json"[..],
        br#"{"version":2,"files":[]}"#,
        br#"{"version":1,"files":[{"name":"../x","bytes":1,"saved":1,"sha256":"00"}]}"#,
    ] {
        fs::write(root.join(INDEX), bad).unwrap();
        let result = trim(root, 1, NOW);
        assert_eq!(result.error.as_deref(), Some(INDEX_INVALID));
        assert_eq!(fs::read(root.join(INDEX)).unwrap(), bad);
        assert!(copy.exists());
        assert_eq!(
            lookup(root, &source("/srv/x"), NOW).unwrap_err(),
            INDEX_INVALID
        );
        let part = reserve(root);
        assert_eq!(part.unwrap_err(), INDEX_INVALID);
    }
    // Too large to read.
    let big = File::create(root.join(INDEX)).unwrap();
    big.set_len(MAX_INDEX_BYTES + 1).unwrap();
    assert_eq!(
        usage(root).error.as_deref(),
        Some("Cannot read the downloaded files index.")
    );
    #[cfg(unix)]
    {
        fs::remove_file(root.join(INDEX)).unwrap();
        std::os::unix::fs::symlink("/srv/elsewhere.json", root.join(INDEX)).unwrap();
        assert_eq!(
            reserve(root).unwrap_err(),
            "The downloaded files index is a link."
        );
        assert_eq!(
            usage(root).error.as_deref(),
            Some("Cannot read the downloaded files index.")
        );
    }
}

#[test]
fn a_download_given_up_leaves_nothing_behind() {
    let folder = Folder::new("dropped");
    let root = &folder.0;
    let part = reserve(root).unwrap();
    let path = part.path().to_path_buf();
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with(PART));
    fs::write(&path, b"half").unwrap();
    drop(part);
    assert!(!path.exists());
    assert_eq!(
        usage(root),
        Usage {
            bytes: 0,
            files: 0,
            error: None
        }
    );
    assert!(!root.join(LOCK).exists());
}

#[test]
fn an_invalid_source_is_refused_before_anything_is_saved() {
    let folder = Folder::new("source");
    let root = &folder.0;
    let part = reserve(root).unwrap();
    fs::write(part.path(), b"x").unwrap();
    let path = part.path().to_path_buf();
    let bad = Source {
        host: "mac".into(),
        path: String::new(),
    };
    assert_eq!(
        complete(part, "x", Some(&bad), NOW).unwrap_err(),
        "The downloaded file source is invalid."
    );
    assert!(!path.exists());
    assert_eq!(
        lookup(root, &bad, NOW).unwrap_err(),
        "The downloaded file source is invalid."
    );
    // An empty source is no source.
    let empty = Source {
        host: String::new(),
        path: String::new(),
    };
    let part = reserve(root).unwrap();
    fs::write(part.path(), b"y").unwrap();
    assert!(complete(part, "y", Some(&empty), NOW).is_ok());
}

#[cfg(unix)]
#[test]
fn a_linked_folder_is_refused() {
    let folder = Folder::new("linked");
    fs::create_dir_all(&folder.0).unwrap();
    let target = folder.0.join("real");
    fs::create_dir_all(&target).unwrap();
    let link = folder.0.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert_eq!(
        reserve(&link).unwrap_err(),
        "The downloaded files folder is unavailable or is a link."
    );
}

/// Against the Qt client's own DownloadCache: with
/// ZERUS_QT_DOWNLOAD_CACHE_CHECK naming client/scripts/qt-download-cache-check
/// built with Qt 6, both apps read and change one folder.
#[test]
#[ignore = "needs ZERUS_QT_DOWNLOAD_CACHE_CHECK, the Qt DownloadCache driver"]
fn qt_and_the_client_share_a_folder() {
    let checker =
        std::env::var("ZERUS_QT_DOWNLOAD_CACHE_CHECK").expect("ZERUS_QT_DOWNLOAD_CACHE_CHECK");
    let folder = Folder::new("qt-shared");
    let contents = Folder::new("qt-shared-contents");
    fs::create_dir_all(&contents.0).unwrap();
    let root = &folder.0;
    let qt = |args: &[&str]| {
        let out = std::process::Command::new(&checker)
            .args(args)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let at = |ms: i64| (NOW + ms).to_string();
    let root_text = root.to_string_lossy().into_owned();
    let report = source("/srv/projects/report.pdf");

    // Qt saves a copy; the client finds it.
    let from_qt = contents.0.join("qt");
    fs::write(&from_qt, b"from qt").unwrap();
    let saved = qt(&[
        "publish",
        &root_text,
        "report.pdf",
        "mac",
        "/srv/projects/report.pdf",
        &from_qt.to_string_lossy(),
        &at(-600_000),
    ]);
    assert_eq!(saved, root.join("report.pdf").to_string_lossy());
    assert_eq!(
        lookup(root, &report, NOW - 500_000).unwrap(),
        Some(root.join("report.pdf"))
    );

    // The client saves a newer copy of the same file; Qt finds the client's.
    let ours = save(
        root,
        "report.pdf",
        b"from the client",
        Some(&report),
        300_000,
    );
    assert_eq!(ours, root.join("report (1).pdf"));
    assert_eq!(
        qt(&[
            "lookup",
            &root_text,
            "mac",
            "/srv/projects/report.pdf",
            &at(0)
        ]),
        ours.to_string_lossy()
    );

    // Qt reads the client's index, and both count the same copies.
    assert_eq!(qt(&["usage", &root_text]), "22 2");
    assert_eq!(
        usage(root),
        Usage {
            bytes: 22,
            files: 2,
            error: None
        }
    );

    // Qt trims the oldest copy past its grace; the client agrees after.
    assert_eq!(qt(&["trim", &root_text, "10", &at(1_000)]), "15 1");
    assert!(!root.join("report.pdf").exists());
    assert_eq!(
        usage(root),
        Usage {
            bytes: 15,
            files: 1,
            error: None
        }
    );
    assert_eq!(
        lookup(root, &report, NOW + 2_000).unwrap(),
        Some(ours.clone())
    );

    // The client trims what Qt saved.
    let more = contents.0.join("more");
    fs::write(&more, b"older from qt").unwrap();
    qt(&[
        "publish",
        &root_text,
        "notes.txt",
        "mac",
        "/srv/notes.txt",
        &more.to_string_lossy(),
        &at(-900_000),
    ]);
    assert_eq!(
        trim(root, 20, NOW + 120_000),
        Usage {
            bytes: 15,
            files: 1,
            error: None
        }
    );
    assert!(!root.join("notes.txt").exists());
    assert_eq!(qt(&["usage", &root_text]), "15 1");
}
