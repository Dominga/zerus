//! The contracts other packages build on: JSON forms, defaults and fixtures.

use serde::de::DeserializeOwned;
use serde::Serialize;
use zerus_team_plan::catalog::DEFAULT_ROUNDS;
use zerus_team_plan::problem::code;
use zerus_team_plan::*;

const CATALOG: &str = include_str!("../fixtures/catalog-builtin.json");
const BUNDLE: &str = include_str!("../fixtures/bundle-bugfix.json");
const TEAM: &str = include_str!("../fixtures/team-queue.json");

/// Parses, writes and parses again; both values must be equal.
fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(text: &str) -> T {
    let first: T = serde_json::from_str(text).expect("fixture parses");
    let written = serde_json::to_string(&first).expect("value serializes");
    let second: T = serde_json::from_str(&written).expect("written form parses");
    assert_eq!(first, second);
    first
}

fn refused<T: DeserializeOwned>(text: &str) {
    assert!(
        serde_json::from_str::<T>(text).is_err(),
        "must be refused: {text}"
    );
}

#[test]
fn fixtures_round_trip() {
    let catalog: Catalog = round_trip(CATALOG);
    assert_eq!(catalog.templates.len(), 3);
    assert_eq!(catalog.pipelines.len(), 4);
    assert_eq!(catalog.planning_rules.len(), 7);
    let bundle: TemplateBundle = round_trip(BUNDLE);
    assert_eq!(bundle.template.id, "bugfix");
    let team: TeamFixture = round_trip(TEAM);
    assert_eq!(team.members.len(), 4);
    assert_eq!(team.nodes.len(), 6);
}

#[test]
fn built_in_pipelines_match_the_specification() {
    let catalog: Catalog = serde_json::from_str(CATALOG).unwrap();
    let by_id = |id: &str| catalog.pipelines.iter().find(|p| p.id == id).unwrap();
    let kinds = |id: &str| by_id(id).stages.iter().map(|s| s.kind).collect::<Vec<_>>();
    assert_eq!(kinds("code"), [StageKind::Review, StageKind::Test]);
    assert_eq!(kinds("reviewed"), [StageKind::Review]);
    assert_eq!(kinds("docs"), [StageKind::Review]);
    assert!(kinds("check").is_empty());
    assert_eq!(by_id("code").accept, Acceptance::Lead);
    assert_eq!(by_id("docs").accept, Acceptance::Pipeline);
    assert_eq!(by_id("check").accept, Acceptance::Lead);
    assert_eq!(by_id("code").rounds(), DEFAULT_ROUNDS);
}

#[test]
fn every_built_in_template_names_known_roles_and_pipelines() {
    let catalog: Catalog = serde_json::from_str(CATALOG).unwrap();
    for template in &catalog.templates {
        assert!(
            catalog
                .pipelines
                .iter()
                .any(|p| p.id == template.default_pipeline),
            "{}",
            template.id
        );
        for seat in &template.roster {
            assert!(
                catalog.roles.iter().any(|r| r.id == seat.role),
                "{}: {}",
                template.id,
                seat.role
            );
        }
    }
}

#[test]
fn documented_defaults() {
    let settings = Settings::default();
    assert_eq!(
        settings.delegations,
        Delegations {
            accept_results: true,
            answer_members: true,
            confirm_trust: true,
            resume_exited: true,
            merge_accepted: true
        }
    );
    assert_eq!(
        settings.caps,
        Caps {
            max_members: 6,
            max_phase_tasks: 50,
            max_open_tasks: 500,
            max_open_questions: 3
        }
    );
    assert_eq!(
        settings.timers,
        Timers {
            report_timeout_seconds: 120,
            start_timeout_seconds: 180,
            inbox_interval_seconds: 120,
            remind_lead_minutes: 15,
            remind_person_minutes: 60,
            lead_stall_minutes: 20,
            answer_escalate_minutes: 10,
            resume_limit_per_hour: 3
        }
    );
    assert!(settings.queue);
    assert_eq!(settings.routing, Routing::Direct);
    assert_eq!(settings.plan_approval, Approval::Review);
    assert_eq!(settings.phase_acceptance, Acceptance::Lead);
    assert_eq!(settings.integration, Integration::Branch);
    assert!(!settings.land_allowed);
    assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), settings);
}

#[test]
fn a_partial_setting_keeps_the_other_defaults() {
    let settings: Settings =
        serde_json::from_str(r#"{"caps":{"max_members":2},"routing":"lead"}"#).unwrap();
    assert_eq!(settings.caps.max_members, 2);
    assert_eq!(settings.caps.max_open_tasks, 500);
    assert_eq!(settings.routing, Routing::Lead);
    assert!(settings.delegations.confirm_trust);
}

#[test]
fn absent_node_fields_default() {
    let node: Node = serde_json::from_str(r#"{"id":1,"phase":0,"title":"x"}"#).unwrap();
    assert_eq!(node.round, 1);
    assert!(node.admitted);
    assert_eq!(node.state, NodeState::Draft);
    assert_eq!(node.kind, NodeKind::Task);
    assert_eq!(node.source, NodeSource::Planned);
    assert!(node.task.is_none() && node.deps.is_empty() && node.writes.is_empty());
    let written = serde_json::to_value(&node).unwrap();
    assert_eq!(
        written,
        serde_json::json!({"id":1,"phase":0,"title":"x","round":1,"admitted":true})
    );
}

#[test]
fn absent_owner_is_the_local_person() {
    let mut value: serde_json::Value = serde_json::from_str(TEAM).unwrap();
    value["team"].as_object_mut().unwrap().remove("owner");
    let team: TeamFixture = serde_json::from_value(value).unwrap();
    assert_eq!(team.team.owner, model::LOCAL_PERSON);
    assert_eq!(team.team.lead, model::LEAD);
    assert_eq!(team.phases[0].n, model::QUEUE_PHASE);
}

#[test]
fn unknown_fields_are_refused() {
    refused::<Problem>(r#"{"code":"x","message":"y","extra":1}"#);
    refused::<Settings>(r#"{"queue":true,"extra":1}"#);
    refused::<Caps>(r#"{"max_members":2,"extra":1}"#);
    refused::<Node>(r#"{"id":1,"phase":0,"title":"x","extra":1}"#);
    refused::<Dep>(r#"{"node":1,"extra":1}"#);
    refused::<Role>(r#"{"id":"r","name":"R","agent":"claude","extra":1}"#);
    refused::<Seat>(r#"{"role":"r","extra":1}"#);
    refused::<Pipeline>(r#"{"id":"p","name":"P","implementer":"r","extra":1}"#);
    refused::<Catalog>(r#"{"roles":[],"extra":1}"#);
    refused::<AccountChoice>(r#"{"mode":"named","account":"a","extra":1}"#);
    refused::<AccountChoice>(r#"{"mode":"freest","colour":"red"}"#);
    refused::<AccountChoice>(r#"{"mode":"freest","account":"a"}"#);
    refused::<AccountChoice>(r#"{"mode":"named"}"#);
    refused::<AccountChoice>(r#"{"account":"a"}"#);
    refused::<AccountChoice>(r#"{"mode":"named","account":""}"#);
    refused::<Role>(
        r#"{"id":"r","name":"R","agent":"claude","account":{"mode":"freest","colour":"red"}}"#,
    );
    refused::<TaskRef>(r#"{"project":"p","uuid":"u","extra":1}"#);
}

#[test]
fn unknown_enum_values_are_refused() {
    refused::<NodeState>(r#""running""#);
    refused::<AccountChoice>(r#"{"mode":"cheapest"}"#);
    refused::<StageKind>(r#""deploy""#);
}

#[test]
fn account_choice_forms() {
    assert_eq!(
        serde_json::to_value(AccountChoice::Freest).unwrap(),
        serde_json::json!({"mode":"freest"})
    );
    let named = AccountChoice::Named {
        account: "account-example".into(),
    };
    assert_eq!(
        serde_json::to_value(&named).unwrap(),
        serde_json::json!({"mode":"named","account":"account-example"})
    );
    assert_eq!(AccountChoice::default(), AccountChoice::Freest);
    let role: Role = serde_json::from_str(r#"{"id":"r","name":"R","agent":"claude"}"#).unwrap();
    assert_eq!(role.account, AccountChoice::Freest);
    assert_eq!(role.context, ContextMode::Keep);
}

#[test]
fn seat_count_defaults_to_one_and_rounds_to_three() {
    let seat: Seat = serde_json::from_str(r#"{"role":"r"}"#).unwrap();
    assert_eq!(seat.count, 1);
    let mut pipeline: Pipeline =
        serde_json::from_str(r#"{"id":"p","name":"P","implementer":"r"}"#).unwrap();
    assert_eq!(pipeline.rounds(), 3);
    pipeline.max_rounds = 5;
    assert_eq!(pipeline.rounds(), 5);
}

#[test]
fn error_codes_are_stable_and_unique() {
    // Callers match these strings across crate and command boundaries.
    assert_eq!(
        code::ALL,
        [
            "teams_inactive",
            "not_found",
            "stale_revision",
            "template_changed",
            "not_allowed",
            "invalid_state",
            "store_busy",
            "storage_failed",
            "unknown_role",
            "unknown_pipeline",
            "unknown_member",
            "unknown_account",
            "no_account_available",
            "no_seat_for_stage",
            "dependency_cycle",
            "over_cap",
            "duplicate",
            "unknown_field",
            "integration_conflict",
            "integration_unavailable",
            "delivery_uncertain",
            "unstaffed_unchanged",
        ]
    );
    let mut codes = code::ALL.to_vec();
    assert_eq!(codes.len(), 22);
    assert!(codes
        .iter()
        .all(|c| c.chars().all(|ch| ch.is_ascii_lowercase() || ch == '_')));
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(codes.len(), 22);
    assert_eq!(code::TEAMS_INACTIVE, "teams_inactive");
    let problem =
        Problem::new(code::NO_ACCOUNT_AVAILABLE, "No account has room").with_refs(["1794358800"]);
    assert_eq!(
        problem.to_string(),
        "no_account_available: No account has room"
    );
    assert_eq!(
        serde_json::to_value(&problem).unwrap()["refs"],
        serde_json::json!(["1794358800"])
    );
}

#[test]
fn snake_case_enum_forms() {
    assert_eq!(
        serde_json::to_value(PhaseState::AwaitingAcceptance).unwrap(),
        "awaiting_acceptance"
    );
    assert_eq!(
        serde_json::to_value(NodeState::InProgress).unwrap(),
        "in_progress"
    );
    assert_eq!(
        serde_json::to_value(MemberState::NotCreated).unwrap(),
        "not_created"
    );
    assert_eq!(
        serde_json::to_value(TeamStatus::Running).unwrap(),
        "running"
    );
}

#[test]
fn an_empty_catalog_writes_no_empty_lists() {
    assert_eq!(
        serde_json::to_value(Catalog::default()).unwrap(),
        serde_json::json!({})
    );
    let rules_only = Catalog {
        planning_rules: vec!["One rule.".into()],
        ..Catalog::default()
    };
    assert_eq!(
        serde_json::to_value(&rules_only).unwrap(),
        serde_json::json!({"planning_rules": ["One rule."]})
    );
}

#[test]
fn storage_failure_has_a_distinct_pinned_problem_fixture() {
    let problem: Problem = round_trip(include_str!("../fixtures/problem-storage-failed.json"));
    assert_eq!(problem.code, "storage_failed");
    assert!(code::ALL.contains(&problem.code.as_str()));
    assert_ne!(problem.code, code::INVALID_STATE);
    assert_eq!(
        problem.message,
        "The result is not known; repeat the same request with the same request ID."
    );
}

#[test]
fn diagnostic_detail_is_additive_and_old_problems_still_decode() {
    let legacy: Problem = serde_json::from_value(
        serde_json::json!({"code":"invalid_state","message":"Rule refused."}),
    )
    .unwrap();
    assert!(serde_json::to_value(legacy)
        .unwrap()
        .get("detail")
        .is_none());
    let value = serde_json::json!({"code":"storage_failed","message":"The read failed and may be repeated.","detail":"database is locked"});
    let problem: Problem = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(problem).unwrap(), value);
}

#[test]
fn busy_before_begin_is_a_distinct_pinned_refusal() {
    let p: Problem = round_trip(include_str!("../fixtures/problem-store-busy.json"));
    assert_eq!(p.code, code::STORE_BUSY);
    assert!(code::ALL.contains(&p.code.as_str()));
    assert_eq!(
        p.message,
        "The team store is busy. Nothing was written; try again."
    );
}

#[test]
fn a_changed_template_has_a_pinned_refusal() {
    let problem: Problem = round_trip(include_str!("../fixtures/problem-template-changed.json"));
    assert_eq!(problem.code, code::TEMPLATE_CHANGED);
    assert_eq!(
        problem.message,
        "The template changed since it was shown. Review it again."
    );
}
