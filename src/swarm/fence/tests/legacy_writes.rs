//! Protocol migration of literal pre-upgrade tasks; no hand-written rows.
use super::*;
use zerus_replica::{Actor, Position, Stage, Target};

fn legacy(m: &Members, schema: i64) -> TaskDoc {
    let store = ProjectStore::open(&m.configs["a"].state.join("replica"), "p").unwrap();
    let (mut task, _) = store
        .new_named_task(
            "Historical task",
            "No conversation yet",
            &[],
            Stage::Todo,
            Target::Any,
            Actor::Person,
            &Position::End,
            "example",
            1,
            None,
            false,
        )
        .unwrap();
    // Schema 4 is the literal writer above. The other cases vary only the
    // raw old-schema metadata with the replica's public fixture helper.
    if schema != 4 {
        task.lower_stored_schema_for_test(schema);
    }
    store.save_legacy_for_test(&mut task).unwrap();
    assert!(!task.execution_managed());
    task
}
#[test]
fn all_legacy_schema_writers_reach_a_historical_root_without_starting() {
    for schema in 4..=7 {
        let m = members(&["a"], &[]);
        let task = legacy(&m, schema);
        let config = &m.configs["a"];
        let net = InProcess::new(&m, Faults::default());
        let mut worker = crate::swarm::replica::CutoverScan::default();
        assert!(worker.advance_with(config, &net).unwrap());
        let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let mut current = store.load(task.uuid()).unwrap();
        assert!(current.floor_record().is_some());
        assert!(current.cutover_seal().unwrap().is_some());
        assert_eq!(current.schema(), zerus_replica::TASK_SCHEMA);
        assert!(current.view().unwrap().execution.is_none());
        assert_eq!(current.stage().unwrap(), Stage::Todo);
        current
            .comment("Ordinary person edit", &Actor::Person, 2)
            .unwrap();
        store.save(&mut current).unwrap();
        drop(store);
        assert!(
            !record_path(&m, task.uuid()).exists(),
            "migration creates no dispatch certificate"
        );
    }
}
#[test]
fn a_hold_keeps_an_unsealed_task_and_its_original_files() {
    let m = members(&["a"], &[]);
    let task = legacy(&m, 4);
    let config = &m.configs["a"];
    {
        let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let mut project = store.project().unwrap();
        project.set_cutover_hold(true, &Actor::Person, 2).unwrap();
        store.save_project(&project).unwrap();
    }
    let net = InProcess::new(&m, Faults::default());
    assert!(catch_up_writer(config, &net, "p", task.uuid(), soon()).is_err());
    let store = ProjectStore::open(&config.state.join("replica"), "p").unwrap();
    assert!(
        store
            .pending_cutover_capture(task.uuid())
            .unwrap()
            .is_none(),
        "a refused floor must not freeze the task"
    );
    drop(store);
    let current = held(&m, "a", task.uuid()).unwrap();
    assert_eq!(current.revision(), task.revision());
    assert!(current.floor_record().is_none() && current.cutover_seal().unwrap().is_none());
}
#[test]
fn returning_prefloor_writers_adopt_the_original_and_refuse_old_rule_edits() {
    for dirty in [false, true] {
        let m = members(&["a", "b"], &[("a", "b")]);
        let task = legacy(&m, 4);
        {
            let store = ProjectStore::open(&m.configs["b"].state.join("replica"), "p").unwrap();
            assert!(store
                .import_updates(vec![Update {
                    document: Document::Task(task.uuid().into()),
                    bytes: task.snapshot().unwrap()
                }])
                .unwrap()
                .refused
                .is_empty());
        }
        synced(&m, "a");
        let net = InProcess::new(
            &m,
            Faults {
                offline: BTreeSet::from([m.ids["b"].clone()]),
                ..Faults::default()
            },
        );
        catch_up_writer(&m.configs["a"], &net, "p", task.uuid(), soon()).unwrap();
        let first = held(&m, "a", task.uuid()).unwrap();
        let before = {
            let store = ProjectStore::open(&m.configs["b"].state.join("replica"), "p").unwrap();
            let mut old = store.load(task.uuid()).unwrap();
            if dirty {
                old.comment("Old-rule edit after disconnection", &Actor::Person, 3)
                    .unwrap();
                store.save_legacy_for_test(&mut old).unwrap();
            }
            old.revision()
        };
        synced(&m, "b");
        let net = InProcess::new(&m, Faults::default());
        let answer = catch_up_writer(&m.configs["b"], &net, "p", task.uuid(), soon());
        let current = held(&m, "b", task.uuid()).unwrap();
        if dirty {
            assert!(answer.is_err());
            assert_eq!(current.revision(), before, "refusal keeps original data");
            assert!(
                current.floor_record().is_none(),
                "a stale branch must not mint F2"
            );
        } else {
            answer.unwrap();
            assert_eq!(current.floor_record(), first.floor_record());
            assert_eq!(
                current.cutover_seal().unwrap().unwrap().identity(),
                first.cutover_seal().unwrap().unwrap().identity()
            );
        }
        assert!(!record_path(&m, task.uuid()).exists());
        assert!(current.view().unwrap().execution.is_none());
    }
}

struct G1LegacyTimeouts<'a> {
    inner: InProcess<'a>,
    optional: BTreeSet<String>,
    wait: bool,
    wait_action: &'static str,
}
impl Net for G1LegacyTimeouts<'_> {
    fn call(
        &self,
        swarm: &str,
        route: Route<'_>,
        command: &str,
        input: Option<&Value>,
        deadline: Instant,
    ) -> Result<Value> {
        if command == "replica-task" && self.optional.contains(route.nodes.last().unwrap()) {
            if self.wait && input.is_some_and(|v| v["cutover"]["action"] == self.wait_action) {
                std::thread::sleep(left(deadline));
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "private optional reader reached its actual deadline",
            )
            .into());
        }
        self.inner.call(swarm, route, command, input, deadline)
    }
}
fn g1_legacy_with_optional_readers(wait: bool) {
    let m = members(
        &["a", "b", "c", "d", "e", "f", "g", "h", "i", "z"],
        &[
            ("a", "b"),
            ("a", "c"),
            ("a", "d"),
            ("a", "e"),
            ("a", "f"),
            ("a", "g"),
            ("a", "h"),
            ("a", "i"),
            ("a", "z"),
        ],
    );
    let task = legacy(&m, 4);
    synced(&m, "a");
    let net = G1LegacyTimeouts {
        inner: InProcess::new(&m, Faults::default()),
        optional: ["b", "c", "d", "e", "f", "g", "h", "i"]
            .iter()
            .map(|name| m.ids[*name].clone())
            .collect(),
        wait,
        wait_action: "export",
    };
    let started = Instant::now();
    let deadline = started + Duration::from_secs(15);
    let result = catch_up_writer(&m.configs["a"], &net, "p", task.uuid(), deadline);
    let current = held(&m, "a", task.uuid()).unwrap();
    eprintln!(
        "G1 legacy wait={wait} elapsed={:?}, result={result:?}, floor={:?}, sealed={}",
        started.elapsed(),
        current.floor_record(),
        current.cutover_seal().unwrap().is_some()
    );
    assert!(
        result.is_ok(),
        "optional read-only timeouts vetoed a genuine legacy task: {result:?}"
    );
    assert!(current.cutover_seal().unwrap().is_some());
    assert!(current.view().unwrap().execution.is_none());
    assert_eq!(current.stage().unwrap(), Stage::Todo);
    assert!(
        !record_path(&m, task.uuid()).exists(),
        "migration manufactured a certificate"
    );
}
#[test]
fn g1_legacy_optional_reader_immediate_negative_control() {
    g1_legacy_with_optional_readers(false);
}
#[test]
fn g1_legacy_optional_readers_keep_the_original_local_budget() {
    g1_legacy_with_optional_readers(true);
}

struct G1DeadlineSource<'a>(G1LegacyTimeouts<'a>);
impl Net for G1DeadlineSource<'_> {
    fn call(
        &self,
        swarm: &str,
        route: Route<'_>,
        command: &str,
        input: Option<&Value>,
        deadline: Instant,
    ) -> Result<Value> {
        if left(deadline).is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "private expired native route",
            )
            .into());
        }
        let answer = self.0.call(swarm, route, command, input, deadline)?;
        if left(deadline).is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "private route completed after its bound",
            )
            .into());
        }
        Ok(answer)
    }
}
fn g1_legacy_existing_admitted_source(
    wait: bool,
    last: bool,
    wait_action: &'static str,
    writer: bool,
) {
    let m = members(
        &["a", "b", "c", "d", "e", "f", "g", "h", "i", "z"],
        &[
            ("a", "b"),
            ("a", "c"),
            ("a", "d"),
            ("a", "e"),
            ("a", "f"),
            ("a", "g"),
            ("a", "h"),
            ("a", "i"),
            ("a", "z"),
        ],
    );
    let task = legacy(&m, 4);
    // Source-query iteration is keyed by actual node identity, not alias.
    let (healthy, healthy_id) = m
        .ids
        .iter()
        .filter(|(name, _)| name.as_str() != "a")
        .min_by(|a, b| if last { b.1.cmp(a.1) } else { a.1.cmp(b.1) })
        .map(|(name, id)| (name.clone(), id.clone()))
        .unwrap();
    {
        let store = ProjectStore::open(&m.configs[&healthy].state.join("replica"), "p").unwrap();
        assert!(store
            .import_updates(vec![Update {
                document: Document::Task(task.uuid().into()),
                bytes: task.snapshot().unwrap()
            }])
            .unwrap()
            .refused
            .is_empty());
    }
    synced(&m, &healthy);
    let bootstrap = InProcess::new(
        &m,
        Faults {
            offline: BTreeSet::from([m.ids["a"].clone()]),
            ..Faults::default()
        },
    );
    catch_up_writer(&m.configs[&healthy], &bootstrap, "p", task.uuid(), soon()).unwrap();
    let original = held(&m, &healthy, task.uuid()).unwrap();
    assert!(original.floor_record().is_some() && original.cutover_seal().unwrap().is_some());
    assert!(held(&m, "a", task.uuid()).unwrap().floor_record().is_none());
    synced(&m, "a");
    let pending = wait_action == "export_pending";
    if pending {
        let store = ProjectStore::open(&m.configs["a"].state.join("replica"), "p").unwrap();
        let (capture, _) = store.prepare_cutover(task.uuid(), &m.ids["a"]).unwrap();
        store
            .retain_cutover_proposal(&original.snapshot().unwrap(), &capture)
            .unwrap();
    }
    let net = G1DeadlineSource(G1LegacyTimeouts {
        inner: InProcess::new(&m, Faults::default()),
        optional: m
            .ids
            .iter()
            .filter(|(name, id)| name.as_str() != "a" && **id != healthy_id)
            .map(|(_, id)| id.clone())
            .collect(),
        wait,
        wait_action: if pending { "export" } else { wait_action },
    });
    let started = Instant::now();
    let deadline = started + Duration::from_secs(15);
    let answer = if writer {
        match prepare_write(&m.configs["a"], &net, "p", task.uuid(), deadline) {
            Ok(Ok(())) => Ok(()),
            other => Err(anyhow::anyhow!("write preparation refused: {other:?}")),
        }
    } else {
        catch_up_writer(&m.configs["a"], &net, "p", task.uuid(), deadline)
    };
    let current = held(&m, "a", task.uuid()).unwrap();
    eprintln!("G1 existing-source wait={wait}, elapsed={:?}, answer={answer:?}, original_floor={:?}, current_floor={:?}",
        started.elapsed(), original.floor_record(), current.floor_record());
    assert!(
        answer.is_ok(),
        "slow optional reads hid the available admitted source: {answer:?}"
    );
    assert_eq!(
        current.floor_record(),
        original.floor_record(),
        "a returning prefix minted F2"
    );
    assert_eq!(
        current.cutover_seal().unwrap().unwrap().identity(),
        original.cutover_seal().unwrap().unwrap().identity(),
        "adopt the same immutable original seal"
    );
    assert!(current.view().unwrap().execution.is_none());
}
#[test]
fn g1_legacy_source_immediate_negative_adopts_original() {
    g1_legacy_existing_admitted_source(false, true, "export", false);
}
#[test]
fn g1_legacy_source_real_timeouts_still_adopt_original() {
    g1_legacy_existing_admitted_source(true, true, "export", false);
}

// The source is first here: optional Prepare calls happen inside root
// preparation after it is found. This independently pins the early reserve
// even when Export lookup itself has a fair, short pool.
#[test]
fn legacy_optional_prepare_immediate_control_adopts_original() {
    g1_legacy_existing_admitted_source(false, false, "prepare", false);
}
#[test]
fn legacy_optional_prepares_keep_the_original_local_reserve() {
    g1_legacy_existing_admitted_source(true, false, "prepare", false);
}

#[test]
fn legacy_write_uses_start_admission_with_a_later_original_source() {
    g1_legacy_existing_admitted_source(true, true, "export", true);
}

/// Arch has only its clean pre-upgrade prefix; the Mac holds the admitted
/// original; va-kids is unreachable. The reachable Mac is slow on Export,
/// but completes inside the actual per-peer deadline it was given.
struct SlowMac<'a> {
    inner: InProcess<'a>,
    mac: String,
}
impl Net for SlowMac<'_> {
    fn call(
        &self,
        swarm: &str,
        route: Route<'_>,
        command: &str,
        input: Option<&Value>,
        deadline: Instant,
    ) -> Result<Value> {
        if command == "replica-task"
            && route.nodes.last() == Some(&self.mac)
            && input.is_some_and(|v| v["cutover"]["action"] == "export")
        {
            std::thread::sleep(left(deadline) / 2);
        }
        self.inner.call(swarm, route, command, input, deadline)
    }
}
#[test]
fn legacy_write_shares_start_with_slow_mac_and_unreachable_peer() {
    let m = members(&["a", "b", "c"], &[("a", "b"), ("a", "c")]);
    let task = legacy(&m, 4);
    {
        let store = ProjectStore::open(&m.configs["b"].state.join("replica"), "p").unwrap();
        assert!(store
            .import_updates(vec![Update {
                document: Document::Task(task.uuid().into()),
                bytes: task.snapshot().unwrap(),
            }])
            .unwrap()
            .refused
            .is_empty());
    }
    synced(&m, "b");
    let original_net = InProcess::new(
        &m,
        Faults {
            offline: BTreeSet::from([m.ids["a"].clone(), m.ids["c"].clone()]),
            ..Faults::default()
        },
    );
    catch_up_writer(&m.configs["b"], &original_net, "p", task.uuid(), soon()).unwrap();
    let original = held(&m, "b", task.uuid()).unwrap();
    synced(&m, "a");
    let net = SlowMac {
        inner: InProcess::new(
            &m,
            Faults {
                offline: BTreeSet::from([m.ids["c"].clone()]),
                ..Faults::default()
            },
        ),
        mac: m.ids["b"].clone(),
    };
    let admission = prepare_write(&m.configs["a"], &net, "p", task.uuid(), soon()).unwrap();
    assert!(admission.is_ok(), "{admission:?}");
    let current = held(&m, "a", task.uuid()).unwrap();
    assert_eq!(current.floor_record(), original.floor_record());
    assert_eq!(
        current.cutover_seal().unwrap(),
        original.cutover_seal().unwrap()
    );
    assert_eq!(current.stage().unwrap(), Stage::Todo);
    assert!(current.view().unwrap().execution.is_none());
    assert!(
        record_path(&m, task.uuid()).exists(),
        "the write must use Start's final admission, not background-only migration"
    );
}

mod g1_remaining_preparation_review {
    use super::*;
    #[test]
    fn g1_source_late_prepares_immediate_control() {
        g1_legacy_existing_admitted_source(false, true, "prepare", true);
    }
    #[test]
    fn g1_source_late_prepares_keep_background_adoption_available() {
        g1_legacy_existing_admitted_source(true, true, "prepare", false);
    }
    #[test]
    fn g1_source_late_prepares_keep_write_admission_available() {
        g1_legacy_existing_admitted_source(true, true, "prepare", true);
    }
}

// A retained original cannot be replaced. Verify its already-admitted source
// through the same product writers, with that source last in native-ID order.
#[test]
fn legacy_pending_original_source_immediate_control() {
    g1_legacy_existing_admitted_source(false, true, "export_pending", true);
}
#[test]
fn legacy_pending_original_source_keeps_background_verification_available() {
    g1_legacy_existing_admitted_source(true, true, "export_pending", false);
}
#[test]
fn legacy_pending_original_source_keeps_write_verification_available() {
    g1_legacy_existing_admitted_source(true, true, "export_pending", true);
}

/// Observe the actual deadlines handed to the native protocol transport.
/// Fair discovery alone must not replace the original local reserve.
struct LegacyBudgetTrace<'a> {
    inner: InProcess<'a>,
    source: String,
    latest_remote: Instant,
}
impl Net for LegacyBudgetTrace<'_> {
    fn call(
        &self,
        swarm: &str,
        route: Route<'_>,
        command: &str,
        input: Option<&Value>,
        deadline: Instant,
    ) -> Result<Value> {
        if command == "replica-task" && route.nodes.last() == Some(&self.source) {
            let action = input.and_then(|v| v["cutover"]["action"].as_str());
            if matches!(action, Some("export" | "prepare")) {
                std::thread::sleep(left(deadline).saturating_sub(Duration::from_millis(100)));
            }
            if action == Some("adopt") {
                assert!(
                    deadline <= self.latest_remote,
                    "remote adoption consumed the original local settlement reserve"
                );
            }
        }
        self.inner.call(swarm, route, command, input, deadline)
    }
}
#[test]
fn legacy_short_writer_budget_keeps_its_original_local_reserve() {
    let m = members(&["a", "b"], &[("a", "b")]);
    let task = legacy(&m, 4);
    {
        let store = ProjectStore::open(&m.configs["b"].state.join("replica"), "p").unwrap();
        assert!(store
            .import_updates(vec![Update {
                document: Document::Task(task.uuid().into()),
                bytes: task.snapshot().unwrap(),
            }])
            .unwrap()
            .refused
            .is_empty());
    }
    synced(&m, "b");
    let original_net = InProcess::new(
        &m,
        Faults {
            offline: BTreeSet::from([m.ids["a"].clone()]),
            ..Faults::default()
        },
    );
    catch_up_writer(&m.configs["b"], &original_net, "p", task.uuid(), soon()).unwrap();
    let original = held(&m, "b", task.uuid()).unwrap();
    synced(&m, "a");
    let deadline = Instant::now() + Duration::from_secs(3);
    let net = LegacyBudgetTrace {
        inner: InProcess::new(&m, Faults::default()),
        source: m.ids["b"].clone(),
        latest_remote: deadline - Duration::from_millis(500),
    };
    catch_up_writer(&m.configs["a"], &net, "p", task.uuid(), deadline).unwrap();
    let current = held(&m, "a", task.uuid()).unwrap();
    assert_eq!(current.floor_record(), original.floor_record());
    assert_eq!(
        current.cutover_seal().unwrap(),
        original.cutover_seal().unwrap()
    );
    assert!(current.view().unwrap().execution.is_none());
}
