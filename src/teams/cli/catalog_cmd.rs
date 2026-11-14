//! The person's own catalog and templates. These commands are the CLI half of
//! the designer: they read and write the one JSON file a person owns, and they
//! create nothing in it until an explicit save or import. Every reply names
//! the actor like the store's replies do, so the interim person rule is
//! visible here too.
use super::actor::{self, Caller};
use super::input::{self, Options};
use super::{request_input, storage};
use crate::config::Config;
use crate::teams::catalog;
use crate::teams::store::{Guard, Store};
use serde_json::{json, Value};
use zerus_team_plan::catalog::{Catalog, Template, TemplateBundle};
use zerus_team_plan::problem::code;
use zerus_team_plan::Problem;

/// One reply: the actor first, then the command's own fields.
fn reply(actor: &Value, fields: Value) -> Result<Value, Problem> {
    let mut object = match fields {
        Value::Object(object) => object,
        _ => return Err(Guard::CatalogReply.problem()),
    };
    object.insert("ok".into(), json!(true));
    object.insert("actor".into(), actor.clone());
    Ok(Value::Object(object))
}

pub(super) fn run(
    config: &Config,
    caller: &Caller,
    group: &str,
    args: &[String],
    provided: Option<&str>,
) -> Result<Value, Problem> {
    let store = Store::open(&config.state)?;
    let identity = actor::resolve(&store, caller)?;
    actor::person_only(&identity, group)?;
    let actor = actor::view(&identity.actor);
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| input::usage(format!("{group} needs a command")))?;
    match (group, command.as_str()) {
        ("catalog", "show") => catalog_show(&actor, &catalog::load(&config.dir)?),
        ("catalog", "export") => catalog_of(&actor, &catalog::person(&config.dir)?),
        ("catalog", "import") => import_catalog(&actor, config, provided),
        ("template", "list") => templates(&actor, config, rest),
        ("template", "show") => show(&actor, config, rest),
        ("template", "save") => save(&actor, config, provided),
        ("template", "import") => import_bundle(&actor, config, provided),
        ("template", "export") => export(&actor, config, rest),
        ("template", "remove") => remove(&actor, config, rest),
        _ => Err(super::unknown(&format!("{group} {command}"))),
    }
}

fn catalog_show(actor: &Value, catalog: &Catalog) -> Result<Value, Problem> {
    let mut view = serde_json::to_value(catalog).map_err(storage)?;
    if let Some(templates) = view.get_mut("templates").and_then(Value::as_array_mut) {
        for row in templates {
            let id = row["id"].as_str().expect("template ID");
            let bundle = zerus_team_plan::bundle(catalog, id)?;
            let encoded = serde_json::to_string(&bundle).map_err(storage)?;
            if encoded.len() > catalog::CREATE_BUNDLE_LIMIT {
                row.as_object_mut()
                    .expect("template object")
                    .insert("over_limit".into(), json!(true));
                continue;
            }
            row.as_object_mut()
                .expect("template object")
                .insert("digest".into(), json!(catalog::template_digest(&bundle)?));
        }
    }
    reply(actor, json!({"catalog":view}))
}

fn catalog_of(actor: &Value, catalog: &Catalog) -> Result<Value, Problem> {
    reply(
        actor,
        json!({"catalog": serde_json::to_value(catalog).map_err(storage)?}),
    )
}

fn words(args: &[String]) -> Result<Options, Problem> {
    Options::parse(args, &[], &[])
}

fn list_reply(actor: &Value, catalog: &Catalog) -> Result<Value, Problem> {
    let rows: Vec<Value> = catalog
        .templates
        .iter()
        .map(|template| {
            json!({
                "id": template.id,
                "name": template.name,
                "description": template.description,
            })
        })
        .collect();
    reply(actor, json!({"templates": rows}))
}

fn templates(actor: &Value, config: &Config, args: &[String]) -> Result<Value, Problem> {
    words(args)?.nothing_else("template list")?;
    list_reply(actor, &catalog::load(&config.dir)?)
}

fn show(actor: &Value, config: &Config, args: &[String]) -> Result<Value, Problem> {
    let options = words(args)?;
    let id = options.required("a template ID")?;
    let catalog = catalog::load(&config.dir)?;
    let template = catalog
        .templates
        .iter()
        .find(|template| template.id == id)
        .ok_or_else(|| unknown_template(id))?;
    reply(
        actor,
        json!({"template": serde_json::to_value(template).map_err(storage)?}),
    )
}

fn export(actor: &Value, config: &Config, args: &[String]) -> Result<Value, Problem> {
    let options = words(args)?;
    let id = options.required("a template ID")?;
    let catalog = catalog::load(&config.dir)?;
    let bundle = catalog::export_bundle(&catalog, id)?;
    reply(
        actor,
        json!({"bundle": serde_json::to_value(bundle).map_err(storage)?}),
    )
}

fn remove(actor: &Value, config: &Config, args: &[String]) -> Result<Value, Problem> {
    let options = words(args)?;
    let id = options.required("a template ID")?;
    let mut person = catalog::person(&config.dir)?;
    let Some(index) = person
        .templates
        .iter()
        .position(|template| template.id == id)
    else {
        return Err(Problem::new(
            code::NOT_FOUND,
            "Only a template in the person's own catalog can be removed.",
        )
        .with_refs([id]));
    };
    person.templates.remove(index);
    catalog::save(&config.dir, &person)?;
    let built_in = catalog::load(&config.dir)?
        .templates
        .iter()
        .any(|template| template.id == id);
    reply(actor, json!({"removed": id, "still_available": built_in}))
}

fn save(actor: &Value, config: &Config, provided: Option<&str>) -> Result<Value, Problem> {
    let value = without_read_only(input::value(&request_input(provided)?, "template")?);
    let template: Template =
        serde_json::from_value(value).map_err(|error| input::serde_problem(error.to_string()))?;
    let mut person = catalog::person(&config.dir)?;
    match person
        .templates
        .iter()
        .position(|saved| saved.id == template.id)
    {
        Some(index) => person.templates[index] = template.clone(),
        None => person.templates.push(template.clone()),
    }
    catalog::save(&config.dir, &person)?;
    reply(actor, json!({"saved": template.id}))
}

fn import_bundle(actor: &Value, config: &Config, provided: Option<&str>) -> Result<Value, Problem> {
    let bundle: TemplateBundle = parsed(&request_input(provided)?, "template bundle")?;
    catalog::import_bundle(&config.dir, &bundle)?;
    reply(actor, json!({"imported": bundle.template.id}))
}

fn import_catalog(
    actor: &Value,
    config: &Config,
    provided: Option<&str>,
) -> Result<Value, Problem> {
    let value = without_read_only(input::value(&request_input(provided)?, "catalog")?);
    let catalog: Catalog =
        serde_json::from_value(value).map_err(|error| input::serde_problem(error.to_string()))?;
    catalog::save(&config.dir, &catalog)?;
    reply(actor, json!({"saved": true}))
}

/// A shown catalog row is a saveable row: `catalog show` adds the read-only
/// `digest` and `over_limit`, and feeding it back ignores those fields whether
/// they match or are stale. They are presentation metadata, never authority.
fn without_read_only(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("digest");
        object.remove("over_limit");
    }
    if let Some(templates) = value.get_mut("templates").and_then(Value::as_array_mut) {
        for row in templates {
            if let Some(object) = row.as_object_mut() {
                object.remove("digest");
                object.remove("over_limit");
            }
        }
    }
    value
}

/// One JSON document of the type the command stores, with an unknown field
/// named by its own code.
fn parsed<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> Result<T, Problem> {
    serde_json::from_value(input::value(text, what)?)
        .map_err(|error| input::serde_problem(error.to_string()))
}

fn unknown_template(id: &str) -> Problem {
    Problem::new(code::NOT_FOUND, "Template does not exist.").with_refs([id])
}
