//! What a catalog and an exported template must satisfy before anything runs.
//!
//! A catalog is usable when every template names roles and pipelines that
//! exist, every pipeline it uses has a seat for its implementer and stages,
//! IDs are unique, and no roster is wider than the team's member cap. A
//! bundle is a catalog of one template, so it is checked the same way, plus
//! the cap of the computer that imports it.

use std::collections::{BTreeSet, HashSet};

use crate::catalog::{Catalog, TemplateBundle};
use crate::model::{Acceptance, Caps};
use crate::problem::{code, Problem};

/// The characters a team name never contains (zerus-63rru.74): every
/// control character, the two line separators, and the nine bidirectional
/// embedding, override and isolate characters. The plan kernel keeps its
/// own copy so it stays light: no dependency on the replica, whose Loro
/// tree stays behind crates/replica (zerus-63rru.75 round 2). A root-crate
/// test asserts this list and the replica's agree for every character.
pub fn forbidden_char(value: &str) -> Option<char> {
    value.chars().find(|ch| {
        let point = *ch as u32;
        ch.is_control() || matches!(point, 0x2028 | 0x2029 | 0x202A..=0x202E | 0x2066..=0x2069)
    })
}

/// The characters a team name never contains (zerus-63rru.74): every
/// control character, the two line separators, and the nine bidirectional
/// embedding, override and isolate characters. The existing size limit
/// still applies and reports its own problem.
pub fn name_problem(name: &str, field: &str, limit: usize) -> Option<Problem> {
    if name.len() > limit {
        return Some(Problem::new(code::OVER_CAP, "Text exceeds its size limit."));
    }
    // The same list as the replica's title check (zerus-63rru.74).
    if let Some(ch) = forbidden_char(name) {
        return Some(Problem::new(
            code::INVALID_STATE,
            format!(
                "{field} contains U+{:04X}, which is not allowed.",
                ch as u32
            ),
        ));
    }
    None
}

/// The character rules at admission (zerus-63rru.74 round 2): the save, the
/// import and the creation refuse a name or an ID outside the set, with the
/// sentence naming the template and the field. Reading stays open: a
/// catalog stored by an older build still loads, lists and runs; the next
/// save or import refuses it.
pub fn admission_problems(catalog: &Catalog) -> Vec<Problem> {
    let mut problems = Vec::new();
    for entry in &catalog.templates {
        if let Some(problem) = name_problem(&entry.name, "The template name", 256) {
            problems.push(problem);
        }
        if let Some(mut problem) = id_problem(&entry.id, "The template ID") {
            problem.message = format!("Template {}: {}", entry.name, problem.message);
            problems.push(problem);
        }
    }
    for role in &catalog.roles {
        if let Some(mut problem) = id_problem(&role.id, "The role ID") {
            // The refusal names the template that actually uses the role:
            // the first that seats it, or none — an unused role says so
            // (zerus-63rru.74 round 3).
            problem.message = match catalog
                .templates
                .iter()
                .find(|t| t.roster.iter().any(|seat| seat.role == role.id))
            {
                Some(t) => format!("Template {}: {}", t.name, problem.message),
                None => format!("An unused role: {}", problem.message),
            };
            problems.push(problem);
        }
    }
    for pipeline in &catalog.pipelines {
        if let Some(mut problem) = id_problem(&pipeline.id, "The pipeline ID") {
            // A template uses a pipeline as its default or through a seated
            // role's own pipeline, as its bundle counts it.
            problem.message = match catalog
                .templates
                .iter()
                .find(|t| used_pipelines(catalog, t).contains(pipeline.id.as_str()))
            {
                Some(t) => format!("Template {}: {}", t.name, problem.message),
                None => format!("An unused pipeline: {}", problem.message),
            };
            problems.push(problem);
        }
    }
    problems
}

/// A role or member ID (zerus-63rru.74): ASCII [a-z0-9] first, then
/// [a-z0-9_-], at most 64 characters. No stricter rule existed before;
/// this one is the only character-set rule and the length cap.
pub fn id_problem(id: &str, field: &str) -> Option<Problem> {
    if id.len() > 64 {
        return Some(Problem::new(code::OVER_CAP, "Text exceeds its size limit."));
    }
    let mut chars = id.chars();
    match chars.next() {
        Some(ch) if ch.is_ascii_lowercase() || ch.is_ascii_digit() => {}
        Some(ch) => {
            let point = ch as u32;
            return Some(Problem::new(
                code::INVALID_STATE,
                format!(
                    "{field} contains U+{point:04X}, which is not allowed; IDs are [a-z0-9] then [a-z0-9_-], at most 64 characters."
                ),
            ));
        }
        None => {
            return Some(Problem::new(
                code::INVALID_STATE,
                format!(
                    "{field} is empty; IDs are [a-z0-9] then [a-z0-9_-], at most 64 characters."
                ),
            ));
        }
    }
    for ch in chars {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_') {
            let point = ch as u32;
            return Some(Problem::new(
                code::INVALID_STATE,
                format!(
                    "{field} contains U+{point:04X}, which is not allowed; IDs are [a-z0-9] then [a-z0-9_-], at most 64 characters."
                ),
            ));
        }
    }
    None
}

/// Only the duplicated IDs of one catalog, for a partial catalog that is
/// merged over another: a merge must not hide a duplicate before validation.
pub fn validate_ids(catalog: &Catalog) -> Vec<Problem> {
    let mut problems = Vec::new();
    duplicates(&catalog.roles, "role", |role| &role.id, &mut problems);
    duplicates(
        &catalog.pipelines,
        "pipeline",
        |pipeline| &pipeline.id,
        &mut problems,
    );
    duplicates(
        &catalog.templates,
        "template",
        |template| &template.id,
        &mut problems,
    );
    unique(problems)
}

/// Everything wrong with a catalog, in catalog order. Empty means usable.
pub fn validate_catalog(catalog: &Catalog) -> Vec<Problem> {
    let mut problems = validate_ids(catalog);
    let roles: HashSet<&str> = catalog.roles.iter().map(|role| role.id.as_str()).collect();
    let pipelines: HashSet<&str> = catalog
        .pipelines
        .iter()
        .map(|pipeline| pipeline.id.as_str())
        .collect();

    // A role names a pipeline even when no template seats it today.
    for role in &catalog.roles {
        if !role.pipeline.is_empty() && !pipelines.contains(role.pipeline.as_str()) {
            problems.push(
                Problem::new(
                    code::UNKNOWN_PIPELINE,
                    format!(
                        "Role {} names a pipeline that is not in the catalog: {}.",
                        role.id, role.pipeline
                    ),
                )
                .with_refs([role.id.as_str(), role.pipeline.as_str()]),
            );
        }
    }

    for pipeline in &catalog.pipelines {
        if !roles.contains(pipeline.implementer.as_str()) {
            problems.push(
                Problem::new(
                    code::UNKNOWN_ROLE,
                    format!(
                        "Pipeline {} names an implementer role that is not in the catalog: {}.",
                        pipeline.id, pipeline.implementer
                    ),
                )
                .with_refs([pipeline.id.as_str(), pipeline.implementer.as_str()]),
            );
        }
        for stage in &pipeline.stages {
            if !roles.contains(stage.role.as_str()) {
                problems.push(
                    Problem::new(
                        code::UNKNOWN_ROLE,
                        format!(
                            "Pipeline {} has a stage role that is not in the catalog: {}.",
                            pipeline.id, stage.role
                        ),
                    )
                    .with_refs([pipeline.id.as_str(), stage.role.as_str()]),
                );
            }
        }
    }

    for template in &catalog.templates {
        let mut seats = HashSet::new();
        let mut members = 0u32;
        for seat in &template.roster {
            if seat.count > 0 {
                seats.insert(seat.role.as_str());
            }
            members = members.saturating_add(seat.count);
            if !roles.contains(seat.role.as_str()) {
                problems.push(
                    Problem::new(
                        code::UNKNOWN_ROLE,
                        format!(
                            "Template {} has a seat for a role that is not in the catalog: {}.",
                            template.id, seat.role
                        ),
                    )
                    .with_refs([template.id.as_str(), seat.role.as_str()]),
                );
            }
        }
        if members > template.settings.caps.max_members {
            problems.push(
                Problem::new(
                    code::OVER_CAP,
                    format!(
                        "Template {} has {} members but its member cap is {}.",
                        template.id, members, template.settings.caps.max_members
                    ),
                )
                .with_refs([template.id.as_str(), "roster"]),
            );
        }
        if template.settings.phase_acceptance == Acceptance::Pipeline {
            problems.push(
                Problem::new(
                    code::INVALID_STATE,
                    format!(
                        "Template {} accepts a phase by its pipeline; a phase is accepted by the lead or by the person.",
                        template.id
                    ),
                )
                .with_refs([template.id.as_str(), "phase_acceptance"]),
            );
        }

        if template.default_pipeline.is_empty() {
            problems.push(
                Problem::new(
                    code::UNKNOWN_PIPELINE,
                    format!("Template {} has no default pipeline.", template.id),
                )
                .with_refs([template.id.as_str(), "default_pipeline"]),
            );
        } else if !pipelines.contains(template.default_pipeline.as_str()) {
            problems.push(
                Problem::new(
                    code::UNKNOWN_PIPELINE,
                    format!(
                        "Template {} has a default pipeline that is not in the catalog: {}.",
                        template.id, template.default_pipeline
                    ),
                )
                .with_refs([template.id.as_str(), template.default_pipeline.as_str()]),
            );
        }

        for pipeline_id in active_pipelines(catalog, template) {
            let Some(pipeline) = catalog
                .pipelines
                .iter()
                .find(|pipeline| pipeline.id == pipeline_id)
            else {
                // The missing default or role pipeline was reported above.
                continue;
            };
            for role in std::iter::once(&pipeline.implementer)
                .chain(pipeline.stages.iter().map(|stage| &stage.role))
            {
                if roles.contains(role.as_str()) && !seats.contains(role.as_str()) {
                    problems.push(
                        Problem::new(
                            code::NO_SEAT_FOR_STAGE,
                            format!(
                                "Template {} uses pipeline {} but has no seat for its role {}.",
                                template.id, pipeline.id, role
                            ),
                        )
                        .with_refs([
                            template.id.as_str(),
                            pipeline.id.as_str(),
                            role.as_str(),
                        ]),
                    );
                }
            }
        }
    }

    unique(problems)
}

/// The pipelines a template's tasks may use: its default and each roster
/// role's own default. A bundle carries them all so an export stays complete.
fn used_pipelines<'a>(
    catalog: &'a Catalog,
    template: &'a crate::catalog::Template,
) -> BTreeSet<&'a str> {
    let mut used = BTreeSet::new();
    if !template.default_pipeline.is_empty() {
        used.insert(template.default_pipeline.as_str());
    }
    for seat in &template.roster {
        if let Some(role) = catalog.roles.iter().find(|role| role.id == seat.role) {
            if !role.pipeline.is_empty() {
                used.insert(role.pipeline.as_str());
            }
        }
    }
    used
}

/// The pipelines a template's tasks may use in this validation: its default
/// and the default of each role it actually seats. A seat with count 0 does
/// not make its role a pipeline user.
fn active_pipelines<'a>(
    catalog: &'a Catalog,
    template: &'a crate::catalog::Template,
) -> BTreeSet<&'a str> {
    let mut used = BTreeSet::new();
    if !template.default_pipeline.is_empty() {
        used.insert(template.default_pipeline.as_str());
    }
    for seat in &template.roster {
        if seat.count == 0 {
            continue;
        }
        if let Some(role) = catalog.roles.iter().find(|role| role.id == seat.role) {
            if !role.pipeline.is_empty() {
                used.insert(role.pipeline.as_str());
            }
        }
    }
    used
}

/// Everything wrong with a bundle imported from elsewhere. `caps` is the
/// widest a team on this computer may be.
pub fn validate_bundle(bundle: &TemplateBundle, caps: &Caps) -> Vec<Problem> {
    let mut problems = Vec::new();
    let template = bundle.template.name.as_str();
    if let Some(problem) = name_problem(&bundle.template.name, "The template name", 256) {
        problems.push(problem);
    }
    if let Some(mut problem) = id_problem(&bundle.template.id, "The template ID") {
        problem.message = format!("Template {template}: {}", problem.message);
        problems.push(problem);
    }
    for role in &bundle.roles {
        if let Some(mut problem) = id_problem(&role.id, "The role ID") {
            problem.message = format!("Template {template}: {}", problem.message);
            problems.push(problem);
        }
    }
    for pipeline in &bundle.pipelines {
        if let Some(mut problem) = id_problem(&pipeline.id, "The pipeline ID") {
            problem.message = format!("Template {template}: {}", problem.message);
            problems.push(problem);
        }
    }
    problems.extend(validate_bundle_inner(bundle, caps));
    problems
}
fn validate_bundle_inner(bundle: &TemplateBundle, caps: &Caps) -> Vec<Problem> {
    let catalog = Catalog {
        roles: bundle.roles.clone(),
        pipelines: bundle.pipelines.clone(),
        templates: vec![bundle.template.clone()],
        planning_rules: Vec::new(),
    };
    let mut problems = validate_catalog(&catalog);
    let members = bundle
        .template
        .roster
        .iter()
        .map(|seat| seat.count)
        .fold(0u32, u32::saturating_add);
    if members > caps.max_members {
        problems.push(
            Problem::new(
                code::OVER_CAP,
                format!(
                    "Bundle {} has {} members but this computer allows {}.",
                    bundle.template.id, members, caps.max_members
                ),
            )
            .with_refs([bundle.template.id.as_str(), "roster"]),
        );
    }
    if bundle.template.settings.caps.max_members > caps.max_members {
        problems.push(
            Problem::new(
                code::OVER_CAP,
                format!(
                    "Bundle {} allows {} members but this computer allows {}.",
                    bundle.template.id, bundle.template.settings.caps.max_members, caps.max_members
                ),
            )
            .with_refs([bundle.template.id.as_str(), "settings.caps.max_members"]),
        );
    }
    unique(problems)
}

/// One template with exactly the roles and pipelines it uses.
pub fn bundle(catalog: &Catalog, template: &str) -> Result<TemplateBundle, Problem> {
    let id = template;
    let template = catalog
        .templates
        .iter()
        .find(|template| template.id == id)
        .cloned()
        .ok_or_else(|| {
            Problem::new(
                code::NOT_FOUND,
                format!("The catalog has no template with ID {id}."),
            )
            .with_refs([id])
        })?;

    if template.default_pipeline.is_empty() {
        return Err(Problem::new(
            code::UNKNOWN_PIPELINE,
            format!("Template {id} has no default pipeline."),
        )
        .with_refs([id, "default_pipeline"]));
    }

    let mut role_ids = BTreeSet::new();
    for seat in &template.roster {
        role_ids.insert(seat.role.clone());
    }
    let pipeline_ids = used_pipelines(catalog, &template)
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();

    let mut pipelines = Vec::new();
    for pipeline in &catalog.pipelines {
        if pipeline_ids.contains(&pipeline.id) {
            role_ids.insert(pipeline.implementer.clone());
            for stage in &pipeline.stages {
                role_ids.insert(stage.role.clone());
            }
            pipelines.push(pipeline.clone());
        }
    }

    let mut roles = Vec::new();
    for role in &catalog.roles {
        if role_ids.remove(&role.id) {
            roles.push(role.clone());
        }
    }
    if let Some(missing) = role_ids.iter().next() {
        return Err(Problem::new(
            code::UNKNOWN_ROLE,
            format!("Template {id} uses a role that is not in the catalog: {missing}."),
        )
        .with_refs([id, missing.as_str()]));
    }
    let found = pipelines
        .iter()
        .map(|pipeline| pipeline.id.as_str())
        .collect::<BTreeSet<_>>();
    if let Some(missing) = pipeline_ids
        .iter()
        .find(|pipeline| !found.contains(pipeline.as_str()))
    {
        return Err(Problem::new(
            code::UNKNOWN_PIPELINE,
            format!("Template {id} uses a pipeline that is not in the catalog: {missing}."),
        )
        .with_refs([id, missing.as_str()]));
    }

    Ok(TemplateBundle {
        template,
        roles,
        pipelines,
    })
}

fn duplicates<T>(items: &[T], kind: &str, id: impl Fn(&T) -> &str, problems: &mut Vec<Problem>) {
    let mut seen = HashSet::new();
    for item in items {
        let id = id(item);
        if !seen.insert(id.to_owned()) {
            problems.push(
                Problem::new(
                    code::DUPLICATE,
                    format!("The catalog has more than one {kind} with ID {id}."),
                )
                .with_refs([kind, id]),
            );
        }
    }
}

/// Leaves one copy of a problem that arose from several checks.
fn unique(problems: Vec<Problem>) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    for problem in problems {
        if !out.contains(&problem) {
            out.push(problem);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::builtin;
    use crate::catalog::Seat;
    use crate::model::Acceptance;
    use crate::problem::Problem;

    fn has(problems: &[Problem], code: &str, needle: &str) -> bool {
        problems
            .iter()
            .any(|problem| problem.code == code && problem.refs.iter().any(|r| r == needle))
    }

    fn ids<T>(items: &[T], id: impl Fn(&T) -> &str) -> Vec<String> {
        items.iter().map(|item| id(item).to_owned()).collect()
    }

    #[test]
    fn a_bad_pipeline_id_names_the_template_whose_seated_role_uses_it() {
        // zerus-63rru.74 round 4: a pipeline reached only through a seated
        // role's own pipeline is used by that template, not "unused".
        let mut catalog = builtin();
        let mut pipeline = catalog.pipelines[0].clone();
        pipeline.id = "pipeline id".into();
        catalog.pipelines.push(pipeline);
        let mut role = catalog.roles[0].clone();
        role.id = "own".into();
        role.pipeline = "pipeline id".into();
        catalog.roles.push(role);
        let template = catalog.templates[0].name.clone();
        catalog.templates[0].roster.push(Seat {
            role: "own".into(),
            count: 1,
            optional: false,
        });
        let problems = admission_problems(&catalog);
        let message = problems
            .iter()
            .map(|problem| problem.message.as_str())
            .find(|message| message.contains("pipeline ID"))
            .expect("the pipeline ID is refused");
        assert!(
            message.starts_with(&format!("Template {template}: ")),
            "{message}"
        );

        // Unseated, the same role no longer uses it: the pipeline is unused.
        catalog.templates[0].roster.pop();
        let problems = admission_problems(&catalog);
        let message = problems
            .iter()
            .map(|problem| problem.message.as_str())
            .find(|message| message.contains("pipeline ID"))
            .expect("the pipeline ID is refused");
        assert!(message.starts_with("An unused pipeline: "), "{message}");
    }

    #[test]
    fn a_valid_catalog_has_no_problems() {
        assert!(validate_catalog(&builtin()).is_empty());
    }

    #[test]
    fn duplicate_ids_are_refused() {
        let mut catalog = builtin();
        let role = catalog.roles[0].clone();
        catalog.roles.push(role);
        assert!(has(
            &validate_catalog(&catalog),
            code::DUPLICATE,
            "developer"
        ));

        let mut catalog = builtin();
        let pipeline = catalog.pipelines[0].clone();
        catalog.pipelines.push(pipeline);
        assert!(has(&validate_catalog(&catalog), code::DUPLICATE, "code"));

        let mut catalog = builtin();
        let template = catalog.templates[0].clone();
        catalog.templates.push(template);
        assert!(has(
            &validate_catalog(&catalog),
            code::DUPLICATE,
            "feature-delivery"
        ));
    }

    #[test]
    fn an_unknown_seat_role_is_refused() {
        let mut catalog = builtin();
        catalog.templates[0].roster.push(Seat {
            role: "ghost".into(),
            count: 1,
            optional: false,
        });
        assert!(has(
            &validate_catalog(&catalog),
            code::UNKNOWN_ROLE,
            "ghost"
        ));
    }

    #[test]
    fn an_unknown_default_pipeline_is_refused() {
        let mut catalog = builtin();
        catalog.templates[0].default_pipeline = "ghost".into();
        assert!(has(
            &validate_catalog(&catalog),
            code::UNKNOWN_PIPELINE,
            "ghost"
        ));
    }

    #[test]
    fn a_role_naming_an_unknown_pipeline_is_refused() {
        let mut catalog = builtin();
        catalog.roles[0].pipeline = "ghost".into();
        assert!(has(
            &validate_catalog(&catalog),
            code::UNKNOWN_PIPELINE,
            "ghost"
        ));
    }

    #[test]
    fn an_unknown_pipeline_implementer_is_refused() {
        let mut catalog = builtin();
        catalog.pipelines[0].implementer = "ghost".into();
        assert!(has(
            &validate_catalog(&catalog),
            code::UNKNOWN_ROLE,
            "ghost"
        ));
    }

    #[test]
    fn an_unknown_stage_role_is_refused() {
        let mut catalog = builtin();
        catalog.pipelines[0].stages[0].role = "ghost".into();
        assert!(has(
            &validate_catalog(&catalog),
            code::UNKNOWN_ROLE,
            "ghost"
        ));
    }

    #[test]
    fn a_stage_with_no_seat_in_the_template_is_refused() {
        let mut catalog = builtin();
        catalog.templates[1]
            .roster
            .retain(|seat| seat.role != "reviewer");
        assert!(has(
            &validate_catalog(&catalog),
            code::NO_SEAT_FOR_STAGE,
            "reviewer"
        ));
    }

    #[test]
    fn an_implementer_with_no_seat_in_the_template_is_refused() {
        let mut catalog = builtin();
        catalog.templates[1]
            .roster
            .retain(|seat| seat.role != "developer");
        assert!(has(
            &validate_catalog(&catalog),
            code::NO_SEAT_FOR_STAGE,
            "developer"
        ));
    }

    #[test]
    fn a_seat_with_count_zero_does_not_count_as_seated() {
        let mut catalog = builtin();
        catalog.templates[1]
            .roster
            .iter_mut()
            .find(|seat| seat.role == "reviewer")
            .expect("the bugfix reviewer seat")
            .count = 0;
        assert!(has(
            &validate_catalog(&catalog),
            code::NO_SEAT_FOR_STAGE,
            "reviewer"
        ));

        let mut catalog = builtin();
        catalog.templates[1]
            .roster
            .iter_mut()
            .find(|seat| seat.role == "developer")
            .expect("the bugfix developer seat")
            .count = 0;
        assert!(has(
            &validate_catalog(&catalog),
            code::NO_SEAT_FOR_STAGE,
            "developer"
        ));
    }

    #[test]
    fn an_empty_default_pipeline_is_refused() {
        let mut catalog = builtin();
        catalog.templates[1].default_pipeline.clear();
        assert!(has(
            &validate_catalog(&catalog),
            code::UNKNOWN_PIPELINE,
            "bugfix"
        ));
        let problem = bundle(&catalog, "bugfix").expect_err("cannot export");
        assert_eq!(problem.code, code::UNKNOWN_PIPELINE);
    }

    #[test]
    fn a_role_outside_every_roster_naming_an_unknown_pipeline_is_refused() {
        let mut catalog = builtin();
        let mut role = catalog.roles[0].clone();
        role.id = "architect".into();
        role.name = "Architect".into();
        role.pipeline = "ghost".into();
        catalog.roles.push(role);
        assert!(has(
            &validate_catalog(&catalog),
            code::UNKNOWN_PIPELINE,
            "ghost"
        ));
    }

    #[test]
    fn duplicate_ids_are_found_in_each_kind_on_their_own() {
        let mut catalog = builtin();
        catalog.roles.push(catalog.roles[0].clone());
        assert!(has(&validate_ids(&catalog), code::DUPLICATE, "developer"));

        let mut catalog = builtin();
        catalog.pipelines.push(catalog.pipelines[0].clone());
        assert!(has(&validate_ids(&catalog), code::DUPLICATE, "code"));

        let mut catalog = builtin();
        catalog.templates.push(catalog.templates[0].clone());
        assert!(has(
            &validate_ids(&catalog),
            code::DUPLICATE,
            "feature-delivery"
        ));
    }

    #[test]
    fn a_roster_over_the_template_cap_is_refused() {
        let mut catalog = builtin();
        catalog.templates[1].settings.caps.max_members = 1;
        assert!(has(&validate_catalog(&catalog), code::OVER_CAP, "bugfix"));
    }

    #[test]
    fn pipeline_acceptance_is_refused_as_a_phase_acceptance() {
        let mut catalog = builtin();
        catalog.templates[1].settings.phase_acceptance = Acceptance::Pipeline;
        let problems = validate_catalog(&catalog);
        assert!(has(&problems, code::INVALID_STATE, "phase_acceptance"));
    }

    #[test]
    fn a_bundle_keeps_only_the_roles_and_pipelines_the_template_uses() {
        let bugfix = bundle(&builtin(), "bugfix").expect("bugfix bundles");
        assert_eq!(bugfix.template.id, "bugfix");
        assert_eq!(
            ids(&bugfix.roles, |role| &role.id),
            ["developer", "reviewer"]
        );
        assert_eq!(
            ids(&bugfix.pipelines, |pipeline| &pipeline.id),
            ["reviewed"]
        );

        let feature = bundle(&builtin(), "feature-delivery").expect("feature bundles");
        assert_eq!(
            ids(&feature.roles, |role| &role.id),
            ["developer", "reviewer", "tester"]
        );
        assert_eq!(ids(&feature.pipelines, |pipeline| &pipeline.id), ["code"]);
    }

    #[test]
    fn the_exported_bugfix_bundle_equals_the_fixture() {
        let fixture: TemplateBundle =
            serde_json::from_str(include_str!("../fixtures/bundle-bugfix.json"))
                .expect("fixture parses");
        assert_eq!(
            bundle(&builtin(), "bugfix").expect("bugfix bundles"),
            fixture
        );
    }

    #[test]
    fn an_unknown_template_is_refused() {
        let problem = bundle(&builtin(), "ghost").expect_err("no such template");
        assert_eq!(problem.code, code::NOT_FOUND);
        assert_eq!(problem.refs, ["ghost"]);
    }

    #[test]
    fn a_bundle_is_complete_on_an_empty_catalog() {
        let original = bundle(&builtin(), "bugfix").expect("bugfix bundles");
        let only = Catalog {
            roles: original.roles.clone(),
            pipelines: original.pipelines.clone(),
            templates: vec![original.template.clone()],
            planning_rules: Vec::new(),
        };
        assert!(validate_catalog(&only).is_empty());
        assert_eq!(bundle(&only, "bugfix").expect("bundles again"), original);
    }

    #[test]
    fn a_bundle_over_the_callers_cap_is_refused() {
        let bundle = bundle(&builtin(), "bugfix").expect("bugfix bundles");
        let caps = Caps {
            max_members: 1,
            ..Caps::default()
        };
        assert!(has(
            &validate_bundle(&bundle, &caps),
            code::OVER_CAP,
            "bugfix"
        ));
    }

    #[test]
    fn a_template_that_widens_the_member_cap_is_refused() {
        let mut bundle = bundle(&builtin(), "bugfix").expect("bugfix bundles");
        bundle.template.settings.caps.max_members = Caps::default().max_members + 1;
        assert!(has(
            &validate_bundle(&bundle, &Caps::default()),
            code::OVER_CAP,
            "bugfix"
        ));
    }
}
