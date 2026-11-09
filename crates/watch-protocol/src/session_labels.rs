//! A bounded local capability observation, independent of watch frames.
//! A reply is useful only for the exact outstanding query and its original
//! deadline. Neither a saved reply nor an installed binary's help proves a live
//! connector. There is no operation, PID, path or execution permit in this ABI.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MARKER: &str = "session-labels-v1";
pub const SCHEMA: u32 = 1;
pub const MAX_BYTES: usize = 16 * 1024;
pub const QUERY_MILLIS: u64 = 2_000;
pub const QUERY_FILE: &str = "session-labels-query.json";
pub const REPLY_FILE: &str = "session-labels-reply.json";
pub const CONFIGURATION_BYTES: usize = 1024 * 1024;

/// Hash this frame with SHA-256. Only the digest crosses the local query;
/// configuration contents can contain credentials and must never be logged.
pub fn configuration_binding(path: &str, contents: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty()
        || path.len() > 32 * 1024
        || path.contains('\0')
        || contents.len() > CONFIGURATION_BYTES
    {
        return None;
    }
    let mut bytes = b"session-labels-config-v1\0".to_vec();
    bytes.extend_from_slice(path.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(contents);
    Some(bytes)
}

/// The maintained clients' configuration location. Keep the choice shared with
/// the connector so a custom --config can never answer for the supported slot.
pub fn configuration_path(
    home: Option<PathBuf>,
    xdg: Option<PathBuf>,
    hgs: Option<PathBuf>,
    windows: bool,
) -> PathBuf {
    if windows {
        if let Some(root) = hgs.filter(|path| path.is_absolute()) {
            return root.join("mobile").join("connector.json");
        }
        return home
            .unwrap_or_default()
            .join(".config")
            .join("hgs")
            .join("mobile")
            .join("connector.json");
    }
    xdg.filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(".config")))
        .unwrap_or_default()
        .join("hgs")
        .join("mobile")
        .join("connector.json")
}

fn hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn generation(value: &str) -> bool {
    hex(value, 32) && value.bytes().any(|c| c != b'0')
}
fn machine(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(at, c)| {
            if [8, 13, 18, 23].contains(&at) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
        && value.bytes().any(|c| c != b'0' && c != b'-')
}

/// What the connector's actual selected hgs reports for its own configuration.
/// It performs no connector query itself, so observations cannot recurse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub schema: u32,
    pub marker: String,
    pub machine: String,
    pub configuration: String,
    pub version: String,
    pub membership_by_id: bool,
    /// A required build capability: an earlier context shape cannot claim it.
    pub labels_by_id: bool,
}
impl Context {
    pub fn valid(&self) -> bool {
        self.schema == SCHEMA
            && self.marker == MARKER
            && machine(&self.machine)
            && hex(&self.configuration, 64)
            && !self.version.is_empty()
            && self.version.len() <= 256
            && !self.version.chars().any(char::is_control)
    }
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_BYTES {
            return None;
        }
        let value: Self = serde_json::from_slice(bytes).ok()?;
        value.valid().then_some(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub schema: u32,
    pub nonce: String,
    pub machine: String,
    pub configuration: String,
    /// Hash of the canonical connector configuration path and its exact bytes.
    /// The responding journal owner must still have that original selection.
    pub connector_configuration: String,
    pub expires_at_millis: u64,
}
impl Query {
    pub fn valid(&self, now: u64) -> bool {
        self.schema == SCHEMA
            && generation(&self.nonce)
            && machine(&self.machine)
            && hex(&self.configuration, 64)
            && hex(&self.connector_configuration, 64)
            && self
                .expires_at_millis
                .checked_sub(now)
                .is_some_and(|left| left > 0 && left <= QUERY_MILLIS)
    }
    pub fn parse(bytes: &[u8], now: u64) -> Option<Self> {
        if bytes.len() > MAX_BYTES {
            return None;
        }
        let value: Self = serde_json::from_slice(bytes).ok()?;
        value.valid(now).then_some(value)
    }
}

/// `generation` is one actual journal owner's data generation, never a Stop
/// confirmation token or native proof. Only `matches` gives a positive answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub schema: u32,
    pub query: Query,
    pub generation: String,
    /// The connector build itself implements ID routing; upgrading hgs does not upgrade it.
    pub routing_by_id: bool,
    pub context: Option<Context>,
}
impl Reply {
    pub fn matches(&self, query: &Query, now: u64) -> bool {
        self.schema == SCHEMA
            && self.routing_by_id
            && query.valid(now)
            && self.query == *query
            && generation(&self.generation)
            && self.context.as_ref().is_some_and(|context| {
                context.valid()
                    && context.membership_by_id
                    && context.labels_by_id
                    && context.machine == query.machine
                    && context.configuration == query.configuration
            })
    }
    pub fn parse(bytes: &[u8], query: &Query, now: u64) -> Option<Self> {
        if bytes.len() > MAX_BYTES {
            return None;
        }
        let value: Self = serde_json::from_slice(bytes).ok()?;
        value.matches(query, now).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_configuration_choice_keeps_the_maintained_clients_path_policy() {
        let home = PathBuf::from("/fixture/home");
        let xdg = PathBuf::from("/fixture/xdg");
        let hgs = PathBuf::from("/fixture/hgs");
        assert_eq!(
            configuration_path(
                Some(home.clone()),
                Some(xdg.clone()),
                Some(hgs.clone()),
                false
            ),
            xdg.join("hgs/mobile/connector.json")
        );
        assert_eq!(
            configuration_path(
                Some(home.clone()),
                Some("relative".into()),
                Some(hgs.clone()),
                false
            ),
            home.join(".config/hgs/mobile/connector.json")
        );
        assert_eq!(
            configuration_path(Some(home.clone()), Some(xdg), Some(hgs.clone()), true),
            hgs.join("mobile/connector.json")
        );
        assert_eq!(
            configuration_path(Some(home.clone()), None, Some("relative".into()), true),
            home.join(".config/hgs/mobile/connector.json")
        );
        assert_ne!(
            configuration_binding("/a", b"bc"),
            configuration_binding("/ab", b"c")
        );
        assert_ne!(
            configuration_binding("/a", b"{}"),
            configuration_binding("/other", b"{}")
        );
        assert!(configuration_binding("", b"{}").is_none());
        assert!(configuration_binding("/a", &vec![0; CONFIGURATION_BYTES + 1]).is_none());
    }
    fn query() -> Query {
        Query {
            schema: SCHEMA,
            nonce: "a".repeat(32),
            machine: "11111111-1111-4111-8111-111111111111".into(),
            configuration: "b".repeat(64),
            connector_configuration: "c".repeat(64),
            expires_at_millis: 3_000,
        }
    }
    fn reply() -> Reply {
        let query = query();
        Reply {
            schema: SCHEMA,
            generation: "c".repeat(32),
            routing_by_id: true,
            context: Some(Context {
                schema: SCHEMA,
                marker: MARKER.into(),
                machine: query.machine.clone(),
                configuration: query.configuration.clone(),
                version: "fixture".into(),
                membership_by_id: true,
                labels_by_id: true,
            }),
            query,
        }
    }
    #[test]
    fn a_positive_is_exact_original_and_never_renews_the_query() {
        let query = query();
        let reply = reply();
        assert!(reply.matches(&query, 1_000));
        assert!(reply.matches(&query, 2_999));
        for now in [0, 999, 3_000, u64::MAX] {
            assert!(!reply.matches(&query, now));
        }
        let mut next = query.clone();
        next.nonce = "d".repeat(32);
        assert!(!reply.matches(&next, 1_000));
        next = query.clone();
        next.expires_at_millis += 1;
        assert!(!reply.matches(&next, 1_001));
    }
    #[test]
    fn missing_changed_and_partial_evidence_never_answer_positive() {
        let query = query();
        for field in [
            "generation",
            "machine",
            "configuration",
            "marker",
            "membership",
            "routing",
            "native routing",
        ] {
            let mut r = reply();
            let c = r.context.as_mut().unwrap();
            match field {
                "generation" => r.generation = "0".repeat(32),
                "machine" => c.machine = "22222222-2222-4222-8222-222222222222".into(),
                "configuration" => c.configuration = "c".repeat(64),
                "marker" => c.marker = "session-labels-v10".into(),
                "routing" => r.routing_by_id = false,
                "native routing" => c.labels_by_id = false,
                _ => c.membership_by_id = false,
            }
            assert!(!r.matches(&query, 1_000), "{field}");
        }
        let mut r = reply();
        r.context = None;
        assert!(!r.matches(&query, 1_000));
    }
    #[test]
    fn bounded_closed_json_refuses_wrong_types_and_unknown_fields() {
        let query = query();
        let bytes = serde_json::to_vec(&reply()).unwrap();
        assert!(Reply::parse(&bytes, &query, 1_000).is_some());
        for bad in [
            b"null".as_slice(),
            b"{}",
            b"[]",
            b"{",
            &[b' '; MAX_BYTES + 1],
        ] {
            assert!(Reply::parse(bad, &query, 1_000).is_none());
        }
        let mut object = serde_json::to_value(reply()).unwrap();
        object["operation"] = serde_json::json!("not an authority channel");
        assert!(Reply::parse(&serde_json::to_vec(&object).unwrap(), &query, 1_000).is_none());
        object.as_object_mut().unwrap().remove("operation");
        object["context"]["membership_by_id"] = serde_json::json!("true");
        assert!(Reply::parse(&serde_json::to_vec(&object).unwrap(), &query, 1_000).is_none());
        object = serde_json::to_value(reply()).unwrap();
        object["context"]
            .as_object_mut()
            .unwrap()
            .remove("labels_by_id");
        assert!(
            Reply::parse(&serde_json::to_vec(&object).unwrap(), &query, 1_000).is_none(),
            "the earlier observation-only CLI context cannot claim full ID routing"
        );
    }
}
