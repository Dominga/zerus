//! Legacy candidate controls plus actual selected-owned-factory supplements.
//! Native positives require the actual NTFS/GUID qualification; supplements
//! never replace the original maintained ordinary-writer/product oracle.
//! Privacy-result mapping controls are PURE; they do not mutate a Windows
//! owner/DACL. No new ACL-changing primitive, forced-ID-reuse primitive or
//! actual actor/store/resident grant is provided by this candidate. Those
//! native/integration qualifications remain open, not silently modeled green.
#![cfg(windows)]
use super::super::coexistence::{self, Seal, Stage};
use super::*;
use crate::state::repositories::directory::stop_capture;
use std::time::{Duration, Instant};

fn until() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
fn private_fixture() -> tempfile::TempDir {
    let base = platform::canonical(std::env::temp_dir()).unwrap();
    let root = tempfile::tempdir_in(base).unwrap();
    platform::private_dir(root.path()).unwrap();
    platform::restrict_dir(root.path()).unwrap();
    drop(writer(root.path()));
    root
}
fn seeded(root: &Path) {
    let guard = writer(root);
    let publisher = Publisher::open_enabled(root, &guard, &executor(), true).unwrap();
    publisher.publish(&fact(2)).unwrap();
}
fn candidate(root: &Path, fresh: bool, aliases: usize) -> Seal {
    let deadline = until();
    match coexistence::fixture_seal(root, executor(), deadline, fresh, aliases) {
        Ok(seal) => seal,
        Err(failed) => {
            let stage = failed.stage();
            // A failed qualification is NOT a skipped/passing positive.
            eprintln!(
                "candidate SETUPREFUSED at {stage:?}, {:?}; no compatibility evidence",
                failed.detail()
            );
            failed.retire();
            panic!("candidate positive not established");
        }
    }
}
fn ordinary_positive(fresh: bool, aliases: usize) {
    let dir = private_fixture();
    if !fresh {
        seeded(dir.path());
    }
    let seal = candidate(dir.path(), fresh, aliases);
    let expected = vec![fact(2)];
    {
        let lease = seal.lease(&executor()).unwrap();
        assert_eq!(lease.read(|facts| facts.to_vec()).unwrap(), expected);
        lease.final_check().unwrap();
    }
    // A real second owning native Writer, not a writer_released boolean.
    let next = crate::state::task_controls::writer::test_open(dir.path(), until()).unwrap();
    next.verify().unwrap();
    drop(next);
    let fact_path = path(dir.path(), &executor());
    let fact_bytes = fs::read(&fact_path).unwrap();
    let lock_bytes = fs::read(dir.path().join(".lock")).unwrap();
    let record = dir.path().join("run.json");
    // Exact ordinary source and backend, no substitute publisher or retry.
    storage::atomic(&record, r#"{"state":"live"}"#).unwrap();
    assert_eq!(fs::read_to_string(&record).unwrap(), r#"{"state":"live"}"#);
    storage::atomic(&record, r#"{"state":"active"}"#).unwrap();
    assert_eq!(
        fs::read_to_string(&record).unwrap(),
        r#"{"state":"active"}"#
    );
    let lease = seal.lease(&executor()).unwrap();
    assert_eq!(lease.read(|facts| facts.to_vec()).unwrap(), expected);
    assert_eq!(fs::read(&fact_path).unwrap(), fact_bytes);
    assert_eq!(fs::read(dir.path().join(".lock")).unwrap(), lock_bytes);
    lease.final_check().unwrap();
    drop(lease);
    seal.retire();
}
#[test]
fn candidate_fresh_seal_keeps_original_ordinary_new_and_replacement_writes() {
    ordinary_positive(true, 0);
}
#[test]
fn candidate_reopened_seal_keeps_original_ordinary_new_and_replacement_writes() {
    ordinary_positive(false, 0);
}
#[test]
fn candidate_all_explicit_owned_root_aliases_settle_before_ordinary_writes() {
    ordinary_positive(false, 2);
}

#[test]
fn retained_native_writer_is_not_accepted_as_a_ready_handoff() {
    let dir = private_fixture();
    seeded(dir.path());
    let deadline = until();
    coexistence::fault(Some(Stage::Retirement));
    let outcome = coexistence::fixture_seal(dir.path(), executor(), deadline, false, 0);
    coexistence::fault(None);
    let failed = match outcome {
        Err(f) => f,
        Ok(s) => {
            s.retire();
            panic!("held native writer became ready")
        }
    };
    assert_eq!(failed.stage(), Stage::Retirement);
    assert_eq!(coexistence::retained(&failed), (true, true, true, true));
    assert!(crate::state::task_controls::writer::test_open(
        dir.path(),
        Instant::now() + Duration::from_millis(15)
    )
    .is_err());
    failed.retire();
    crate::state::task_controls::writer::test_open(dir.path(), until())
        .unwrap()
        .verify()
        .unwrap();
}
#[test]
fn omitted_original_root_alias_never_produces_a_ready_seal() {
    let dir = private_fixture();
    seeded(dir.path());
    let deadline = until();
    stop_capture::fault_omit_root(true);
    let result = coexistence::fixture_seal(dir.path(), executor(), deadline, false, 2);
    stop_capture::fault_omit_root(false);
    let failed = match result {
        Err(f) => f,
        Ok(s) => {
            s.retire();
            panic!("incomplete root cohort was accepted")
        }
    };
    assert_eq!(failed.stage(), Stage::Conversion);
    assert_eq!(coexistence::retained(&failed), (false, true, true, true));
    failed.retire();
    crate::state::task_controls::writer::test_open(dir.path(), until())
        .unwrap()
        .verify()
        .unwrap();
}
#[test]
fn every_conversion_cut_keeps_data_custody_until_explicit_retirement() {
    for stage in [
        Stage::OriginalWriter,
        Stage::Publisher,
        Stage::Fact,
        Stage::Snapshot,
        Stage::WriterObject,
        Stage::Namespace,
        Stage::Keeper,
        Stage::Retirement,
        Stage::Conversion,
        Stage::Handoff,
    ] {
        let dir = private_fixture();
        seeded(dir.path());
        let deadline = until();
        let bytes = fs::read(path(dir.path(), &executor())).unwrap();
        coexistence::fault(Some(stage));
        let result = coexistence::fixture_seal(dir.path(), executor(), deadline, false, 0);
        coexistence::fault(None);
        let failed = match result {
            Err(f) => f,
            Ok(s) => {
                s.retire();
                panic!("conversion cut did not refuse")
            }
        };
        assert_eq!(
            failed.stage(),
            stage,
            "an earlier real failure is an unestablished cut"
        );
        let retained = coexistence::retained(&failed);
        if !matches!(stage, Stage::OriginalWriter | Stage::Publisher) {
            assert!(retained.1);
        }
        if matches!(
            stage,
            Stage::WriterObject
                | Stage::Namespace
                | Stage::Keeper
                | Stage::Retirement
                | Stage::Conversion
                | Stage::Handoff
        ) {
            assert!(retained.2);
        }
        assert_eq!(fs::read(path(dir.path(), &executor())).unwrap(), bytes);
        failed.retire();
        crate::state::task_controls::writer::test_open(dir.path(), until())
            .unwrap()
            .verify()
            .unwrap();
    }
}
#[test]
fn active_current_lease_retains_original_bounded_writer_refusal() {
    let dir = private_fixture();
    seeded(dir.path());
    let seal = candidate(dir.path(), false, 0);
    let lease = seal.lease(&executor()).unwrap();
    assert_eq!(lease.read(|facts| facts.to_vec()).unwrap(), [fact(2)]);
    assert!(storage::atomic(&dir.path().join("run.json"), r#"{"state":"live"}"#).is_err());
    lease.final_check().unwrap();
    drop(lease);
    storage::atomic(&dir.path().join("run.json"), r#"{"state":"live"}"#).unwrap();
    seal.lease(&executor()).unwrap().final_check().unwrap();
    seal.retire();
}
#[test]
fn same_phase_legacy_capture_conflict_remains_a_separate_negative() {
    let dir = private_fixture();
    let guard = writer(dir.path());
    let p = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
    p.publish(&fact(2)).unwrap();
    let captured = p.capture().unwrap();
    drop(guard);
    captured.verify().unwrap();
    assert!(storage::atomic(&dir.path().join("run.json"), r#"{"state":"live"}"#).is_err());
    captured.verify().unwrap();
}
#[test]
fn same_size_fact_change_refuses_without_repair_or_failure_time_rebase() {
    let dir = private_fixture();
    seeded(dir.path());
    let seal = candidate(dir.path(), false, 0);
    let file = path(dir.path(), &executor());
    let original = fs::read(&file).unwrap();
    let mut changed = original.clone();
    let at = changed.iter().position(|b| *b == b'2').unwrap();
    changed[at] = b'3';
    fs::write(&file, &changed).unwrap();
    assert!(seal.lease(&executor()).is_err());
    assert_eq!(fs::read(&file).unwrap(), changed);
    fs::write(&file, &original).unwrap();
    assert!(seal.lease(&executor()).is_err()); // poisoned, never rebase
    seal.retire();
}
#[test]
fn malformed_oversized_removed_and_multilink_fact_never_gain_current_authority() {
    for mode in ["malformed", "oversized", "removed", "multilink"] {
        let dir = private_fixture();
        seeded(dir.path());
        let seal = candidate(dir.path(), false, 0);
        let file = path(dir.path(), &executor());
        match mode {
            "malformed" => fs::write(&file, b"{broken").unwrap(),
            "oversized" => fs::write(&file, vec![b' '; BYTES + 1]).unwrap(),
            "removed" => {
                if fs::remove_file(&file).is_err() {
                    record(
                        ReportCase::LegacyRemoved,
                        ReportOutcome::Unknown,
                        ReportReason::SetupRefused,
                    )
                    .unwrap();
                    continue;
                }
            }
            _ => {
                if fs::hard_link(&file, file.with_extension("link")).is_err() {
                    record(
                        ReportCase::LegacyMultiLink,
                        ReportOutcome::Unknown,
                        ReportReason::SetupRefused,
                    )
                    .unwrap();
                    continue;
                }
            }
        }
        let before = fs::read(&file).ok();
        assert!(seal.lease(&executor()).is_err());
        assert_eq!(fs::read(&file).ok(), before);
        if mode == "removed" || mode == "multilink" {
            let case = if mode == "removed" {
                ReportCase::LegacyRemoved
            } else {
                ReportCase::LegacyMultiLink
            };
            record(
                case,
                ReportOutcome::ExercisedRefusal,
                ReportReason::Complete,
            )
            .unwrap();
        }
        seal.retire();
    }
}
#[test]
fn equal_byte_replacement_and_changed_writer_refuse_if_the_fault_can_be_established() {
    for lock in [false, true] {
        let dir = private_fixture();
        seeded(dir.path());
        let seal = candidate(dir.path(), false, 0);
        let file = if lock {
            dir.path().join(".lock")
        } else {
            path(dir.path(), &executor())
        };
        let bytes = fs::read(&file).unwrap();
        if fs::rename(&file, file.with_extension("kept")).is_err() {
            let case = if lock {
                ReportCase::LegacyWriter
            } else {
                ReportCase::LegacyEqualFact
            };
            record(case, ReportOutcome::Unknown, ReportReason::SetupRefused).unwrap();
            continue;
        }
        platform::private_open(OpenOptions::new().write(true).create_new(true))
            .open(&file)
            .unwrap();
        fs::write(&file, &bytes).unwrap();
        assert!(seal.lease(&executor()).is_err());
        assert_eq!(fs::read(&file).unwrap(), bytes);
        let case = if lock {
            ReportCase::LegacyWriter
        } else {
            ReportCase::LegacyEqualFact
        };
        record(
            case,
            ReportOutcome::ExercisedRefusal,
            ReportReason::Complete,
        )
        .unwrap();
        seal.retire();
    }
}
#[test]
fn facts_directory_rename_or_reparse_never_retargets_a_lease() {
    let dir = private_fixture();
    seeded(dir.path());
    let seal = candidate(dir.path(), false, 0);
    let facts = dir.path().join("task-stop-facts");
    let kept = dir.path().join("kept-facts");
    if fs::rename(&facts, &kept).is_err() {
        record(
            ReportCase::LegacyFacts,
            ReportOutcome::Unknown,
            ReportReason::SetupRefused,
        )
        .unwrap();
        return;
    }
    if std::os::windows::fs::symlink_dir(&kept, &facts).is_err() {
        record(
            ReportCase::LegacyFacts,
            ReportOutcome::Unknown,
            ReportReason::ReparseUnavailable,
        )
        .unwrap();
        fs::rename(&kept, &facts).unwrap();
        return;
    }
    assert!(seal.lease(&executor()).is_err());
    record(
        ReportCase::LegacyFacts,
        ReportOutcome::ExercisedRefusal,
        ReportReason::Complete,
    )
    .unwrap();
    seal.retire();
    fs::remove_dir(&facts).unwrap();
    fs::rename(&kept, &facts).unwrap();
}
#[test]
fn an_exact_different_executor_never_uses_an_original_seal() {
    for field in 0..6 {
        let dir = private_fixture();
        seeded(dir.path());
        let seal = candidate(dir.path(), false, 0);
        let mut other = executor();
        match field {
            0 => other.run = "resumed-run".into(),
            1 => other.birth = "different-birth".into(),
            2 => other.session_id = id(9001),
            3 => other.computer = id(9002),
            4 => other.conversation = Some("different-conversation".into()),
            _ => other.child = Some("different-child".into()),
        }
        assert!(seal.lease(&other).is_err());
        assert!(seal.lease(&executor()).is_err());
        seal.retire();
    }
}
#[test]
fn a_failed_final_check_poisoned_lease_cannot_recover_or_escape_as_a_new_current_lease() {
    let dir = private_fixture();
    seeded(dir.path());
    let seal = candidate(dir.path(), false, 0);
    let lease = seal.lease(&executor()).unwrap();
    assert_eq!(lease.read(|facts| facts.to_vec()).unwrap(), [fact(2)]);
    coexistence::fault(Some(Stage::Final));
    assert!(lease.final_check().is_err());
    coexistence::fault(None);
    assert!(lease.read(|facts| facts.to_vec()).is_err());
    drop(lease);
    assert!(seal.lease(&executor()).is_err());
    seal.retire();
}
#[test]
fn an_expired_capture_retains_failed_owner_custody_and_never_renews_its_budget() {
    let dir = private_fixture();
    seeded(dir.path());
    let result = coexistence::fixture_seal(
        dir.path(),
        executor(),
        Instant::now() - Duration::from_secs(1),
        false,
        0,
    );
    let failed = match result {
        Err(f) => f,
        Ok(s) => {
            s.retire();
            panic!("expired capture became ready")
        }
    };
    assert_eq!(failed.stage(), Stage::OriginalWriter);
    failed.retire();
    crate::state::task_controls::writer::test_open(dir.path(), until())
        .unwrap()
        .verify()
        .unwrap();
}

#[test]
fn dormant_default_entry_cannot_open_or_initialize_a_missing_root() {
    let dir = private_fixture();
    let missing = dir.path().join("missing");
    let mut invalid = executor();
    invalid.birth.clear();
    let result = coexistence::seal(&missing, invalid, until());
    let failed = match result {
        Err(f) => f,
        Ok(s) => {
            s.retire();
            panic!("false-gate candidate became ready")
        }
    };
    assert_eq!(failed.stage(), Stage::OriginalWriter);
    assert_eq!(coexistence::retained(&failed), (false, false, false, false));
    assert!(!missing.exists());
    failed.retire();
}
#[test]
fn an_unbounded_alias_request_refuses_without_creating_an_authority_baseline() {
    let dir = private_fixture();
    seeded(dir.path());
    let result = coexistence::fixture_seal(dir.path(), executor(), until(), false, 3);
    let failed = match result {
        Err(f) => f,
        Ok(s) => {
            s.retire();
            panic!("unbounded cohort accepted")
        }
    };
    assert_eq!(failed.stage(), Stage::Namespace);
    assert_eq!(coexistence::retained(&failed), (true, false, false, false));
    failed.retire();
    crate::state::task_controls::writer::test_open(dir.path(), until())
        .unwrap()
        .verify()
        .unwrap();
}
#[test]
fn renamed_root_is_never_authority_for_the_old_current_name() {
    let outer = private_fixture();
    let root = outer.path().join("native-root");
    fs::create_dir(&root).unwrap();
    drop(writer(&root));
    seeded(&root);
    let seal = candidate(&root, false, 0);
    let kept = outer.path().join("kept-native-root");
    if fs::rename(&root, &kept).is_err() {
        record(
            ReportCase::LegacyRoot,
            ReportOutcome::Unknown,
            ReportReason::SetupRefused,
        )
        .unwrap();
        return;
    }
    fs::create_dir(&root).unwrap();
    assert!(seal.lease(&executor()).is_err());
    record(
        ReportCase::LegacyRoot,
        ReportOutcome::ExercisedRefusal,
        ReportReason::Complete,
    )
    .unwrap();
    seal.retire();
    fs::remove_dir(&root).unwrap();
    fs::rename(&kept, &root).unwrap();
}
#[test]
fn a_redirected_ancestor_never_retargets_the_selected_root() {
    let outer = private_fixture();
    let ancestor = outer.path().join("parent");
    let root = ancestor.join("native-root");
    fs::create_dir_all(&root).unwrap();
    drop(writer(&root));
    seeded(&root);
    let seal = candidate(&root, false, 0);
    let kept = outer.path().join("kept-parent");
    if fs::rename(&ancestor, &kept).is_err() {
        record(
            ReportCase::LegacyAncestor,
            ReportOutcome::Unknown,
            ReportReason::SetupRefused,
        )
        .unwrap();
        return;
    }
    if std::os::windows::fs::symlink_dir(&kept, &ancestor).is_err() {
        record(
            ReportCase::LegacyAncestor,
            ReportOutcome::Unknown,
            ReportReason::ReparseUnavailable,
        )
        .unwrap();
        fs::rename(&kept, &ancestor).unwrap();
        return;
    }
    assert!(seal.lease(&executor()).is_err());
    record(
        ReportCase::LegacyAncestor,
        ReportOutcome::ExercisedRefusal,
        ReportReason::Complete,
    )
    .unwrap();
    seal.retire();
    fs::remove_dir(&ancestor).unwrap();
    fs::rename(&kept, &ancestor).unwrap();
}

#[test]
fn a_panicking_read_poisoned_the_seal_and_keeps_custody_until_retirement() {
    let dir = private_fixture();
    seeded(dir.path());
    let seal = candidate(dir.path(), false, 0);
    let lease = seal.lease(&executor()).unwrap();
    let result: std::result::Result<(), Stage> = lease.read(|_| panic!("private read fixture"));
    assert_eq!(result, Err(Stage::Final));
    assert!(lease.final_check().is_err());
    drop(lease);
    assert!(seal.lease(&executor()).is_err());
    seal.retire();
    crate::state::task_controls::writer::test_open(dir.path(), until())
        .unwrap()
        .verify()
        .unwrap();
}

#[test]
fn overflow_keeps_exact_original_data_and_refuses_a_ready_handoff() {
    let dir = private_fixture();
    {
        let guard = writer(dir.path());
        let p = Publisher::open_enabled(dir.path(), &guard, &executor(), true).unwrap();
        for n in 2..=66 {
            p.publish(&fact(n)).unwrap();
        }
    }
    let file = path(dir.path(), &executor());
    let bytes = fs::read(&file).unwrap();
    let result = coexistence::fixture_seal(dir.path(), executor(), until(), false, 0);
    let failed = match result {
        Err(f) => f,
        Ok(s) => {
            s.retire();
            panic!("overflow became a ready proof")
        }
    };
    assert_eq!(failed.stage(), Stage::Snapshot);
    assert_eq!(coexistence::retained(&failed), (true, true, false, false));
    assert_eq!(fs::read(&file).unwrap(), bytes);
    failed.retire();
    crate::state::task_controls::writer::test_open(dir.path(), until())
        .unwrap()
        .verify()
        .unwrap();
}

// Fixed PUBLIC-SAFE test result channel. These rows contain no runtime path,
// handle, volume, GUID, ACL/SID, environment value or fact content. The SINGLE
// process budget has no reset API. Local budgets in pure tests are models only.
#[derive(Clone, Copy)]
pub(super) enum ReportCase {
    SelectedOriginal,
    SelectedCallback,
    LegacyRemoved,
    LegacyMultiLink,
    LegacyEqualFact,
    LegacyWriter,
    LegacyFacts,
    LegacyRoot,
    LegacyAncestor,
    Channel,
}
#[derive(Clone, Copy)]
pub(super) enum ReportOutcome {
    Verified,
    ExercisedRefusal,
    Unknown,
    Incomplete,
}
#[derive(Clone, Copy)]
pub(super) enum ReportReason {
    Complete,
    FinalRecheck,
    SetupRefused,
    ReparseUnavailable,
    WriteUnconfirmed,
    BoundExceeded,
}
impl ReportCase {
    fn tag(self) -> &'static str {
        match self {
            Self::SelectedOriginal => "selected-original",
            Self::SelectedCallback => "selected-callback",
            Self::LegacyRemoved => "legacy-removed",
            Self::LegacyMultiLink => "legacy-multilink",
            Self::LegacyEqualFact => "legacy-equal-fact",
            Self::LegacyWriter => "legacy-writer",
            Self::LegacyFacts => "legacy-facts",
            Self::LegacyRoot => "legacy-root",
            Self::LegacyAncestor => "legacy-ancestor",
            Self::Channel => "channel",
        }
    }
}
impl ReportOutcome {
    fn tag(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::ExercisedRefusal => "exercised-refusal",
            Self::Unknown => "unknown",
            Self::Incomplete => "incomplete",
        }
    }
}
impl ReportReason {
    fn tag(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::FinalRecheck => "final-recheck",
            Self::SetupRefused => "setup-refused",
            Self::ReparseUnavailable => "reparse-unavailable",
            Self::WriteUnconfirmed => "write-unconfirmed",
            Self::BoundExceeded => "bound-exceeded",
        }
    }
}
const REPORT_ROWS: usize = 16;
const REPORT_BYTES: usize = 256;
const REPORT_TOTAL: usize = REPORT_ROWS * REPORT_BYTES;
fn encoded_record(case: ReportCase, outcome: ReportOutcome, reason: ReportReason) -> Vec<u8> {
    format!("Zerus capture result: {{\"schema\":1,\"case\":\"{}\",\"outcome\":\"{}\",\"reason\":\"{}\"}}\n",case.tag(),outcome.tag(),reason.tag()).into_bytes()
}
#[derive(Default)]
struct ReportBudget {
    rows: usize,
    bytes: usize,
    incomplete: bool,
}
impl ReportBudget {
    fn emit(&mut self, out: &mut impl std::io::Write, bytes: &[u8]) -> std::io::Result<()> {
        if self.incomplete {
            return Err(std::io::Error::other("capture report remains incomplete"));
        }
        // Keep ONE slot for an explicit incomplete sentinel. It counts toward
        // the same16row/4KiB cap, not an unbudgeted overflow/reset message.
        if bytes.len() > REPORT_BYTES
            || self.rows >= REPORT_ROWS - 1
            || self.bytes + bytes.len() > (REPORT_ROWS - 1) * REPORT_BYTES
        {
            self.incomplete = true;
            let sentinel = encoded_record(
                ReportCase::Channel,
                ReportOutcome::Incomplete,
                ReportReason::BoundExceeded,
            );
            if self.rows < REPORT_ROWS
                && sentinel.len() <= REPORT_BYTES
                && self.bytes + sentinel.len() <= REPORT_TOTAL
            {
                self.rows += 1;
                self.bytes += sentinel.len();
                out.write_all(&sentinel)?;
                out.flush()?;
            }
            return Err(std::io::Error::other("capture report bound exceeded"));
        }
        self.rows += 1;
        self.bytes += bytes.len(); // reserve attempted output too
        if out.write_all(bytes).and_then(|()| out.flush()).is_err() {
            self.incomplete = true;
            return Err(std::io::Error::other("capture result output failed"));
        }
        Ok(())
    }
}
static PROCESS_REPORT: std::sync::Mutex<ReportBudget> = std::sync::Mutex::new(ReportBudget {
    rows: 0,
    bytes: 0,
    incomplete: false,
});
pub(super) fn record(
    case: ReportCase,
    outcome: ReportOutcome,
    reason: ReportReason,
) -> std::io::Result<()> {
    let row = encoded_record(case, outcome, reason);
    let mut budget = PROCESS_REPORT
        .lock()
        .map_err(|_| std::io::Error::other("capture report state poisoned"))?;
    budget.emit(&mut std::io::stdout().lock(), &row)
}

#[cfg(test)]
mod report_controls {
    use super::*;
    #[derive(Default)]
    struct Output {
        bytes: Vec<u8>,
        flushes: usize,
        fail: bool,
    }
    impl std::io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.fail {
                return Err(std::io::Error::other("private output fixture"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }
    #[test]
    fn closed_records_are_bounded_valid_and_flushed_without_a_global_reset() {
        let row = encoded_record(
            ReportCase::SelectedOriginal,
            ReportOutcome::Verified,
            ReportReason::Complete,
        );
        let mut budget = ReportBudget::default();
        let mut out = Output::default();
        budget.emit(&mut out, &row).unwrap();
        assert_eq!(out.flushes, 1);
        assert!(row.len() <= REPORT_BYTES);
        let json = row.strip_prefix(b"Zerus capture result: ").unwrap();
        let value: serde_json::Value = serde_json::from_slice(json).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert_eq!(value["case"], "selected-original");
        assert_eq!(budget.rows, 1);
        assert_eq!(budget.bytes, row.len());
    }
    #[test]
    fn overflow_emits_one_budgeted_incomplete_sentinel_and_never_resets() {
        let row = encoded_record(
            ReportCase::LegacyRoot,
            ReportOutcome::Unknown,
            ReportReason::SetupRefused,
        );
        let mut budget = ReportBudget::default();
        let mut out = Output::default();
        for _ in 0..REPORT_ROWS - 1 {
            budget.emit(&mut out, &row).unwrap();
        }
        assert!(budget.emit(&mut out, &row).is_err());
        assert!(budget.incomplete);
        assert_eq!(budget.rows, REPORT_ROWS);
        assert!(budget.bytes <= REPORT_TOTAL);
        assert!(out.bytes.len() <= REPORT_TOTAL);
        let old = out.bytes.clone();
        assert!(budget.emit(&mut out, &row).is_err());
        assert_eq!(out.bytes, old);
        assert!(String::from_utf8(old).unwrap().contains("incomplete"));
    }
    #[test]
    fn byte_overflow_or_output_failure_is_incomplete_never_silently_truncated() {
        let mut b = ReportBudget::default();
        let mut out = Output::default();
        assert!(b.emit(&mut out, &[b'x'; REPORT_BYTES + 1]).is_err());
        assert!(b.incomplete);
        assert!(!out.bytes.windows(3).any(|x| x == b"xxx"));
        let mut b = ReportBudget::default();
        let mut out = Output {
            fail: true,
            ..Output::default()
        };
        let row = encoded_record(
            ReportCase::SelectedCallback,
            ReportOutcome::ExercisedRefusal,
            ReportReason::FinalRecheck,
        );
        assert!(b.emit(&mut out, &row).is_err());
        assert!(b.incomplete);
        assert_eq!(b.rows, 1);
        out.fail = false;
        assert!(b.emit(&mut out, &row).is_err());
        assert!(out.bytes.is_empty());
    }
}

// NEW SELECTED-PATH CONTROLS. Unlike the legacy candidate comparison helpers
// above, every capture below calls the admitted task_controls::capture_owned_stops.
fn selected_fixture() -> tempfile::TempDir {
    let base = platform::canonical(std::env::temp_dir()).unwrap();
    let dir = tempfile::tempdir_in(base).unwrap();
    platform::private_dir(dir.path()).unwrap();
    platform::restrict_dir(dir.path()).unwrap();
    let empty = platform::private_open(OpenOptions::new().read(true).write(true).create_new(true))
        .open(dir.path().join(".lock"))
        .unwrap();
    assert_eq!(empty.metadata().unwrap().len(), 0);
    drop(empty); // UNLOCKED fixture initialization
    dir
}
fn selected_owner(root: &Path, deadline: Instant) -> crate::state::task_controls::writer::Writer {
    crate::state::task_controls::writer::test_open(root, deadline)
        .unwrap_or_else(|_| panic!("selected original Writer setup failed"))
}
fn selected_seed(owned: &crate::state::task_controls::writer::Writer, reopened: bool) {
    {
        let p = Publisher::open_enabled(
            owned.root().unwrap(),
            owned.guard().unwrap(),
            &executor(),
            true,
        )
        .unwrap();
        p.publish(&fact(2)).unwrap();
    }
    if reopened {
        let p = Publisher::open_enabled(
            owned.root().unwrap(),
            owned.guard().unwrap(),
            &executor(),
            false,
        )
        .unwrap();
        assert_eq!(p.snapshot().unwrap().facts, [fact(2)]);
    } // all Publishers borrowing this SAME owner end before its move
}
fn selected_ready(
    root: &Path,
    reopened: bool,
) -> (crate::state::task_controls::SelectedCapture, Instant) {
    let deadline = until();
    let owned = selected_owner(root, deadline);
    selected_seed(&owned, reopened);
    let capture = crate::state::task_controls::capture_owned_stops(owned, &executor()).unwrap();
    (capture, deadline)
}
#[test]
fn selected_factory_new_and_replacement_keep_exact_evidence_and_original_owner_retirement() {
    for reopened in [false, true] {
        let dir = selected_fixture();
        let (capture, deadline) = selected_ready(dir.path(), reopened);
        {
            let lease = capture.lease(&executor()).unwrap();
            let (scope, facts) = lease
                .read_scoped(|ex, facts| (ex.clone(), facts.to_vec()))
                .unwrap();
            assert_eq!(scope, executor());
            assert_eq!(facts, [fact(2)]);
            lease.final_check().unwrap();
        }
        let next = selected_owner(dir.path(), deadline);
        next.verify().unwrap();
        {
            let lease = capture.lease(&executor()).unwrap();
            lease.final_check().unwrap();
        }
        drop(next);
        let fact_file = path(dir.path(), &executor());
        let bytes = fs::read(&fact_file).unwrap();
        let lock = fs::read(dir.path().join(".lock")).unwrap();
        let record = dir.path().join("run.json");
        assert!(!record.exists());
        storage::atomic(&record, r#"{"state":"live"}"#).unwrap();
        assert_eq!(fs::read_to_string(&record).unwrap(), r#"{"state":"live"}"#);
        storage::atomic(&record, r#"{"state":"active"}"#).unwrap();
        assert_eq!(
            fs::read_to_string(&record).unwrap(),
            r#"{"state":"active"}"#
        );
        let lease = capture.lease(&executor()).unwrap();
        assert_eq!(
            lease.read_scoped(|_, facts| facts.to_vec()).unwrap(),
            [fact(2)]
        );
        assert_eq!(fs::read(&fact_file).unwrap(), bytes);
        assert_eq!(fs::read(dir.path().join(".lock")).unwrap(), lock);
        lease.final_check().unwrap();
        drop(lease);
        capture.retire();
    }
}
#[test]
fn selected_factory_never_creates_known_run_history_and_keeps_failed_original_owner() {
    let dir = selected_fixture();
    let owned = selected_owner(dir.path(), until());
    let failed = match crate::state::task_controls::capture_owned_stops(owned, &executor()) {
        Ok(c) => {
            c.retire();
            panic!("missing history became a selected capture")
        }
        Err(f) => f,
    };
    assert!(!dir.path().join("task-stop-facts").exists());
    assert!(crate::state::task_controls::writer::test_open(
        dir.path(),
        Instant::now() + Duration::from_millis(15)
    )
    .is_err());
    failed.retire();
    selected_owner(dir.path(), until()).verify().unwrap();
}
#[test]
fn selected_factory_keeps_damaged_existing_bytes_for_explicit_failure_retirement() {
    for mode in ["malformed", "oversized"] {
        let dir = selected_fixture();
        let owned = selected_owner(dir.path(), until());
        selected_seed(&owned, false);
        let file = path(dir.path(), &executor());
        let bytes = if mode == "malformed" {
            b"{broken".to_vec()
        } else {
            vec![b' '; BYTES + 1]
        };
        fs::write(&file, &bytes).unwrap();
        let failed = match crate::state::task_controls::capture_owned_stops(owned, &executor()) {
            Ok(c) => {
                c.retire();
                panic!("damaged history became a selected capture")
            }
            Err(f) => f,
        };
        assert_eq!(fs::read(&file).unwrap(), bytes);
        assert!(crate::state::task_controls::writer::test_open(
            dir.path(),
            Instant::now() + Duration::from_millis(15)
        )
        .is_err());
        failed.retire();
        selected_owner(dir.path(), until()).verify().unwrap();
    }
}
#[test]
fn selected_factory_retirement_fault_never_drops_the_original_lock_early() {
    let dir = selected_fixture();
    let owned = selected_owner(dir.path(), until());
    selected_seed(&owned, false);
    let bytes = fs::read(path(dir.path(), &executor())).unwrap();
    coexistence::fault(Some(Stage::Retirement));
    let result = crate::state::task_controls::capture_owned_stops(owned, &executor());
    coexistence::fault(None);
    let failed = match result {
        Ok(c) => {
            c.retire();
            panic!("retirement cut became ready")
        }
        Err(f) => f,
    };
    assert!(crate::state::task_controls::writer::test_open(
        dir.path(),
        Instant::now() + Duration::from_millis(15)
    )
    .is_err());
    assert_eq!(fs::read(path(dir.path(), &executor())).unwrap(), bytes);
    failed.retire();
    selected_owner(dir.path(), until()).verify().unwrap();
}
#[test]
fn selected_factory_original_deadline_still_expires_after_the_owner_has_retired() {
    let dir = selected_fixture();
    let deadline = Instant::now() + Duration::from_secs(2);
    let owned = selected_owner(dir.path(), deadline);
    selected_seed(&owned, false);
    let capture = crate::state::task_controls::capture_owned_stops(owned, &executor()).unwrap();
    capture.lease(&executor()).unwrap().final_check().unwrap();
    // Wait ONLY the original stored budget; never construct a replacement
    // owner or pass a later Instant into the selected capture/lease API.
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
    );
    assert!(capture.lease(&executor()).is_err());
    capture.retire();
}
#[test]
fn selected_factory_final_failure_poisoning_cannot_be_rebased() {
    let dir = selected_fixture();
    let (capture, _) = selected_ready(dir.path(), false);
    let lease = capture.lease(&executor()).unwrap();
    assert_eq!(
        lease
            .read_scoped(|ex, facts| (ex.clone(), facts.to_vec()))
            .unwrap(),
        (executor(), vec![fact(2)])
    );
    coexistence::fault(Some(Stage::Final));
    assert!(lease.final_check().is_err());
    coexistence::fault(None);
    assert!(lease.read_scoped(|_, facts| facts.len()).is_err());
    drop(lease);
    assert!(capture.lease(&executor()).is_err());
    capture.retire();
}
#[test]
fn selected_actual_callback_time_same_object_mutation_forces_final_refusal_without_sentinel() {
    let dir = selected_fixture();
    let (capture, deadline) = selected_ready(dir.path(), false);
    let file = path(dir.path(), &executor());
    let original = fs::read(&file).unwrap();
    let before = platform::FileIdentity::at(&file).unwrap().0;
    let mut changed = original.clone();
    let at = changed.iter().position(|b| *b == b'2').unwrap();
    changed[at] = b'3';
    assert_eq!(original.len(), changed.len());
    let lease = capture.lease(&executor()).unwrap();
    let view = lease
        .read_scoped(|ex, facts| (ex.clone(), facts.to_vec()))
        .unwrap();
    assert_eq!(view, (executor(), vec![fact(2)]));
    let entered = std::cell::Cell::new(false);
    let confirmed = std::cell::Cell::new(false);
    assert!(Instant::now() < deadline);
    let result = lease.read_scoped(|_, _| {
        entered.set(true);
        // An IO error can follow partial mutation: only a complete same-object
        // write+exact readback can establish this required native witness.
        if fs::write(&file, &changed).is_ok() {
            confirmed.set(matches!(
                (platform::FileIdentity::at(&file), fs::read(&file)),
                (Ok((after, _)), Ok(bytes))
                    if after.device == before.device
                        && after.inode == before.inode
                        && bytes == changed
            ));
        }
        0x5e11_u32 // must NOT escape after a confirmed mutation
    });
    assert!(entered.get(), "valid selected callback was never entered");
    if !confirmed.get() {
        record(
            ReportCase::SelectedCallback,
            ReportOutcome::Unknown,
            ReportReason::WriteUnconfirmed,
        )
        .unwrap();
        drop(lease);
        capture.retire();
        return;
    }
    assert!(
        Instant::now() < deadline,
        "deadline exhaustion is not a final-byte-check witness"
    );
    assert!(
        result.is_err(),
        "sentinel escaped after actual callback-time mutation"
    );
    assert!(lease.final_check().is_err());
    assert!(lease.read_scoped(|_, facts| facts.len()).is_err());
    drop(lease);
    assert!(capture.lease(&executor()).is_err());
    assert_eq!(fs::read(&file).unwrap(), changed);
    record(
        ReportCase::SelectedCallback,
        ReportOutcome::ExercisedRefusal,
        ReportReason::FinalRecheck,
    )
    .unwrap();
    capture.retire();
}

#[test]
fn selected_factory_omitted_cohort_refuses_ready_and_physically_retires_the_original_owner() {
    let dir = selected_fixture();
    let deadline = until();
    let owned = selected_owner(dir.path(), deadline);
    selected_seed(&owned, false);
    let bytes = fs::read(path(dir.path(), &executor())).unwrap();
    stop_capture::fault_omit_root(true);
    let result = crate::state::task_controls::capture_owned_stops(owned, &executor());
    stop_capture::fault_omit_root(false);
    let failed = match result {
        Ok(c) => {
            c.retire();
            panic!("omitted selected cohort became ready")
        }
        Err(f) => f,
    };
    // This fault happens AFTER the actual original Writer is dropped. Failure
    // custody keeps evidence/cohort pins, not a fabricated released Boolean.
    selected_owner(dir.path(), deadline).verify().unwrap();
    assert_eq!(fs::read(path(dir.path(), &executor())).unwrap(), bytes);
    failed.retire();
}
#[test]
fn selected_factory_overflow_never_yields_empty_history_or_a_ready_lease() {
    let dir = selected_fixture();
    let owned = selected_owner(dir.path(), until());
    selected_seed(&owned, false);
    let overflow = fact(66);
    let snapshot = Snapshot {
        schema: 1,
        executor: executor(),
        facts: (2..=65).map(fact).collect(),
        overflow: Some(Overflow {
            id: overflow.id.clone(),
            original: overflow,
        }),
    };
    let bytes = serde_json::to_vec(&snapshot).unwrap();
    let file = path(dir.path(), &executor());
    fs::write(&file, &bytes).unwrap();
    let failed = match crate::state::task_controls::capture_owned_stops(owned, &executor()) {
        Ok(c) => {
            c.retire();
            panic!("overflow became a selected lease")
        }
        Err(f) => f,
    };
    assert_eq!(fs::read(&file).unwrap(), bytes);
    failed.retire();
    selected_owner(dir.path(), until()).verify().unwrap();
}
#[test]
fn selected_factory_expired_owned_writer_stays_in_failed_custody_until_retirement() {
    let dir = selected_fixture();
    let deadline = Instant::now() + Duration::from_millis(250);
    let owned = selected_owner(dir.path(), deadline);
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
    );
    let failed = match crate::state::task_controls::capture_owned_stops(owned, &executor()) {
        Ok(c) => {
            c.retire();
            panic!("expired original owner became a selected lease")
        }
        Err(f) => f,
    };
    assert!(!dir.path().join("task-stop-facts").exists());
    assert!(crate::state::task_controls::writer::test_open(
        dir.path(),
        Instant::now() + Duration::from_millis(15)
    )
    .is_err());
    failed.retire();
    selected_owner(dir.path(), until()).verify().unwrap();
}
