//! Preallocated identity of one new terminal launch. This is data, never
//! native admission, consent or permission to adopt an existing session.
use super::*;

pub(crate) mod helper;

/// Distinct from the creation request's HGS_LAUNCH_ID. An old caller may
/// inherit HGS_SESSION_ID, so that variable alone cannot allocate a new row.
pub(crate) const DECLARATION: &str = "HGS_LAUNCH_SESSION_ID";

#[derive(Debug, PartialEq)]
pub(crate) struct Allocation {
    session: String,
    run: String,
}

impl Allocation {
    pub(crate) fn fresh() -> Self {
        Self {
            session: uuid::Uuid::new_v4().to_string(),
            run: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub(crate) fn session(&self) -> &str {
        &self.session
    }
    pub(crate) fn run(&self) -> &str {
        &self.run
    }

    pub(crate) fn environment(&self) -> [(&'static str, String); 3] {
        [
            (DECLARATION, self.session.clone()),
            ("HGS_SESSION_ID", self.session.clone()),
            ("HGS_RUN_ID", self.run.clone()),
        ]
    }

    /// Only the explicit new-launch declaration opts into this handoff.
    /// Legacy callers keep their existing bootstrap; a parent's public SID
    /// never becomes the child's new identity by inheritance.
    fn decode(
        declared: Option<String>,
        session: Option<String>,
        run: Option<String>,
    ) -> Result<Option<Self>> {
        let Some(declared) = declared.filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let valid = |id: &str| {
            session_id::canonical(id) && uuid::Uuid::parse_str(id).is_ok_and(|id| !id.is_nil())
        };
        let run = run
            .filter(|id| valid(id))
            .ok_or("The new launch's run identity is unavailable")?;
        if !valid(&declared) || session.as_deref() != Some(declared.as_str()) {
            return Err("The new launch's session identity changed".into());
        }
        Ok(Some(Self {
            session: declared,
            run,
        }))
    }

    /// Consume the internal declaration before any native help/agent child.
    /// The public identity remains for the supervisor and is checked again
    /// against the final record before it can be saved.
    pub(super) fn take() -> Result<Option<Self>> {
        let declared = nonempty_env(DECLARATION);
        std::env::remove_var(DECLARATION);
        Self::decode(
            declared,
            nonempty_env("HGS_SESSION_ID"),
            nonempty_env("HGS_RUN_ID"),
        )
    }
}

/// A required private argv form, carried in the existing `--run` count
/// slot. Older helpers parse only an integer and refuse BEFORE bootstrap.
/// Unix new/fork constructors emit it only for a positively capable helper.
/// ID-layout publication and duplicate-label writers remain separately gated.
pub(crate) fn required_count(session: &str, run: &str, count: usize) -> Result<String> {
    if !valid_id(session) || !valid_id(run) {
        return Err("The required launch identity is unavailable".into());
    }
    Ok(format!("sid1:{session}:{run}:{count}"))
}

fn valid_id(id: &str) -> bool {
    session_id::canonical(id) && uuid::Uuid::parse_str(id).is_ok_and(|id| !id.is_nil())
}

pub(crate) struct Count {
    pub(crate) value: usize,
    pub(crate) required: bool,
}

/// The legacy integer grammar is unchanged. A versioned form never falls
/// back to it, or to an untracked launch, when any identity is unavailable.
pub(crate) fn resume_count(raw: &str) -> Result<Count> {
    let count = parse_count(raw)?;
    if let Some((session, run)) = count.1 {
        let new = nonempty_env(DECLARATION);
        let saved = nonempty_env("HGS_RESUME_SESSION_ID");
        if nonempty_env("HGS_SESSION_ID").as_deref() != Some(session)
            || nonempty_env("HGS_RUN_ID").as_deref() != Some(run)
            || nonempty_env("HGS_SESSION").is_none()
            || std::env::var("HGS_TRACKING").as_deref() == Ok("0")
            || !matches!((new.as_deref(),saved.as_deref()),
                (Some(id),None) | (None,Some(id)) if id == session)
        {
            return Err("The required launch identity changed; nothing was started".into());
        }
    }
    Ok(Count {
        value: count.0,
        required: count.1.is_some(),
    })
}

fn parse_count(raw: &str) -> Result<(usize, Option<(&str, &str)>)> {
    let number = |value: &str| {
        value
            .parse::<usize>()
            .map_err(|_| "invalid --run resume-argument count".to_owned())
    };
    let Some(bound) = raw.strip_prefix("sid1:") else {
        return Ok((number(raw)?, None));
    };
    if raw.len() > 128 {
        return Err("The required launch count exceeds its bound".into());
    }
    let parts = bound.split(':').collect::<Vec<_>>();
    let [session, run, count] = parts.as_slice() else {
        return Err("The required launch count is malformed".into());
    };
    if !valid_id(session) || !valid_id(run) {
        return Err("The required launch identity is invalid".into());
    }
    Ok((number(count)?, Some((session, run))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_counts_bind_both_ids_and_never_become_legacy_counts() {
        let allocation = Allocation::fresh();
        let raw = required_count(allocation.session(), allocation.run(), 3).unwrap();
        assert_eq!(
            parse_count(&raw).unwrap(),
            (3, Some((allocation.session(), allocation.run())))
        );
        for bad in [
            format!("{raw}:extra"),
            raw.replace("sid1:", "sid2:"),
            format!("sid1:{}:{}:NaN", allocation.session(), allocation.run()),
            format!("sid1:{}:{}:0", uuid::Uuid::nil(), allocation.run()),
            format!(
                "sid1:{}:{}:0",
                allocation.session(),
                "ABCDEF01-2345-4678-9abc-def012345678"
            ),
            "sid1:".to_owned(),
            format!("sid1:{}", "x".repeat(129)),
        ] {
            assert!(parse_count(&bad).is_err(), "{bad}");
        }
        assert_eq!(parse_count("0").unwrap(), (0, None));
        assert_eq!(parse_count("03").unwrap(), (3, None));
        assert!(required_count("", allocation.run(), 0).is_err());
    }

    #[test]
    fn a_new_launch_holds_one_distinct_session_and_run_before_spawn() {
        let one = Allocation::fresh();
        let two = Allocation::fresh();
        assert_ne!(one, two);
        assert_ne!(one.session(), one.run());
        assert_eq!(one.environment(), one.environment());
        assert_eq!(
            Allocation::decode(
                Some(one.session.clone()),
                Some(one.session.clone()),
                Some(one.run.clone())
            ),
            Ok(Some(one))
        );
    }

    #[test]
    fn inherited_legacy_context_never_allocates_and_a_declared_mismatch_refuses() {
        let one = Allocation::fresh();
        let two = Allocation::fresh();
        for marker in [None, Some(String::new())] {
            assert_eq!(
                Allocation::decode(marker, Some(one.session.clone()), Some(one.run.clone())),
                Ok(None)
            );
        }
        for sid in [None, Some(two.session.clone()), Some("invalid".into())] {
            assert!(
                Allocation::decode(Some(one.session.clone()), sid, Some(one.run.clone())).is_err()
            );
        }
        for run in [
            None,
            Some("invalid".into()),
            Some(uuid::Uuid::nil().to_string()),
        ] {
            assert!(
                Allocation::decode(Some(one.session.clone()), Some(one.session.clone()), run)
                    .is_err()
            );
        }
    }
}
