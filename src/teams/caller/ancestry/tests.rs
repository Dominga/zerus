use super::*;

fn own(parent: u32) -> Inspection {
    Inspection::Own(Process {
        parent,
        birth: Some("birth".into()),
    })
}
fn empty(_: u32) -> std::io::Result<Vec<u8>> {
    Ok(Vec::new())
}
fn denied(_: u32) -> std::io::Result<Vec<u8>> {
    Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
}

#[test]
fn unreadable_top_is_allowed_only_with_an_identified_foreign_parent() {
    let table = |pid| {
        Ok(if pid == 2 {
            own(1)
        } else {
            Inspection::Foreign
        })
    };
    let chain = walk(2, table, denied).unwrap();
    assert_eq!(chain.len(), 1);
    assert!(chain[0].top);
    assert!(walk(2, |_| Err(unreadable()), denied).is_err());
}

#[test]
fn readable_top_keeps_its_claim_but_parent_zero_refuses() {
    let chain = walk(
        2,
        |pid| {
            Ok(if pid == 2 {
                own(1)
            } else {
                Inspection::Foreign
            })
        },
        |_| Ok(b"HGS_SESSION=agent\0HGS_RUN_ID=run\0".to_vec()),
    )
    .unwrap();
    assert_eq!(
        chain[0]
            .claim
            .as_ref()
            .map(|c| (c.name.clone(), c.run.clone())),
        Some(("agent".into(), "run".into()))
    );
    assert!(walk(2, |_| Ok(own(0)), empty).is_err());
    assert!(walk(2, |_| Ok(own(0)), denied).is_err());
}

/// A withheld environment is unreadable: tolerated at the verified top, and
/// below it refused with the sentence that tells a person where to run.
#[test]
fn a_withheld_environment_refuses_below_the_top_and_is_tolerated_at_it() {
    let table = |pid| {
        Ok(match pid {
            4 => own(3),
            3 => own(2),
            2 => own(1),
            _ => Inspection::Foreign,
        })
    };
    let withheld_at = |at: u32| {
        move |pid: u32| {
            if pid == at {
                Err(super::super::withheld())
            } else {
                Ok(Vec::new())
            }
        }
    };
    assert_eq!(
        walk(4, table, withheld_at(3)).unwrap_err(),
        super::super::withheld_below_top()
    );
    let chain = walk(4, table, withheld_at(2)).unwrap();
    assert_eq!(chain.len(), 3);
    assert!(chain[2].top && chain[2].claim.is_none());
    assert_eq!(walk(4, table, denied).unwrap_err(), unreadable());
}

/// Native identities and edges, with controlled environment evidence: both
/// branches run even on Macs that expose their system shells' environments.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn native_processes_follow_readable_and_withheld_evidence() {
    let me = std::process::id();
    let chain = walk(me, inspect, empty).expect("the native process-only walk");
    assert!(chain.len() > 1, "the test has a same-user parent");
    let top = chain.last().unwrap();
    assert!(top.top);
    let readable = walk(top.pid, inspect, |_| {
        Ok(b"HGS_SESSION=agent\0HGS_RUN_ID=run\0".to_vec())
    })
    .unwrap();
    assert_eq!(readable.len(), 1);
    let claim = readable[0].claim.as_ref().unwrap();
    assert_eq!((claim.name.as_str(), claim.run.as_str()), ("agent", "run"));
    let hidden = |_| Err(super::super::withheld());
    let withheld_top = walk(top.pid, inspect, hidden).unwrap();
    assert_eq!(withheld_top.len(), 1);
    assert!(withheld_top[0].top && withheld_top[0].claim.is_none());
    assert_eq!(
        walk(me, inspect, hidden).unwrap_err(),
        super::super::withheld_below_top()
    );
}

/// The caller's own markers come from the process itself, never from the
/// system's read of it, which macOS may withhold for a signed release `hgs`.
/// The system's read starts at the parent.
#[test]
fn the_callers_own_markers_come_from_itself_never_from_the_system_read() {
    let table = |pid| {
        Ok(match pid {
            3 => own(2),
            2 => own(1),
            _ => Inspection::Foreign,
        })
    };
    let chain = from_caller(
        3,
        table,
        || b"HGS_SESSION=own\0HGS_RUN_ID=run\0".to_vec(),
        |pid| {
            assert_ne!(pid, 3, "the caller was read through the system");
            Err(super::super::withheld())
        },
    )
    .unwrap();
    assert_eq!(chain.len(), 2);
    let own_claim = chain[0].claim.as_ref().map(|claim| claim.name.as_str());
    assert_eq!(own_claim, Some("own"));
    assert!(chain[1].top && chain[1].claim.is_none());
}

#[test]
fn a_foreign_parent_zero_is_still_refused() {
    // On macOS pid 0 is a foreign (root-owned) process, so the walk must
    // refuse a same-user parent of zero rather than treat it as a top.
    assert!(walk(
        2,
        |pid| Ok(if pid == 2 {
            own(0)
        } else {
            Inspection::Foreign
        }),
        empty
    )
    .is_err());
}

#[test]
fn unreadable_middle_unknown_parent_loop_and_cap_refuse() {
    assert!(walk(
        3,
        |pid| Ok(match pid {
            3 => own(2),
            2 => own(1),
            _ => Inspection::Foreign,
        }),
        denied
    )
    .is_err());
    assert!(walk(
        3,
        |pid| if pid == 3 {
            Ok(own(2))
        } else {
            Err(unreadable())
        },
        empty
    )
    .is_err());
    assert!(walk(3, |_| Ok(own(3)), empty).is_err());
    assert!(walk(3, |pid| Ok(own(pid + 1)), empty).is_err());
}

#[test]
fn exactly_sixty_four_processes_may_reach_the_verified_top() {
    let chain = walk(
        1,
        |pid| {
            Ok(if pid == 65 {
                Inspection::Foreign
            } else {
                own(pid + 1)
            })
        },
        empty,
    )
    .unwrap();
    assert!(walk(1, |pid| Ok(own(if pid == 64 { 0 } else { pid + 1 })), empty).is_err());
    assert_eq!(chain.len(), 64);
}

#[test]
fn a_replaced_process_during_environment_inspection_refuses() {
    let mut own_reads = 0;
    assert!(walk(
        2,
        |pid| {
            if pid == 1 {
                return Ok(Inspection::Foreign);
            }
            own_reads += 1;
            Ok(own(if own_reads == 1 { 1 } else { 3 }))
        },
        empty
    )
    .is_err());
}

#[test]
fn readable_partial_or_invalid_claims_do_not_disappear() {
    for bytes in [
        b"HGS_SESSION=agent\0".as_slice(),
        b"HGS_SESSION=\xff\0HGS_RUN_ID=run\0",
    ] {
        assert!(walk(
            2,
            |pid| Ok(if pid == 2 {
                own(1)
            } else {
                Inspection::Foreign
            }),
            |_| Ok(bytes.to_vec())
        )
        .is_err());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn effective_owner_not_real_owner_decides_foreign() {
    assert!(effective_owner("Uid:\t2000\t1000\t2000\t2000\n", 1000).unwrap());
    assert!(!effective_owner("Uid:\t1000\t2000\t1000\t1000\n", 1000).unwrap());
    assert!(effective_owner("Uid:\t1000\t1000\t1000\n", 1000).is_err());
}

/// On macOS PID 0 is the kernel task, owned by root, and the real inspection
/// reports it as foreign: without the parent-zero rule a same-user process
/// reporting parent 0 would end the walk as a verified top. No real same-user
/// process has parent 0 on macOS, so this is this test's own process, read
/// natively, with only its parent replaced.
#[cfg(target_os = "macos")]
#[test]
fn the_foreign_kernel_task_makes_a_same_user_parent_of_zero_refuse() {
    assert!(matches!(inspect(0), Ok(Inspection::Foreign)));
    let me = std::process::id();
    let Ok(Inspection::Own(real)) = inspect(me) else {
        panic!("this process is readable");
    };
    assert!(real.birth.is_some() && real.parent != 0);
    let orphaned = Process {
        parent: 0,
        birth: real.birth,
    };
    let table = |pid| {
        if pid == me {
            Ok(Inspection::Own(orphaned.clone()))
        } else {
            inspect(pid)
        }
    };
    assert!(walk(me, table, empty).is_err());
}

#[cfg(target_os = "linux")]
fn zombie(pid: u32) -> bool {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat.rsplit_once(')').unwrap().1.split_whitespace().next() == Some("Z")
}

#[cfg(target_os = "macos")]
fn zombie(pid: u32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim_start()
        .starts_with('Z')
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn native_inspection_refuses_a_zombie_and_a_dead_process() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    struct Job(std::process::Child);
    impl Drop for Job {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let code="import os,sys; p=os.fork(); (os._exit(0) if p==0 else None); print(p,flush=True); sys.stdin.read(1); os.waitpid(p,0)";
    let mut job = Job(Command::new("python3")
        .arg("-c")
        .arg(code)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap());
    let mut text = String::new();
    BufReader::new(job.0.stdout.take().unwrap())
        .read_line(&mut text)
        .unwrap();
    let pid = text.trim().parse::<u32>().unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if zombie(pid) {
            break;
        }
        assert!(std::time::Instant::now() < until);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(inspect(pid).is_err());
    job.0.stdin.take().unwrap().write_all(b"g").unwrap();
    assert!(job.0.wait().unwrap().success());
    assert!(inspect(u32::MAX).is_err());
}

#[test]
fn an_unavailable_native_birth_is_only_tolerated_for_an_unreadable_verified_top() {
    let table = |pid| {
        Ok(if pid == 2 {
            Inspection::Own(Process {
                parent: 1,
                birth: None,
            })
        } else {
            Inspection::Foreign
        })
    };
    assert!(walk(2, table, empty).is_err());
    assert!(walk(2, table, denied).is_ok());
}

/// The marked claim's host check walks the process table alone: no row
/// carries a claim, a top without a birth is kept as the person path keeps
/// it, and a birth is still required below the top.
#[test]
fn a_walk_without_an_environment_reader_carries_no_claim_and_keeps_the_births() {
    let birthless = |parent| {
        Inspection::Own(Process {
            parent,
            birth: None,
        })
    };
    let table = |pid| {
        Ok(match pid {
            4 => own(3),
            3 => birthless(2),
            _ => Inspection::Foreign,
        })
    };
    let chain = walk_reading(4, table, None).unwrap();
    assert_eq!(
        chain
            .iter()
            .map(|row| (row.pid, row.top))
            .collect::<Vec<_>>(),
        [(4, false), (3, true)]
    );
    assert!(chain.iter().all(|row| row.claim.is_none()));
    let middle = |pid| {
        Ok(match pid {
            4 => own(3),
            3 => birthless(2),
            2 => own(1),
            _ => Inspection::Foreign,
        })
    };
    assert_eq!(walk_reading(4, middle, None).unwrap_err(), unreadable());
}
