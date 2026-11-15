use super::*;
use serde_json::{json, Value};

// Hand-written minimal graph inputs isolate arithmetic and refusal boundaries.
// The separate producer test below covers operation-written rows end to end.
fn node(id: u32, estimate: u32, deps: &[u32]) -> Node {
    serde_json::from_value(json!({"id":id,"phase":1,"title":"Metric control",
        "estimate_minutes":estimate,"deps":deps.iter().map(|node|json!({"node":node})).collect::<Vec<_>>()}))
    .unwrap()
}

#[test]
fn duration_is_the_weighted_path_not_the_number_of_layers() {
    let nodes = [
        node(1, 40, &[]),
        node(2, 5, &[1]),
        node(3, 10, &[]),
        node(4, 15, &[3]),
        node(5, 10, &[4]),
    ];
    let m = project(&nodes, true);
    assert_eq!(m.critical_path_minutes, Some(45));
    assert_eq!(m.average_parallelism_tenths, Some(18));
    assert!(m.unavailable.is_none() && m.warnings.is_empty());
    let mut reversed = nodes.to_vec();
    reversed.reverse();
    assert_eq!(project(&reversed, true), m);
}

#[test]
fn a_diamond_counts_all_work_and_only_the_longest_join_input() {
    let m = project(
        &[
            node(1, 10, &[]),
            node(2, 20, &[1]),
            node(3, 30, &[1]),
            node(4, 10, &[2, 3]),
        ],
        true,
    );
    assert_eq!(m.critical_path_minutes, Some(50));
    assert_eq!(m.average_parallelism_tenths, Some(14));
}

#[test]
fn parallelism_rounds_a_half_tenth_up_without_floating_point() {
    let m = project(&[node(1, 8, &[]), node(2, 2, &[])], true);
    assert_eq!(m.average_parallelism_tenths, Some(13));
}

#[test]
fn soft_dependencies_wait_for_start_and_origins_add_no_duration() {
    let mut b = node(2, 10, &[1]);
    b.deps[0].kind = DepKind::Soft;
    let mut c = node(3, 5, &[]);
    c.origin = Some(1);
    let m = project(&[node(1, 40, &[]), b, c], true);
    assert_eq!(m.critical_path_minutes, Some(40));
    assert_eq!(m.average_parallelism_tenths, Some(14));
}

#[test]
fn zero_partial_and_incomplete_estimates_never_become_numeric_metrics() {
    for nodes in [
        vec![node(1, 0, &[])],
        vec![node(1, 10, &[]), node(2, 0, &[1])],
    ] {
        let m = project(&nodes, true);
        assert_eq!(m.unavailable, Some(Unavailable::MissingEstimates));
        assert_eq!(m.critical_path_minutes, None);
        assert_eq!(m.average_parallelism_tenths, None);
        assert_eq!(
            m.known_estimates,
            nodes.iter().filter(|n| n.estimate_minutes > 0).count()
        );
    }
    let m = project(&[node(1, 30, &[])], false);
    assert_eq!(m.unavailable, Some(Unavailable::IncompletePhase));
    assert_eq!(m.critical_path_minutes, None);
}

#[test]
fn out_of_scope_waits_are_counted_and_excluded_from_intrinsic_phase_duration() {
    let m = project(&[node(1, 10, &[99]), node(2, 20, &[1])], true);
    assert_eq!(m.critical_path_minutes, Some(30));
    assert_eq!(m.external_dependencies, 1);
}

#[test]
fn invalid_or_excessive_graphs_have_no_numbers_and_no_hard_refusal_warnings() {
    let mut other_phase = node(2, 20, &[]);
    other_phase.phase = 2;
    for nodes in [
        vec![node(1, 10, &[2]), node(2, 20, &[1])],
        vec![node(1, 10, &[]), node(1, 20, &[])],
        vec![node(0, 10, &[])],
        vec![node(1, 10, &[]), other_phase],
    ] {
        let m = project(&nodes, true);
        assert_eq!(m.unavailable, Some(Unavailable::InvalidGraph));
        assert!(m.critical_path_minutes.is_none() && m.warnings.is_empty());
    }
    assert_eq!(
        project(&[], true).unavailable,
        Some(Unavailable::EmptyPhase)
    );
    let nodes: Vec<_> = (1..=MAX_NODES as u32 + 1)
        .map(|id| node(id, 1, &[]))
        .collect();
    assert_eq!(
        project(&nodes, true).unavailable,
        Some(Unavailable::ScopeTooLarge)
    );
    let mut n = node(1, 1, &[]);
    n.deps = (2..=MAX_DEPENDENCIES as u32 + 2)
        .map(|node| zerus_team_plan::Dep {
            node,
            kind: DepKind::Hard,
        })
        .collect();
    assert_eq!(
        project(&[n], true).unavailable,
        Some(Unavailable::ScopeTooLarge)
    );
}

#[test]
fn maximum_snapshot_and_u32_estimates_remain_bounded_and_do_not_overflow() {
    let nodes: Vec<_> = (1..=MAX_NODES as u32)
        .map(|id| node(id, u32::MAX, &[]))
        .collect();
    let m = project(&nodes, true);
    assert_eq!(m.critical_path_minutes, Some(u64::from(u32::MAX)));
    assert_eq!(m.average_parallelism_tenths, Some(MAX_NODES as u64 * 10));
}

fn product_view() -> crate::teams::view::TeamView {
    use crate::actions::Exit;
    use crate::teams::{read::decode, TeamsFollow, TeamsModel, TeamsUpdate};
    let source: Value = serde_json::from_str(include_str!(
        "../../../../../../crates/team-plan/fixtures/cli-phase-metrics.json"
    ))
    .unwrap();
    let now = std::time::Instant::now();
    let mut model = TeamsModel::default();
    let team = source["draft"]["team"].as_str().unwrap();
    model.follow(
        TeamsFollow {
            team: Some(team.into()),
            ..Default::default()
        },
        now,
    );
    for key in ["status", "queue"] {
        let read = model.next(now).unwrap();
        let value = decode(
            &read,
            Exit::Code {
                code: 0,
                stdout: serde_json::to_vec(&source[key]).unwrap(),
                stderr: String::new(),
            },
        )
        .unwrap();
        model.apply(&read, Ok(value), now);
    }
    model.follow_phases(team, Some(1), true, now).unwrap();
    let mut latest = None;
    for key in ["phases", "draft"] {
        let read = model.next(now).unwrap();
        let value = decode(
            &read,
            Exit::Code {
                code: 0,
                stdout: serde_json::to_vec(&source[key]).unwrap(),
                stderr: String::new(),
            },
        )
        .unwrap();
        for (_, update) in model.apply(&read, Ok(value), now) {
            if let TeamsUpdate::Team(v) = update {
                latest = Some((*v).clone());
            }
        }
    }
    latest.unwrap()
}

#[test]
fn operation_written_estimates_reach_metrics_through_reader_model_and_batch() {
    let view = product_view();
    let m = &view.plan.as_ref().unwrap().draft.as_ref().unwrap().metrics;
    assert_eq!(m.critical_path_minutes, Some(45));
    assert_eq!(m.average_parallelism_tenths, Some(14));
    assert!(m.warnings.is_empty());
    let key = format!("team:{}", view.bar.id);
    let mut c = crate::Coalescer::new();
    c.push(crate::Change::teams(
        crate::ComputerId("phase-metrics-fixture".into()),
        key,
        crate::teams::TeamsUpdate::Team(std::sync::Arc::new(view)),
    ));
    let wire = serde_json::to_value(c.take()).unwrap();
    let _: crate::Batch = serde_json::from_value(wire.clone()).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/teams/fixtures/phase-metrics.json");
    assert_eq!(
        wire["teams"][0]["payload"],
        serde_json::from_str::<Value>(&std::fs::read_to_string(path).unwrap()).unwrap()
    );
}

#[test]
#[ignore = "writes the UI metric fixture through real core projection/model/coalescer"]
fn regenerate_phase_metrics_fixture() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../src/ui/teams/fixtures/phase-metrics.json");
    std::fs::write(
        path,
        serde_json::to_string_pretty(&product_view()).unwrap() + "\n",
    )
    .unwrap();
}
