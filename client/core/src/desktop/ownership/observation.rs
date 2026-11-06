//! Closed read wire data. Decoding this data never constructs native evidence.
use super::{State, Status};
use crate::desktop::digest;
use serde::{Deserialize, Serialize};
use zerus_desktop_migration::canonical_uuid;

pub const ABI: &str = "desktop-ownership-observation-v1";
pub const HELP_MARKER: &str = "Desktop ownership observation ABI: desktop-ownership-observation-v1";

/// Expected history and current endpoint, never an Origin or a permission.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub request_id: String,
    pub configuration: String,
    pub journal: String,
    pub owner_epoch: String,
    pub service_epoch: String,
    pub generation: u64,
}
impl Query {
    pub fn valid(&self) -> bool {
        canonical_uuid(&self.request_id)
            && digest(&self.configuration)
            && canonical_uuid(&self.journal)
            && canonical_uuid(&self.owner_epoch)
            && canonical_uuid(&self.service_epoch)
            && self.generation > 0
            && self.generation < (1u64 << 53)
    }
    pub fn matches(&self, status: &Status, epoch: &str, generation: u64) -> bool {
        self.valid()
            && status.matches(&self.configuration)
            && status.state == State::Service
            && status.identity_current
            && !status.rearm_required
            && status.journal.as_deref() == Some(self.journal.as_str())
            && status.owner_epoch.as_deref() == Some(self.owner_epoch.as_str())
            && self.service_epoch == epoch
            && self.generation == generation
    }
}

/// The native actor returns this only after comparing the authenticated
/// requesting Origin with its stored Origin. It remains transport data; the
/// native consumer must retain and check its original exchange independently.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub abi: String,
    pub query: Query,
    pub current: Status,
}
impl Observation {
    pub fn matches(&self, query: &Query) -> bool {
        self.abi == ABI
            && &self.query == query
            && query.matches(&self.current, &query.service_epoch, query.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn query() -> Query {
        Query {
            request_id: "11111111-1111-4111-8111-111111111111".into(),
            configuration: "a".repeat(64),
            journal: "22222222-2222-4222-8222-222222222222".into(),
            owner_epoch: "33333333-3333-4333-8333-333333333333".into(),
            service_epoch: "44444444-4444-4444-8444-444444444444".into(),
            generation: 1,
        }
    }
    #[test]
    fn wire_never_accepts_supplied_origin_or_grant_authority() {
        let query = query();
        let mut value = serde_json::to_value(&query).unwrap();
        for field in ["origin", "caller_pid", "frontend_image", "grant", "receipt"] {
            value[field] = json!(true);
            assert!(
                serde_json::from_value::<Query>(value.clone()).is_err(),
                "{field}"
            );
            value.as_object_mut().unwrap().remove(field);
        }
        let request = crate::desktop::ui_requests::Request::DesktopOwnershipObserve {
            query: Box::new(query.clone()),
        };
        assert_eq!(request.operation(), "desktop_ownership_observe");
        assert_eq!(request.id(), query.request_id);
        assert!(crate::desktop::ui_requests::Envelope {
            abi: crate::desktop::ui_requests::ABI.into(),
            attempt: query.request_id.clone(),
            request,
        }
        .valid());
        for generation in [0, 1u64 << 53, u64::MAX] {
            assert!(!Query {
                generation,
                ..query.clone()
            }
            .valid());
        }
    }
    #[test]
    fn an_observation_matches_only_the_exact_requested_incarnation_and_current_endpoint() {
        let query = query();
        let status = Status {
            abi: super::super::ABI.into(),
            configuration: query.configuration.clone(),
            journal: Some(query.journal.clone()),
            revision: 1,
            state: State::Service,
            rearm_required: false,
            request_id: None,
            owner_epoch: Some(query.owner_epoch.clone()),
            identity_current: true,
        };
        let observation = Observation {
            abi: ABI.into(),
            query: query.clone(),
            current: status,
        };
        assert!(observation.matches(&query));
        let mut changed = query.clone();
        changed.service_epoch = query.owner_epoch.clone();
        assert!(!observation.matches(&changed));
        changed = query.clone();
        changed.generation += 1;
        assert!(!observation.matches(&changed));
        let mut value = serde_json::to_value(observation).unwrap();
        value["current"]["identity_current"] = false.into();
        assert!(!serde_json::from_value::<Observation>(value)
            .unwrap()
            .matches(&query));
    }
}
