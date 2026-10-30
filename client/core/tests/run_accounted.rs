//! Existing runner outcomes with opt-in actual lifetime accounting. Synthetic
//! owned children only; no agent, service, task or process-group operation.
#![cfg(unix)]

use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};
use zerus_client_core::{
    desktop::drain::{Gate, Observation, Role},
    run::{run_accounted, Stopped},
    Exit, Invocation,
};

fn invocation(code: &str, stdin: Option<Vec<u8>>) -> Invocation {
    Invocation {
        args: vec!["-c".into(), code.into()],
        stdin,
        timeout: Duration::from_secs(5),
        request_id: None,
    }
}

#[test]
fn ordinary_no_input_exit_and_legacy_stderr_truncation_are_preserved() {
    let gate = Gate::new(1).unwrap();
    let exit = run_accounted(
        Path::new("python3"),
        &invocation(
            "import sys;print('result');sys.stderr.write('e'*70000);sys.exit(7)",
            None,
        ),
        &[],
        1024,
        &AtomicBool::new(false),
        Arc::new(gate.admit(Role::ReadDelivery).unwrap()),
    );
    assert_eq!(
        exit,
        Ok(Exit::Code {
            code: 7,
            stdout: b"result\n".to_vec(),
            stderr: "e".repeat(64 << 10),
        })
    );
    assert_eq!(gate.close().wait_until(Instant::now()), Observation::Idle);
}

#[test]
fn pre_cancel_and_spawn_failure_release_without_a_child_charge() {
    let gate = Gate::new(1).unwrap();
    let inv = invocation("", None);
    assert_eq!(
        run_accounted(
            Path::new("unavailable-accounted-helper"),
            &inv,
            &[],
            1024,
            &AtomicBool::new(true),
            Arc::new(gate.admit(Role::ReadDelivery).unwrap()),
        ),
        Err(Stopped::Cancelled)
    );
    assert_eq!(gate.snapshot().unwrap().admitted, 0);
    assert!(matches!(
        run_accounted(
            Path::new("unavailable-accounted-helper"),
            &inv,
            &[],
            1024,
            &AtomicBool::new(false),
            Arc::new(gate.admit(Role::ReadDelivery).unwrap()),
        ),
        Ok(Exit::StartFailed(_))
    ));
    assert_eq!(gate.close().wait_until(Instant::now()), Observation::Idle);
}

#[test]
fn explicit_cancel_waits_for_only_the_owned_child_and_releases_its_resources() {
    let root = std::env::temp_dir().join(format!("hgs-accounted-cancel-{}", std::process::id()));
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let release = Release(root);
    let ready = release.0.join("ready");
    let gate = Gate::new(1).unwrap();
    let charge = Arc::new(gate.admit(Role::ReadDelivery).unwrap());
    let cancel = Arc::new(AtomicBool::new(false));
    let stopped = cancel.clone();
    let witness = ready.clone();
    let job = std::thread::spawn(move || {
        run_accounted(
            Path::new("python3"),
            &invocation("import os,time;open(os.environ['ACCOUNTED_READY'],'w').write('ready');time.sleep(10)", None),
            &[("ACCOUNTED_READY", witness.to_str().unwrap())],
            1024,
            &stopped,
            charge,
        )
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    cancel.store(true, std::sync::atomic::Ordering::Release);
    let closed = gate.close();
    assert!(ready.exists(), "owned child did not reach its ready marker");
    assert_eq!(job.join().unwrap(), Err(Stopped::Cancelled));
    assert_eq!(
        closed.wait_until(Instant::now() + Duration::from_secs(2)),
        Observation::Idle
    );
}

struct Release(PathBuf);
impl Drop for Release {
    fn drop(&mut self) {
        // A failed assertion still lets this test's child exit itself. Keep
        // the private folder on failure rather than race its release read.
        let _ = std::fs::write(self.0.join("release"), []);
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn each_retained_pipe_stays_charged_after_the_legacy_exit_is_returned() {
    for (index, pipe) in ["stdin", "stdout", "stderr"].into_iter().enumerate() {
        let root =
            std::env::temp_dir().join(format!("hgs-accounted-{}-{index}", std::process::id()));
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let release = Release(root);
        let release_path = release.0.join("release");
        let helper = "import os,time\ndeadline=time.monotonic()+10\nwhile not os.path.exists(os.environ['ACCOUNTED_RELEASE']) and time.monotonic()<deadline:\n time.sleep(.01)\n";
        let code = format!(
            "import subprocess,sys\nsubprocess.Popen([sys.executable,'-c',{}],\
            stdin={},stdout={},stderr={})\nprint('{{}}',flush=True)",
            serde_json::to_string(helper).unwrap(),
            if pipe == "stdin" {
                "sys.stdin"
            } else {
                "subprocess.DEVNULL"
            },
            if pipe == "stdout" {
                "sys.stdout"
            } else {
                "subprocess.DEVNULL"
            },
            if pipe == "stderr" {
                "sys.stderr"
            } else {
                "subprocess.DEVNULL"
            },
        );
        let gate = Gate::new(1).unwrap();
        let input = (pipe == "stdin").then(|| vec![b'x'; 1 << 20]);
        let result = run_accounted(
            Path::new("python3"),
            &invocation(&code, input),
            &[("ACCOUNTED_RELEASE", release_path.to_str().unwrap())],
            1024,
            &AtomicBool::new(false),
            Arc::new(gate.admit(Role::ReadDelivery).unwrap()),
        );
        assert!(
            matches!(result, Ok(Exit::Code { code: 0, .. })),
            "{result:?}"
        );
        let closed = gate.close();
        assert_eq!(
            closed.wait_until(Instant::now()),
            Observation::Pending,
            "{pipe}"
        );
        std::fs::write(&release_path, []).unwrap();
        assert_eq!(
            closed.wait_until(Instant::now() + Duration::from_secs(5)),
            Observation::Idle,
            "{pipe}"
        );
    }
}
