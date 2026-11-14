//! Milestone policy shares the caller proof's resident-host classification.
use zerus_team_plan::{
    catalog::{Catalog, TemplateBundle},
    problem::code,
    Problem,
};

/// The exact agent kind names a team seat may be launched with in this
/// milestone. A name is matched exactly: another spelling of a known kind
/// would save a seat that can never launch, so it is refused here.
const KNOWN_AGENTS: [&str; 4] = ["claude", "codex", "kimi", "dsh"];

pub(crate) fn validate_roster(bundle: &TemplateBundle) -> Result<(), Problem> {
    let template = bundle.template.id.as_str();
    agent_of(Some(template), "lead.agent", &bundle.template.lead.agent)?;
    for seat in &bundle.template.roster {
        if let Some(role) = bundle.roles.iter().find(|role| role.id == seat.role) {
            let field = format!("roles.{}.agent", role.id);
            agent_of(Some(template), &field, &role.agent)?;
        }
    }
    Ok(())
}

/// Every role's agent field in a whole catalog, seated or not. This runs
/// before a save prunes each template's bundle, so a role that no template
/// seats is checked too; a seated role still names its template.
pub(crate) fn validate_catalog_roles(catalog: &Catalog) -> Result<(), Problem> {
    for role in &catalog.roles {
        let field = format!("roles.{}.agent", role.id);
        let template = catalog
            .templates
            .iter()
            .find(|template| template.roster.iter().any(|seat| seat.role == role.id))
            .map(|template| template.id.as_str());
        agent_of(template, &field, &role.agent)?;
    }
    Ok(())
}

/// One agent field of one catalog or template. A refusal names the field and,
/// when a template seats the role, that template, so a later save or import
/// says which older template blocks it and which field to correct. An unseated
/// role has no template to name.
fn agent_of(template: Option<&str>, field: &str, agent: &str) -> Result<(), Problem> {
    if KNOWN_AGENTS.contains(&agent) {
        return Ok(());
    }
    let of_template = match template {
        Some(template) => format!(" of template '{template}'"),
        None => String::new(),
    };
    if crate::teams::caller::host_served_agent(agent) {
        return Err(Problem::new(
            code::INVALID_STATE,
            format!("The agent field '{field}'{of_template} names '{agent}', a session of agent kind '{agent}' served by a resident agent host; such a session cannot be a team lead or member in this milestone."),
        )
        .with_refs([agent, field]));
    }
    Err(Problem::new(
        code::INVALID_STATE,
        format!("The agent field '{field}'{of_template} names '{agent}', which is not a known agent kind; a team seat's agent must be exactly claude, codex, kimi or dsh."),
    )
    .with_refs([agent, field]))
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use zerus_team_plan::catalog::{Role, Seat};
    fn catalog_with(agent: &str, lead: bool) -> Catalog {
        let mut catalog = builtin();
        if lead {
            for template in &mut catalog.templates {
                template.lead.agent = agent.into();
            }
        } else {
            catalog
                .roles
                .iter_mut()
                .find(|r| r.id == "developer")
                .unwrap()
                .agent = agent.into();
        }
        catalog
    }
    fn older_person_with(agent: &str, lead: bool) -> Catalog {
        let bundle = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        let mut template = bundle.template;
        template.id = "legacy-bugfix".into();
        template.name = "Legacy bugfix".into();
        let mut roles = bundle.roles;
        if lead {
            template.lead.agent = agent.into();
        } else {
            let mut role = roles
                .iter()
                .find(|role| role.id == "developer")
                .unwrap()
                .clone();
            role.id = "legacy-member".into();
            role.name = "Legacy member".into();
            role.agent = agent.into();
            template.roster.push(Seat {
                role: role.id.clone(),
                count: 1,
                optional: false,
            });
            roles.push(role);
        }
        Catalog {
            roles,
            pipelines: bundle.pipelines,
            templates: vec![template],
            planning_rules: Vec::new(),
        }
    }
    fn saved_person(dir: &Path, catalog: &Catalog) -> Vec<u8> {
        let file = path(dir);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let bytes = serde_json::to_vec(catalog).unwrap();
        std::fs::write(&file, &bytes).unwrap();
        bytes
    }
    fn unseated_role(agent: &str) -> Role {
        let mut role = builtin().roles[0].clone();
        role.id = "architect".into();
        role.name = "Architect".into();
        role.agent = agent.into();
        role.pipeline.clear();
        role
    }
    fn person_with_unseated(agent: &str) -> Catalog {
        Catalog {
            roles: vec![unseated_role(agent)],
            ..Catalog::default()
        }
    }
    fn refuse_agent(agent: &str, lead: bool) -> Problem {
        let dir = tempfile::tempdir().unwrap();
        let problem = save(dir.path(), &catalog_with(agent, lead)).unwrap_err();
        assert_eq!(problem.code, code::INVALID_STATE);
        assert!(problem.message.contains("agent field"), "{problem:?}");
        assert!(
            problem.message.contains(&format!("'{agent}'")),
            "{problem:?}"
        );
        assert!(!path(dir.path()).exists(), "nothing is written");
        problem
    }
    #[test]
    fn saving_a_template_refuses_host_served_leads_and_members() {
        {
            let agent = "opencode";
            for lead in [true, false] {
                let problem = refuse_agent(agent, lead);
                assert!(problem.message.contains("served by a resident agent host"));
            }
        }
    }
    #[test]
    fn any_other_spelling_is_refused_and_names_the_agent_field() {
        for agent in [
            "Claude",
            "CLAUDE",
            " claude",
            "claude ",
            "DeepSeek",
            "deepseek",
            "OpenCode",
            " opencode",
            "opencode ",
            "Kimi",
        ] {
            for lead in [true, false] {
                let problem = refuse_agent(agent, lead);
                assert!(
                    problem.message.contains("not a known agent kind"),
                    "{problem:?}"
                );
            }
        }
    }
    #[test]
    fn every_exact_known_agent_kind_is_saved_and_imported() {
        for agent in ["claude", "codex", "kimi", "dsh"] {
            for lead in [true, false] {
                let dir = tempfile::tempdir().unwrap();
                save(dir.path(), &catalog_with(agent, lead)).unwrap();
                let catalog = load(dir.path()).unwrap();
                export_bundle(&catalog, "bugfix").unwrap();
                let imported = tempfile::tempdir().unwrap();
                import_bundle(imported.path(), &older_person_bundle(agent, lead)).unwrap();
                assert!(path(imported.path()).exists());
            }
        }
    }
    fn older_person_bundle(agent: &str, lead: bool) -> TemplateBundle {
        let mut bundle = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        if lead {
            bundle.template.lead.agent = agent.into();
        } else {
            bundle
                .roles
                .iter_mut()
                .find(|role| role.id == "developer")
                .unwrap()
                .agent = agent.into();
        }
        bundle
    }
    #[test]
    fn using_an_older_saved_template_refuses_a_host_served_roster() {
        {
            let agent = "opencode";
            for lead in [true, false] {
                let dir = tempfile::tempdir().unwrap();
                let file = path(dir.path());
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                let bytes = serde_json::to_vec(&catalog_with(agent, lead)).unwrap();
                std::fs::write(&file, &bytes).unwrap();
                let catalog = load(dir.path()).unwrap();
                let problem = export_bundle(&catalog, "bugfix").unwrap_err();
                assert_eq!(problem.code, code::INVALID_STATE);
                assert!(problem.message.contains("served by a resident agent host"));
                assert_eq!(std::fs::read(file).unwrap(), bytes, "no repair on read");
            }
        }
    }
    #[test]
    fn a_later_save_or_import_names_an_older_saved_template_with_a_refused_agent() {
        for lead in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let bytes = saved_person(dir.path(), &older_person_with("opencode", lead));
            let mut person = person(dir.path()).unwrap();
            let mut extra = builtin().roles[0].clone();
            extra.id = "architect".into();
            extra.name = "Architect".into();
            person.roles.push(extra);
            let problem = save(dir.path(), &person).unwrap_err();
            assert_eq!(problem.code, code::INVALID_STATE);
            assert!(problem.message.contains("'legacy-bugfix'"), "{problem:?}");
            assert!(problem.message.contains("'opencode'"), "{problem:?}");
            assert_eq!(std::fs::read(path(dir.path())).unwrap(), bytes, "no rewrite");
            let bundle = export_bundle(&builtin(), "feature-delivery").unwrap();
            let problem = import_bundle(dir.path(), &bundle).unwrap_err();
            assert_eq!(problem.code, code::INVALID_STATE);
            assert!(problem.message.contains("'legacy-bugfix'"), "{problem:?}");
            assert_eq!(std::fs::read(path(dir.path())).unwrap(), bytes, "no rewrite");
        }
    }
    #[test]
    fn saving_refuses_an_unseated_role_with_a_refused_agent() {
        for agent in ["DeepSeek", "opencode"] {
            let dir = tempfile::tempdir().unwrap();
            let problem = save(dir.path(), &person_with_unseated(agent)).unwrap_err();
            assert_eq!(problem.code, code::INVALID_STATE);
            assert!(
                problem.message.contains("'roles.architect.agent'"),
                "{problem:?}"
            );
            assert!(problem.message.contains(&format!("'{agent}'")), "{problem:?}");
            assert!(!path(dir.path()).exists(), "nothing is written for {agent}");
        }
    }
    #[test]
    fn saving_keeps_an_unseated_role_with_an_exact_known_agent() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &person_with_unseated("dsh")).unwrap();
        assert!(path(dir.path()).exists());
    }
    #[test]
    fn a_later_save_or_import_names_an_older_saved_unseated_role_with_a_refused_agent() {
        for agent in ["DeepSeek", "opencode"] {
            let dir = tempfile::tempdir().unwrap();
            let bytes = saved_person(dir.path(), &person_with_unseated(agent));
            let mut person = person(dir.path()).unwrap();
            let mut extra = builtin().roles[1].clone();
            extra.id = "reviewer-two".into();
            extra.name = "Reviewer two".into();
            extra.pipeline.clear();
            person.roles.push(extra);
            let problem = save(dir.path(), &person).unwrap_err();
            assert_eq!(problem.code, code::INVALID_STATE);
            assert!(
                problem.message.contains("'roles.architect.agent'"),
                "{problem:?}"
            );
            assert!(problem.message.contains(&format!("'{agent}'")), "{problem:?}");
            assert_eq!(
                std::fs::read(path(dir.path())).unwrap(),
                bytes,
                "no rewrite on save"
            );
            let bundle = export_bundle(&builtin(), "feature-delivery").unwrap();
            let problem = import_bundle(dir.path(), &bundle).unwrap_err();
            assert_eq!(problem.code, code::INVALID_STATE);
            assert!(
                problem.message.contains("'roles.architect.agent'"),
                "{problem:?}"
            );
            assert!(problem.message.contains(&format!("'{agent}'")), "{problem:?}");
            assert_eq!(
                std::fs::read(path(dir.path())).unwrap(),
                bytes,
                "no rewrite on import"
            );
        }
    }
    #[test]
    fn every_builtin_roster_uses_agents_the_caller_proof_can_bind() {
        let catalog = builtin();
        for template in &catalog.templates {
            assert!(!crate::teams::caller::host_served_agent(
                &template.lead.agent
            ));
            for seat in &template.roster {
                let role = catalog.roles.iter().find(|r| r.id == seat.role).unwrap();
                assert!(!crate::teams::caller::host_served_agent(&role.agent));
            }
            export_bundle(&catalog, &template.id).unwrap();
        }
    }
    #[test]
    fn deepseek_leads_and_members_can_be_saved_and_selected() {
        for lead in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            save(dir.path(), &catalog_with("dsh", lead)).unwrap();
            let catalog = load(dir.path()).unwrap();
            export_bundle(&catalog, "bugfix").unwrap();
        }
    }
}
