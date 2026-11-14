//! A person's catalog on disk, under the `hgs` configuration directory.
//!
//! The file holds only the person's own entries; [`load`] merges them over the
//! built-in catalog by ID. Nothing here creates the file except an explicit
//! save or import.
#![cfg_attr(not(test), allow(dead_code))] // The `hgs team` CLI in package 5 is the caller.

mod digest;
mod roster;
pub(crate) use digest::template_digest;
pub(crate) use roster::{validate_catalog_roles, validate_roster};

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use zerus_team_plan::catalog::{Catalog, TemplateBundle};
use zerus_team_plan::model::Caps;
use zerus_team_plan::problem::code;
use zerus_team_plan::{builtin, bundle, validate_bundle, validate_catalog, validate_ids, Problem};

/// The largest encoded bundle a create request accepts (`validate_create`).
/// A template over it can never be created, so `catalog show` lists it without
/// spending digest work.
pub(crate) const CREATE_BUNDLE_LIMIT: usize = 131_072;

/// The folder that holds the catalog below the configuration directory.
const DIR: &str = "teams";
/// The one JSON file a person edits and the designer saves.
const FILE: &str = "catalog.json";

/// Where the person's catalog lives.
pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(DIR).join(FILE)
}

/// Only the person's own entries, or an empty catalog when there is no file.
pub fn person(config_dir: &Path) -> Result<Catalog, Problem> {
    match std::fs::read_to_string(path(config_dir)) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Catalog::default()),
        Err(error) => Err(read_error(error)),
    }
}

/// The built-in catalog with the person's entries merged over it by ID,
/// checked so a broken file is refused instead of shown.
pub fn load(config_dir: &Path) -> Result<Catalog, Problem> {
    let person = person(config_dir)?;
    first_problem(validate_ids(&person))?;
    let catalog = merge(&builtin(), &person);
    first_problem(validate_catalog(&catalog))?;
    Ok(catalog)
}

/// A base catalog with the second catalog's entries replacing entries of the
/// same ID and new ones appended. Empty planning rules keep the base's.
pub fn merge(base: &Catalog, person: &Catalog) -> Catalog {
    Catalog {
        roles: merge_by_id(&base.roles, &person.roles, |role| &role.id),
        pipelines: merge_by_id(&base.pipelines, &person.pipelines, |pipeline| &pipeline.id),
        templates: merge_by_id(&base.templates, &person.templates, |template| &template.id),
        planning_rules: if person.planning_rules.is_empty() {
            base.planning_rules.clone()
        } else {
            person.planning_rules.clone()
        },
    }
}

fn merge_by_id<T: Clone>(base: &[T], over: &[T], id: impl Fn(&T) -> &str) -> Vec<T> {
    let mut out = base.to_vec();
    for item in over {
        match out.iter().position(|existing| id(existing) == id(item)) {
            Some(index) => out[index] = item.clone(),
            None => out.push(item.clone()),
        }
    }
    out
}

/// Replaces the person's catalog. Writes nothing unless the merged catalog is
/// valid.
pub fn save(config_dir: &Path, catalog: &Catalog) -> Result<(), Problem> {
    first_problem(validate_ids(catalog))?;
    let merged = merge(&builtin(), catalog);
    first_problem(validate_catalog(&merged))?;
    // The character rules apply at admission (zerus-63rru.74 round 2): the
    // save and the import refuse, the load stays open.
    first_problem(zerus_team_plan::admission_problems(&merged))?;
    validate_catalog_roles(&merged)?;
    for template in &merged.templates {
        export_bundle(&merged, &template.id)?;
    }
    write(config_dir, catalog)
}

/// Adds a bundle's template, roles and pipelines to the person's catalog.
/// Writes nothing when the bundle is incomplete or over this computer's cap.
pub fn import_bundle(config_dir: &Path, bundle: &TemplateBundle) -> Result<(), Problem> {
    // The import is an admission (zerus-63rru.74 round 2).
    first_problem(validate_bundle(bundle, &Caps::default()))?;
    first_problem(zerus_team_plan::admission_problems(&Catalog {
        roles: bundle.roles.clone(),
        pipelines: bundle.pipelines.clone(),
        templates: vec![bundle.template.clone()],
        planning_rules: Vec::new(),
    }))?;
    let mut person = person(config_dir)?;
    first_problem(validate_ids(&person))?;
    person.templates = merge_by_id(
        &person.templates,
        std::slice::from_ref(&bundle.template),
        |template| &template.id,
    );
    person.roles = merge_by_id(&person.roles, &bundle.roles, |role| &role.id);
    person.pipelines = merge_by_id(&person.pipelines, &bundle.pipelines, |pipeline| {
        &pipeline.id
    });
    save(config_dir, &person)
}

/// One template with exactly the roles and pipelines it uses.
pub fn export_bundle(catalog: &Catalog, template: &str) -> Result<TemplateBundle, Problem> {
    let bundle = bundle(catalog, template)?;
    validate_roster(&bundle)?;
    Ok(bundle)
}

fn write(config_dir: &Path, catalog: &Catalog) -> Result<(), Problem> {
    let directory = config_dir.join(DIR);
    crate::platform::private_dir(&directory).map_err(write_error)?;
    let text = serde_json::to_string_pretty(catalog)
        .map_err(|error| write_error(io::Error::other(error.to_string())))?;
    let mut temporary = tempfile::NamedTempFile::new_in(&directory).map_err(write_error)?;
    temporary.write_all(text.as_bytes()).map_err(write_error)?;
    temporary.write_all(b"\n").map_err(write_error)?;
    temporary.as_file().sync_all().map_err(write_error)?;
    crate::platform::persist(temporary, path(config_dir))
        .map_err(|error| write_error(error.error))?;
    Ok(())
}

fn parse(text: &str) -> Result<Catalog, Problem> {
    serde_json::from_str(text).map_err(|error| {
        let message = error.to_string();
        match unknown_field(&message) {
            Some(field) => Problem::new(
                code::UNKNOWN_FIELD,
                format!("The team catalog has an unknown field: {field}."),
            )
            .with_refs([field]),
            None => Problem::new(
                code::INVALID_STATE,
                format!("The team catalog is not valid JSON: {message}"),
            ),
        }
    })
}

/// The field serde names in `unknown field \`name\`, expected ...`.
fn unknown_field(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("unknown field `")?;
    rest.split('`').next()
}

fn first_problem(problems: Vec<Problem>) -> Result<(), Problem> {
    match problems.into_iter().next() {
        Some(problem) => Err(problem),
        None => Ok(()),
    }
}

fn read_error(error: io::Error) -> Problem {
    Problem::new(
        code::INVALID_STATE,
        format!("Cannot read the team catalog: {error}"),
    )
}

fn write_error(error: io::Error) -> Problem {
    Problem::new(
        code::INVALID_STATE,
        format!("Cannot write the team catalog: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use zerus_team_plan::builtin::builtin;
    use zerus_team_plan::catalog::{Catalog, Role};
    use zerus_team_plan::problem::code;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("a temporary folder")
    }

    fn write(dir: &Path, text: &str) {
        let file = path(dir);
        fs::create_dir_all(file.parent().expect("a parent")).unwrap();
        fs::write(file, text).unwrap();
    }

    /// zerus-63rru.74: a template with a forbidden name or a role with a
    /// forbidden ID is refused at the catalog save, with the sentence that
    /// names the field and the code point.
    #[test]
    fn a_template_with_a_forbidden_name_or_role_id_is_refused_at_save() {
        let dir = temp();
        let mut catalog = builtin();
        catalog.templates[0].name = "Template\u{202D}name".into();
        let problem = save(dir.path(), &catalog).unwrap_err();
        assert_eq!(problem.code, code::INVALID_STATE, "{problem:?}");
        assert!(
            problem.message.contains("The template name")
                && problem.message.contains("U+202D"),
            "{problem:?}"
        );
        let mut catalog = builtin();
        catalog.roles[0].id = "role id".into();
        let problem = save(dir.path(), &catalog).unwrap_err();
        assert_eq!(problem.code, code::INVALID_STATE, "{problem:?}");
        assert!(problem.message.contains("The role ID"), "{problem:?}");
    }

    /// Round 2, P2-ID: a catalog stored before the rule still loads, lists
    /// and runs; the next save refuses it with the sentence naming the
    /// template and the field.
    #[test]
    fn an_older_catalog_with_an_id_outside_the_set_loads_and_is_refused_on_save() {
        let dir = temp();
        write(
            dir.path(),
            r#"{"roles":[{"id":"role id","name":"Old role","agent":"codex"}]}"#,
        );
        let loaded = load(dir.path()).unwrap();
        let old = loaded
            .roles
            .iter()
            .find(|role| role.id == "role id")
            .expect("the stored role");
        assert_eq!(old.name, "Old role");
        let problem = save(dir.path(), &loaded).unwrap_err();
        assert_eq!(problem.code, code::INVALID_STATE, "{problem:?}");
        // The stored role is used by no template: the refusal says so and
        // names no template (round 3's attribution).
        assert!(
            problem.message.contains("unused role")
                && problem.message.contains("role ID"),
            "{problem:?}"
        );
    }

    /// Round 3, P3-1: the refusal names the template that uses the bad
    /// role, not the first template; an unused role says it is unused.
    #[test]
    fn the_save_names_the_template_that_uses_the_bad_role_or_says_unused() {
        let dir = temp();
        let mut catalog = builtin();
        let mut second = catalog.templates[0].clone();
        second.id = "second".into();
        second.name = "Second template".into();
        second.roster.push(zerus_team_plan::catalog::Seat {
            role: "role id".into(),
            count: 1,
            optional: false,
        });
        catalog.templates.push(second);
        let mut old = catalog.roles[0].clone();
        old.id = "role id".into();
        old.name = "Old role".into();
        catalog.roles.push(old);
        let problem = save(dir.path(), &catalog).unwrap_err();
        assert_eq!(problem.code, code::INVALID_STATE, "{problem:?}");
        assert!(
            problem.message.contains("Second template")
                && problem.message.contains("role ID"),
            "{problem:?}"
        );
        // An unused role names no template.
        let mut catalog = builtin();
        let mut orphan = catalog.roles[0].clone();
        orphan.id = "role id".into();
        orphan.name = "Orphan".into();
        catalog.roles.push(orphan);
        let problem = save(dir.path(), &catalog).unwrap_err();
        assert_eq!(problem.code, code::INVALID_STATE, "{problem:?}");
        assert!(
            problem.message.contains("unused role"),
            "{problem:?}"
        );
    }

    #[test]
    fn the_built_in_catalog_loads_when_the_person_has_no_file() {
        let dir = temp();
        assert_eq!(load(dir.path()).unwrap(), builtin());
        assert!(!path(dir.path()).exists());
        assert!(!dir.path().join(DIR).exists(), "reading creates nothing");
    }

    #[test]
    fn a_persons_entry_overrides_a_built_in_one_with_the_same_id() {
        let dir = temp();
        write(
            dir.path(),
            r#"{"roles":[{"id":"developer","name":"Builder","agent":"codex"}]}"#,
        );
        let loaded = load(dir.path()).unwrap();
        let developer = loaded
            .roles
            .iter()
            .find(|role| role.id == "developer")
            .expect("the built-in developer");
        assert_eq!(developer.name, "Builder");
        assert_eq!(developer.agent, "codex");
        assert_eq!(loaded.roles.len(), 3, "the other built-in roles stay");
        assert!(loaded
            .roles
            .iter()
            .any(|role| role.id == "reviewer" && role.name == "Reviewer"));
    }

    #[test]
    fn a_person_template_replaces_the_built_in_template_of_the_same_id() {
        let dir = temp();
        write(
            dir.path(),
            r#"{"templates":[{"id":"bugfix","name":"My bugfix","lead":{"agent":"claude"},"roster":[{"role":"developer","count":1},{"role":"reviewer","count":1}],"default_pipeline":"reviewed"}]}"#,
        );
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.templates.len(), 3);
        let bugfix = loaded
            .templates
            .iter()
            .find(|template| template.id == "bugfix")
            .expect("the person's bugfix");
        assert_eq!(bugfix.name, "My bugfix");
    }

    #[test]
    fn an_unknown_field_is_refused_and_the_file_is_left_untouched() {
        let dir = temp();
        let text = r#"{"roles":[],"colour":"red"}"#;
        write(dir.path(), text);
        let problem = load(dir.path()).unwrap_err();
        assert_eq!(problem.code, code::UNKNOWN_FIELD);
        assert_eq!(problem.refs, ["colour"]);
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), text);
    }

    #[test]
    fn an_imported_bundle_is_complete_on_an_empty_catalog() {
        let dir = temp();
        let bugfix = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        import_bundle(dir.path(), &bugfix).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(
            export_bundle(&loaded, "bugfix").expect("bugfix exports again"),
            bugfix
        );
        assert!(
            loaded
                .templates
                .iter()
                .any(|template| template.id == "feature-delivery"),
            "the built-in templates stay"
        );
        let person = person(dir.path()).unwrap();
        assert_eq!(person.templates.len(), 1, "only the imported template");
        assert_eq!(person.roles.len(), 2);
        assert_eq!(person.pipelines.len(), 1);
    }

    #[test]
    fn importing_the_same_bundle_twice_changes_nothing() {
        let dir = temp();
        let bugfix = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        import_bundle(dir.path(), &bugfix).unwrap();
        let once = fs::read_to_string(path(dir.path())).unwrap();
        import_bundle(dir.path(), &bugfix).unwrap();
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), once);
    }

    #[test]
    fn an_incomplete_bundle_is_refused_and_writes_nothing() {
        let dir = temp();
        write(dir.path(), "{}");
        let mut bundle = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        bundle.pipelines[0].stages[0].role = "ghost".into();
        let problem = import_bundle(dir.path(), &bundle).unwrap_err();
        assert!(
            matches!(
                problem.code.as_str(),
                code::UNKNOWN_ROLE | code::NO_SEAT_FOR_STAGE
            ),
            "{problem}"
        );
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), "{}");
    }

    #[test]
    fn a_saved_entry_is_merged_over_the_built_in_catalog() {
        let dir = temp();
        let mut architect: Role = builtin().roles[0].clone();
        architect.id = "architect".into();
        architect.name = "Architect".into();
        let only = Catalog {
            roles: vec![architect],
            ..Catalog::default()
        };
        save(dir.path(), &only).unwrap();

        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.roles.len(), 4);
        assert!(loaded.roles.iter().any(|role| role.id == "architect"));
        assert_eq!(person(dir.path()).unwrap().roles.len(), 1);
    }

    #[test]
    fn a_save_that_breaks_the_merged_catalog_writes_nothing() {
        let dir = temp();
        let mut template = builtin().templates[1].clone();
        template.id = "broken".into();
        template.roster[0].role = "ghost".into();
        let only = Catalog {
            templates: vec![template],
            ..Catalog::default()
        };
        let problem = save(dir.path(), &only).unwrap_err();
        assert_eq!(problem.code, code::UNKNOWN_ROLE);
        assert!(!path(dir.path()).exists());
    }

    #[test]
    fn malformed_json_is_refused_as_invalid_state() {
        let dir = temp();
        write(dir.path(), "not json");
        assert_eq!(load(dir.path()).unwrap_err().code, code::INVALID_STATE);
    }

    #[test]
    fn duplicate_ids_in_the_person_file_are_refused() {
        let dir = temp();
        let texts = [
            r#"{"roles":[{"id":"developer","name":"One","agent":"claude"},{"id":"developer","name":"Two","agent":"claude"}]}"#,
            r#"{"pipelines":[{"id":"code","name":"One","implementer":"developer"},{"id":"code","name":"Two","implementer":"developer"}]}"#,
            r#"{"templates":[{"id":"bugfix","name":"One","lead":{"agent":"claude"},"roster":[{"role":"developer","count":1}],"default_pipeline":"reviewed"},{"id":"bugfix","name":"Two","lead":{"agent":"claude"},"roster":[{"role":"developer","count":1}],"default_pipeline":"reviewed"}]}"#,
        ];
        for text in texts {
            write(dir.path(), text);
            let problem = load(dir.path()).unwrap_err();
            assert_eq!(problem.code, code::DUPLICATE, "{text}");
            assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), text);
        }
    }

    #[test]
    fn save_refuses_duplicate_ids_in_each_kind() {
        let dir = temp();
        let mut catalogs = Vec::new();
        let role = builtin().roles[0].clone();
        catalogs.push(Catalog {
            roles: vec![role.clone(), role],
            ..Catalog::default()
        });
        let pipeline = builtin().pipelines[0].clone();
        catalogs.push(Catalog {
            pipelines: vec![pipeline.clone(), pipeline],
            ..Catalog::default()
        });
        let template = builtin().templates[1].clone();
        catalogs.push(Catalog {
            templates: vec![template.clone(), template],
            ..Catalog::default()
        });
        for catalog in catalogs {
            let problem = save(dir.path(), &catalog).unwrap_err();
            assert_eq!(problem.code, code::DUPLICATE);
            assert!(!path(dir.path()).exists(), "nothing is written");
        }
    }

    #[test]
    fn a_person_template_with_an_empty_default_pipeline_is_refused() {
        let dir = temp();
        write(
            dir.path(),
            r#"{"templates":[{"id":"bugfix","name":"Bugfix","lead":{"agent":"claude"},"roster":[{"role":"developer","count":1},{"role":"reviewer","count":1}],"default_pipeline":""}]}"#,
        );
        let problem = load(dir.path()).unwrap_err();
        assert_eq!(problem.code, code::UNKNOWN_PIPELINE);

        let mut bundle = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        bundle.template.default_pipeline.clear();
        let problem = import_bundle(dir.path(), &bundle).unwrap_err();
        assert_eq!(problem.code, code::UNKNOWN_PIPELINE);
    }

    #[test]
    fn import_refuses_duplicate_ids_in_the_person_file() {
        let dir = temp();
        let text = r#"{"roles":[{"id":"developer","name":"One","agent":"claude"},{"id":"developer","name":"Two","agent":"claude"}]}"#;
        write(dir.path(), text);
        let bundle = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        let problem = import_bundle(dir.path(), &bundle).unwrap_err();
        assert_eq!(problem.code, code::DUPLICATE);
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), text);
    }

    #[test]
    fn a_person_template_with_a_zero_count_seat_is_refused() {
        let dir = temp();
        write(
            dir.path(),
            r#"{"templates":[{"id":"bugfix","name":"Bugfix","lead":{"agent":"claude"},"roster":[{"role":"developer","count":1},{"role":"reviewer","count":0}],"default_pipeline":"reviewed"}]}"#,
        );
        let problem = load(dir.path()).unwrap_err();
        assert_eq!(problem.code, code::NO_SEAT_FOR_STAGE);

        let mut bundle = export_bundle(&builtin(), "bugfix").expect("bugfix exports");
        bundle.template.roster[1].count = 0;
        let problem = import_bundle(dir.path(), &bundle).unwrap_err();
        assert_eq!(problem.code, code::NO_SEAT_FOR_STAGE);
    }

    #[test]
    fn an_unknown_field_inside_an_account_choice_is_refused() {
        let dir = temp();
        let text = r#"{"roles":[{"id":"developer","name":"Developer","agent":"claude","account":{"mode":"freest","colour":"red"}}]}"#;
        write(dir.path(), text);
        let problem = load(dir.path()).unwrap_err();
        assert_eq!(problem.code, code::UNKNOWN_FIELD);
        assert_eq!(problem.refs, ["colour"]);
        assert_eq!(fs::read_to_string(path(dir.path())).unwrap(), text);
    }

    #[test]
    fn an_unknown_template_cannot_be_exported() {
        let problem = export_bundle(&builtin(), "ghost").unwrap_err();
        assert_eq!(problem.code, code::NOT_FOUND);
    }
}
