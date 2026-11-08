//! Preallocated identity of one new terminal launch. This is data, never
//! native admission, consent or permission to adopt an existing session.
use super::*;

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

#[cfg(test)]
mod tests {
    use super::*;

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
