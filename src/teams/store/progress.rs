//! A bounded, metadata-only projection for one team-list page. Each count
//! stops after COUNT_CAP + 1 indexed rows; the UI retains the lower-bound flag.
use super::*;
use std::collections::BTreeMap;

pub(crate) const COUNT_CAP: u32 = 1000;
/// Existing owner-card kinds that say progress needs intervention. The actor
/// uses team_actor_blocked for unreadable members and blocked service steps;
/// silent_lead is its "Lead not working" timer. Approvals and questions do
/// not establish Stuck, nor does a delivery_uncertain inbox item. The
/// project_limit service item is read separately from the actor ledger.
const STUCK_KINDS: &[&str] = &[
    "launch_blocked",
    "launch_uncertain",
    "silent_lead",
    "team_actor_blocked",
];

/// The actor ledger keeps this blocker independently of an acknowledged card.
pub(crate) const STUCK_SERVICE_KINDS: &[&str] = &["project_limit"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct TeamProgress {
    pub tasks: u32,
    pub accepted: u32,
    pub questions: u32,
    pub members: u32,
    pub phases: u32,
    pub phases_done: u32,
    pub stuck: bool,
    pub tasks_more: bool,
    pub accepted_more: bool,
    pub questions_more: bool,
    pub members_more: bool,
    pub phases_more: bool,
    pub phases_done_more: bool,
}

fn bounded(value: u32) -> (u32, bool) {
    (value.min(COUNT_CAP), value > COUNT_CAP)
}

/// One SQL statement for the entire page, with indexed, capped probes. No
/// queue, archive payload, bundle, journal or per-team command is loaded.
impl Store {
    pub(crate) fn team_progress(
        &self,
        teams: &[TeamRow],
    ) -> Result<BTreeMap<String, TeamProgress>> {
        self.active()?;
        if teams.len() > MAX_PAGE as usize {
            return Err(Problem::new(
                code::OVER_CAP,
                "Team summary page exceeds its bound.",
            ));
        }
        let Some(db) = &self.db else {
            return Ok(BTreeMap::new());
        };
        if teams.is_empty() {
            return Ok(BTreeMap::new());
        }
        let ids = teams.iter().map(|t| t.id.as_str()).collect::<Vec<_>>();
        let sql = progress_sql(ids.len());
        let mut query = db.prepare(&sql).map_err(storage)?;
        let rows = query
            .query_map(rusqlite::params_from_iter(ids), |r| {
                let (tasks, tasks_more) = bounded(r.get(1)?);
                let (accepted, accepted_more) = bounded(r.get(2)?);
                let (questions, questions_more) = bounded(r.get(3)?);
                let (members, members_more) = bounded(r.get(4)?);
                let (phases, phases_more) = bounded(r.get(5)?);
                let (phases_done, phases_done_more) = bounded(r.get(6)?);
                Ok((
                    r.get(0)?,
                    TeamProgress {
                        tasks,
                        accepted,
                        questions,
                        members,
                        phases,
                        phases_done,
                        stuck: r.get(7)?,
                        tasks_more,
                        accepted_more,
                        questions_more,
                        members_more,
                        phases_more,
                        phases_done_more,
                    },
                ))
            })
            .map_err(storage)?;
        rows.collect::<std::result::Result<BTreeMap<_, _>, _>>()
            .map_err(storage)
    }
}

pub(super) fn progress_sql(rows: usize) -> String {
    let ids = std::iter::repeat_n("(?)", rows)
        .collect::<Vec<_>>()
        .join(",");
    let cap = COUNT_CAP + 1;
    let task_count = |table: &str, accepted: bool| {
        format!(
        "(SELECT COUNT(*) FROM (SELECT 1 FROM {table} WHERE team=t.id AND COALESCE(json_extract(data,'$.kind'),'task')='task'{} LIMIT {cap}))",
        if accepted { " AND node_state='accepted'" } else { " AND node_state IN ('draft','waiting','pending','in_progress','reported','accepted')" })
    };
    let count = |table: &str, predicate: &str| {
        format!(
            "(SELECT COUNT(*) FROM (SELECT 1 FROM {table} WHERE team=t.id{predicate} LIMIT {cap}))"
        )
    };
    let kinds = STUCK_KINDS
        .iter()
        .map(|kind| format!("'{kind}'"))
        .collect::<Vec<_>>()
        .join(",");
    let members = format!("(SELECT COUNT(*) FROM members WHERE team=t.id AND id=(SELECT lead FROM teams WHERE id=t.id))+{}", count("members", " AND state IN ('\"active\"','\"paused\"') AND id<>(SELECT lead FROM teams WHERE id=t.id)"));
    format!("WITH listed(id) AS (VALUES {ids}) SELECT t.id,{}+{},{}+{},{},{},{},{},EXISTS(SELECT 1 FROM cards WHERE team=t.id AND state='open' AND kind IN ({kinds})) FROM listed t",
        task_count("nodes", false), task_count("node_archive", false),
        task_count("nodes", true), task_count("node_archive", true),
        count("inbox", " AND state='open' AND kind GLOB 'question:*'"),
        members,
        count("phases", ""), count("phases", " AND json_extract(data,'$.state')='done'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teams::ops::tests::{Harness, TEAM};

    #[test]
    fn progress_probes_use_indexes_and_never_load_bundles_or_unbounded_archives() {
        let h = Harness::new();
        let db = h.store.db.as_ref().unwrap();
        let sql = format!("EXPLAIN QUERY PLAN {}", progress_sql(1));
        let mut statement = db.prepare(&sql).unwrap();
        let plan = statement
            .query_map([TEAM], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .join("\n");
        for index in [
            "list_nodes_progress",
            "list_archive_progress",
            "list_inbox_questions",
            "list_members_progress",
            "list_phases_progress",
            "list_cards_blocked",
        ] {
            assert!(plan.contains(index), "{index}: {plan}");
        }
        let rows = h.store.teams(1, None).unwrap().rows;
        assert_eq!(h.store.team_progress(&rows).unwrap().len(), 1);
        let too_many = vec![rows[0].clone(); 201];
        assert_eq!(
            h.store.team_progress(&too_many).unwrap_err().code,
            code::OVER_CAP
        );
    }
}
