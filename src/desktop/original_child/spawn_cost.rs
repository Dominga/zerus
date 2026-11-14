//! Opt-in synthetic spawn timings. No agent, session or graphical terminal runs.
use super::*;
use zerus_client_core::terminal::{Attach, AttachEvent, Target};

fn measure(label: &str, mut sample: impl FnMut() -> Duration) {
    for _ in 0..8 {
        sample();
    }
    let mut micros: Vec<u128> = (0..64).map(|_| sample().as_micros()).collect();
    micros.sort_unstable();
    println!(
        "SPAWN_COST {}",
        serde_json::json!({
            "family": label, "samples": micros.len(), "median_us": micros[micros.len()/2],
            "p95_us": micros[micros.len()*95/100], "min_us": micros[0],
            "phase": std::env::var("ZERUS_SPAWN_COST_PHASE").unwrap_or_default(),
        })
    );
}
fn command() -> Command {
    let mut command = Command::new("/usr/bin/true");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
fn spawned(start: impl FnOnce() -> std::io::Result<std::process::Child>) -> Duration {
    let before = Instant::now();
    let mut child = start().unwrap();
    let elapsed = before.elapsed();
    assert!(child.wait().unwrap().success());
    elapsed
}
#[test]
#[ignore = "opt-in before/after synthetic spawn cost measurement"]
fn unix_spawn_cost() {
    measure("std-command-control", || spawned(|| command().spawn()));
    measure("root-spawn-turn", || {
        spawned(|| crate::platform::spawn_in_turn(&mut command()))
    });
    measure("root-plain-spawn", || {
        spawned(|| crate::platform::spawn(&mut command()))
    });
    measure("watch-source-pipes", || {
        spawned(|| {
            let mut command = command();
            command.stdout(Stdio::piped());
            crate::platform::spawn_service_child(&mut command)
        })
    });
    measure("ssh-pipe-spawn", || {
        let before = Instant::now();
        let mut child = crate::platform::spawn_ssh(&mut command(), true, true, true).unwrap();
        let elapsed = before.elapsed();
        drop(child.stdin.take());
        assert!(child.child.wait().unwrap().success());
        elapsed
    });
    measure("detached-setsid", || {
        spawned(|| crate::platform::spawn_detached(&mut command()))
    });
    measure("core-detached-system", || {
        use zerus_client_core::terminal_launch::{Desktop, Start, System, Window};
        let before = Instant::now();
        System
            .start(&Start {
                program: "/usr/bin/true".into(),
                arguments: Vec::new(),
                directory: "/".into(),
                window: Window::Plain,
            })
            .unwrap();
        before.elapsed()
    });
    let custody = tests::custody();
    measure("custody-pre-exec", || {
        spawned(|| {
            let _turn = crate::platform::spawn_turn();
            let mut command = command();
            custody.inherit(&mut command)?;
            command.spawn()
        })
    });
    measure("core-input-adapter", || {
        let before = Instant::now();
        let input = original_input::prepare(true).unwrap();
        let mut command = Command::new("/bin/cat");
        command.stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = input.spawn(&mut command).unwrap();
        let elapsed = before.elapsed();
        let mut writer = child.stdin.take().unwrap();
        let owner = InputOwner::acquire(&writer, true).unwrap();
        writer.write_all(b"synthetic input").unwrap();
        owner.finish().unwrap();
        drop(writer);
        assert!(child.wait().unwrap().success());
        elapsed
    });
    measure("root-bounded-roundtrip", || {
        let before = Instant::now();
        let result = crate::platform::run_bounded(
            &mut command(),
            [16; 2],
            Instant::now() + Duration::from_secs(5),
            &|| false,
        )
        .unwrap();
        assert!(result.0.success());
        before.elapsed()
    });
    measure("original-frozen-roundtrip", || {
        let before = Instant::now();
        let result = run(
            Path::new("/bin/cat"),
            &[],
            tests::spec(b"synthetic input"),
            &custody,
            Instant::now() + Duration::from_secs(5),
            |_| Ok(()),
        );
        assert_eq!(
            result,
            Ended::Exited {
                code: 0,
                output: b"synthetic input".to_vec()
            }
        );
        before.elapsed()
    });
    measure("forkpty-start", || {
        let target = Target {
            hgs: "/usr/bin/true".into(),
            host: String::new(),
            name: "spawn-cost".into(),
            run_id: String::new(),
        };
        let before = Instant::now();
        let (attach, events) = Attach::start(&target, 80, 24).unwrap();
        let elapsed = before.elapsed();
        loop {
            if let AttachEvent::Finished { code } =
                events.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert_eq!(code, 0);
                break;
            }
        }
        drop(attach);
        elapsed
    });
}
