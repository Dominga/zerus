//! Different native processes exercise both ends of one private pipe.
use super::*;
use crate::host::token::Token;
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    sync::{atomic::AtomicBool, mpsc, Arc},
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

const CHILD: &str = "desktop::transport::windows_tests::outgoing_connection_child";
const ROOT: &str = "ZERUS_DESKTOP_PIPE_ROLE_ROOT";
const CONFIG: &str = "ZERUS_DESKTOP_PIPE_ROLE_CONFIG";
const SERVER: &str = "ZERUS_DESKTOP_PIPE_ROLE_SERVER";

#[test]
#[ignore = "run only by the bounded pipe-role parent"]
fn outgoing_connection_child() {
    let location = Location {
        root: std::env::var_os(ROOT).unwrap().into(),
        configuration: std::env::var(CONFIG).unwrap(),
    };
    let expected: u32 = std::env::var(SERVER).unwrap().parse().unwrap();
    assert_ne!(expected, std::process::id());
    let mut stream = connect(&location).unwrap().unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    stream.write_all(b"R").unwrap();
    // Exact old failure: the outgoing end cannot impersonate an incoming
    // client. It must not silently fall back to this child's own identity.
    assert!(peer_pid(&stream).is_err());
    assert!(Token::of_thread().unwrap().is_none());
    assert_eq!(server_pid(&stream).unwrap(), expected);
    let mut reply = [0];
    stream.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"K");
    assert_eq!(server_pid(&stream).unwrap(), expected);
    assert!(Token::of_thread().unwrap().is_none());
    println!("\nDESKTOP_PIPE_ROLE_RESULT verified");
}

struct Resources {
    root: tempfile::TempDir,
    listener: Arc<Listener>,
}
struct Close(Arc<Listener>);
impl Drop for Close {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[test]
fn outgoing_server_and_incoming_client_identities_use_the_correct_pipe_end() {
    let root = tempfile::tempdir().unwrap();
    let location = Location {
        root: root.path().to_path_buf(),
        configuration: uuid::Uuid::new_v4().to_string(),
    };
    // Keep the one private root through actual child and pipe completion,
    // including an uncertain bounded runner result.
    let listener = Arc::new(Listener::bind(&location).unwrap());
    let resources = Arc::new(Resources { root, listener });
    let _close = Close(resources.listener.clone());
    let (spawned, expected) = mpsc::channel();
    let server = {
        let listener = resources.listener.clone();
        std::thread::spawn(move || -> Result<()> {
            let mut stream = listener.accept()?;
            stream.set_read_timeout(Some(TIMEOUT))?;
            stream.set_write_timeout(Some(TIMEOUT))?;
            let mut hello = [0];
            stream.read_exact(&mut hello)?;
            anyhow::ensure!(&hello == b"R", "unexpected private fixture input");
            authenticate(&stream)?;
            let pid = peer_pid(&stream)?;
            anyhow::ensure!(
                pid == expected.recv_timeout(TIMEOUT)? && pid != std::process::id(),
                "incoming peer must be the original separate child"
            );
            stream.write_all(b"K")?;
            Ok(())
        })
    };
    let mut environment = Vec::new();
    for key in ["SystemRoot", "WINDIR", "PATH", "TEMP", "TMP"] {
        if let Ok(value) = std::env::var(key) {
            environment.push((key.to_owned(), value));
        }
    }
    environment.extend([
        (ROOT.into(), resources.root.path().to_str().unwrap().into()),
        (CONFIG.into(), location.configuration),
        (SERVER.into(), std::process::id().to_string()),
    ]);
    let refs: Vec<_> = environment
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let child_env = environment.clone();
    let invocation = zerus_client_core::actions::Invocation {
        args: [
            "--ignored",
            "--exact",
            CHILD,
            "--test-threads=1",
            "--nocapture",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
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
            let child = command
                .env_clear()
                .envs(child_env)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?;
            let _ = spawned.send(child.id());
            Ok(child)
        },
        resources.clone(),
        |error, stderr| failure = Some(format!("{error:?}: {}", stderr.unwrap_or("no stderr"))),
    );
    resources.listener.close();
    let server = server.join().expect("private server thread panicked");
    let output = output.unwrap_or_else(|error| {
        panic!("native pipe control failed: {error:?}; {failure:?}; server={server:?}")
    });
    server.unwrap();
    assert!(output.len() <= 8192);
    let output = String::from_utf8(output).unwrap();
    assert_eq!(
        output
            .lines()
            .filter(|line| *line == "DESKTOP_PIPE_ROLE_RESULT verified")
            .count(),
        1,
        "{output}"
    );
}
