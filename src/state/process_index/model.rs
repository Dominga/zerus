use super::*;
pub(crate) const ANCESTORS: usize = 64;
pub(crate) const PAGE_ROWS: usize = 128;
pub(crate) const PAGE_BYTES: usize = 64 * 1024;
pub(crate) const ENTRY_BYTES: usize = 4096;
pub(crate) const CHANGE_BYTES: usize = 16 * 1024;
pub(crate) const JOURNAL_ROWS: u64 = 262_144;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scope {
    pub machine: String,
    pub configuration: String,
    pub boot: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum Birth {
    Linux(u64),
    Mac { seconds: u64, micros: u32 },
    Windows(u64),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Process {
    pub pid: u32,
    pub birth: Birth,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Role {
    Agent,
    Supervisor,
    Host,
    Hosted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    Native,
    DeepSeek,
    OpenCode,
    DeepSeekHost,
    OpenCodeHost,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub kind: Kind,
    pub key: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Anchor {
    pub process: Process,
    pub role: Role,
    pub source: Source,
    pub revision: u64,
    pub fingerprint: String,
    pub published_at: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Edge {
    pub process: Process,
    pub role: Role,
    pub session_id: String,
    pub run: String,
    pub conversation: Option<String>,
    pub source: Source,
    pub revision: u64,
    pub fingerprint: String,
    pub published_at: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Publication {
    pub anchors: Vec<Anchor>,
    pub edges: Vec<Edge>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    pub id: String,
    pub source: Source,
    pub before: u64,
    /// Immutable descriptive locator. Its owner must qualify the native path;
    /// the string itself never grants filesystem or caller authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    pub publication: Publication,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub source: Source,
    pub revision: u64,
    pub publication: Publication,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ticket {
    pub epoch: String,
    pub id: String,
    pub seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Build {
    pub epoch: String,
    pub generation: u64,
    pub high_water: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub epoch: String,
    pub generation: u64,
    pub revision: u64,
    pub high_water: u64,
    pub complete: bool,
}
/// Index metadata only. Even NoMatch grants no actor, gesture or effect authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Lookup {
    Candidate {
        anchor: Anchor,
        stamp: Stamp,
        lookups: usize,
    },
    NoMatch {
        stamp: Stamp,
        lookups: usize,
    },
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangeRow {
    pub seq: u64,
    pub change: Change,
    pub settled: bool,
}
/// `bytes` bounds serialized row payload; native wire framing needs its own bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Page<T> {
    pub rows: Vec<T>,
    pub bytes: usize,
    pub next: Option<String>,
    pub stamp: Stamp,
}
fn text(s: &str, max: usize) -> Result<()> {
    ensure!(
        !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control),
        "Invalid process-index text"
    );
    Ok(())
}
fn integer(n: u64) -> Result<()> {
    ensure!(n <= i64::MAX as u64, "Process-index integer overflow");
    Ok(())
}
impl Scope {
    pub(crate) fn validate(&self) -> Result<()> {
        text(&self.machine, 128)?;
        text(&self.configuration, 256)?;
        text(&self.boot, 256)
    }
}
impl Source {
    pub(super) fn validate(&self) -> Result<()> {
        text(&self.key, 128)?;
        ensure!(
            self.key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
            "Invalid process-index source key"
        );
        Ok(())
    }
    pub(super) fn encoded(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string(self)?)
    }
}
impl Process {
    pub(super) fn validate(&self) -> Result<()> {
        ensure!(self.pid > 0, "Invalid process ID");
        match self.birth {
            Birth::Linux(n) | Birth::Windows(n) => ensure!(n > 0, "Missing precise process birth"),
            Birth::Mac { seconds, micros } => ensure!(
                seconds > 0 && micros < 1_000_000,
                "Invalid precise Mac birth"
            ),
        };
        Ok(())
    }
    pub(super) fn encoded_birth(&self) -> Result<String> {
        self.validate()?;
        Ok(serde_json::to_string(&self.birth)?)
    }
}
impl Publication {
    pub(super) fn binding_eq(&self, other: &Self) -> bool {
        let mut a = self.clone();
        let mut b = other.clone();
        for p in [&mut a, &mut b] {
            for x in &mut p.anchors {
                x.published_at = 0;
            }
            for x in &mut p.edges {
                x.published_at = 0;
            }
        }
        a == b
    }

    pub(super) fn validate(&self, source: &Source, revision: u64) -> Result<()> {
        source.validate()?;
        integer(revision)?;
        ensure!(
            revision > 0 && self.anchors.len() <= 2 && self.edges.len() <= PAGE_ROWS,
            "Invalid process-index publication size/revision"
        );
        let mut pids = std::collections::BTreeSet::new();
        for a in &self.anchors {
            a.process.validate()?;
            ensure!(
                a.source == *source && a.revision == revision,
                "Anchor source/revision changed"
            );
            ensure!(pids.insert(a.process.pid), "Duplicate process anchor");
            ensure!(
                a.role != Role::Hosted,
                "Hosted membership cannot own a process"
            );
            if a.role == Role::Host {
                ensure!(
                    matches!(source.kind, Kind::DeepSeekHost | Kind::OpenCodeHost),
                    "Unqualified host source kind"
                );
            }
            ensure!(
                a.fingerprint.len() == 64
                    && a.fingerprint
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
                "Invalid binding fingerprint"
            );
            integer(a.published_at)?;
            ensure!(
                serde_json::to_vec(a)?.len() <= ENTRY_BYTES,
                "Oversized process anchor"
            );
        }
        let mut keys = std::collections::BTreeSet::new();
        for e in &self.edges {
            e.process.validate()?;
            ensure!(
                e.source == *source && e.revision == revision,
                "Edge source/revision changed"
            );
            let uuid = uuid::Uuid::parse_str(&e.session_id)?;
            ensure!(
                !uuid.is_nil() && uuid.to_string() == e.session_id,
                "Invalid session UUID"
            );
            text(&e.run, 512)?;
            if let Some(cid) = &e.conversation {
                text(cid, 2048)?;
            }
            ensure!(
                e.fingerprint.len() == 64
                    && e.fingerprint
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
                "Invalid edge binding fingerprint"
            );
            ensure!(e.role != Role::Host, "A host is not a hosted session");
            integer(e.published_at)?;
            ensure!(
                serde_json::to_vec(e)?.len() <= ENTRY_BYTES,
                "Oversized session edge"
            );
            ensure!(
                keys.insert((e.process.pid, e.key()?)),
                "Duplicate session edge"
            );
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= CHANGE_BYTES - 1024,
            "Oversized process-index publication"
        );
        Ok(())
    }
}
impl Edge {
    pub(super) fn key(&self) -> Result<String> {
        Ok(format!(
            "{}:{}:{:?}",
            self.source.encoded()?,
            self.session_id,
            self.role
        ))
    }
}
impl Change {
    pub(super) fn validate(&self) -> Result<()> {
        let id = uuid::Uuid::parse_str(&self.id)?;
        ensure!(
            !id.is_nil() && id.to_string() == self.id,
            "Invalid change UUID"
        );
        ensure!(self.before < i64::MAX as u64, "Change revision overflow");
        if let Some(locator) = &self.locator {
            text(locator, 1024)?;
        }
        self.publication.validate(&self.source, self.before + 1)?;
        ensure!(
            serde_json::to_vec(self)?.len() <= CHANGE_BYTES,
            "Oversized change intent"
        );
        Ok(())
    }
}
impl Snapshot {
    pub(super) fn validate(&self) -> Result<()> {
        self.publication.validate(&self.source, self.revision)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Work {
    pub lookups: usize,
    pub anchor_rows: usize,
    pub source_reads: usize,
    pub anchor_vm_steps: i32,
    pub anchor_fullscan_steps: i32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Progress {
    pub build: Build,
    pub after: String,
    pub scanned: bool,
    pub stamp: Stamp,
}
