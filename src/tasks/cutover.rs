//! `task cutover`: a project's cutover to execution attempts as this
//! computer knows it, and the person's hold on it (zerus-cg5h.4 S2b.2 C2).
//! Read-only and bounded by the project's members; information only: it
//! grants nothing and dispatches nothing.
use super::{actor, fail, flag, machines, now, option, project_arg, project_in, store, Result};
use crate::config::Config;
use crate::swarm::{self, TaskMember};
use serde_json::{json, Value};
use std::time::UNIX_EPOCH;
use std::time::Duration;
use zerus_replica::Actor;

/// One fence check's whole budget, counted once from its start (C3).
const FENCE_BUDGET: Duration = Duration::from_secs(15);

/// The most task folder entries, and the longest time, one status reads.
const SCAN_ENTRIES: usize = 200_000;
const SCAN_TIME: Duration = Duration::from_millis(500);

/// `task cutover --project P [--hold on|off [--confirm]]`, or
/// `task cutover --project P --task T --check`.
pub(super) fn cutover(config: &Config, args: &[String]) -> Result<Value> {
    if option(args, "--task")?.is_some() {
        // The one budget starts at the command's entry: this computer's
        // identity, the project's name, its lock and the universe all count
        // against it (review-n on #1452).
        return check_entry(config, args, std::time::Instant::now() + FENCE_BUDGET);
    }
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
    // A bounded count: past its budget the phase is unknown, never a guess,
    // and the project is released within it.
    let scanned = store.cut_over_within(SCAN_ENTRIES, SCAN_TIME).map_err(fail)?;
    let (cut_over, latest) = match scanned {
        Some((count, latest)) => (
            Some(count),
            latest
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64),
        ),
        None => (None, None),
    };
    let members = swarm::task_members(config).map_err(fail)?;
    let closed = match cut_over {
        None => Some(json!({"member": null, "project": id, "reason": "scan_budget", "at": null})),
        Some(0) => None,
        Some(_) => members.iter().find_map(|m| closing(m, &id, latest)),
    };
    let phase = match (cut_over, &closed) {
        (None, _) => "unknown",
        (Some(0), _) => "not_cut_over",
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
        "protocol": replica.and_then(|r| r.protocol),
        "task_schema": replica.and_then(|r| r.task_schema),
        "store": replica.and_then(|r| r.store.clone()),
        "seen_at": replica.map(|r| ms(r.seen_at)),
        "inventory": inventory,
    })
}

/// The first reason this member keeps the project's cutover open, if any
/// (`swarm::cutover_open`): anything this does not positively know as
/// complete keeps it open.
fn closing(member: &TaskMember, project: &str, latest: Option<i64>) -> Option<Value> {
    let (reason, at) = swarm::cutover_open(member.replica.as_ref(), project, latest)?;
    Some(json!({"member": member.alias, "project": project, "reason": reason, "at": at}))
}

/// `--project P --task T --check`, all within `deadline` from the command's
/// entry: this computer's identity and the projects in one timed catalog
/// admission, then the project's lock, the task and the fence.
fn check_entry(config: &Config, args: &[String], deadline: std::time::Instant) -> Result<Value> {
    let task = option(args, "--task")?.ok_or_else(|| fail("--check needs --task T"))?;
    if !flag(args, "--check") || option(args, "--hold")?.is_some() {
        return Err(fail("--task takes --check, and nothing else"));
    }
    let wanted = option(args, "--project")?.ok_or_else(|| fail("--check needs --project P"))?;
    let (here, projects) = crate::swarm::identity_and_projects_until(config, deadline)
        .map_err(fail)?
        .ok_or_else(|| fail("the catalog is busy past the check's budget"))?;
    actor(&here, args, false)?;
    let (id, name) = project_in(&projects, wanted)?;
    fence_check(config, &id, &name, task, deadline)
}

/// `--task T --check` from its entry: the project's lock, the task's name
/// and the universe all within `deadline` (review-n and desktop-u on #1452).
fn fence_check(config: &Config, id: &str, name: &str, task: &str, deadline: std::time::Instant) -> Result<Value> {
    let wait = deadline.saturating_duration_since(std::time::Instant::now());
    let store = zerus_replica::ProjectStore::open_within(&config.state.join("replica"), id, wait)
        .map_err(fail)?
        .filter(|_| std::time::Instant::now() < deadline)
        .ok_or_else(|| fail("the project's tasks are busy past the check's budget"))?;
    let uuid = store.find(task).map_err(fail)?.uuid().to_owned();
    // No project lock is held across the network.
    drop(store);
    check(config, id, name, &uuid, deadline)
}

/// `--task T --check`: the fence for the task, proved now within one budget.
/// It reads every node that may hold the task (the writer universe), then
/// exchanges the task with each over its verified route until all of them
/// and this computer hold ONE version with a valid floor and a raw stored
/// schema of at least 7, then reads the universe again and passes the final
/// admission without reserving anything. It exchanges and saves the task's
/// replica data and repairs like a sync; it creates no floor and no attempt,
/// and grants no send authority.
fn check(config: &Config, id: &str, name: &str, uuid: &str, deadline: std::time::Instant) -> Result<Value> {
    // The task's certificate, read only: whether it would be reused now
    // (C3e). The check never writes one.
    let certificate = match swarm::fence::admit_certified(config, id, uuid, deadline, |_, _| Ok(()))
        .map_err(fail)?
    {
        Ok(()) => json!({"valid": true}),
        Err(closed) => json!({"valid": false, "reason": closed.reason}),
    };
    let (universe, proved) =
        swarm::fence::prove(config, &swarm::fence::Ssh(config), id, uuid, deadline).map_err(fail)?;
    let (closed, proof) = match proved {
        Ok(proof) => match proof.admit(config, deadline, |_, _| Ok(())).map_err(fail)? {
            Ok(()) => (None, Some(json!({"version": proof.version, "rounds": proof.rounds,
                                          "nodes": proof.nodes, "admissible": true}))),
            Err(closed) => (Some(closed), None),
        },
        Err(closed) => (Some(closed), None),
    };
    // A result complete only after the budget is never reported open.
    let (closed, proof) = if std::time::Instant::now() >= deadline {
        (Some(swarm::fence::Closed { node: None, reason: "deadline" }), None)
    } else {
        (closed, proof)
    };
    Ok(json!({
        "project": {"id": id, "name": name},
        "task": uuid,
        "universe": universe,
        "incarnation": universe.incarnation(),
        "universe_closed": universe.closed().map(|(node, reason)| json!({"node": node, "reason": reason})),
        "closed": closed.map(|c| json!({"node": c.node, "reason": c.reason})),
        "proof": proof,
        "certificate": certificate,
    }))
}

/// The first closing reason in words; clients build their own.
fn prose(closed: &Value, project: &str) -> String {
    let member = closed["member"].as_str().unwrap_or_default();
    match closed["reason"].as_str().unwrap_or_default() {
        "unseen" => format!("{member} has not answered since it joined"),
        "task_schema" => format!("{member}'s Zerus does not write the execution task schema yet"),
        "protocol" => format!("{member}'s Zerus speaks another task replica protocol"),
        "store" => format!("{member} did not name a valid replica store"),
        "scan_budget" => format!("the tasks of {project} were too many to count here in time"),
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
    use zerus_replica::TASK_SCHEMA;

    /// review-n on #1452 R2: the budget covers the check's entry too: a
    /// catalog lock held past it refuses within it.
    #[test]
    fn the_check_s_budget_starts_at_its_entry() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            home: dir.path().to_str().unwrap().into(),
            dir: dir.path().join("config"),
            state: dir.path().join("state"),
            selves: vec!["example-local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: std::collections::BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let held = crate::swarm::lock_catalog_for_tests(&config);
        let args: Vec<String> =
            ["--project", "p", "--task", "T-000001", "--check"].iter().map(|a| a.to_string()).collect();
        let started = std::time::Instant::now();
        let refused = check_entry(&config, &args, started + Duration::from_millis(150));
        let waited = started.elapsed();
        drop(held);
        assert!(format!("{}", refused.unwrap_err()).contains("past the check's budget"));
        assert!(waited >= Duration::from_millis(150) && waited < Duration::from_secs(3), "{waited:?}");
    }

    /// review-n on #1452: the check's budget covers its own project lock. A
    /// lock held past the budget refuses within it, never blocking past it
    /// and never answering after it.
    #[test]
    fn the_check_s_budget_covers_its_project_lock() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            home: dir.path().to_str().unwrap().into(),
            dir: dir.path().join("config"),
            state: dir.path().join("state"),
            selves: vec!["example-local".into()],
            peers: vec![],
            base_peers: vec![],
            peers_from_environment: false,
            machines: std::collections::BTreeMap::new(),
            tab: false,
            tab_colors: String::new(),
        };
        let held = zerus_replica::ProjectStore::open(&config.state.join("replica"), "p").unwrap();
        let started = std::time::Instant::now();
        let refused = fence_check(&config, "p", "P", "T-000001", started + Duration::from_millis(150));
        let waited = started.elapsed();
        drop(held);
        assert!(format!("{}", refused.unwrap_err()).contains("past the check's budget"));
        assert!(waited >= Duration::from_millis(150) && waited < Duration::from_secs(3), "{waited:?}");
        // An exhausted budget is refused at once, even with the lock free.
        let started = std::time::Instant::now();
        assert!(fence_check(&config, "p", "P", "T-000001", started).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

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

    const STORE: &str = "11111111-1111-4111-8111-111111111111";

    fn complete() -> ReplicaMember {
        serde_json::from_value(json!({
            "node": "node-1", "protocol": 1, "task_schema": 7, "store": STORE, "seen_at": 30,
            "projects": {"p1": {"complete": true, "exchanged": true, "at": 30}},
        }))
        .unwrap()
    }

    #[test]
    fn a_member_keeps_the_cutover_open_for_its_first_reason() {
        assert_eq!(reason(&member(None), None).as_deref(), Some("unseen"));
        let mut replica = complete();
        replica.protocol = Some(2);
        assert_eq!(reason(&member(Some(replica.clone())), None).as_deref(), Some("protocol"));
        replica.protocol = None;
        assert_eq!(reason(&member(Some(replica.clone())), None).as_deref(), Some("protocol"));
        replica.protocol = Some(1);
        replica.task_schema = Some(6);
        assert_eq!(reason(&member(Some(replica.clone())), None).as_deref(), Some("task_schema"));
        replica.task_schema = None;
        assert_eq!(reason(&member(Some(replica.clone())), None).as_deref(), Some("task_schema"));
        replica.task_schema = Some(TASK_SCHEMA);
        replica.projects.clear();
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
        let replica = complete();
        assert_eq!(reason(&member(Some(replica.clone())), Some(30)), None);
        assert_eq!(
            reason(&member(Some(replica)), Some(31)).as_deref(),
            Some("inventory_before_cutover")
        );
    }

    /// review-n and desktop-u on #1432: a status fails closed. A missing or
    /// invalid store, or a project not complete (with or without a reason),
    /// keeps the cutover open.
    #[test]
    fn anything_not_known_complete_keeps_the_cutover_open() {
        assert!(closing(&member(Some(complete())), "p1", Some(30)).is_none());
        for store in [None, Some(""), Some("not-a-store")] {
            let mut replica = complete();
            replica.store = store.map(str::to_owned);
            assert_eq!(reason(&member(Some(replica)), Some(30)).as_deref(), Some("store"), "{store:?}");
        }
        let mut replica = complete();
        let project = replica.projects.get_mut("p1").unwrap();
        project.complete = false;
        project.reason = None;
        assert_eq!(reason(&member(Some(replica)), Some(30)).as_deref(), Some("incomplete"));
        // An exchanged readable subset is still not a complete project.
        let mut replica = complete();
        let project = replica.projects.get_mut("p1").unwrap();
        project.complete = false;
        project.exchanged = true;
        project.reason = Some(crate::swarm::Incomplete::DamagedTasks);
        assert_eq!(reason(&member(Some(replica)), Some(30)).as_deref(), Some("damaged_tasks"));
        assert!(prose(
            &json!({"member": null, "project": "p1", "reason": "scan_budget", "at": null}),
            "orchard"
        )
        .contains("too many"));
    }
}
