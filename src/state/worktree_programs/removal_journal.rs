//! Distinct durable final-removal correlation. Reserving a UUID or reading a
//! Complete stop never constructs Remove consent or an executing continuation.
use super::journal::{exists, read, write};
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use zerus_client_core::{
    desktop::termination_outcome::hash,
    worktree_programs::{
        cleanup::{Input, Reason, Report, State},
        stop,
    },
};

const LIMIT: usize = zerus_client_core::worktree_programs::custody::FRAME_BYTES;
const INVALID: &str =
    "The original removal is unavailable or unconfirmed. Review its original result.";
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Original {
    version: u32,
    input: Input,
    binding: String,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Link {
    version: u32,
    request_id: String,
    binding: String,
}

pub(super) struct Store(journal::Store);
pub(super) enum Reservation {
    Fresh(Box<Claim>),
    Existing(Report),
}
pub(crate) struct Claim {
    store: Store,
    input: Input,
    _group: crate::platform::FileLock,
    consumed: AtomicBool,
}
fn leaf(id: &str, part: &str) -> Result<String> {
    if !zerus_client_core::creation::canonical_uuid(id) {
        return Err(INVALID.into());
    }
    Ok(format!("cleanup-{id}.{part}.json"))
}
impl Store {
    /// Read-only and indexed by the exact original ID. No missing directory,
    /// terminal result or historical Complete stop is ever resumed here.
    pub(super) fn open(state: &Path) -> Result<Option<Self>> {
        journal::Store::open(state, false).map(|store| store.map(Self))
    }
    pub(super) fn lookup(&self, id: &str) -> Result<Option<(Input, Report)>> {
        self.0.current()?;
        let original = read(&self.0.directory, &leaf(id, "original")?, LIMIT)?;
        let result = read(&self.0.directory, &leaf(id, "result")?, wire::MAX_BYTES)?;
        let Some(bytes) = original else {
            if result.is_some() {
                return Err(INVALID.into());
            }
            return Ok(None);
        };
        let original: Original = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
        if original.version != 1
            || !original.input.valid()
            || original.input.request_id != id
            || original.binding != original.input.binding()
        {
            return Err(INVALID.into());
        }
        let report = match result {
            Some(bytes) => Report::decode(&bytes, &original.input).map_err(|_| INVALID)?,
            None => Report {
                request_id: id.into(),
                binding: original.binding,
                state: State::Unknown,
                reason: Some(Reason::Unconfirmed),
            },
        };
        self.0.current()?;
        Ok(Some((original.input, report)))
    }
    pub(super) fn reserve(self, input: Input, now: u64) -> Result<Reservation> {
        if !input.valid() {
            return Err(INVALID.into());
        }
        // The original stop helper owns the SAME group file until it actually
        // releases it. Holder/pipe/loan retirement is checked independently.
        let group = self.0.group(&input.stop)?;
        if let Some((old, report)) = self.lookup(&input.request_id)? {
            if old != input {
                return Err(INVALID.into());
            }
            return Ok(Reservation::Existing(report));
        }
        if !input.stop.manifest.valid(now) {
            return Err(INVALID.into());
        }
        let (old, report) = self.0.lookup(&input.stop.request_id)?.ok_or(INVALID)?;
        if old != *input.stop || report.state != stop::State::Complete {
            return Err(INVALID.into());
        }
        let key = hash(
            &serde_json::to_vec(&(
                &old.manifest.configuration,
                &old.manifest.capture.machine,
                old.termination_id()?,
            ))
            .map_err(|_| INVALID)?,
        );
        let fence = format!("cleanup-origin-{key}.json");
        if read(&self.0.directory, &fence, 1024)?.is_some() {
            return Err(INVALID.into());
        }
        let binding = input.binding();
        // Fence first: even a failed original/receipt write must prevent a new
        // UUID from replaying this live Remove continuation after uncertainty.
        write(
            &self.0.directory,
            &fence,
            &Link {
                version: 1,
                request_id: input.request_id.clone(),
                binding: binding.clone(),
            },
            1024,
        )?;
        write(
            &self.0.directory,
            &leaf(&input.request_id, "original")?,
            &Original {
                version: 1,
                input: input.clone(),
                binding,
            },
            LIMIT,
        )?;
        self.0.current()?;
        Ok(Reservation::Fresh(Box::new(Claim {
            store: self,
            input,
            _group: group,
            consumed: AtomicBool::new(false),
        })))
    }
}
impl Claim {
    #[cfg(test)]
    pub(crate) fn fixture(input: Input) -> (tempfile::TempDir, Box<Self>) {
        use std::os::unix::fs::PermissionsExt;
        // The no-follow store requires canonical ancestors; Darwin's TMPDIR
        // commonly starts at the /var symlink. Create fixture state below the
        // resolved parent, preserving the production pinning refusal.
        let state = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let stop = *input.stop.clone();
        let journal::Reservation::Fresh(claim) = journal::Store::open(state.path(), true)
            .unwrap()
            .unwrap()
            .reserve(stop.clone(), stop.manifest.issued)
            .unwrap()
        else {
            panic!("fresh synthetic stop")
        };
        claim
            .finish(stop::Report {
                request_id: stop.request_id.clone(),
                binding: stop.binding(),
                state: stop::State::Complete,
                members: stop
                    .manifest
                    .programs
                    .iter()
                    .map(|program| stop::Member {
                        program: program.clone(),
                        observation: stop::Observation::Exited,
                    })
                    .collect(),
            })
            .unwrap();
        let Reservation::Fresh(claim) = Store::open(state.path())
            .unwrap()
            .unwrap()
            .reserve(input, stop.manifest.issued)
            .unwrap()
        else {
            panic!("fresh removal data")
        };
        (state, claim)
    }
    pub(crate) fn input(&self) -> &Input {
        &self.input
    }
    pub(crate) fn current(&self) -> Result<()> {
        let (input, report) = self.store.lookup(&self.input.request_id)?.ok_or(INVALID)?;
        if input != self.input
            || report.state != State::Unknown
            || exists(&self.store.0.directory, &leaf(&input.request_id, "result")?)?
        {
            return Err(INVALID.into());
        }
        let (stop, result) = self
            .store
            .0
            .lookup(&self.input.stop.request_id)?
            .ok_or(INVALID)?;
        if stop != *self.input.stop || result.state != stop::State::Complete {
            return Err(INVALID.into());
        }
        Ok(())
    }
    /// One-way local bookkeeping, NOT deletion permission. Only the later
    /// private typed native admission may consume it, under its original TTL.
    pub(crate) fn spend(&self) -> Result<()> {
        self.current()?;
        self.consumed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| INVALID.into())
    }
    pub(super) fn finish(self, report: Report) -> Result<Report> {
        self.current()?;
        if !report.valid(&self.input) {
            return Err(INVALID.into());
        }
        write(
            &self.store.0.directory,
            &leaf(&self.input.request_id, "result")?,
            &report,
            wire::MAX_BYTES,
        )?;
        let (original, saved) = self.store.lookup(&self.input.request_id)?.ok_or(INVALID)?;
        if original != self.input || saved != report {
            return Err(INVALID.into());
        }
        Ok(saved)
    }
}

#[cfg(test)]
mod tests {
    // Real private filesystem journals; original stop results are synthetic
    // DATA. These tests create neither native permission nor deletion effects.
    use super::*;
    use std::fs;
    fn private() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        directory
    }
    fn input() -> Input {
        Input {
            request_id: uuid::Uuid::new_v4().to_string(),
            stop: Box::new(journal::tests::input()),
        }
    }
    fn save_stop(state: &Path, input: &Input, result: Option<stop::State>) {
        let store = journal::Store::open(state, true).unwrap().unwrap();
        let journal::Reservation::Fresh(claim) = store.reserve(*input.stop.clone(), 1000).unwrap()
        else {
            panic!("fresh stop data")
        };
        if let Some(state) = result {
            let report = stop::Report {
                request_id: input.stop.request_id.clone(),
                binding: input.stop.binding(),
                state,
                members: if matches!(state, stop::State::Complete | stop::State::Incomplete) {
                    input
                        .stop
                        .manifest
                        .programs
                        .iter()
                        .map(|program| stop::Member {
                            program: program.clone(),
                            observation: if state == stop::State::Complete {
                                stop::Observation::Exited
                            } else {
                                stop::Observation::Surviving
                            },
                        })
                        .collect()
                } else {
                    vec![]
                },
            };
            claim.finish(report).unwrap();
        }
    }
    fn store(state: &Path) -> Store {
        Store::open(state).unwrap().unwrap()
    }
    #[test]
    fn missing_unknown_incomplete_refused_or_wrong_stop_never_reserves_removal() {
        let state = private();
        assert!(Store::open(state.path()).unwrap().is_none());
        assert!(!state.path().join("worktree-program-stops").exists());
        for kind in [
            None,
            Some(stop::State::Unknown),
            Some(stop::State::Incomplete),
            Some(stop::State::Refused),
        ] {
            let state = private();
            let input = input();
            save_stop(state.path(), &input, kind);
            assert!(
                store(state.path()).reserve(input, 1001).is_err(),
                "{kind:?}"
            );
        }
        let state = private();
        let mut input = input();
        save_stop(state.path(), &input, Some(stop::State::Complete));
        input.stop.source = stop::Source::Service {
            epoch: uuid::Uuid::new_v4().to_string(),
        };
        assert!(
            store(state.path()).reserve(input, 1001).is_err(),
            "complete evidence for another original is not sufficient"
        );
    }
    #[test]
    fn a_lost_or_partial_removal_is_original_id_only_and_never_mints_a_new_claim() {
        for result in [None, Some(State::Unknown), Some(State::Removed)] {
            let state = private();
            let input = input();
            save_stop(state.path(), &input, Some(stop::State::Complete));
            let Reservation::Fresh(claim) =
                store(state.path()).reserve(input.clone(), 1001).unwrap()
            else {
                panic!("fresh removal data")
            };
            claim.current().unwrap();
            claim.spend().unwrap();
            assert!(claim.spend().is_err());
            let mut inherited = journal::tests::ForkedDescriptors::stopped();
            let mut different = input.clone();
            different.request_id = uuid::Uuid::new_v4().to_string();
            assert!(
                store(state.path())
                    .reserve(different.clone(), 1001)
                    .is_err(),
                "the original helper still owns its group"
            );
            if let Some(state) = result {
                claim
                    .finish(Report {
                        request_id: input.request_id.clone(),
                        binding: input.binding(),
                        state,
                        reason: (state == State::Unknown).then_some(Reason::Unconfirmed),
                    })
                    .unwrap();
            } else {
                drop(claim);
            }
            inherited.still_stopped();
            // Drop above/at end of the branch is actual journal ownership,
            // not a claim that a helper/pipe or deletion has physically ended.
            let Reservation::Existing(saved) =
                store(state.path()).reserve(input.clone(), 1001).unwrap()
            else {
                panic!("a stored UUID can never reconstruct Claim")
            };
            assert_eq!(saved.state, result.unwrap_or(State::Unknown));
            assert!(
                store(state.path()).reserve(different, 1001).is_err(),
                "even a NEW UUID cannot replay the original removal"
            );
        }
    }
    #[test]
    fn a_finished_stop_releases_its_group_before_cleanup_despite_a_forked_copy() {
        let state = private();
        let input = input();
        let stop = *input.stop.clone();
        let journal::Reservation::Fresh(claim) = journal::Store::open(state.path(), true)
            .unwrap()
            .unwrap()
            .reserve(stop.clone(), 1000)
            .unwrap()
        else {
            panic!("fresh stop data");
        };
        let mut child = journal::tests::ForkedDescriptors::stopped();
        assert!(store(state.path()).reserve(input.clone(), 1001).is_err());
        claim
            .finish(stop::Report {
                request_id: stop.request_id.clone(),
                binding: stop.binding(),
                state: stop::State::Complete,
                members: stop
                    .manifest
                    .programs
                    .iter()
                    .map(|program| stop::Member {
                        program: program.clone(),
                        observation: stop::Observation::Exited,
                    })
                    .collect(),
            })
            .unwrap();
        let Reservation::Fresh(claim) = store(state.path()).reserve(input, 1001).unwrap() else {
            panic!("complete stop data permits the separate fresh cleanup reservation");
        };
        child.still_stopped();
        drop(claim);
    }
    #[test]
    fn a_changed_stop_result_after_reservation_cannot_admit_the_first_delete() {
        let state = private();
        let input = input();
        save_stop(state.path(), &input, Some(stop::State::Complete));
        let Reservation::Fresh(claim) = store(state.path()).reserve(input.clone(), 1001).unwrap()
        else {
            panic!("fresh");
        };
        let receipt = state
            .path()
            .join("worktree-program-stops")
            .join(format!("{}.result.json", input.stop.request_id));
        let original = fs::read(&receipt).unwrap();
        let unknown = stop::Report {
            request_id: input.stop.request_id.clone(),
            binding: input.stop.binding(),
            state: stop::State::Unknown,
            members: vec![],
        };
        fs::write(&receipt, serde_json::to_vec(&unknown).unwrap()).unwrap();
        assert!(claim.current().is_err());
        assert!(claim.spend().is_err());
        fs::write(&receipt, original).unwrap();
        claim.current().unwrap();
        claim.spend().unwrap();
    }
    #[test]
    fn the_complete_original_including_first_epoch_is_immutable_and_bound() {
        let state = private();
        let input = input();
        save_stop(state.path(), &input, Some(stop::State::Complete));
        let Reservation::Fresh(claim) = store(state.path()).reserve(input.clone(), 1001).unwrap()
        else {
            panic!("fresh")
        };
        claim
            .finish(Report {
                request_id: input.request_id.clone(),
                binding: input.binding(),
                state: State::Kept,
                reason: Some(Reason::Changed),
            })
            .unwrap();
        let path = state
            .path()
            .join("worktree-program-stops")
            .join(leaf(&input.request_id, "original").unwrap());
        let bytes = fs::read(&path).unwrap();
        let mut changed: Value = serde_json::from_slice(&bytes).unwrap();
        changed["input"]["stop"]["source"]["epoch"] = uuid::Uuid::new_v4().to_string().into();
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(store(state.path()).lookup(&input.request_id).is_err());
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            store(state.path())
                .lookup(&input.request_id)
                .unwrap()
                .unwrap()
                .0,
            input
        );
    }
}
