use super::*;

fn file(root: &Path, name: &str, value: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, value).unwrap();
    path
}

#[test]
fn missing_configs_and_absent_permission_keys_describe_native_allow() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let paths = [root.join("opencode.json"), root.join("opencode.jsonc")];
    assert_eq!(inspect(&paths, &[]), Some(Policy::Default.describe()));
    let p = file(
        root,
        "opencode.json",
        r#"{"model":"example/model","apiKey":"fixture-auth-marker","experimental":{"warming":true}}"#,
    );
    let before = fs::read(&p).unwrap();
    let modified = fs::metadata(&p).unwrap().modified().unwrap();
    assert_eq!(inspect(&paths, &[]), Some(Policy::Default.describe()));
    assert_eq!(fs::read(&p).unwrap(), before);
    assert_eq!(fs::metadata(p).unwrap().modified().unwrap(), modified);
    assert!(!inspect(&paths, &[])
        .unwrap()
        .1
        .contains("fixture-auth-marker"));
}

#[test]
fn supports_legacy_and_current_allow_ask_and_deny_without_writing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    for (effect, want) in [
        ("allow", Policy::Allow),
        ("ask", Policy::Ask),
        ("deny", Policy::Deny),
    ] {
        for body in [
            format!(r#"{{"permission":"{effect}"}}"#),
            format!(r#"{{"permission":{{"*":"{effect}"}}}}"#),
            format!(r#"{{"permissions":[{{"action":"*","resource":"*","effect":"{effect}"}}]}}"#),
        ] {
            let path = file(root, "opencode.json", &body);
            assert_eq!(
                inspect(std::slice::from_ref(&path), &[]),
                Some(want.describe())
            );
            assert_eq!(fs::read_to_string(path).unwrap(), body);
        }
    }
}

#[test]
fn jsonc_comments_commas_escaped_strings_and_bom_keep_their_meaning() {
    let tmp = tempfile::tempdir().unwrap();
    let path=file(tmp.path(),"opencode.jsonc","\u{feff}{ // comment\n\"url\":\"https://example.test/a//b/*c*/\\\"\", /* comment */ \"permission\":\"ask\", }");
    assert_eq!(inspect(&[path], &[]), Some(Policy::Ask.describe()));
}

#[test]
fn global_json_jsonc_explicit_and_system_rules_follow_load_order() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let mut paths = vec![
        file(root, "opencode.json", r#"{"permission":"deny"}"#),
        file(root, "opencode.jsonc", r#"{"permission":"ask"}"#),
    ];
    assert_eq!(inspect(&paths, &[]), Some(Policy::Ask.describe()));
    paths.push(file(root, "explicit.json", r#"{"permission":"allow"}"#));
    assert_eq!(inspect(&paths, &[]), Some(Policy::Allow.describe()));
    paths.push(file(
        root,
        "system.json",
        r#"{"permissions":[{"action":"*","resource":"*","effect":"deny"}]}"#,
    ));
    assert_eq!(inspect(&paths, &[]), Some(Policy::Deny.describe()));
}

#[test]
fn ordered_exceptions_are_custom_until_a_later_catch_all_replaces_them() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let p = file(
        root,
        "opencode.json",
        r#"{"permissions":[{"action":"shell","resource":"*","effect":"ask"}]}"#,
    );
    assert_eq!(
        inspect(std::slice::from_ref(&p), &[]),
        Some(Policy::Custom.describe())
    );
    fs::write(&p,r#"{"permissions":[{"action":"*","resource":"*","effect":"ask"},{"action":"read","resource":"*","effect":"allow"}]}"#).unwrap();
    assert_eq!(
        inspect(std::slice::from_ref(&p), &[]),
        Some(Policy::Custom.describe())
    );
    fs::write(&p, r#"{"permission":{"*":"deny","read":"allow"}}"#).unwrap();
    assert_eq!(
        inspect(std::slice::from_ref(&p), &[]),
        Some(Policy::Custom.describe())
    );
    let q = file(root, "opencode.jsonc", r#"{"permission":"allow"}"#);
    assert_eq!(inspect(&[p, q], &[]), Some(Policy::Allow.describe()));
}

#[test]
fn malformed_unreadable_oversize_and_unexpanded_policy_stay_unverified() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    for body in [
        "",
        "{",
        "[]",
        "null",
        "{/*",
        r#"{"permission":null}"#,
        r#"{"permission":"maybe"}"#,
        r#"{"permission":"{env:MODE}"}"#,
        r#"{"permissions":[{"effect":"ask"}]}"#,
        r#"{"policies":[{"action":"tool.use","resource":"*","effect":"deny"}]}"#,
        r#"{"agents":{"build":{"permissions":[]}}}"#,
        r#"{"mode":{"build":{"permission":{"*":"deny"}}}}"#,
        r#"{"default_agent":"custom-agent"}"#,
        r#"{"default_agent":null}"#,
    ] {
        let p = file(root, "opencode.json", body);
        assert_eq!(inspect(&[p], &[]), None, "{body}");
    }
    let p = root.join("opencode.json");
    fs::remove_file(&p).unwrap();
    fs::create_dir(&p).unwrap();
    assert_eq!(inspect(std::slice::from_ref(&p), &[]), None);
    fs::remove_dir(&p).unwrap();
    fs::write(&p, vec![b' '; FILE_LIMIT as usize + 1]).unwrap();
    assert_eq!(inspect(std::slice::from_ref(&p), &[]), None);
    fs::write(&p, [0xff]).unwrap();
    assert_eq!(inspect(std::slice::from_ref(&p), &[]), None);
    assert_eq!(inspect(&[PathBuf::from("relative.json")], &[]), None);
    fs::remove_file(&p).unwrap();
    fs::write(root.join("managed.plist"), "managed").unwrap();
    assert_eq!(inspect(&[p], &[root.join("managed.plist")]), None);
}

#[cfg(unix)]
#[test]
fn unreadable_files_dangling_symlinks_and_non_regular_files_are_not_absent() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let p = file(root, "opencode.json", "{}");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o0)).unwrap();
    if unsafe { libc::geteuid() } != 0 {
        assert_eq!(inspect(std::slice::from_ref(&p), &[]), None);
    }
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&p).unwrap();
    symlink(root.join("missing.json"), &p).unwrap();
    assert_eq!(inspect(std::slice::from_ref(&p), &[]), None);
    fs::remove_file(&p).unwrap();
    let name = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert_eq!(inspect(&[p], &[]), None);
}
