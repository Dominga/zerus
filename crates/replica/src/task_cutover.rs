//! The schema-8 seal never grants exemptions beyond the genuine historical
//! floor. Store adoption and native dispatch admission remain separate.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: i64 = 8;
const COMMIT: &str = "zerus-task-causal-cutover-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub node: String,
    pub store: String,
    pub version: crate::Version,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotCutOver {
    pub node: String,
    pub store: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutoverSeal {
    pub format: u64,
    pub task: String,
    pub project: String,
    /// None only for a task born sealed by its in-process creation witness.
    pub root: Option<execution::Floor>,
    pub prefix: crate::Version,
    pub peer: String,
    pub counter: i32,
    pub genesis_peer: String,
    pub genesis_counter: i32,
    pub baseline: crate::Version,
    pub cutover: String,
    pub captures: BTreeMap<String, Capture>,
    pub not_cut_over: BTreeMap<String, NotCutOver>,
}

impl CutoverSeal {
    pub fn identity(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(self).expect("typed seal"))
        )
    }
}

impl TaskDoc {
    /// Read-only legacy decoding is not migration authority. An unrooted
    /// pre-seal document can be retained as data, but cannot mint a seal or
    /// pass any local mutation/native-dispatch gate.
    pub(crate) fn validate_import_history(&self) -> Result<()> {
        if self.meta().get("floor").is_none()
            && self.meta().get("cutover").is_none()
            && self.stored_schema() < SCHEMA
        {
            return Ok(());
        }
        self.validate_cutover_history()
    }
    /// Read-only evidence, never a store-adoption grant.
    pub fn cutover_seal(&self) -> Result<Option<CutoverSeal>> {
        string(&self.meta(), "cutover")
            .map(|value| serde_json::from_str(&value).map_err(Into::into))
            .transpose()
    }

    /// Validate the intrinsic historical root and each later causal span.
    /// No caller supplies a baseline that could grandfather a stale branch.
    pub fn validate_cutover_history(&self) -> Result<()> {
        let declared = self.cutover_seal()?;
        if !self.execution_managed() && declared.is_none() {
            if self.meta().get("cutover").is_some() || self.stored_schema() == SCHEMA {
                return Err(Error::new("the task cutover has no historical root"));
            }
            return Ok(());
        }
        let root = if let Some(seal) = &declared {
            seal.root.clone()
        } else {
            Some(
                self.floor_record()
                    .ok_or_else(|| Error::new("this task has no provable historical floor"))?,
            )
        };
        let history = match &root {
            Some(floor) => crate::cutover_root::inspect(&self.doc, floor)?,
            None => crate::cutover_root::inspect_genesis(&self.doc)?,
        };
        if history.task != self.uuid {
            return Err(Error::new("the historical root belongs to another task"));
        }
        crate::cutover::validate(&self.doc, &history.prefix, history.id)?;
        let Some((id, text)) = history.seal else {
            if self.stored_schema() == SCHEMA {
                return Err(Error::new("the task has no provable schema-8 seal"));
            }
            return Ok(());
        };
        let seal: CutoverSeal = serde_json::from_str(&text)?;
        if seal.format != 1
            || seal.task != self.uuid
            || seal.project != history.project
            || seal.root != root
            || seal.prefix != crate::sync::version(&history.prefix)
            || seal.peer != id.peer.to_string()
            || seal.counter != id.counter
            || seal.genesis_peer != history.genesis.peer.to_string()
            || seal.genesis_counter != history.genesis.counter
            || !ids::is_uuid(&seal.cutover)
            || seal.captures.len() + seal.not_cut_over.len() > 256
        {
            return Err(Error::new("the task seal does not match its original root"));
        }
        for (node, capture) in &seal.captures {
            if node != &capture.node
                || !ids::is_uuid(node)
                || !ids::is_uuid(&capture.store)
                || !ids::is_uuid(&capture.token)
                || crate::canonical_version(&capture.version).is_none()
            {
                return Err(Error::new(
                    "the task seal has an invalid participant capture",
                ));
            }
        }
        for (node, absent) in &seal.not_cut_over {
            if node != &absent.node
                || !ids::is_uuid(node)
                || absent
                    .store
                    .as_ref()
                    .is_some_and(|store| !ids::is_uuid(store))
                || seal.captures.contains_key(node)
                || ![
                    "unroutable",
                    "unreachable",
                    "unverified",
                    "deadline",
                    "task_schema",
                    "fence_unsupported",
                    "stale_copy",
                ]
                .contains(&absent.reason.as_str())
            {
                return Err(Error::new("the task seal has an invalid not-cut-over fact"));
            }
        }
        let change = self
            .doc
            .get_change(id)
            .ok_or_else(|| Error::new("the seal operation is missing"))?;
        let baseline = crate::sync::vector(&seal.baseline)?;
        if !seal.captures.is_empty() {
            let mut union = crate::Version::new();
            for capture in seal.captures.values() {
                for (peer, count) in &capture.version {
                    let known = union.entry(peer.clone()).or_default();
                    *known = (*known).max(*count);
                }
            }
            if union != seal.baseline {
                return Err(Error::new(
                    "the seal baseline is not exactly the fixed participant captures",
                ));
            }
        }
        if change.id != id
            || change.len != 2
            || change.message() != COMMIT
            || self.doc.frontiers_to_vv(&change.deps).as_ref() != Some(&baseline)
        {
            return Err(Error::new(
                "the seal is not the dedicated frozen-baseline commit",
            ));
        }
        let after = id
            .counter
            .checked_add(2)
            .ok_or_else(|| Error::new("the seal counter overflowed"))?;
        let ops = self
            .doc
            .export_json_in_id_span(loro::IdSpan::new(id.peer, id.counter, after));
        let ops: Vec<_> = ops.into_iter().flat_map(|change| change.ops).collect();
        let meta = self.meta().id();
        if ops.len() != 2
            || ops.iter().any(|op| op.container != meta)
            || !matches!(&ops[0].content, loro::JsonOpContent::Map(loro::JsonMapOp::Insert{key,value}) if key == "cutover" && value == &LoroValue::String(text.as_str().into()))
            || !matches!(&ops[1].content, loro::JsonOpContent::Map(loro::JsonMapOp::Insert{key,value}) if key == "schema" && value == &LoroValue::I64(SCHEMA))
        {
            return Err(Error::new("the seal commit contains unexpected operations"));
        }
        let sealed = self
            .doc
            .frontiers_to_vv(&loro::Frontiers::from(loro::ID::new(id.peer, after - 1)))
            .ok_or_else(|| Error::new("the seal's causal closure is missing"))?;
        crate::cutover::validate(&self.doc, &sealed, id)?;
        Ok(())
    }

    /// Construct a proposal from fixed captures. This changes only this
    /// document; ProjectStore must durably adopt it before any local write.
    pub fn seal_cutover(
        &mut self,
        captures: BTreeMap<String, Capture>,
        not_cut_over: BTreeMap<String, NotCutOver>,
    ) -> Result<CutoverSeal> {
        self.seal_inner(captures, not_cut_over, false)
    }

    pub(crate) fn seal_genesis(&mut self) -> Result<CutoverSeal> {
        if !self.created_here || self.saved.is_some() {
            return Err(Error::new(
                "an imported task cannot manufacture a new genesis root",
            ));
        }
        self.seal_inner(BTreeMap::new(), BTreeMap::new(), true)
    }

    fn seal_inner(
        &mut self,
        captures: BTreeMap<String, Capture>,
        not_cut_over: BTreeMap<String, NotCutOver>,
        born: bool,
    ) -> Result<CutoverSeal> {
        self.writable()?;
        self.validate_cutover_history()?;
        if let Some(seal) = self.cutover_seal()? {
            return Ok(seal);
        }
        let root = if born {
            None
        } else {
            Some(
                self.floor_record()
                    .ok_or_else(|| Error::new("this task has no migration root"))?,
            )
        };
        let historical = match &root {
            Some(floor) => crate::cutover_root::inspect(&self.doc, floor)?,
            None => crate::cutover_root::inspect_genesis(&self.doc)?,
        };
        let baseline = self
            .applied_version()
            .ok_or_else(|| Error::new("the task has unapplied changes"))?;
        let peer = self.doc.peer_id();
        let counter = self.doc.oplog_vv().get(&peer).copied().unwrap_or(0);
        let seal = CutoverSeal {
            format: 1,
            task: self.uuid().to_owned(),
            project: self.view()?.project,
            root,
            prefix: crate::sync::version(&historical.prefix),
            peer: peer.to_string(),
            counter,
            genesis_peer: historical.genesis.peer.to_string(),
            genesis_counter: historical.genesis.counter,
            baseline,
            cutover: ids::new_uuid(),
            captures,
            not_cut_over,
        };
        self.doc.set_next_commit_message(COMMIT);
        self.meta()
            .insert("cutover", serde_json::to_string(&seal)?)?;
        self.meta().insert("schema", SCHEMA)?;
        self.doc.commit();
        Ok(seal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> TaskDoc {
        TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-a1b2c3",
            text: "original",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap()
    }

    #[test]
    fn never_certified_first_floor_can_be_sealed_without_rebasing_it() {
        let mut current = task();
        current.floor().unwrap();
        let original = current.floor_record().unwrap();
        current
            .comment("a legitimate later edit", &Actor::Person, 2)
            .unwrap();
        let seal = current
            .seal_cutover(BTreeMap::new(), BTreeMap::new())
            .unwrap();
        assert_eq!(seal.root, Some(original));
        assert_ne!(seal.prefix, seal.baseline);
        assert_eq!(current.cutover_seal().unwrap(), Some(seal));
        current.validate_cutover_history().unwrap();
    }

    #[test]
    fn merged_old_branch_cannot_become_a_new_seals_baseline() {
        let mut current = task();
        let mut offline = TaskDoc::from_bytes(&current.snapshot().unwrap()).unwrap();
        current.floor().unwrap();
        offline
            .comment("old concurrent edit", &Actor::Person, 2)
            .unwrap();
        current.doc.import(&offline.snapshot().unwrap()).unwrap();
        current.repair_schema().unwrap();
        assert!(current
            .seal_cutover(BTreeMap::new(), BTreeMap::new())
            .is_err());
    }

    #[test]
    fn ordinary_import_refuses_an_old_branch_before_mutating_the_task() {
        let mut current = task();
        let mut offline = TaskDoc::from_bytes(&current.snapshot().unwrap()).unwrap();
        current.floor().unwrap();
        offline.comment("old branch", &Actor::Person, 2).unwrap();
        let before = current.version();
        assert!(current.import(&offline.snapshot().unwrap()).is_err());
        assert_eq!(current.version(), before);
    }

    #[test]
    fn an_existing_seal_is_not_replaced_by_another_proposal() {
        let mut current = task();
        current.floor().unwrap();
        let first = current
            .seal_cutover(BTreeMap::new(), BTreeMap::new())
            .unwrap();
        let before = current.version();
        assert_eq!(
            current
                .seal_cutover(BTreeMap::new(), BTreeMap::new())
                .unwrap(),
            first
        );
        assert_eq!(current.version(), before);
    }
}
