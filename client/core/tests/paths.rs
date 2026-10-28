//! The Qt client's own answers (`tray/src/MachinePlatform.h` with Qt 6.12's
//! `QDir::cleanPath`), recorded by a harness that calls it on each input.
use zerus_client_core::paths::{self, Style};

// path, clean (POSIX, Windows), absolute (POSIX, Windows), windows_shaped,
// leaf (POSIX, Windows)
type Answers = (
    &'static str,
    &'static str,
    &'static str,
    bool,
    bool,
    bool,
    &'static str,
    &'static str,
);

#[rustfmt::skip]
const PATHS: &[Answers] = &[
    ("", "", "", false, false, false, "", ""),
    ("/", "/", "/", true, false, false, "", ""),
    ("//", "/", "/", true, false, false, "", ""),
    ("/a/b/", "/a/b", "/a/b", true, false, false, "", ""),
    ("/a//b/./c/..", "/a/b", "/a/b", true, false, false, "..", ".."),
    ("a/../..", "..", "..", false, false, false, "..", ".."),
    ("../a", "../a", "../a", false, false, false, "a", "a"),
    ("./", ".", ".", false, false, false, "", ""),
    ("/..", "/..", "/", true, false, false, "..", ".."),
    ("/../a", "/../a", "/a", true, false, false, "a", "a"),
    ("/a/../..", "/..", "/", true, false, false, "..", ".."),
    ("C:\\", "C:\\", "C:/", false, true, true, "C:\\", ""),
    ("c:/", "c:", "c:/", false, true, true, "", ""),
    ("C:", "C:", "C:", false, false, false, "C:", "C:"),
    ("C:work", "C:work", "C:work", false, false, false, "C:work", "C:work"),
    ("C:\\work\\..\\..", "C:\\work\\..\\..", "C:/", false, true, true, "C:\\work\\..\\..", ".."),
    ("C:/a/./b/", "C:/a/b", "C:/a/b", false, true, true, "", ""),
    ("\\\\server\\share", "\\\\server\\share", "//server/share", false, true, true, "\\\\server\\share", ""),
    ("//server/share/", "/server/share", "//server/share", true, true, true, "", ""),
    ("//server", "/server", "//server", true, false, false, "server", "server"),
    ("//server/", "/server", "//server", true, false, false, "", ""),
    ("\\\\?\\C:\\x", "\\\\?\\C:\\x", "//?/C:/x", false, false, true, "\\\\?\\C:\\x", "x"),
    ("\\\\.\\pipe", "\\\\.\\pipe", "//./pipe", false, false, true, "\\\\.\\pipe", ""),
    ("\\work", "\\work", "/work", false, false, false, "\\work", "work"),
    ("//server/share/../..", "/", "//server/share", true, true, true, "..", ".."),
    ("x//y/", "x/y", "x/y", false, false, false, "", ""),
    ("/a/b/..", "/a", "/a", true, false, false, "..", ".."),
    ("D:/x/../../y", "y", "D:/y", false, true, true, "y", "y"),
    ("\\\\server\\share\\dir\\..", "\\\\server\\share\\dir\\..", "//server/share", false, true, true, "\\\\server\\share\\dir\\..", ".."),
    (".", ".", ".", false, false, false, ".", "."),
    ("..", "..", "..", false, false, false, "..", ".."),
    ("a/./b", "a/b", "a/b", false, false, false, "b", "b"),
    ("/../..", "/../..", "/", true, false, false, "..", ".."),
    ("a/..", ".", ".", false, false, false, "..", ".."),
    ("../..", "../..", "../..", false, false, false, "..", ".."),
    ("a/b/../../..", "..", "..", false, false, false, "..", ".."),
    ("/a/./../b", "/b", "/b", true, false, false, "b", "b"),
    ("C:/..", ".", "C:/", false, true, true, "..", ".."),
    ("C:/../x", "x", "C:/x", false, true, true, "x", "x"),
    ("//server/share/a/../../..", "/", "//server/share", true, true, true, "..", ".."),
    ("\\\\server\\share\\a", "\\\\server\\share\\a", "//server/share/a", false, true, true, "\\\\server\\share\\a", "a"),
    ("Ä/ß", "Ä/ß", "Ä/ß", false, false, false, "ß", "ß"),
    ("/a/b\\c", "/a/b\\c", "/a/b/c", true, false, false, "b\\c", "c"),
];

// path, folder, within (POSIX, Windows), same (POSIX, Windows)
#[rustfmt::skip]
const PAIRS: &[(&str, &str, bool, bool, bool, bool)] = &[
    ("/a/b", "/a", true, false, false, false),
    ("/a", "/a", true, false, true, true),
    ("/ab", "/a", false, false, false, false),
    ("/a", "/", true, false, false, false),
    ("/a/b", "a", false, false, false, false),
    ("/a/", "/a", true, false, false, false),
    ("C:/Work/x", "c:\\work", false, true, false, false),
    ("C:/workshop", "C:/work", false, false, false, false),
    ("C:\\a", "C:/", false, true, false, false),
    ("//server/share/x", "\\\\SERVER\\share", false, true, false, false),
    ("C:/a", "C:", false, false, false, false),
    ("/a/b", "/a/", false, false, false, false),
    ("C:/x", "\\\\?\\C:", false, false, false, false),
    ("x", "x", false, false, true, true),
    ("", "", false, false, true, true),
    (":/x/y", ":/x", true, false, false, false),
    ("C:/ÄB/x", "c:/äb", false, true, false, false),
    ("//server/share", "//server/share/", false, false, false, false),
    ("C:/", "C:/", false, true, true, true),
    ("C:/a", "c:\\", false, true, false, false),
];

#[test]
fn single_paths_answer_as_qt_does() {
    for &(
        path,
        posix,
        windows,
        absolute_posix,
        absolute_windows,
        shaped,
        leaf_posix,
        leaf_windows,
    ) in PATHS
    {
        assert_eq!(
            paths::clean(path, Style::Posix),
            posix,
            "clean posix {path:?}"
        );
        assert_eq!(
            paths::clean(path, Style::Windows),
            windows,
            "clean windows {path:?}"
        );
        assert_eq!(
            paths::absolute(path, Style::Posix),
            absolute_posix,
            "absolute posix {path:?}"
        );
        assert_eq!(
            paths::absolute(path, Style::Windows),
            absolute_windows,
            "absolute windows {path:?}"
        );
        assert_eq!(
            paths::windows_shaped(path),
            shaped,
            "windows_shaped {path:?}"
        );
        assert_eq!(
            paths::leaf(path, Style::Posix),
            leaf_posix,
            "leaf posix {path:?}"
        );
        assert_eq!(
            paths::leaf(path, Style::Windows),
            leaf_windows,
            "leaf windows {path:?}"
        );
    }
}

#[test]
fn pairs_answer_as_qt_does() {
    for &(path, folder, within_posix, within_windows, same_posix, same_windows) in PAIRS {
        assert_eq!(
            paths::within(path, folder, Style::Posix),
            within_posix,
            "within posix {path:?} {folder:?}"
        );
        assert_eq!(
            paths::within(path, folder, Style::Windows),
            within_windows,
            "within windows {path:?} {folder:?}"
        );
        assert_eq!(
            paths::same(path, folder, Style::Posix),
            same_posix,
            "same posix {path:?} {folder:?}"
        );
        assert_eq!(
            paths::same(path, folder, Style::Windows),
            same_windows,
            "same windows {path:?} {folder:?}"
        );
    }
}

#[test]
fn split_and_join_answer_as_qfileinfo_and_qdir_do() {
    // QFileInfo(path).path(), fileName(), QDir(path()).filePath(fileName() + "-work-1").
    for (path, parent, name, joined) in [
        ("/repo", "/", "repo", "/repo-work-1"),
        ("/a/b", "/a", "b", "/a/b-work-1"),
        ("/a/b/", "/a/b", "", "/a/b/-work-1"),
        ("/", "/", "", "/-work-1"),
        ("repo", ".", "repo", "./repo-work-1"),
        (
            "//server/share/repo",
            "//server/share",
            "repo",
            "//server/share/repo-work-1",
        ),
        ("/a/b-c.d", "/a", "b-c.d", "/a/b-c.d-work-1"),
    ] {
        let (found_parent, found_name) = paths::split(path, Style::Posix);
        assert_eq!(
            (found_parent.as_str(), found_name.as_str()),
            (parent, name),
            "{path:?}"
        );
        assert_eq!(
            paths::join(&found_parent, &format!("{found_name}-work-1")),
            joined,
            "{path:?}"
        );
    }
    // A Windows build's QFileInfo keeps the drive root's separator.
    assert_eq!(
        paths::split("C:/repo", Style::Windows),
        ("C:/".into(), "repo".into())
    );
    assert_eq!(
        paths::split("C:\\srv\\repo", Style::Windows),
        ("C:/srv".into(), "repo".into())
    );
    assert_eq!(paths::join("C:/", "repo-work-1"), "C:/repo-work-1");
}

#[test]
fn a_peer_is_posix_and_the_local_computer_follows_its_platform() {
    assert_eq!(paths::of(Some("windows"), false), Style::Posix);
    assert_eq!(paths::of(Some("windows"), true), Style::Windows);
    assert_eq!(paths::of(Some("macos"), true), Style::Posix);
    assert_eq!(paths::of(None, true), paths::build());
    assert_eq!(paths::of(Some(""), true), paths::build());
}

#[test]
fn native_shows_backslashes_only_on_windows() {
    assert_eq!(
        paths::native("C:/srv/repo", Style::Windows),
        "C:\\srv\\repo"
    );
    assert_eq!(paths::native("/srv/repo", Style::Posix), "/srv/repo");
}
