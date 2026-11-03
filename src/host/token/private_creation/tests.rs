//! All token changes happen in one isolated, bounded test child.
use super::*;
use crate::{
    desktop::creation_test::{Location, Store},
    platform,
};
use std::{
    fs,
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use windows_sys::Win32::Security::{
    Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
    CreateWellKnownSid, ImpersonateSelf, RevertToSelf, SecurityImpersonation, TokenDefaultDacl,
    TokenElevation, TokenElevationType, TokenGroups, TokenPrivileges, TokenSessionId,
    WinBuiltinAdministratorsSid, ACL, OWNER_SECURITY_INFORMATION, TOKEN_DEFAULT_DACL, TOKEN_GROUPS,
    TOKEN_PRIVILEGES,
};

const CHILD: &str = "host::token::private_creation::tests::creation_owner_child";
const ROOT: &str = "ZERUS_PRIVATE_CREATION_TEST_ROOT";
const PARENT: &str = "ZERUS_PRIVATE_CREATION_PARENT_TOKEN";

#[derive(Debug, PartialEq, Eq)]
struct Unchanged {
    user: Sid,
    integrity: super::super::Integrity,
    token: (u32, i32),
    groups: Vec<(Sid, u32)>,
    privileges: Vec<(u32, i32, u32)>,
    dacl: Option<Vec<u8>>,
    session: u32,
    elevation: u32,
    elevation_type: u32,
}
fn scalar(token: &Token, class: TOKEN_INFORMATION_CLASS) -> u32 {
    let bytes = token.information(class).unwrap();
    unsafe { *bytes.as_ptr().cast::<u32>() }
}
fn snapshot(token: &Token) -> Unchanged {
    let group_bytes = token.information(TokenGroups).unwrap();
    let groups = unsafe { &*group_bytes.as_ptr().cast::<TOKEN_GROUPS>() };
    assert!(groups.GroupCount <= 1024);
    let groups =
        unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) }
            .iter()
            .map(|g| (unsafe { Sid::copy(g.Sid) }, g.Attributes))
            .collect();
    let privilege_bytes = token.information(TokenPrivileges).unwrap();
    let privileges = unsafe { &*privilege_bytes.as_ptr().cast::<TOKEN_PRIVILEGES>() };
    assert!(privileges.PrivilegeCount <= 1024);
    let privileges = unsafe {
        std::slice::from_raw_parts(
            privileges.Privileges.as_ptr(),
            privileges.PrivilegeCount as usize,
        )
    }
    .iter()
    .map(|p| (p.Luid.LowPart, p.Luid.HighPart, p.Attributes))
    .collect();
    let dacl_bytes = token.information(TokenDefaultDacl).unwrap();
    let dacl = unsafe { &*dacl_bytes.as_ptr().cast::<TOKEN_DEFAULT_DACL>() }.DefaultDacl;
    let dacl = if dacl.is_null() {
        None
    } else {
        let size = unsafe { (*dacl).AclSize } as usize;
        assert!((std::mem::size_of::<ACL>()..=65535).contains(&size));
        Some(unsafe { std::slice::from_raw_parts(dacl.cast::<u8>(), size) }.to_vec())
    };
    Unchanged {
        user: token.user().unwrap(),
        integrity: token.integrity().unwrap(),
        token: id(token).unwrap(),
        groups,
        privileges,
        dacl,
        session: scalar(token, TokenSessionId),
        elevation: scalar(token, TokenElevation),
        elevation_type: scalar(token, TokenElevationType),
    }
}
fn administrator() -> Sid {
    let mut bytes = [0u32; 17];
    let mut size = std::mem::size_of_val(&bytes) as u32;
    assert_ne!(
        unsafe {
            CreateWellKnownSid(
                WinBuiltinAdministratorsSid,
                std::ptr::null_mut(),
                bytes.as_mut_ptr().cast(),
                &mut size,
            )
        },
        0
    );
    unsafe { Sid::copy(bytes.as_mut_ptr().cast()) }
}
fn file_owner(path: &Path) -> Sid {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL};
    let file = fs::OpenOptions::new()
        .access_mode(READ_CONTROL)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .unwrap();
    let mut owner = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                file.as_raw_handle() as HANDLE,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    assert!(!owner.is_null());
    let result = unsafe { Sid::copy(owner) };
    unsafe { LocalFree(descriptor.cast()) };
    result
}
fn user_file(path: &Path, user: &Sid) {
    assert_eq!(
        &file_owner(path),
        user,
        "new file owner must be the actual User SID"
    );
    assert!(platform::verify_private(path, platform::Private::File).unwrap());
}

#[test]
#[ignore = "run only by the bounded isolated-token parent"]
fn creation_owner_child() {
    let root = std::path::PathBuf::from(std::env::var_os(ROOT).expect("private child root"));
    let token = writable_current().unwrap();
    let (low, high) = id(&token).unwrap();
    assert_ne!(
        format!("{low}:{high}"),
        std::env::var(PARENT).expect("original parent token identity"),
        "fixture must mutate only its own separate primary token"
    );
    let user = token.user().unwrap();
    let admin = administrator();
    // Native fixture arrangement only: pre-existing directories are user-owned,
    // like the maintained helper's directories in the failing hosted task.
    set_owner(&token, &user).unwrap();
    let bad = Location {
        root: root.join("bad/desktop"),
        configuration: "fixture".into(),
    };
    bad.prepare().unwrap();
    // Reproduce the exact native failure class under an eligible own token.
    // A standard-user local developer cannot select this group as owner.
    if let Err(error) = set_owner(&token, &admin) {
        if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
            panic!("hosted native owner control is not qualified: {error}");
        }
        println!("\nPRIVATE_CREATION_RESULT {{\"eligible\":false}}");
        return;
    }
    assert_eq!(token.default_owner().unwrap(), admin);
    let error = bad
        .lock()
        .err()
        .expect("Administrators-owned owner.lock must refuse");
    assert!(format!("{error:#}").contains("private single-link regular files"));
    let bad_path = bad.root.join("owner.lock");
    assert_eq!(file_owner(&bad_path), admin);
    let bad_identity = platform::file_identity(&bad_path).unwrap();
    let before = snapshot(&token);
    prepare().unwrap();
    assert_eq!(token.default_owner().unwrap(), user);
    assert_eq!(snapshot(&token), before, "only creation owner may change");
    // Existing unsafe files are not repaired, replaced or accepted.
    assert!(bad.lock().is_err());
    assert_eq!(file_owner(&bad_path), admin);
    let after = platform::file_identity(&bad_path).unwrap();
    assert_eq!(
        (bad_identity.device, bad_identity.inode),
        (after.device, after.inode)
    );

    let good = Location {
        root: root.join("good/desktop"),
        configuration: "fixture".into(),
    };
    good.prepare().unwrap();
    let _owner = good.lock().unwrap();
    user_file(&good.root.join("owner.lock"), &user);
    let _store = Store::open(&good, &uuid::Uuid::new_v4().to_string()).unwrap();
    for name in ["state.sqlite3", "state.sqlite3-wal", "state.sqlite3-shm"] {
        user_file(&good.root.join(name), &user);
    }
    crate::desktop::creation_test::database_files(&good).unwrap();
    let rollback = good.root.join("rollback.sqlite3");
    let db = rusqlite::Connection::open(&rollback).unwrap();
    db.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE fixture(value); BEGIN IMMEDIATE; INSERT INTO fixture VALUES(1);").unwrap();
    user_file(&rollback, &user);
    user_file(&good.root.join("rollback.sqlite3-journal"), &user);
    db.execute_batch("ROLLBACK").unwrap();
    let temporary = tempfile::NamedTempFile::new_in(&good.root).unwrap();
    user_file(temporary.path(), &user);
    let replaced = good.root.join("replacement.json");
    platform::persist(temporary, &replaced).unwrap();
    user_file(&replaced, &user);
    // The no-op does not change even the token's modification identity.
    let bytes = token.information(TokenStatistics).unwrap();
    let modified = unsafe { &*bytes.as_ptr().cast::<TOKEN_STATISTICS>() }.ModifiedId;
    prepare().unwrap();
    let bytes = token.information(TokenStatistics).unwrap();
    let next = unsafe { &*bytes.as_ptr().cast::<TOKEN_STATISTICS>() }.ModifiedId;
    assert_eq!(
        (modified.LowPart, modified.HighPart),
        (next.LowPart, next.HighPart)
    );
    // Impersonation is never silently converted into a creation context.
    assert_ne!(unsafe { ImpersonateSelf(SecurityImpersonation) }, 0);
    let refusal = prepare().unwrap_err();
    assert_ne!(unsafe { RevertToSelf() }, 0);
    assert_eq!(refusal.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(snapshot(&token), before);
    assert_eq!(token.default_owner().unwrap(), user);
    println!("\nPRIVATE_CREATION_RESULT {{\"eligible\":true,\"admin_default_refused\":true,\"existing_admin_still_refused\":true,\"new_files_user_owned\":true,\"other_token_properties_unchanged\":true}}");
}

#[test]
fn administrators_default_is_reproduced_and_corrected_in_one_isolated_child() {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
    let original = Token::current().unwrap();
    let original_owner = original.default_owner().unwrap();
    let original_state = snapshot(&original);
    let root = Arc::new(tempfile::tempdir().unwrap());
    let mut environment = Vec::new();
    for key in [
        "SystemRoot",
        "WINDIR",
        "PATH",
        "TEMP",
        "TMP",
        "GITHUB_ACTIONS",
    ] {
        if let Ok(value) = std::env::var(key) {
            environment.push((key.to_owned(), value));
        }
    }
    environment.push((ROOT.into(), root.path().to_str().unwrap().into()));
    let (low, high) = original_state.token;
    environment.push((PARENT.into(), format!("{low}:{high}")));
    let refs: Vec<_> = environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let child_env = environment.clone();
    let invocation = zerus_client_core::actions::Invocation {
        args: vec![
            "--ignored".into(),
            "--exact".into(),
            CHILD.into(),
            "--test-threads=1".into(),
            "--nocapture".into(),
        ],
        stdin: Some(vec![]),
        timeout: Duration::from_secs(15),
        request_id: None,
    };
    let mut failure = None;
    let output = zerus_client_core::run::run_owned_observed(
        &std::env::current_exe().unwrap(),
        &invocation,
        &refs,
        &AtomicBool::new(false),
        move |command| {
            command
                .env_clear()
                .envs(child_env)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
        },
        root,
        |error, stderr| failure = Some(format!("{error:?}: {}", stderr.unwrap_or("no stderr"))),
    )
    .unwrap_or_else(|error| panic!("isolated creation control failed: {error:?}; {failure:?}"));
    assert!(output.len() <= 8192);
    let text = String::from_utf8(output).unwrap();
    let rows: Vec<_> = text
        .lines()
        .filter_map(|l| l.strip_prefix("PRIVATE_CREATION_RESULT "))
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "one actual executed child control required: {text}"
    );
    let result: serde_json::Value = serde_json::from_str(rows[0]).unwrap();
    if result["eligible"] == false {
        assert_ne!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    } else {
        for field in [
            "admin_default_refused",
            "existing_admin_still_refused",
            "new_files_user_owned",
            "other_token_properties_unchanged",
        ] {
            assert_eq!(result[field], true, "{field}");
        }
    }
    assert_eq!(snapshot(&original), original_state);
    assert_eq!(
        original.default_owner().unwrap(),
        original_owner,
        "child cannot change parent creation defaults"
    );
}
