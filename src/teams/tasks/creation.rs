//! A saved stage task is guarded before its service outcome binds live_tasks.
use super::*;

const PENDING: &str = "SELECT EXISTS(SELECT 1 FROM stage_tasks s JOIN nodes n ON n.team=s.team AND n.id=s.node WHERE s.request_id=?1 AND s.team=?2 AND s.state='reserved' AND COALESCE(json_extract(n.data,'$.kind'),'task')='stage' AND n.node_state NOT IN ('accepted','cancelled'))";

pub(super) fn pending(store: &Store, task: &TaskRef, team: &str) -> Result<bool> {
    store.db.as_ref().map_or(Ok(false), |db| {
        db.query_row(PENDING, params![task.uuid, team], |r| r.get(0))
            .map_err(storage)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_creation_uses_the_unique_request_id_index() {
        let h = crate::teams::ops::tests::Harness::new();
        let db = h.store.db.as_ref().unwrap();
        let mut query = db
            .prepare(&format!("EXPLAIN QUERY PLAN {PENDING}"))
            .unwrap();
        let plan = query
            .query_map(
                params![
                    uuid::Uuid::new_v4().to_string(),
                    crate::teams::ops::tests::TEAM
                ],
                |r| r.get::<_, String>(3),
            )
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        assert!(
            plan.contains("SEARCH s USING INDEX") && plan.contains("request_id=?"),
            "{plan}"
        );
    }
}
