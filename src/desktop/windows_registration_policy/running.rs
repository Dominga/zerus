//! A bounded read-only candidate selection, never task or process authority.
use anyhow::{ensure, Result};
use std::time::Instant;

pub const LIMIT: i32 = 256;

/// Native activity classification, never sufficient for exit/drain by itself.
pub fn idle(state: i32) -> Result<bool> {
    // TASK_STATE_READY / RUNNING / QUEUED. Unknown and disabled refuse.
    ensure!(
        matches!(state, 2..=4),
        "Owned task activity is unavailable."
    );
    Ok(state == 3)
}

pub fn select<T>(
    count: i32,
    owned_path: &str,
    until: Instant,
    mut item: impl FnMut(i32) -> Result<(String, T)>,
) -> Result<Vec<T>> {
    ensure!(
        owned_path.starts_with('\\')
            && owned_path.len() > 1
            && !owned_path[1..].contains(['\\', '\0']),
        "Owned root task path is unavailable."
    );
    ensure!(
        (0..=LIMIT).contains(&count),
        "Running-task inventory exceeds its verification bound."
    );
    ensure!(
        Instant::now() < until,
        "Running-task verification deadline expired."
    );
    let mut selected = Vec::new();
    for index in 1..=count {
        ensure!(
            Instant::now() < until,
            "Running-task verification deadline expired."
        );
        let (path, candidate) = item(index)?;
        ensure!(
            path.starts_with('\\') && (2..=32768).contains(&path.len()) && !path.contains('\0'),
            "Running-task path is unavailable."
        );
        if path == owned_path {
            ensure!(
                selected.is_empty(),
                "Multiple owned task instances require inspection."
            );
            selected.push(candidate);
        }
    }
    ensure!(
        Instant::now() < until,
        "Running-task verification deadline expired."
    );
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    const OWNED: &str = "\\hgs-desktop-fixture";
    fn until() -> Instant {
        // Structural tests never wait on this deadline. Expiry is tested with
        // an already expired original value, without scheduler timing races.
        Instant::now() + Duration::from_secs(60)
    }

    #[test]
    fn only_the_exact_root_path_can_produce_a_candidate() {
        let paths = [
            "\\other",
            "\\folder\\hgs-desktop-fixture",
            OWNED,
            "\\HGS-desktop-fixture",
        ];
        let mut calls = Vec::new();
        let selected = select(4, OWNED, until(), |i| {
            calls.push(i);
            Ok((paths[i as usize - 1].into(), i))
        })
        .unwrap();
        assert_eq!(selected, [3]);
        assert_eq!(calls, [1, 2, 3, 4]);
    }

    #[test]
    fn empty_and_other_paths_supply_no_instance_or_drain_proof() {
        assert!(
            select::<()>(0, OWNED, until(), |_| panic!("empty inventory"))
                .unwrap()
                .is_empty()
        );
        assert!(select(1, OWNED, until(), |_| Ok((
            "\\folder\\hgs-desktop-fixture".into(),
            ()
        )))
        .unwrap()
        .is_empty());
        assert!(!idle(2).unwrap());
        assert!(!idle(4).unwrap());
        assert!(idle(3).unwrap());
        for unknown in [-1, 0, 1, 5] {
            assert!(idle(unknown).is_err());
        }
    }

    #[test]
    fn duplicates_and_unreadable_paths_refuse_without_selecting_another_task() {
        assert!(select(2, OWNED, until(), |i| Ok((OWNED.into(), i))).is_err());
        assert!(select::<()>(1, OWNED, until(), |_| anyhow::bail!("unknown native path")).is_err());
        for path in ["", "relative", "\\", "\\unknown\0path"] {
            assert!(select(1, OWNED, until(), |_| Ok((path.into(), ()))).is_err());
        }
        for path in ["", "relative", "\\folder\\hgs-desktop-fixture"] {
            assert!(select::<()>(1, path, until(), |_| panic!("invalid owned root")).is_err());
        }
    }

    #[test]
    fn unknown_or_oversized_counts_refuse_before_any_item_is_read() {
        for count in [i32::MIN, -1, LIMIT + 1, i32::MAX] {
            assert!(select::<()>(count, OWNED, until(), |_| panic!("unbounded read")).is_err());
        }
        let mut read = 0;
        assert!(select(LIMIT, OWNED, until(), |i| {
            read += 1;
            Ok(("\\other".into(), i))
        })
        .unwrap()
        .is_empty());
        assert_eq!(read, LIMIT);
    }

    #[test]
    fn an_expired_original_deadline_refuses_even_an_empty_snapshot() {
        let expired = Instant::now() - Duration::from_secs(1);
        for count in [0, 1] {
            assert!(select::<()>(count, OWNED, expired, |_| panic!("expired read")).is_err());
        }
    }
}
