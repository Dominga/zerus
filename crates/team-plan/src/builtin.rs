//! The catalog that ships with Zerus: three templates, four pipelines and the
//! roles they name.
//!
//! It is the same data as `crates/team-plan/fixtures/catalog-builtin.json`,
//! which other packages read and the desktop shows before a person changes
//! anything.

use crate::catalog::{
    AccountChoice, Catalog, LeadSpec, Pipeline, Role, Seat, Stage, StageKind, Template,
};
use crate::model::{Acceptance, Caps, ContextMode, Routing, Settings};

const DEVELOPER: &str = "Implement the task in your own worktree. Start from the base commit named in the task, keep to its write set, run the checks that can fail for your change, commit, and report what you changed and how you checked it.";
const REVIEWER: &str = "Review the reported commit against the task's criteria. You never change what you review. Give a verdict: pass, or return with the exact findings.";
const TESTER: &str = "Check the reported commit by running it: the task's criteria and the scenarios a person would try. You never implement. Give a verdict: pass, or return with what failed and how to see it.";

const PLANNING_RULES: [&str; 7] = [
    "Aim at 20 to 25 tasks in a phase and offer a split above 25.",
    "Use a hard dependency only where it is needed, and none across phases: use a barrier phase.",
    "Plan shared foundations first.",
    "A reviewer never writes what it reviews.",
    "A tester never implements.",
    "Make each task text self-contained, with criteria a reviewer can check.",
    "The lead never builds, tests or merges a member's work by hand.",
];

fn role(id: &str, name: &str, prompt: &str) -> Role {
    Role {
        id: id.into(),
        name: name.into(),
        prompt: prompt.into(),
        agent: "claude".into(),
        account: AccountChoice::Freest,
        model: String::new(),
        effort: String::new(),
        pipeline: String::new(),
        context: ContextMode::Keep,
    }
}

fn seat(role: &str, count: u32) -> Seat {
    Seat {
        role: role.into(),
        count,
        optional: false,
    }
}

fn stage(kind: StageKind, role: &str) -> Stage {
    Stage {
        kind,
        role: role.into(),
    }
}

fn pipeline(id: &str, name: &str, stages: Vec<Stage>, accept: Acceptance) -> Pipeline {
    Pipeline {
        id: id.into(),
        name: name.into(),
        implementer: "developer".into(),
        stages,
        max_rounds: 0,
        accept,
    }
}

fn lead() -> LeadSpec {
    LeadSpec {
        agent: "claude".into(),
        account: AccountChoice::Freest,
        model: String::new(),
        effort: String::new(),
        rules: String::new(),
    }
}

fn template(
    id: &str,
    name: &str,
    description: &str,
    roster: Vec<Seat>,
    pipeline: &str,
    settings: Settings,
) -> Template {
    Template {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        lead: lead(),
        roster,
        default_pipeline: pipeline.into(),
        settings,
    }
}

/// The built-in catalog, equal to `fixtures/catalog-builtin.json`.
pub fn builtin() -> Catalog {
    Catalog {
        roles: vec![
            role("developer", "Developer", DEVELOPER),
            role("reviewer", "Reviewer", REVIEWER),
            role("tester", "Tester", TESTER),
        ],
        pipelines: vec![
            pipeline(
                "code",
                "Code: review, test, lead",
                vec![
                    stage(StageKind::Review, "reviewer"),
                    stage(StageKind::Test, "tester"),
                ],
                Acceptance::Lead,
            ),
            pipeline(
                "reviewed",
                "Reviewed: review, lead",
                vec![stage(StageKind::Review, "reviewer")],
                Acceptance::Lead,
            ),
            pipeline(
                "docs",
                "Docs: review, then accepted",
                vec![stage(StageKind::Review, "reviewer")],
                Acceptance::Pipeline,
            ),
            pipeline("check", "Check: lead", Vec::new(), Acceptance::Lead),
        ],
        templates: vec![
            template(
                "feature-delivery",
                "Feature delivery",
                "Planned work on one goal: two developers, a reviewer and a tester.",
                vec![seat("developer", 2), seat("reviewer", 1), seat("tester", 1)],
                "code",
                Settings {
                    routing: Routing::Lead,
                    ..Settings::default()
                },
            ),
            template(
                "bugfix",
                "Bugfix",
                "A queue of independent fixes: a developer and a reviewer.",
                vec![seat("developer", 1), seat("reviewer", 1)],
                "reviewed",
                Settings::default(),
            ),
            template(
                "small",
                "Small",
                "A small task or a tight budget: one implementer and an independent checker.",
                vec![seat("developer", 1), seat("reviewer", 1)],
                "reviewed",
                Settings {
                    routing: Routing::Lead,
                    caps: Caps {
                        max_members: 2,
                        ..Caps::default()
                    },
                    ..Settings::default()
                },
            ),
        ],
        planning_rules: PLANNING_RULES
            .iter()
            .map(|rule| (*rule).to_owned())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Acceptance, Routing};
    use crate::validate::validate_catalog;

    const FIXTURE: &str = include_str!("../fixtures/catalog-builtin.json");

    #[test]
    fn built_in_equals_the_fixture() {
        let fixture: Catalog = serde_json::from_str(FIXTURE).expect("fixture parses");
        assert_eq!(builtin(), fixture);
    }

    #[test]
    fn built_in_has_the_documented_counts() {
        let catalog = builtin();
        assert_eq!(catalog.roles.len(), 3, "developer, reviewer, tester");
        assert_eq!(catalog.pipelines.len(), 4, "code, reviewed, docs, check");
        assert_eq!(
            catalog.templates.len(),
            3,
            "feature-delivery, bugfix, small"
        );
        assert_eq!(catalog.planning_rules.len(), 7);
        assert!(
            validate_catalog(&catalog).is_empty(),
            "{:?}",
            validate_catalog(&catalog)
        );
    }

    #[test]
    fn the_docs_pipeline_accepts_on_its_own_and_the_others_wait_for_the_lead() {
        let catalog = builtin();
        let accept = |id: &str| {
            catalog
                .pipelines
                .iter()
                .find(|pipeline| pipeline.id == id)
                .expect("a built-in pipeline")
                .accept
        };
        assert_eq!(accept("code"), Acceptance::Lead);
        assert_eq!(accept("reviewed"), Acceptance::Lead);
        assert_eq!(accept("check"), Acceptance::Lead);
        assert_eq!(accept("docs"), Acceptance::Pipeline);
    }

    #[test]
    fn feature_delivery_routes_through_the_lead_and_small_keeps_two_members() {
        let catalog = builtin();
        let template = |id: &str| {
            catalog
                .templates
                .iter()
                .find(|template| template.id == id)
                .expect("a built-in template")
        };
        assert_eq!(template("feature-delivery").settings.routing, Routing::Lead);
        assert_eq!(template("bugfix").settings.routing, Routing::Direct);
        assert_eq!(template("small").settings.routing, Routing::Lead);
        assert_eq!(template("small").settings.caps.max_members, 2);
        let seats = |id: &str| {
            template(id)
                .roster
                .iter()
                .map(|seat| (seat.role.as_str(), seat.count))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            seats("feature-delivery"),
            [("developer", 2), ("reviewer", 1), ("tester", 1)]
        );
        assert_eq!(seats("bugfix"), [("developer", 1), ("reviewer", 1)]);
    }
}
