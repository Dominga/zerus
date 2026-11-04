//! `task cutover`: a project's cutover to execution attempts as this
//! computer knows it, and the person's hold on it (zerus-cg5h.4 S2b.2 C2).
//! Read-only and bounded by the project's members; information only: it
//! grants nothing and dispatches nothing.
use super::{actor, fail, flag, machines, now, option, project_arg, store, Result};
use crate::config::Config;
use crate::swarm::{self, TaskMember};
use serde_json::{json, Value};
use std::time::UNIX_EPOCH;
use zerus_replica::{Actor, TASK_SCHEMA};

/// `task cutover --project P [--hold on|off [--confirm]]`.
pub(super) fn cutover(config: &Config, args: &[String]) -> Result<Value> {
    let who = actor(&machines(config)?.here, args, false)?;
    let (id, name) = project_arg(config, args, &who)?;
    let store = store(config, &id)?;
    if let Some(hold) = option(args, "--hold")? {
        let on = match hold {
            "on" => true,
            "off" => false,
            _ => return Err(fail("--hold takes on or off")),
        };
        if !matches!(who, Actor::Person) {
            return Err(fail("only a person sets or clears the cutover hold"));
        }
        // Clearing it lets the project's cutover begin: an explicit choice.
        if !on && !flag(args, "--confirm") {
            return Err(fail(
                "clearing the cutover hold lets this project's tasks cut over to execution \
                 attempts; pass --confirm to clear it",
            ));
        }
        let mut doc = store.project().map_err(fail)?;
        doc.set_cutover_hold(on, &who, now()).map_err(fail)?;
        store.save_project(&doc).map_err(fail)?;
    }
    let hold = store.project().map_err(fail)?.cutover_hold();
    let (cut_over, latest) = store.cut_over().map_err(fail)?;
    let latest = latest
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);
    let members = swarm::task_members(config).map_err(fail)?;
    let closed = (cut_over > 0)
        .then(|| members.iter().find_map(|m| closing(m, &id, latest)))
        .flatten();
    let phase = match (cut_over, &closed) {
        (0, _) => "not_cut_over",
        (_, Some(_)) => "cutting_over",
        (_, None) => "active",
    };
    let closed_text = closed.as_ref().map(|c| prose(c, &name));
    Ok(json!({
        "project": {"id": id, "name": name},
        "phase": phase,
        "hold": match hold {
            Some(hold) => json!({"on": hold.on, "by": hold.by, "at": hold.at}),
            None => json!({"on": false, "by": null, "at": null}),
        },
        "cut_over_tasks": cut_over,
        "last_cutover_write": latest,
        "members": members.iter().map(|m| member(m, &id)).collect::<Vec<_>>(),
        "closed": closed,
        "closed_text": closed_text,
    }))
}

fn ms(at: u64) -> i64 {
    at as i64
}

/// One member as the status shows it; times are milliseconds.
fn member(member: &TaskMember, project: &str) -> Value {
    let replica = member.replica.as_ref();
    let inventory = replica.and_then(|r| r.projects.get(project)).map(|e| {
        json!({"complete": e.complete, "at": ms(e.at), "reason": e.reason})
    });
    json!({
        "alias": member.alias,
        "node": member.node,
        "task_schema": replica.and_then(|r| r.task_schema),
        "store": replica.and_then(|r| r.store.clone()),
        "seen_at": replica.map(|r| ms(r.seen_at)),
        "inventory": inventory,
    })
}

/// The first reason this member keeps the project's cutover open, if any:
/// never seen, an older task schema, no exchange of the project, an
/// incomplete one, or none since the latest cutover write here.
fn closing(member: &TaskMember, project: &str, latest: Option<i64>) -> Option<Value> {
    let closed = |reason: &str, at: Option<i64>| {
        Some(json!({"member": member.alias, "project": project, "reason": reason, "at": at}))
    };
    let Some(replica) = &member.replica else {
        return closed("unseen", None);
    };
    if replica.task_schema.is_none_or(|schema| schema < TASK_SCHEMA) {
        return closed("task_schema", Some(ms(replica.seen_at)));
    }
    let Some(exchanged) = replica.projects.get(project) else {
        return closed("no_inventory", None);
    };
    if let Some(reason) = exchanged.reason.filter(|_| !exchanged.complete) {
        return closed(reason.as_str(), Some(ms(exchanged.at)));
    }
    if latest.is_some_and(|latest| ms(exchanged.at) < latest) {
        return closed("inventory_before_cutover", Some(ms(exchanged.at)));
    }
    None
}

/// The first closing reason in words; clients build their own.
fn prose(closed: &Value, project: &str) -> String {
    let member = closed["member"].as_str().unwrap_or_default();
    match closed["reason"].as_str().unwrap_or_default() {
        "unseen" => format!("{member} has not answered since it joined"),
        "task_schema" => format!("{member}'s Zerus does not write the execution task schema yet"),
        "no_inventory" => format!("{member} has not exchanged the tasks of {project}"),
        "inventory_before_cutover" => {
            format!("{member}'s last inventory of {project} predates the latest cutover here")
        }
        reason => format!("{member}'s inventory of {project} was incomplete ({reason})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::ReplicaMember;

    fn member(replica: Option<ReplicaMember>) -> TaskMember {
        TaskMember {
            alias: "mac".into(),
            node: "node-1".into(),
            replica,
        }
    }

    fn reason(member: &TaskMember, latest: Option<i64>) -> Option<String> {
        closing(member, "p1", latest).map(|c| c["reason"].as_str().unwrap().to_owned())
    }

    #[test]
    fn a_member_keeps_the_cutover_open_for_its_first_reason() {
        assert_eq!(reason(&member(None), None).as_deref(), Some("unseen"));
        let mut replica: ReplicaMember = serde_json::from_value(json!({
            "node": "node-1", "task_schema": 5, "seen_at": 10,
        }))
        .unwrap();
        assert_eq!(reason(&member(Some(replica.clone())), None).as_deref(), Some("task_schema"));
        replica.task_schema = None;
        assert_eq!(reason(&member(Some(replica.clone())), None).as_deref(), Some("task_schema"));
        replica.task_schema = Some(TASK_SCHEMA);
        assert_eq!(reason(&member(Some(replica.clone())), None).as_deref(), Some("no_inventory"));
        replica.projects = serde_json::from_value(json!({
            "p1": {"complete": false, "at": 20, "reason": "page_budget"},
        }))
        .unwrap();
        let closed = closing(&member(Some(replica.clone())), "p1", None).unwrap();
        assert_eq!(
            closed,
            json!({"member": "mac", "project": "p1", "reason": "page_budget", "at": 20})
        );
        assert!(prose(&closed, "orchard").contains("incomplete (page_budget)"));
        replica.projects = serde_json::from_value(json!({"p1": {"complete": true, "at": 30}})).unwrap();
        assert_eq!(reason(&member(Some(replica.clone())), Some(30)), None);
        assert_eq!(
            reason(&member(Some(replica)), Some(31)).as_deref(),
            Some("inventory_before_cutover")
        );
    }
}
