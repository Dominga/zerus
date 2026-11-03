#![cfg(windows)]
//! Disposable hosted-native checks. The child is this test image, not an owner GUI.
#[path = "support/windows_frontend_peer.rs"]
mod native_peer;
use native_peer::{client, marker, sid_text, Child, DONE};
use std::{
    env,
    fs::OpenOptions,
    os::windows::fs::OpenOptionsExt,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY},
    Storage::FileSystem::FILE_SHARE_READ,
};
use zerus_client_core::desktop::{self, server_trust::Verdict, windows, Observation};

#[test]
#[ignore = "mandatory hosted windows-2025 native adversarial fixture; CI selects this exact test"]
fn hosted_same_user_frontend_impostors_are_unknown() {
    assert_eq!(env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert_eq!(
        env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted")
    );
    let image = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(env::current_exe().unwrap())
        .unwrap();
    let image_id = windows::file_id(&image).unwrap();
    let own = windows::own_facts().unwrap();
    assert!(own.integrity >= 0x2000);
    let home = env::temp_dir().join(format!("zerus-native-peer-{}", uuid::Uuid::new_v4()));
    let names = [
        windows::activation_name("hgs-tray", &home),
        format!("com.hgdev.hgs-tray.gui-{}", sid_text(&own.user).unwrap()),
    ];
    for (low, mode) in [
        (false, "reply"),
        (true, "reply"),
        (true, "denied"),
        (true, "busy"),
    ] {
        let child = Child::start(low, mode, &names).unwrap();
        let facts = child.facts().unwrap();
        assert_eq!(facts.user, own.user);
        assert_eq!(facts.session, own.session);
        assert_eq!(facts.image, Some(image_id));
        assert_eq!(facts.integrity, if low { 0x1000 } else { own.integrity });
        let result = catch_unwind(AssertUnwindSafe(|| {
            for (index, name) in names.iter().enumerate() {
                if mode == "reply" {
                    let connection = client(name).unwrap();
                    let verified = windows::verify_server(&connection, image_id);
                    if low {
                        assert!(matches!(
                            verified,
                            windows::Server::Refused(Verdict::LowerIntegrity)
                        ));
                    } else {
                        assert!(matches!(verified, windows::Server::Verified(_)));
                    }
                    drop(connection);
                } else {
                    let error = client(name).unwrap_err();
                    assert_eq!(
                        error.raw_os_error(),
                        Some(if mode == "denied" {
                            ERROR_ACCESS_DENIED as i32
                        } else {
                            ERROR_PIPE_BUSY as i32
                        })
                    );
                }
                // The production probe accepts profile names only; the dotted
                // compatibility pipe is checked directly here and by forwarding.
                if index == 0 {
                    let observation = windows::probe_at(
                        name,
                        &uuid::Uuid::new_v4().to_string(),
                        &"a".repeat(64),
                        Instant::now() + desktop::PROBE_WAIT,
                        image_id,
                    );
                    if low {
                        assert!(
                            matches!(observation, Observation::Unknown),
                            "refusal must never become absence or a GUI hello"
                        );
                    } else {
                        assert!(matches!(observation, Observation::LiveNext { .. }));
                    }
                }
            }
        }));
        let capture = child.finish().unwrap();
        assert!(capture.natural);
        assert_eq!(capture.exit, 0);
        let done = marker(&capture.output, DONE).unwrap();
        assert_eq!(done["closed"], 2);
        assert_eq!(done["forwards"], 0);
        assert_eq!(done["queries"], if low { 0 } else { 1 });
        assert_eq!(
            done["bytes"],
            if low {
                0
            } else {
                desktop::query(&uuid::Uuid::new_v4().to_string())
                    .unwrap()
                    .len()
            }
        );
        println!("native frontend case low={low} mode={mode} clean_exit=true no_forward=true");
        if let Err(panic) = result {
            resume_unwind(panic);
        }
    }
}
