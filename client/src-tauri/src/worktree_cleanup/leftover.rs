//! A computer's partial-removal leftovers through its hgs: the bounded
//! vanished-repository listing and the two explicit actions. Each runs only
//! after that hgs verifies the computer and shows the leftover marker; an
//! older CLI lists nothing and acts on nothing. An unknown outcome is never
//! retried.
use serde_json::Value;
use zerus_client_core::{
    default_hgs, run_limited, worktree_leftover as leftover, worktree_review as review, worktrees,
    ComputerId, Exit, Invocation,
};

/// Runs one call with its reply bound; `None` when the reply was too large.
pub(crate) type Runner = dyn Fn(&Invocation, usize) -> Option<Exit> + Send + Sync;

pub(crate) fn system(inv: &Invocation, limit: usize) -> Option<Exit> {
    run_limited(&default_hgs(), inv, &[], limit).ok()
}

/// The explicit action the owner confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Keep,
    /// `consent` is the owner's acceptance of programs whose folders cannot
    /// be read; it is sent only when the row offered it.
    Delete {
        consent: bool,
    },
}

/// The computer's help, once its hgs verifies `--expect-machine`.
fn help(run: &Runner, inv: &Invocation) -> Result<String, String> {
    let exit = run(&worktrees::guard_probe(inv), 64 * 1024)
        .ok_or("This computer's hgs could not be checked.")?;
    let help = match &exit {
        Exit::Code {
            code: 0, stdout, ..
        } => String::from_utf8_lossy(stdout).into_owned(),
        _ => String::new(),
    };
    worktrees::guard_supported(exit)?;
    Ok(help)
}

/// Leftovers whose repository is gone, at most [`leftover::ROWS`], and
/// whether more remain. An hgs without leftovers has none to list.
pub(crate) fn listing(
    run: &Runner,
    computer: &str,
    machine: &str,
) -> Result<(Vec<Value>, bool), String> {
    let mut inv = leftover::listing_invocation(&ComputerId(computer.into()));
    worktrees::guard(&mut inv.args, machine);
    if !leftover::has_marker(&help(run, &inv)?, leftover::LEFTOVERS_MARKER) {
        return Ok((Vec::new(), false));
    }
    let exit = run(&inv, review::REVIEW_LIMIT).ok_or("The leftover listing is too large.")?;
    leftover::listing(exit, computer)
}

fn update(computer: &str) -> String {
    if computer.is_empty() {
        "Update hgs on this computer to delete leftover files.".into()
    } else {
        format!("Update hgs on {computer} to delete leftover files.")
    }
}

/// Runs one confirmed action exactly once. A refusal of the probe means
/// nothing ran; anything after the call started is its own verified outcome
/// or unknown.
pub(crate) fn act(
    run: &Runner,
    computer: &str,
    machine: &str,
    item: &leftover::Leftover,
    action: Action,
) -> leftover::Settled {
    let request = uuid::Uuid::new_v4().to_string();
    let host = ComputerId(computer.into());
    let (mut inv, marker) = match action {
        Action::Keep => (
            leftover::keep_invocation(&host, item, &request),
            leftover::LEFTOVERS_MARKER,
        ),
        Action::Delete { consent } => (
            leftover::delete_invocation(&host, item, &request, consent),
            leftover::DELETE_MARKER,
        ),
    };
    worktrees::guard(&mut inv.args, machine);
    match help(run, &inv) {
        Err(detail) => return leftover::Settled::Refused { detail },
        Ok(help) if !leftover::has_marker(&help, marker) => {
            return leftover::Settled::Refused {
                detail: update(computer),
            }
        }
        Ok(_) => {}
    }
    let Some(exit) = run(&inv, review::REMOVE_LIMIT) else {
        return leftover::Settled::Unknown {
            detail: "The reply was too large to verify.".into(),
        };
    };
    match action {
        Action::Keep => leftover::kept(exit, computer, &request, item),
        Action::Delete { .. } => leftover::deleted(exit, computer, &request, item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    const MACHINE: &str = "11111111-1111-4111-8111-111111111111";

    fn item(consent: bool) -> leftover::Leftover {
        let mut row = json!({"path":"/srv/example/w","kind":"leftover","verdict":"partially_removed",
            "common_dir":"/srv/example/.git","branch":"feature/w","size":10,"size_complete":true,
            "reasons":[],"deletable":true,"fingerprint":"cd".repeat(32),
            "removal":{"path":"/srv/example/w","status":"partial"}});
        if consent {
            row["deletable"] = json!(false);
            row["process_fingerprint"] = json!("v2:1:proof");
        }
        leftover::leftover(&row, true).unwrap()
    }
    type Log = Arc<Mutex<Vec<Vec<String>>>>;
    /// A fake hgs: its help, then one reply per call, logging every call.
    fn fake(
        help: &'static str,
        reply: impl Fn(&Invocation) -> Exit + Send + Sync + 'static,
    ) -> (Arc<Runner>, Log) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        let run: Arc<Runner> = Arc::new(move |inv: &Invocation, _| {
            seen.lock().unwrap().push(inv.args.clone());
            Some(if inv.args.last().is_some_and(|a| a == "--help") {
                Exit::Code {
                    code: 0,
                    stdout: help.as_bytes().to_vec(),
                    stderr: String::new(),
                }
            } else {
                reply(inv)
            })
        });
        (run, log)
    }
    fn request(inv: &Invocation) -> String {
        let at = inv.args.iter().position(|a| a == "--request-id").unwrap();
        inv.args[at + 1].clone()
    }
    const CURRENT: &str = "guarded-create-v1\nworktree-leftovers-v1\nworktree-leftover-delete-v1\n";

    #[test]
    fn delete_runs_once_on_the_verified_computer_with_consent_only_when_offered() {
        let (run, log) = fake(CURRENT, |inv| Exit::Code {
            code: 0,
            stdout: json!({"status":"deleted","path":"/srv/example/w",
                "common_dir":"/srv/example/.git","request_id":request(inv)})
            .to_string()
            .into_bytes(),
            stderr: String::new(),
        });
        let settled = act(
            &*run,
            "example-peer",
            MACHINE,
            &item(true),
            Action::Delete { consent: true },
        );
        assert_eq!(settled, leftover::Settled::Deleted);
        let calls = log.lock().unwrap().clone();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], ["@example-peer", "--help"]);
        assert_eq!(
            calls[1][..5],
            [
                "@example-peer",
                "--expect-machine",
                MACHINE,
                "worktrees",
                "delete-leftover"
            ]
        );
        assert!(calls[1].contains(&"--confirm-delete".to_owned()));
        assert_eq!(
            calls[1][calls[1].len() - 2..],
            ["--process-fingerprint", "v2:1:proof"]
        );
        // Without consent the fingerprint stays native.
        log.lock().unwrap().clear();
        act(
            &*run,
            "example-peer",
            MACHINE,
            &item(true),
            Action::Delete { consent: false },
        );
        assert!(!log.lock().unwrap()[1].contains(&"--process-fingerprint".to_owned()));
    }

    #[test]
    fn an_older_or_unverified_hgs_runs_nothing() {
        for help in [
            "guarded-create-v1 worktree-leftovers-v1",
            "worktree-leftovers-v1 worktree-leftover-delete-v1",
        ] {
            let (run, log) = fake(help, |_| panic!("must not run"));
            let settled = act(
                &*run,
                "example-peer",
                MACHINE,
                &item(false),
                Action::Delete { consent: false },
            );
            assert!(
                matches!(settled, leftover::Settled::Refused { .. }),
                "{help}"
            );
            assert_eq!(log.lock().unwrap().len(), 1);
        }
        let (run, log) = fake("guarded-create-v1", |_| panic!("must not run"));
        assert_eq!(listing(&*run, "", MACHINE).unwrap(), (Vec::new(), false));
        assert!(matches!(
            act(&*run, "", MACHINE, &item(false), Action::Keep),
            leftover::Settled::Refused { .. }
        ));
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_lost_or_oversized_reply_is_unknown_and_never_retried() {
        let (run, log) = fake(CURRENT, |_| Exit::TimedOut);
        let settled = act(
            &*run,
            "",
            MACHINE,
            &item(false),
            Action::Delete { consent: false },
        );
        assert!(matches!(settled, leftover::Settled::Unknown { .. }));
        assert_eq!(log.lock().unwrap().len(), 2);
        let log = Arc::new(Mutex::new(0));
        let count = log.clone();
        let run: Arc<Runner> = Arc::new(move |inv: &Invocation, _| {
            *count.lock().unwrap() += 1;
            inv.args
                .last()
                .is_some_and(|a| a == "--help")
                .then(|| Exit::Code {
                    code: 0,
                    stdout: CURRENT.as_bytes().to_vec(),
                    stderr: String::new(),
                })
        });
        let settled = act(&*run, "", MACHINE, &item(false), Action::Keep);
        assert!(matches!(settled, leftover::Settled::Unknown { .. }));
        assert_eq!(*log.lock().unwrap(), 2);
    }

    #[test]
    fn the_listing_is_one_bounded_call_per_computer() {
        let (run, log) = fake(CURRENT, |_| Exit::Code {
            code: 0,
            stdout: json!({"schema":1,"leftovers":[{"path":"/srv/gone/w"}],"partial":true})
                .to_string()
                .into_bytes(),
            stderr: String::new(),
        });
        let (rows, partial) = listing(&*run, "example-peer", MACHINE).unwrap();
        assert_eq!((rows.len(), partial), (1, true));
        assert_eq!(
            log.lock().unwrap()[1],
            [
                "@example-peer",
                "--expect-machine",
                MACHINE,
                "worktrees",
                "leftovers",
                "--json"
            ]
        );
    }
}
