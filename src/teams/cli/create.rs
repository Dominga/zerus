//! Create uses one resolved bundle, including when a receipt is replayed.
use super::{
    actor::{self, Caller},
    input::{self, Options},
    request_input, storage,
};
use crate::{
    config::Config,
    teams::{
        catalog,
        ops::{self, Operation, Request},
        store::Store,
    },
};
use serde_json::{json, Map, Value};
use zerus_team_plan::{model::Delegations, problem::code, Problem, TemplateBundle};

pub(super) fn run(
    config: &Config,
    caller: &Caller,
    args: &[String],
    provided: Option<&str>,
) -> Result<Value, Problem> {
    let options = Options::parse(args, &["--template"], &[])?;
    options.nothing_else("create")?;
    let template = options
        .get("--template")
        .ok_or_else(|| input::usage("create needs --template TEMPLATE"))?;
    let mut store = Store::open(&config.state)?;
    let identity = actor::resolve(&store, caller)?;
    actor::person_only(&identity, "create")?;
    let mut fields = input::object(&request_input(provided)?)?;
    super::commands::refuse_operation_word(&fields)?;
    // The field is a digest when it is there, or it is left out. Present but
    // empty (null or "") is its own refusal, never an unknown field.
    if let Some(value) = fields.get("expected_template") {
        if !value.as_str().is_some_and(|text| !text.is_empty()) {
            return Err(input::invalid(
                "expected_template must be a template digest, or left out.",
            )
            .with_refs(["expected_template"]));
        }
    }
    complete_delegations(&fields)?;
    let (bundle, replay) = snapshot(&store, config, template, &fields)?;
    fields.insert("op".into(), json!("create"));
    fields.insert(
        "bundle".into(),
        serde_json::to_value(bundle).map_err(storage)?,
    );
    let request: Request = serde_json::from_value(Value::Object(fields))
        .map_err(|e| input::serde_problem(e.to_string()))?;
    if !replay {
        check_template(&request)?;
    }
    let reply = ops::apply(&mut store, &identity.actor, &request, input::now_ms())?;
    super::commands::reply_view(&identity.actor, &request.team, &reply)
}
fn snapshot(
    store: &Store,
    config: &Config,
    template: &str,
    fields: &Map<String, Value>,
) -> Result<(Box<TemplateBundle>, bool), Problem> {
    if let Some(id) = fields.get("request_id").and_then(Value::as_str) {
        if let Some(saved) = store.creation_request(id)? {
            if let Operation::Create { bundle, .. } = saved.operation {
                if bundle.template.id == template {
                    return Ok((bundle, true));
                }
            }
            return Err(Problem::new(
                code::DUPLICATE,
                "Request ID was used with a different operation or template.",
            ));
        }
    }
    let catalog = catalog::load(&config.dir)?;
    let bundle = zerus_team_plan::bundle(&catalog, template)?;
    #[cfg(test)]
    after_snapshot();
    Ok((Box::new(bundle), false))
}
fn check_template(request: &Request) -> Result<(), Problem> {
    let Operation::Create {
        bundle,
        expected_template,
        ..
    } = &request.operation
    else {
        unreachable!()
    };
    if let Some(expected) = expected_template {
        if catalog::template_digest(bundle)? != *expected {
            return Err(Problem::new(
                code::TEMPLATE_CHANGED,
                "The template changed since it was shown. Review it again.",
            ));
        }
    }
    Ok(())
}
fn complete_delegations(fields: &Map<String, Value>) -> Result<(), Problem> {
    let Some(value) = fields.get("delegations") else {
        return Ok(());
    };
    let shape = serde_json::to_value(Delegations::default()).map_err(storage)?;
    let required = shape.as_object().expect("delegations are an object");
    let complete = value.as_object().is_some_and(|object| {
        object.len() == required.len()
            && required
                .keys()
                .all(|key| object.get(key).is_some_and(Value::is_boolean))
    });
    if !complete {
        return Err(
            input::invalid("delegations must name all five boolean fields.")
                .with_refs(["delegations"]),
        );
    }
    Ok(())
}

#[cfg(test)]
thread_local! { static AFTER_SNAPSHOT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(super) fn set_after_snapshot(hook: impl FnOnce() + 'static) {
    AFTER_SNAPSHOT.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}
#[cfg(test)]
fn after_snapshot() {
    let hook = AFTER_SNAPSHOT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}
