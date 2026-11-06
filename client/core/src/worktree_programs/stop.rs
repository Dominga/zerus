//! Closed original stop/result data. These values cannot reconstruct the live
//! native confirmation, selected route, Actions hold or a stop/Force permit.
use super::{Manifest, Program, MAX_BYTES, MAX_PROGRAMS};
use crate::{
    creation::canonical_uuid,
    desktop::termination_outcome::{hash, INPUT_BYTES},
};
use serde::{Deserialize, Serialize};

pub const INPUT_LIMIT: usize = 128 << 10;
pub const HELP_MARKER: &str = "Worktree program stop ABI: worktree-program-stop-v1";

/// Original routing/ownership DATA. Native caller evidence and current Root
/// ownership are checked independently; serializing a mode grants nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Source {
    Service {
        epoch: String,
    },
    Native {
        request_id: String,
        journal: Option<String>,
        revision: u64,
    },
}
impl Source {
    pub fn mode(&self) -> crate::desktop::ownership::Mode {
        match self {
            Self::Service { .. } => crate::desktop::ownership::Mode::Service,
            Self::Native { .. } => crate::desktop::ownership::Mode::Native,
        }
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::Service { epoch } => canonical_uuid(epoch),
            Self::Native {
                request_id,
                journal,
                revision,
            } => {
                canonical_uuid(request_id)
                    && journal.as_deref().is_none_or(canonical_uuid)
                    && *revision <= crate::desktop::ownership::MAX_REVISION
                    && (journal.is_some() || *revision == 0)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Kind {
    Normal,
    Force {
        previous_request_id: String,
        previous_binding: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub request_id: String,
    pub confirmation_id: String,
    pub source: Source,
    pub manifest: Manifest,
    /// EXACT original scoped Terminate stdin, never a reconstructed request.
    pub original: String,
    pub archive_id: String,
    pub stop: Kind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Original {
    request_id: String,
    action: String,
    expected_run_id: String,
    expected_conversation_id: String,
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Input {
    /// Historical shape/integrity only. Execution additionally requires the
    /// still-live native permit, current manifest and fresh native checks.
    pub fn valid(&self) -> bool {
        let Ok(original) = self.original() else {
            return false;
        };
        let ids = [
            &self.request_id,
            &self.confirmation_id,
            &self.manifest.id,
            &self.archive_id,
            &original.request_id,
        ];
        self.source.valid()
            && self.manifest.valid(self.manifest.issued)
            && ids.iter().all(|id| canonical_uuid(id))
            && ids
                .iter()
                .enumerate()
                .all(|(i, id)| !ids[i + 1..].contains(id))
            && match &self.stop {
                Kind::Normal => true,
                Kind::Force {
                    previous_request_id,
                    previous_binding,
                } => {
                    canonical_uuid(previous_request_id)
                        && digest(previous_binding)
                        && ids.iter().all(|id| *id != previous_request_id)
                }
            }
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= INPUT_LIMIT)
    }
    fn original(&self) -> Result<Original, &'static str> {
        if self.original.len() > INPUT_BYTES {
            return Err("program_stop_original_invalid");
        }
        let original: Original =
            serde_json::from_str(&self.original).map_err(|_| "program_stop_original_invalid")?;
        if !canonical_uuid(&original.request_id)
            || original.action != "terminate"
            || original.expected_run_id != self.manifest.capture.target.run_id
            || original.expected_conversation_id != self.manifest.capture.target.conversation_id
        {
            return Err("program_stop_original_invalid");
        }
        Ok(original)
    }
    pub fn termination_id(&self) -> Result<String, &'static str> {
        self.original().map(|original| original.request_id)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > INPUT_LIMIT {
            return Err("program_stop_input_oversized");
        }
        let input: Self =
            serde_json::from_slice(bytes).map_err(|_| "program_stop_input_invalid")?;
        input
            .valid()
            .then_some(input)
            .ok_or("program_stop_input_invalid")
    }
    /// Covers the COMPLETE input, including exact original bytes and both IDs.
    pub fn binding(&self) -> String {
        hash(&serde_json::to_vec(self).expect("closed program stop input"))
    }
    /// A new Force list may contain only surviving ORIGINAL identities from
    /// the exact prior known result, for the same termination/checkout scope.
    /// This check is DATA correlation, never a Force permit.
    pub fn follows(&self, previous: &Self, report: &Report) -> bool {
        let Kind::Force {
            previous_request_id,
            previous_binding,
        } = &self.stop
        else {
            return false;
        };
        let Some(survivors) = report.survivors(previous) else {
            return false;
        };
        self.valid()
            && previous.valid()
            && matches!(previous.stop, Kind::Normal)
            && [&self.request_id, &self.confirmation_id, &self.manifest.id]
                .iter()
                .all(|id| **id != previous.confirmation_id && **id != previous.manifest.id)
            && previous_request_id == &previous.request_id
            && previous_binding == &previous.binding()
            && self.original == previous.original
            && self.source == previous.source
            && self.archive_id == previous.archive_id
            && self.manifest.capture == previous.manifest.capture
            && self.manifest.configuration == previous.manifest.configuration
            && self.manifest.boot == previous.manifest.boot
            && self.manifest.checkout == previous.manifest.checkout
            && !self.manifest.programs.is_empty()
            && self
                .manifest
                .programs
                .iter()
                .all(|now| survivors.iter().any(|old| old.same_process(now)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Durable original reservation. Lost results project Unknown, never retry.
    Started,
    Complete,
    Incomplete,
    Refused,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Exited,
    Outside,
    Surviving,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub program: Program,
    pub observation: Observation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub request_id: String,
    pub binding: String,
    pub state: State,
    pub members: Vec<Member>,
}
impl Report {
    pub fn valid(&self, original: &Input) -> bool {
        if !original.valid()
            || self.request_id != original.request_id
            || self.binding != original.binding()
            || self.members.len() > MAX_PROGRAMS
        {
            return false;
        }
        match self.state {
            State::Started | State::Refused | State::Unknown => self.members.is_empty(),
            State::Complete | State::Incomplete => {
                self.members.len() == original.manifest.programs.len()
                    && self
                        .members
                        .iter()
                        .zip(&original.manifest.programs)
                        .all(|(row, old)| row.program == *old)
                    && (self.state == State::Incomplete)
                        == self
                            .members
                            .iter()
                            .any(|m| m.observation == Observation::Surviving)
            }
        }
    }
    pub fn decode(bytes: &[u8], original: &Input) -> Result<Self, &'static str> {
        if bytes.len() > MAX_BYTES {
            return Err("program_stop_report_oversized");
        }
        let report: Self =
            serde_json::from_slice(bytes).map_err(|_| "program_stop_report_invalid")?;
        report
            .valid(original)
            .then_some(report)
            .ok_or("program_stop_report_invalid")
    }
    pub fn survivors(&self, original: &Input) -> Option<Vec<Program>> {
        (self.valid(original) && self.state == State::Incomplete).then(|| {
            self.members
                .iter()
                .filter(|m| m.observation == Observation::Surviving)
                .map(|m| m.program.clone())
                .collect()
        })
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(in crate::worktree_programs) fn input() -> Input {
        let manifest = super::super::tests::manifest();
        let original = manifest
            .capture
            .invocation("22222222-2222-4222-8222-222222222222")
            .unwrap()
            .stdin
            .unwrap();
        Input {
            manifest,
            request_id: "33333333-3333-4333-8333-333333333333".into(),
            confirmation_id: "44444444-4444-4444-8444-444444444444".into(),
            source: Source::Service {
                epoch: "11111111-1111-4111-8111-111111111111".into(),
            },
            original: String::from_utf8(original).unwrap(),
            archive_id: "55555555-5555-4555-8555-555555555555".into(),
            stop: Kind::Normal,
        }
    }
    fn report(input: &Input) -> Report {
        Report {
            request_id: input.request_id.clone(),
            binding: input.binding(),
            state: State::Incomplete,
            members: input
                .manifest
                .programs
                .iter()
                .map(|program| Member {
                    program: program.clone(),
                    observation: Observation::Surviving,
                })
                .collect(),
        }
    }
    #[test]
    fn original_source_is_integrity_bound_data_and_force_cannot_change_it() {
        let original = input();
        let mut changed = original.clone();
        changed.source = Source::Service {
            epoch: "99999999-9999-4999-8999-999999999999".into(),
        };
        assert!(changed.valid());
        assert_ne!(changed.binding(), original.binding());
        changed.source = Source::Native {
            request_id: "99999999-9999-4999-8999-999999999999".into(),
            journal: None,
            revision: 0,
        };
        assert!(changed.valid());
        assert_ne!(changed.binding(), original.binding());
        assert_eq!(
            changed.source.mode(),
            crate::desktop::ownership::Mode::Native
        );
        let old = report(&original);
        changed.request_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into();
        changed.confirmation_id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into();
        changed.manifest.id = "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into();
        changed.stop = Kind::Force {
            previous_request_id: original.request_id.clone(),
            previous_binding: original.binding(),
        };
        assert!(!changed.follows(&original, &old));
        changed.source = original.source.clone();
        assert!(changed.follows(&original, &old));
        let mut raw = serde_json::to_value(&original).unwrap();
        raw.as_object_mut().unwrap().remove("source");
        assert!(Input::decode(&serde_json::to_vec(&raw).unwrap()).is_err());
        assert!(!Source::Native {
            request_id: "99999999-9999-4999-8999-999999999999".into(),
            journal: None,
            revision: 1
        }
        .valid());
    }
    #[test]
    fn complete_input_and_original_bytes_are_bound_and_unknown_results_never_offer_force() {
        let i = input();
        assert!(i.valid());
        assert_eq!(Input::decode(&serde_json::to_vec(&i).unwrap()).unwrap(), i);
        let r = report(&i);
        assert!(r.valid(&i));
        assert_eq!(r.survivors(&i).unwrap(), i.manifest.programs);
        let complete = Report {
            state: State::Complete,
            members: r
                .members
                .iter()
                .map(|m| Member {
                    observation: Observation::Exited,
                    ..m.clone()
                })
                .collect(),
            ..r.clone()
        };
        assert!(complete.valid(&i));
        assert!(complete.survivors(&i).is_none());
        for state in [
            State::Started,
            State::Complete,
            State::Refused,
            State::Unknown,
        ] {
            let r = Report {
                state,
                members: Vec::new(),
                ..r.clone()
            };
            assert!(r.survivors(&i).is_none());
        }
        let mut whitespace = i.clone();
        whitespace.original.push('\n');
        assert!(whitespace.valid());
        assert_ne!(i.binding(), whitespace.binding());
        assert!(!r.valid(&whitespace));
        let mut changed = i.clone();
        changed.request_id = i.termination_id().unwrap();
        assert!(!changed.valid());
        let mut raw = serde_json::to_value(&i).unwrap();
        raw["permit"] = true.into();
        assert!(Input::decode(&serde_json::to_vec(&raw).unwrap()).is_err());
        assert!(Input::decode(&vec![b' '; INPUT_LIMIT + 1]).is_err());
    }
    #[test]
    fn force_needs_a_new_confirmation_and_exact_known_survivors_of_the_original() {
        let previous = input();
        let result = report(&previous);
        let mut force = previous.clone();
        force.request_id = "66666666-6666-4666-8666-666666666666".into();
        force.confirmation_id = "77777777-7777-4777-8777-777777777777".into();
        force.manifest.id = "88888888-8888-4888-8888-888888888888".into();
        force.stop = Kind::Force {
            previous_request_id: previous.request_id.clone(),
            previous_binding: previous.binding(),
        };
        assert!(force.follows(&previous, &result));
        let mut changed = force.clone();
        changed.confirmation_id = previous.confirmation_id.clone();
        assert!(!changed.follows(&previous, &result));
        let mut changed = force.clone();
        changed.manifest.id = previous.manifest.id.clone();
        assert!(!changed.follows(&previous, &result));
        let mut changed = force.clone();
        changed.manifest.programs[0].birth = "reused".into();
        assert!(!changed.follows(&previous, &result));
        let mut changed = force.clone();
        changed.manifest.checkout.inode += 1;
        assert!(!changed.follows(&previous, &result));
        let mut changed = force.clone();
        changed.archive_id = force.manifest.id.clone();
        assert!(!changed.follows(&previous, &result));
        let mut changed = force.clone();
        changed.manifest.programs.clear();
        assert!(!changed.follows(&previous, &result));
        for state in [
            State::Started,
            State::Complete,
            State::Refused,
            State::Unknown,
        ] {
            let unknown = Report {
                state,
                members: Vec::new(),
                ..result.clone()
            };
            assert!(!force.follows(&previous, &unknown));
        }
    }
}
