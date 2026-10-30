use super::*;
use crate::migration::{sources, Bundle, BundleItem, SourceStream};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use zerus_desktop_migration::{SourceRecord, SOURCE};

const IMPORT: &str = "11111111-1111-4111-8111-111111111111";
const OP: &str = "22222222-2222-4222-8222-222222222222";
const REQUEST: &str = "33333333-3333-4333-8333-333333333333";
const TASK: &str = "44444444-4444-4444-8444-444444444444";
const PRIVATE: &str = "PRIVATE BODY MUST NOT APPEAR IN STATUS";

#[derive(Default, Clone)]
struct Memory {
    pages: BTreeMap<String, Vec<u8>>,
    snapshots: BTreeMap<String, Vec<u8>>,
    page_reads: usize,
    body_reads: usize,
}
impl EvidenceReader for Memory {
    fn page(&mut self, reference: &PageRef) -> Result<Vec<u8>, Error> {
        self.page_reads += 1;
        self.pages
            .get(&reference.sha256)
            .cloned()
            .ok_or(Error::Unavailable)
    }
    fn snapshot(&mut self, reference: &SnapshotRef) -> Result<Vec<u8>, Error> {
        self.body_reads += 1;
        self.snapshots
            .get(&reference.sha256)
            .cloned()
            .ok_or(Error::Unavailable)
    }
}

struct Fixture {
    record: ImportRecord,
    memory: Memory,
}
impl Fixture {
    fn new(sources: Vec<SourceStream>) -> Self {
        let mut memory = Memory::default();
        let mut record = None;
        for item in Bundle::new(
            SOURCE,
            false,
            "a".repeat(64),
            IMPORT.into(),
            1,
            sources,
            64 << 20,
        )
        .unwrap()
        {
            match item.unwrap() {
                BundleItem::Snapshot { reference, reader } => {
                    memory
                        .snapshots
                        .insert(reference.sha256, reader.into_inner());
                }
                BundleItem::Page { reference, reader } => {
                    memory.pages.insert(reference.sha256, reader.into_inner());
                }
                BundleItem::Record(value) => record = Some(value),
            }
        }
        Self {
            record: record.unwrap(),
            memory,
        }
    }
    fn projection(&self) -> Projection {
        let bytes = serde_json::to_vec(&self.record).unwrap();
        Projection::new(
            &ImportRecordRef {
                version: 1,
                sha256: sha256(&bytes),
            },
            &bytes,
            &"a".repeat(64),
            IMPORT,
        )
        .unwrap()
    }
    fn page(&mut self) -> Result<Page, Error> {
        self.projection()
            .page(None, Limits::default(), &mut self.memory, || false)
    }
    fn rewrite_body(&mut self, index: usize, change: impl FnOnce(&mut Value)) {
        let reference = &self.record.pages[index];
        let bytes = self.memory.pages.remove(&reference.sha256).unwrap();
        let mut page: zerus_desktop_migration::SnapshotPage =
            serde_json::from_slice(&bytes).unwrap();
        let mut value: Value =
            serde_json::from_slice(&self.memory.snapshots[&page.snapshot.sha256]).unwrap();
        change(&mut value);
        let body = serde_json::to_vec(&value).unwrap();
        page.snapshot = SnapshotRef {
            sha256: sha256(&body),
            bytes: body.len() as u64,
        };
        self.memory
            .snapshots
            .insert(page.snapshot.sha256.clone(), body);
        let bytes = serde_json::to_vec(&page).unwrap();
        let reference = PageRef {
            sha256: sha256(&bytes),
            bytes: bytes.len() as u64,
        };
        self.memory.pages.insert(reference.sha256.clone(), bytes);
        self.record.pages[index] = reference;
    }
}

fn operation(id: &str, state: &str) -> Value {
    json!({"id":id,"creation_schema":1,"state":state,"cmd":"codex", "tag":"synthetic",
        "target":"/srv/example-private-checkout","host":"peer-fixture","created":1,
        "launch_id":REQUEST,"clone_id":OP,"worktree_id":TASK,"launch_dispatched":true,
        "error":PRIVATE,"unknown_private":{"body":PRIVATE}})
}
fn creations(state: &str) -> SourceStream {
    let bytes = serde_json::to_vec(
        &json!({"schema":1,"header_private":PRIVATE,"operations":{OP:operation(OP,state)}}),
    )
    .unwrap();
    sources::creations(sources::Origin::Next, &bytes).unwrap()
}
fn start(phase: &str) -> SourceStream {
    let bytes = serde_json::to_vec(&json!({"schema":1,"id":REQUEST,"phase":phase,"error":PRIVATE,
        "intent":{"plan":{"schema":1,"id":REQUEST,"project":"project-fixture","task":TASK,
            "request":{"request_id":REQUEST,"text":PRIVATE,"expected_run_id":"original-run","expected_conversation_id":"original-conversation"},
            "unknown_private":PRIVATE}},"private":PRIVATE})).unwrap();
    sources::task_start_delivery("project-fixture", TASK, REQUEST, &bytes).unwrap()
}
fn message(origin: sources::Origin) -> SourceStream {
    let bytes = serde_json::to_vec(&json!({"id":REQUEST,"phase":"started",
        "intent":{"mode":"append","dispatch":"original-dispatch","content":{"text":PRIVATE}},"private":PRIVATE})).unwrap();
    sources::task_delivery(origin, "project-fixture", TASK, REQUEST, &bytes).unwrap()
}

#[test]
fn interrupted_creations_are_only_historical_review_evidence() {
    for state in [
        "saving",
        "queued",
        "cloning",
        "worktree",
        "launching",
        "confirming",
        "recover",
        "unknown",
        "storage_error",
    ] {
        let mut f = Fixture::new(vec![creations(state)]);
        let page = f.page().unwrap();
        assert_eq!(page.operations.len(), 1);
        let op = &page.operations[0];
        assert_eq!(op.id, OP);
        assert_eq!(op.phase, state);
        assert_eq!(op.requests["clone"], OP);
        assert_eq!(op.requests["worktree"], TASK);
        assert_eq!(op.requests["launch"], REQUEST);
        assert_eq!(op.recorded_state, RecordedState::NeedsReview);
        assert_eq!(page.stop, Stop::ManifestEnd);
        assert_eq!(page.evidence, Evidence::Staged);
    }
}

#[test]
fn terminal_states_are_recorded_outcomes_and_never_current_receipts() {
    for state in ["bound", "stopped", "failed"] {
        let mut f = Fixture::new(vec![creations(state)]);
        let original = f.memory.snapshots.clone();
        for evidence in [Evidence::Staged, Evidence::Drained] {
            f.record.evidence = evidence;
            let page = f.page().unwrap();
            assert_eq!(page.evidence, evidence);
            assert_eq!(page.operations[0].recorded_state, RecordedState::Terminal);
            assert_eq!(page.operations[0].phase, state);
            assert_eq!(f.memory.snapshots, original);
        }
    }
}

#[test]
fn original_task_scopes_and_delivery_ids_do_not_use_current_ui_choices() {
    let mut f = Fixture::new(vec![
        message(sources::Origin::Qt),
        message(sources::Origin::Next),
        start("review"),
    ]);
    let page = f.page().unwrap();
    assert_eq!(page.operations.len(), 3);
    for op in page.operations {
        assert_eq!(op.id, REQUEST);
        assert_eq!(op.requests["delivery"], REQUEST);
        assert_eq!(
            op.scope,
            Scope::Task {
                project: "project-fixture".into(),
                task: TASK.into()
            }
        );
        assert_eq!(op.recorded_state, RecordedState::NeedsReview);
    }
}

#[test]
fn task_start_phases_are_retained_without_converting_imported_state_to_authority() {
    for phase in ["delivering", "review", "started", "released"] {
        let mut f = Fixture::new(vec![start(phase)]);
        let page = f.page().unwrap();
        let op = &page.operations[0];
        assert_eq!(op.phase, phase);
        assert_eq!(op.kind, Kind::TaskStart);
        assert_eq!(
            op.recorded_state,
            if matches!(phase, "started" | "released") {
                RecordedState::Terminal
            } else {
                RecordedState::NeedsReview
            }
        );
    }
}

#[test]
fn summaries_debug_errors_and_cursors_never_copy_private_bodies() {
    let mut f = Fixture::new(vec![
        creations("unknown"),
        start("review"),
        message(sources::Origin::Next),
    ]);
    let projection = f.projection();
    let page = projection
        .page(
            None,
            Limits::new(1, MIN_BYTES).unwrap(),
            &mut f.memory,
            || false,
        )
        .unwrap();
    let rest = projection
        .page(page.next.as_ref(), Limits::default(), &mut f.memory, || {
            false
        })
        .unwrap();
    let summary = format!(
        "{page:?} {rest:?} {:?} {} {}",
        projection.coverage(),
        serde_json::to_string(&rest).unwrap(),
        Error::InvalidEvidence
    );
    for private in [
        PRIVATE,
        "/srv/example-private-checkout",
        "unknown_private",
        "original-conversation",
    ] {
        assert!(!summary.contains(private), "leaked {private}");
    }
}

#[test]
fn manifest_identity_and_digest_must_all_match_before_reading() {
    let f = Fixture::new(vec![creations("unknown")]);
    let bytes = serde_json::to_vec(&f.record).unwrap();
    for (version, digest, config, id) in [
        (2, sha256(&bytes), "a".repeat(64), IMPORT),
        (1, "b".repeat(64), "a".repeat(64), IMPORT),
        (1, sha256(&bytes), "b".repeat(64), IMPORT),
        (1, sha256(&bytes), "a".repeat(64), OP),
    ] {
        assert!(matches!(
            Projection::new(
                &ImportRecordRef {
                    version,
                    sha256: digest
                },
                &bytes,
                &config,
                id
            ),
            Err(Error::InvalidEvidence)
        ));
    }
    let huge = vec![b' '; (256 << 10) + 1];
    assert!(matches!(
        Projection::new(
            &ImportRecordRef {
                version: 1,
                sha256: sha256(&huge)
            },
            &huge,
            &"a".repeat(64),
            IMPORT
        ),
        Err(Error::InvalidEvidence)
    ));
}

#[test]
fn tampered_page_and_body_refuse_instead_of_showing_empty_or_partial_success() {
    for body in [false, true] {
        let mut f = Fixture::new(vec![start("review")]);
        let store = if body {
            &mut f.memory.snapshots
        } else {
            &mut f.memory.pages
        };
        store.values_mut().next().unwrap().push(b' ');
        assert_eq!(f.page(), Err(Error::InvalidEvidence));
    }
    let mut f = Fixture::new(vec![start("review")]);
    f.memory.snapshots.clear();
    assert_eq!(f.page(), Err(Error::Unavailable));
}

#[test]
fn source_binding_and_envelope_scope_are_rechecked_even_for_valid_digests() {
    for field in ["project", "task", "marker_id"] {
        let mut f = Fixture::new(vec![start("review")]);
        f.rewrite_body(0, |v| {
            v[field] = json!(if field == "project" {
                "another-project"
            } else {
                OP
            })
        });
        assert_eq!(f.page(), Err(Error::InvalidEvidence));
    }
    let mut f = Fixture::new(vec![start("review")]);
    f.record.sources[0].sha256 = "b".repeat(64);
    assert_eq!(f.page(), Err(Error::InvalidEvidence));
}

#[test]
fn damaged_creation_and_task_records_remain_unavailable() {
    let mut f = Fixture::new(vec![creations("unknown")]);
    f.rewrite_body(1, |v| v["content"]["id"] = json!(REQUEST));
    assert_eq!(f.page(), Err(Error::InvalidEvidence));
    let mut f = Fixture::new(vec![message(sources::Origin::Next)]);
    f.rewrite_body(0, |v| v["record"]["version"] = json!(2));
    assert_eq!(f.page(), Err(Error::InvalidEvidence));
    let mut f = Fixture::new(vec![start("review")]);
    f.rewrite_body(0, |v| {
        v["record"]["intent"]["plan"]["request"]["request_id"] = json!(OP)
    });
    assert_eq!(f.page(), Err(Error::InvalidEvidence));
}

#[test]
fn cursors_are_exact_manifest_generations_and_never_follow_new_imports() {
    let mut f = Fixture::new(vec![creations("unknown")]);
    let projection = f.projection();
    let first = projection
        .page(
            None,
            Limits::new(1, MIN_BYTES).unwrap(),
            &mut f.memory,
            || false,
        )
        .unwrap();
    let cursor = first.next.unwrap();
    let encoded = serde_json::to_vec(&cursor).unwrap();
    let cursor: Cursor = serde_json::from_slice(&encoded).unwrap();
    let next = projection
        .page(Some(&cursor), Limits::default(), &mut f.memory, || false)
        .unwrap();
    assert_eq!(next.operations.len(), 1);
    assert_eq!(next.operations[0].manifest_page, 1);
    f.record.created_at += 1;
    assert_eq!(
        f.projection()
            .page(Some(&cursor), Limits::default(), &mut f.memory, || false),
        Err(Error::InvalidCursor)
    );
    for index in [2, usize::MAX] {
        let mut invalid = cursor.clone();
        invalid.next_page = index;
        assert_eq!(
            projection.page(Some(&invalid), Limits::default(), &mut f.memory, || false),
            Err(Error::InvalidCursor)
        );
    }
}

#[test]
fn cancellation_can_resume_the_exact_page_without_owning_any_effect() {
    let mut f = Fixture::new(vec![start("review")]);
    let projection = f.projection();
    let mut checks = 0;
    let page = projection
        .page(None, Limits::default(), &mut f.memory, || {
            checks += 1;
            checks == 2
        })
        .unwrap();
    assert_eq!(page.stop, Stop::Cancelled);
    assert!(page.operations.is_empty());
    assert_eq!(f.memory.page_reads, 1);
    assert_eq!(f.memory.body_reads, 0);
    let resumed = projection
        .page(page.next.as_ref(), Limits::default(), &mut f.memory, || {
            false
        })
        .unwrap();
    assert_eq!(resumed.operations[0].id, REQUEST);
    let page = projection
        .page(None, Limits::default(), &mut f.memory, || true)
        .unwrap();
    assert_eq!(page.read_bytes, 0);
    assert_eq!(page.stop, Stop::Cancelled);
}

#[test]
fn byte_budget_limits_reads_and_preserves_stable_order_without_retaining_bodies() {
    let mut operations = serde_json::Map::new();
    let mut ids = Vec::new();
    for n in 1..=4 {
        let id = format!("{n:08x}-1111-4111-8111-111111111111");
        let mut op = operation(&id, "unknown");
        op["private_padding"] = json!("x".repeat(800_000));
        operations.insert(id.clone(), op);
        ids.push(id);
    }
    let bytes = serde_json::to_vec(&json!({"schema":1,"operations":operations})).unwrap();
    let mut f = Fixture::new(vec![
        sources::creations(sources::Origin::Qt, &bytes).unwrap()
    ]);
    let projection = f.projection();
    let first = projection
        .page(
            None,
            Limits::new(MAX_PAGES, MIN_BYTES).unwrap(),
            &mut f.memory,
            || false,
        )
        .unwrap();
    assert_eq!(first.stop, Stop::ByteLimit);
    assert_eq!(first.operations.len(), 2);
    assert!(first.read_bytes <= MIN_BYTES);
    let next = projection
        .page(
            first.next.as_ref(),
            Limits::new(MAX_PAGES, MIN_BYTES).unwrap(),
            &mut f.memory,
            || false,
        )
        .unwrap();
    assert_eq!(next.stop, Stop::ManifestEnd);
    assert_eq!(
        first
            .operations
            .into_iter()
            .chain(next.operations)
            .map(|v| v.id)
            .collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn unsupported_and_missing_domains_are_explicit_and_never_read_as_empty_success() {
    let source = SourceStream::new(
        SourceRecord {
            family: "next-executor-launches".into(),
            schema: 7,
            format: "future-v7".into(),
            sha256: "b".repeat(64),
            build: None,
        },
        std::iter::once(Ok(json!({"secret":PRIVATE}))),
    );
    let mut f = Fixture::new(vec![source]);
    let coverage = f.projection().coverage();
    assert_eq!(coverage.sources[0].support, Support::Unsupported);
    assert_eq!(coverage.unavailable.len(), 10);
    let page = f.page().unwrap();
    assert_eq!(page.unsupported_pages, vec![0]);
    assert_eq!(f.memory.body_reads, 0);
    assert!(page.operations.is_empty());
    let f = Fixture::new(vec![creations("unknown"), start("review")]);
    let coverage = f.projection().coverage();
    assert!(!coverage.unavailable.contains(&Domain::Creations));
    assert!(!coverage.unavailable.contains(&Domain::TaskStarts));
    assert!(coverage.unavailable.contains(&Domain::TaskMessages));
    assert!(coverage
        .unavailable
        .contains(&Domain::PendingExecutorLaunches));
    let mut empty = Fixture::new(vec![]);
    assert_eq!(empty.projection().coverage().unavailable.len(), 10);
    assert_eq!(empty.page().unwrap().stop, Stop::ManifestEnd);
}

#[test]
fn other_owners_evidence_is_not_decoded_or_claimed_by_g2() {
    let source = SourceStream::new(
        SourceRecord {
            family: "next-pending-reads".into(),
            schema: 1,
            format: "pending-reads-json-v1".into(),
            sha256: "b".repeat(64),
            build: None,
        },
        std::iter::once(Ok(json!({"secret":PRIVATE}))),
    );
    let mut f = Fixture::new(vec![source]);
    assert_eq!(
        f.projection().coverage().sources[0].support,
        Support::OtherResponsibility
    );
    assert_eq!(f.page().unwrap().scanned_pages, 1);
    assert_eq!(f.memory.body_reads, 0);
}

#[test]
fn caller_cannot_raise_the_page_or_byte_caps_or_admit_a_nonprogressing_budget() {
    for (pages, bytes) in [
        (0, MIN_BYTES),
        (MAX_PAGES + 1, MIN_BYTES),
        (1, 0),
        (1, MIN_BYTES - 1),
        (1, MAX_BYTES + 1),
    ] {
        assert!(matches!(
            Limits::new(pages, bytes),
            Err(Error::InvalidLimits)
        ));
    }
}

#[test]
fn maximum_page_size_stops_before_reading_the_next_private_body() {
    let operations = (1..=128)
        .map(|n| {
            let id = format!("{n:08x}-1111-4111-8111-111111111111");
            (id.clone(), operation(&id, "unknown"))
        })
        .collect::<serde_json::Map<_, _>>();
    let bytes = serde_json::to_vec(&json!({"schema":1,"operations":operations})).unwrap();
    let mut f = Fixture::new(vec![
        sources::creations(sources::Origin::Next, &bytes).unwrap()
    ]);
    let projection = f.projection();
    let page = projection
        .page(
            None,
            Limits::new(MAX_PAGES, MAX_BYTES).unwrap(),
            &mut f.memory,
            || false,
        )
        .unwrap();
    assert_eq!(page.stop, Stop::PageLimit);
    assert_eq!(page.scanned_pages, MAX_PAGES);
    assert_eq!(page.operations.len(), MAX_PAGES - 1); // private journal header
    assert_eq!(f.memory.page_reads, MAX_PAGES);
    assert_eq!(f.memory.body_reads, MAX_PAGES);
    assert_eq!(page.next.unwrap().next_page, MAX_PAGES);
}

#[test]
fn recognized_names_with_future_schema_or_format_stay_unsupported() {
    for (schema, format) in [(2, "creation-json-v1"), (1, "future-v2")] {
        let source = SourceStream::new(
            SourceRecord {
                family: "next-creations".into(),
                schema,
                format: format.into(),
                sha256: "b".repeat(64),
                build: None,
            },
            std::iter::once(Ok(json!({"secret":PRIVATE}))),
        );
        let mut f = Fixture::new(vec![source]);
        assert_eq!(
            f.projection().coverage().sources[0].support,
            Support::Unsupported
        );
        assert!(f
            .projection()
            .coverage()
            .unavailable
            .contains(&Domain::Creations));
        assert_eq!(f.page().unwrap().unsupported_pages, vec![0]);
        assert_eq!(f.memory.body_reads, 0);
    }
}
