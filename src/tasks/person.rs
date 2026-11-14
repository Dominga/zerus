//! Refusal-only interim check for ordinary task commands. It discovers no
//! session identity, scans no records and grants no verified Person authority.
use super::{Result, fail};
use crate::state::dsh::ANCESTRY_STEPS;
use std::{collections::BTreeSet, io};

pub(super) const REFUSED: &str = "an agent session runs this command; run person actions from a terminal outside the agent's process tree";

pub(crate) fn check() -> Result<()> {
    if marked_ancestor(
        std::process::id(),
        |pid| crate::platform::process_identity(pid).map(|identity| identity.parent),
        crate::teams::caller::environment,
    ) {
        return Err(fail(REFUSED));
    }
    Ok(())
}

fn marked(bytes: &[u8]) -> bool {
    bytes
        .split(|byte| *byte == 0)
        .any(|entry| entry.starts_with(b"HGS_SESSION=") || entry.starts_with(b"HGS_RUN_ID="))
}

fn marked_ancestor(
    mut pid: u32,
    mut parent: impl FnMut(u32) -> Option<u32>,
    mut environment: impl FnMut(u32) -> io::Result<Vec<u8>>,
) -> bool {
    let mut seen = BTreeSet::new();
    for _ in 0..ANCESTRY_STEPS {
        if pid == 0 || !seen.insert(pid) {
            break;
        }
        let Some(next) = parent(pid) else { break };
        // A withheld Mac shell environment is not evidence of an agent.
        // Continue above it: a readable agent ancestor may still carry markers.
        if environment(pid).is_ok_and(|bytes| marked(&bytes)) {
            return true;
        }
        pid = next;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn withheld_person_environment_is_not_evidence_of_an_agent() {
        assert!(!marked_ancestor(
            3,
            |pid| Some(pid - 1),
            |pid| {
                if pid == 2 {
                    Err(io::ErrorKind::PermissionDenied.into())
                } else {
                    Ok(b"PATH=/bin\0".to_vec())
                }
            }
        ));
    }

    #[test]
    fn a_readable_agent_above_a_withheld_shell_still_refuses() {
        assert!(marked_ancestor(
            3,
            |pid| Some(pid - 1),
            |pid| {
                match pid {
                    2 => Err(io::ErrorKind::PermissionDenied.into()),
                    1 => Ok(b"HGS_RUN_ID=run-example\0".to_vec()),
                    _ => Ok(Vec::new()),
                }
            }
        ));
    }

    #[test]
    fn marker_names_are_exact_and_even_empty_values_are_evidence() {
        for bytes in [b"HGS_SESSION=\0".as_slice(), b"HGS_RUN_ID=run\0"] {
            assert!(marked(bytes));
        }
        assert!(!marked(
            b"X_HGS_SESSION=x\0HGS_SESSION_EXTRA=x\0HGS_SESSION_ID=x\0HGS_AGENT=claude\0"
        ));
    }

    #[test]
    fn ancestry_is_bounded_and_repeated_processes_end_the_walk() {
        let mut reads = 0;
        assert!(!marked_ancestor(
            ANCESTRY_STEPS as u32 + 2,
            |pid| Some(pid - 1),
            |_| {
                reads += 1;
                Ok(Vec::new())
            }
        ));
        assert_eq!(reads, ANCESTRY_STEPS);
        reads = 0;
        assert!(!marked_ancestor(
            2,
            |_| Some(2),
            |_| {
                reads += 1;
                Ok(Vec::new())
            }
        ));
        assert_eq!(reads, 1);
    }
}
