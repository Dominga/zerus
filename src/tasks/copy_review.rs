//! Person-confirmed local copy review, separate from every task execution path.
use super::{actor, fail, machines, option, project_arg, Result};
use crate::config::Config;
use serde_json::{json, Value};
use std::time::Duration;
use zerus_replica::{Actor, ProjectStore};

pub(super) fn execute(config: &Config, args: &[String]) -> Result<Value> {
    let mut index = 0;
    let mut seen = std::collections::BTreeSet::new();
    while index < args.len() {
        if !seen.insert(args[index].as_str()) {
            return Err(fail("review-copy refuses duplicate options"));
        }
        match args[index].as_str() {
            "--json" => index += 1,
            "--project" | "--uuid" | "--confirm" if index + 1 < args.len() => index += 2,
            _ => return Err(fail("usage: hgs task review-copy --project P --uuid UUID [--confirm FINGERPRINT] --json")),
        }
    }
    if !seen.contains("--project") || !seen.contains("--uuid") {
        return Err(fail("review-copy requires --project and the exact --uuid"));
    }
    let who = actor(&machines(config)?.here, args, false)?;
    let (id, name) = project_arg(config, args, &who)?;
    let uuid =
        option(args, "--uuid")?.ok_or_else(|| fail("review-copy requires the exact task UUID"))?;
    let confirmation = option(args, "--confirm")?;
    if confirmation.is_some() && !matches!(who, Actor::Person) {
        return Err(fail(
            "only a person confirms moving a damaged local task copy aside",
        ));
    }
    let mut store =
        ProjectStore::open_copy_review(&config.state.join("replica"), &id, Duration::from_secs(2))
            .map_err(fail)?
            .ok_or_else(|| fail("the task store is busy; nothing was moved"))?;
    let mut reply =
        json!({"abi":"task-copy-review-v1","project":{"id":id,"name":name},"uuid":uuid});
    match confirmation {
        None => {
            let review = store.prepare_copy_review(uuid).map_err(fail)?;
            reply["state"] = json!("review");
            reply["fingerprint"] = json!(review.fingerprint);
            reply["files"] = json!(review.files);
        }
        Some(expected) => {
            if expected.len() != 64
                || !expected
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(fail("review-copy requires its exact review fingerprint"));
            }
            match store.move_reviewed_copy(uuid, expected) {
                Ok(outcome) => {
                    reply["state"] = json!(outcome.state);
                    reply["backup"] = json!(outcome.backup);
                    reply["reason"] = json!(outcome.reason);
                }
                Err(error) => {
                    reply["state"] = json!("refused");
                    reply["reason"] = json!(error.to_string());
                }
            }
        }
    }
    Ok(reply)
}
