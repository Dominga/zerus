#![cfg(windows)]
//! Disposable hosted-native checks. The child is this test image, not an owner GUI.
#[path = "support/windows_frontend_peer.rs"]
mod native_peer;
use native_peer::{client, marker, sid_text, Child, DONE};
use std::{
    env,
    fs::OpenOptions,
    io,
    os::windows::fs::OpenOptionsExt,
    os::windows::io::AsRawHandle,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
    ptr, thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_NO_DATA, ERROR_PIPE_BUSY},
    Storage::FileSystem::{ReadFile, WriteFile, FILE_SHARE_READ},
    System::Pipes::{GetNamedPipeServerProcessId, SetNamedPipeHandleState, PIPE_NOWAIT},
};
use zerus_client_core::desktop::{self, server_trust::Verdict, windows, Hello, Observation};

// Deliberately bypass verification ONLY for the child we just created and
// still own. Prove that Low really can return the nonce-correct fabricated
// hello: a broken trust check must not be masked by a broken fake server.
fn known_owned_peer_hello(child: &Child, name: &str) {
    let until = Instant::now() + Duration::from_secs(1);
    let pipe = loop {
        match client(name) {
            Ok(pipe) => break pipe,
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                    && Instant::now() < until =>
            {
                thread::sleep(Duration::from_millis(1))
            }
            Err(error) => panic!("original control connection: {error}"),
        }
    };
    let mut server = 0;
    assert_ne!(
        unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut server) },
        0
    );
    assert_eq!(
        server, child.pid,
        "never send this control to an adopted server"
    );
    let mode = PIPE_NOWAIT;
    assert_ne!(
        unsafe { SetNamedPipeHandleState(pipe.as_raw_handle(), &mode, ptr::null(), ptr::null()) },
        0
    );
    let nonce = uuid::Uuid::new_v4().to_string();
    let query = desktop::query(&nonce).unwrap();
    let mut count = 0;
    assert_ne!(
        unsafe {
            WriteFile(
                pipe.as_raw_handle(),
                query.as_ptr(),
                query.len() as u32,
                &mut count,
                ptr::null_mut(),
            )
        },
        0
    );
    assert_eq!(count as usize, query.len());
    let mut reply = Vec::new();
    loop {
        assert!(Instant::now() < until, "owned Low hello control timed out");
        let mut bytes = [0u8; 512];
        let mut count = 0;
        let ok = unsafe {
            ReadFile(
                pipe.as_raw_handle(),
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                &mut count,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            assert_eq!(
                io::Error::last_os_error().raw_os_error(),
                Some(ERROR_NO_DATA as i32)
            );
        }
        reply.extend_from_slice(&bytes[..count as usize]);
        assert!(reply.len() <= desktop::REPLY_BYTES);
        if let Some(lf) = reply.iter().position(|b| *b == b'\n') {
            assert_eq!(lf + 1, reply.len());
            let hello: Hello = serde_json::from_slice(&reply[..lf]).unwrap();
            assert!(hello.identity_matches(&nonce, &"a".repeat(64)));
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

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
    for (low, mode, control) in [
        (false, "reply", false),
        (true, "reply", true),
        (true, "reply", false),
        (true, "denied", false),
        (true, "busy", false),
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
                    if control {
                        known_owned_peer_hello(&child, name);
                    }
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
        native_peer::assert_pipes_absent(&names);
        assert!(capture.natural);
        assert_eq!(capture.exit, 0);
        let done = marker(&capture.output, DONE).unwrap();
        assert_eq!(done["closed"], 2);
        assert_eq!(done["forwards"], 0);
        assert_eq!(done["queries"], if low && !control { 0 } else { 1 });
        assert_eq!(
            done["bytes"],
            if low && !control {
                0
            } else {
                desktop::query(&uuid::Uuid::new_v4().to_string())
                    .unwrap()
                    .len()
            }
        );
        println!("native frontend case low={low} mode={mode} control={control} clean_exit=true no_forward=true");
        if let Err(panic) = result {
            resume_unwind(panic);
        }
    }
}
