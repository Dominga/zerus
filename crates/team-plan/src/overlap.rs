//! Whether two write sets touch the same files.
//!
//! Imported from WhiteWh/zerus at `ca4cc4481` (`src/state/teams/model.rs`:
//! `Task::output_overlaps` and `Task::paths_overlap`) without the fork's
//! hard-coded list of folders whose paths only advise. Here every path holds,
//! and paths are read so that no spelling hides an overlap.

/// Whether any path of `a` contains, equals or lies inside any path of `b`.
/// An empty write set overlaps nothing.
pub fn overlaps(a: &[String], b: &[String]) -> bool {
    let b: Vec<Vec<&str>> = b.iter().map(|path| components(path)).collect();
    a.iter()
        .map(|path| components(path))
        .any(|a| b.iter().any(|b| contains(&a, b)))
}

/// Whether one path contains the other, compared by components, so
/// `src/api` never contains `src/apis`.
pub fn paths_overlap(a: &str, b: &str) -> bool {
    contains(&components(a), &components(b))
}

/// Whether the shorter path is a prefix of the longer one, part by part.
fn contains(a: &[&str], b: &[&str]) -> bool {
    a.iter().zip(b).all(|(a, b)| same_part(a, b))
}

/// Whether two parts may name one file or folder. ASCII parts compare
/// without regard to case, as on a case-insensitive checkout. Case and
/// normalization rules for other characters differ between file systems
/// (NTFS maps both sigmas to one capital; some file systems treat composed
/// and decomposed letters as one), so a part with a non-ASCII character
/// matches any part in its place: unknown counts as the same.
fn same_part(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        a.eq_ignore_ascii_case(b)
    } else {
        true
    }
}

/// A path as the folders it lies in, read so that no spelling hides an
/// overlap. A write set is relative to the repository, so an absolute path
/// (a leading separator or a drive prefix) is the whole repository, which is
/// no parts. Otherwise `/` and `\` both separate, `.` and empty parts drop
/// out, `..` steps back, and a path that leaves the repository is the whole
/// repository. A pattern counts as the folder it starts in, unless a `..`
/// follows it, which could lead anywhere. Parts compare by [`same_part`].
fn components(path: &str) -> Vec<&str> {
    if absolute(path) {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut pattern = false;
    for part in path.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if pattern || parts.pop().is_none() {
                    return Vec::new();
                }
            }
            _ if pattern => {}
            _ if part.contains(['*', '?', '[', '{']) => pattern = true,
            _ => parts.push(part),
        }
    }
    parts
}

/// Whether a path starts at a root: a leading separator, or a drive prefix
/// such as `C:`.
fn absolute(path: &str) -> bool {
    match path.as_bytes() {
        [b'/' | b'\\', ..] => true,
        [drive, b':', ..] => drive.is_ascii_alphabetic(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn a_folder_holds_what_is_inside_it_and_nothing_beside_it() {
        assert!(paths_overlap("src/api", "src/api/routes.rs"));
        assert!(paths_overlap("src/api/routes.rs", "src/api"));
        assert!(paths_overlap("src/api/routes.rs", "src/api/routes.rs"));
        assert!(!paths_overlap("src/api", "src/apis"));
        assert!(!paths_overlap("src/api/a.rs", "src/api/b.rs"));
    }

    #[test]
    fn spelling_does_not_hide_an_overlap() {
        assert!(paths_overlap("./src/a.rs", "src/a.rs"));
        assert!(paths_overlap("src//a.rs", "src/a.rs"));
        assert!(paths_overlap("src/", "src/a.rs"));
        assert!(paths_overlap("/src/a.rs", "src/a.rs"));
        assert!(paths_overlap("src\\a.rs", "src/a.rs"));
        assert!(paths_overlap("src/x/../a.rs", "src/a.rs"));
        assert!(!paths_overlap("src/x/../a.rs", "src/x/a.rs"));
    }

    #[test]
    fn the_whole_repository_overlaps_everything() {
        for whole in ["", ".", "/", "./", "../elsewhere", "src/../../x"] {
            assert!(paths_overlap(whole, "docs/readme.md"), "{whole:?}");
        }
    }

    #[test]
    fn a_pattern_holds_the_folder_it_starts_in() {
        assert!(paths_overlap("src/teams/*", "src/teams/cli.rs"));
        assert!(paths_overlap("src/**/mod.rs", "src/teams/mod.rs"));
        assert!(paths_overlap(
            "client/src/ui/*.tsx",
            "client/src/ui/teams/Board.tsx"
        ));
        assert!(paths_overlap("crates/{a,b}/lib.rs", "crates/c/lib.rs"));
        assert!(!paths_overlap("client/*.ts", "src/main.rs"));
    }

    #[test]
    fn a_top_level_area_is_not_only_advice() {
        // The fork let `client`, `crates/replica` or `src` pass as advice.
        assert!(paths_overlap("client", "client/src/app.tsx"));
        assert!(paths_overlap(
            "crates/replica",
            "crates/replica/src/task.rs"
        ));
        assert!(paths_overlap("src", "src/main.rs"));
        assert!(paths_overlap("Makefile", "Makefile"));
    }

    #[test]
    fn a_parent_step_after_a_pattern_covers_the_whole_repository() {
        assert!(paths_overlap("src/*/../../README.md", "README.md"));
        assert!(paths_overlap("src/*/..", "docs/guide.md"));
        assert!(paths_overlap("src/**/../x.rs", "client/app.tsx"));
        // Even where the step back would stay inside a folder.
        assert!(paths_overlap("src/teams/*/../cli.rs", "docs/guide.md"));
        // A pattern without a later parent step still holds only its folder.
        assert!(!paths_overlap("src/*/x.rs", "docs/guide.md"));
    }

    #[test]
    fn spellings_that_differ_only_in_case_overlap() {
        assert!(paths_overlap("src/API.rs", "src/api.rs"));
        assert!(paths_overlap("Src", "src/main.rs"));
        assert!(paths_overlap("CLIENT/src/App.tsx", "client/SRC/app.tsx"));
        assert!(overlaps(
            &set(&["Docs/Readme.md"]),
            &set(&["docs/README.MD"])
        ));
        assert!(!paths_overlap("src/API.rs", "src/apis.rs"));
    }

    #[test]
    fn a_non_ascii_part_matches_any_part_in_its_place() {
        // Capital sigma lowercases to U+03C3, final sigma stays U+03C2;
        // NTFS maps both to the capital, so they name one file there.
        assert!(paths_overlap("src/\u{3a3}.rs", "src/\u{3c2}.rs"));
        // A composed and a decomposed accented letter.
        assert!(paths_overlap("docs/caf\u{e9}.md", "docs/cafe\u{301}.md"));
        // Two different Cyrillic names in one folder.
        assert!(paths_overlap(
            "docs/\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442}.md",
            "docs/\u{43c}\u{438}\u{440}.md"
        ));
        // A non-ASCII name and an ASCII sibling.
        assert!(paths_overlap("src/\u{e4}.rs", "src/a.rs"));
        assert!(overlaps(&set(&["src/a.rs"]), &set(&["src/\u{e4}.rs"])));
        // Byte-equal non-ASCII names are one name.
        assert!(paths_overlap(
            "docs/\u{e9}t\u{e9}.md",
            "docs/\u{e9}t\u{e9}.md"
        ));
    }

    #[test]
    fn non_ascii_names_in_different_folders_do_not_overlap() {
        assert!(!paths_overlap("src/\u{e4}.rs", "docs/\u{f6}.md"));
        // Parts after an unknown one are still compared.
        assert!(!paths_overlap("\u{3a3}/a.rs", "\u{3c3}/b.rs"));
        assert!(paths_overlap("\u{3a3}/a.rs", "\u{3c3}/A.RS"));
    }

    #[test]
    fn an_absolute_path_covers_the_whole_repository() {
        for absolute in [
            "/outside/repo/src/a.rs",
            "/src/a.rs",
            r"\\server\share\src\a.rs",
            "C:\\repo\\src\\a.rs",
            "c:/repo/src/a.rs",
            "D:relative.rs",
        ] {
            assert!(paths_overlap(absolute, "docs/guide.md"), "{absolute:?}");
            assert!(paths_overlap("docs/guide.md", absolute), "{absolute:?}");
        }
        // A colon later in a name, or after a digit, is no drive.
        assert!(!paths_overlap("docs/a:b.md", "src/a.rs"));
        assert!(!paths_overlap("1:notes.md", "src/a.rs"));
    }

    #[test]
    fn sets_overlap_when_any_pair_does() {
        assert!(overlaps(&set(&["docs/a.md", "src/x.rs"]), &set(&["src"])));
        assert!(!overlaps(&set(&["docs/a.md"]), &set(&["src", "client"])));
        assert!(!overlaps(&[], &set(&["src"])));
        assert!(!overlaps(&set(&["src"]), &[]));
    }
}
