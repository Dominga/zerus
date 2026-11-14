use super::super::ops::{KeyState, TaskObservation};
use super::*;
use rusqlite::params;

pub(crate) fn team(db: &Connection, id: &str) -> Result<Option<Team>> {
    db.query_row("SELECT id,project,name,goal,home,owner,status,lead,settings,bundle,integration_branch,base_commit,revision FROM teams WHERE id=?1", [id], |r| Ok(Team {
        id:r.get(0)?,project:r.get(1)?,name:r.get(2)?,goal:r.get(3)?,home:r.get(4)?,owner:r.get(5)?,
        status:sql_json(r,6)?,lead:r.get(7)?,settings:sql_json(r,8)?,bundle:sql_json(r,9)?,
        integration_branch:r.get(10)?,base_commit:r.get(11)?,revision:r.get(12)?,
    })).optional().map_err(storage)
}
pub(crate) fn member(db: &Connection, team: &str, id: &str) -> Result<Option<Member>> {
    db.query_row(
        "SELECT data FROM members WHERE team=?1 AND id=?2",
        params![team, id],
        |r| sql_json(r, 0),
    )
    .optional()
    .map_err(storage)
}
pub(crate) const NODE_COLUMNS: &str = "team,id,data,rank,admitted,routed,hold,hold_owner,hand_back,task_revision,task_stage,task_cancelled,generation,decided_by,integrated,integration_commit,requested,integration_required";

/// The earlier implementers a target recorded (zerus-63rru.20), read by
/// the handout's eligibility from the store's own rows.
pub(crate) fn earlier_implementers(
    db: &rusqlite::Connection,
    team: &str,
    target: u32,
) -> rusqlite::Result<Vec<String>> {
    let mut statement =
        db.prepare("SELECT member FROM target_implementers WHERE team=?1 AND target=?2")?;
    statement
        .query_map(rusqlite::params![team, target], |r| r.get(0))
        .and_then(|rows| rows.collect())
}

pub(crate) fn node_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    let mut node: Node = sql_json(r, 0)?;
    node.rank = r.get(1)?;
    node.admitted = r.get(2)?;
    node.hold = r.get(3)?;
    let state: String = r.get(4)?;
    node.state = serde_json::from_value(serde_json::Value::String(state)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    node.decided_by = match r.get::<_, Option<String>>(5)? {
        Some(s) => Some(serde_json::from_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?),
        None => None,
    };
    node.integrated = r.get(6)?;
    Ok(node)
}
/// The node columns every node read takes, from `table` (`nodes` or
/// `node_archive`), its hold given by [`held`].
pub(crate) fn node_select(table: &str) -> String {
    format!(
        "{table}.data,{table}.rank,{table}.admitted,{},{table}.node_state,{table}.decided_by,{table}.integrated",
        held(table)
    )
}
/// Whether a node of `table` is held: the one answer readiness, the rows and
/// every operation take (the plan, "A hold has an owner"). A node is held by
/// an explicit hold (`hold`, owned by the person or the lead in
/// `hold_owner`), by a hand-back to the team's next step (`hand_back`), or
/// by an unsettled decision of its own, whose hold is the decision's and is
/// never stored: it ends with the decision, however the decision ends.
pub(crate) fn held(table: &str) -> String {
    format!("({table}.hold OR {table}.hand_back OR EXISTS(SELECT 1 FROM decisions d WHERE d.team={table}.team AND d.node={table}.id AND d.state<>'settled'))")
}
/// Who set a node's explicit hold: `person`, `lead`, or nobody.
pub(crate) fn hold_owner(db: &Connection, team: &str, node: u32) -> Result<Option<String>> {
    db.query_row(
        "SELECT hold_owner FROM nodes WHERE team=?1 AND id=?2 AND hold",
        params![team, node],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
    .map(Option::flatten)
}
/// The one writer of an explicit hold: set with its owner, or released.
pub(crate) fn set_explicit_hold(
    db: &Connection,
    team: &str,
    node: u32,
    owner: Option<&str>,
) -> Result<()> {
    db.execute(
        "UPDATE nodes SET hold=?3 IS NOT NULL,hold_owner=?3 WHERE team=?1 AND id=?2",
        params![team, node, owner],
    )
    .map_err(storage)?;
    Ok(())
}
/// Ends every hand-back of a team: the stop's Cancel is each node's next
/// decision, and takes the hold over while it is pending. An explicit hold
/// stays.
pub(crate) fn end_hand_backs(db: &Connection, team: &str) -> Result<()> {
    db.execute(
        "UPDATE nodes SET hand_back=0 WHERE team=?1 AND hand_back",
        [team],
    )
    .map_err(storage)?;
    Ok(())
}
/// Whether a node is held for the team's next step.
pub(crate) fn handed_back(db: &Connection, team: &str, node: u32) -> Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM nodes WHERE team=?1 AND id=?2 AND hand_back)",
        params![team, node],
        |r| r.get(0),
    )
    .map_err(storage)
}
/// The one writer of a hand-back's hold: a node held for the team's next
/// step, the lead's decision after a hand-back or a pipeline's own step (a
/// stage in review, the person's acceptance). That next decision takes it
/// over or ends it; an explicit hold is never touched here.
pub(crate) fn set_hand_back(db: &Connection, team: &str, node: u32, value: bool) -> Result<()> {
    db.execute(
        "UPDATE nodes SET hand_back=?3 WHERE team=?1 AND id=?2",
        params![team, node, value],
    )
    .map_err(storage)?;
    Ok(())
}

pub(crate) fn deps(db: &Connection, team: &str, id: u32) -> Result<Vec<Dep>> {
    let mut statement=db.prepare("SELECT dependency,kind FROM dependencies WHERE team=?1 AND node=?2 ORDER BY dependency LIMIT 201").map_err(storage)?;
    let deps = statement
        .query_map(params![team, id], |r| {
            Ok(Dep {
                node: r.get(0)?,
                kind: sql_json(r, 1)?,
            })
        })
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)?;
    if deps.len() > 200 {
        return Err(Guard::Dependencies.problem());
    }
    Ok(deps)
}
pub(crate) fn node(db: &Connection, team: &str, id: u32) -> Result<Option<Node>> {
    let sql = format!(
        "SELECT {} FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT {} FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1",
        node_select("nodes"),
        node_select("node_archive")
    );
    let mut node = db
        .query_row(&sql, params![team, id], node_row)
        .optional()
        .map_err(storage)?;
    if let Some(node) = node.as_mut() {
        node.deps = deps(db, team, id)?;
    }
    Ok(node)
}

/// The payload holds only node metadata. State, task stage, admission and
/// queue order have independent columns; no serialized NodeState is persisted.
pub(crate) fn node_payload(node: &Node) -> Result<String> {
    let mut value = serde_json::to_value(node).map_err(storage)?;
    if let Some(fields) = value.as_object_mut() {
        for name in [
            "state",
            "rank",
            "admitted",
            "hold",
            "decided_by",
            "integrated",
            "deps",
        ] {
            fields.remove(name);
        }
    }
    encode(&value)
}

/// A decided node, accepted or cancelled, leaves the live queue. One whose
/// task is bound leaves it only when the newest observation agrees: the
/// task has ended (cancelled or Done) and its attempts do not conflict.
/// Otherwise what was decided stays recorded on the node, and the next
/// observation decides; the service observes only the live queue. A node
/// whose cancel is still pending stays where the cancel step finds it, in
/// a live and in a stopped team: that cancel is settled by its own answer
/// or by an observation, never by this archive.
const LEAVES: &str = "team=?1 AND node_state IN ('accepted','cancelled') AND (NOT EXISTS(SELECT 1 FROM live_tasks l WHERE l.team=nodes.team AND l.node=nodes.id) OR EXISTS(SELECT 1 FROM task_observations o WHERE o.team=nodes.team AND o.node=nodes.id AND NOT o.attempt_conflict AND (json_extract(o.data,'$.cancelled') OR json_extract(o.data,'$.stage')='done'))) AND NOT EXISTS(SELECT 1 FROM decisions d WHERE d.team=nodes.team AND d.node=nodes.id AND d.state<>'settled' AND d.kind='cancel')";
pub(crate) fn archive_decided(db: &Connection, team: &str, actor: &str) -> Result<()> {
    let ids = {
        let mut statement = db
            .prepare(&format!(
                "SELECT id FROM nodes WHERE {LEAVES} ORDER BY id LIMIT 601"
            ))
            .map_err(storage)?;
        let mapped = statement
            .query_map([team], |r| r.get::<_, u32>(0))
            .map_err(storage)?;
        mapped
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)?
    };
    if ids.len() > 600 {
        return Err(Guard::LiveNodes.problem());
    }
    for node in ids {
        // A settings change can make an in-flight accepted task terminal.
        settle_decisions(db, team, Some(node), actor, true)?;
    }
    db.execute(
        &format!("UPDATE nodes SET requested=NULL WHERE {LEAVES}"),
        [team],
    )
    .map_err(storage)?;
    db.execute(
        &format!(
            "INSERT INTO node_archive({NODE_COLUMNS}) SELECT {NODE_COLUMNS} FROM nodes WHERE {LEAVES}"
        ),
        [team],
    )
    .map_err(storage)?;
    db.execute(&format!("DELETE FROM nodes WHERE {LEAVES}"), [team])
        .map_err(storage)?;
    Ok(())
}

pub(crate) fn settle_decisions(
    db: &Connection,
    team: &str,
    node: Option<u32>,
    actor: &str,
    preserve_ready: bool,
) -> Result<()> {
    // Terminal settlement cannot revoke a merge that was already authorized.
    // Revocation marks an awaiting accept that was once Ready. Keep that mark
    // even on rejection: its receipt is provenance, never a new write permit.
    db.execute(&format!("UPDATE decisions SET stopped_ready=(kind='accept' AND ((?4 AND state=?5) OR (state=?6 AND stopped_ready))),state=?2 WHERE team=?1 AND state<>'{}' AND (?3 IS NULL OR node=?3)",DecisionState::Settled.as_str()),params![team,DecisionState::Settled,node,preserve_ready,DecisionState::Ready,DecisionState::AwaitingOwner]).map_err(storage)?;
    close_cards(db, team, node, actor)?;
    Ok(())
}
/// Hands a node back to the lead: its unsettled decision settles, the node is
/// held, and the lead gets one inbox item of the cause's own kind. The one
/// writer of that outcome: the step to layout 10, a member's removal, and a
/// writer of a Return whose holder was removed. Each caller moves the team's
/// revision once.
/// The open `member_started` items of a member, for the actor's close
/// decision (zerus-63rru.86 round 3): the member's segment with its
/// delimiter, so a prefix of another member never matches.
pub(crate) fn open_member_started(db: &Connection, team: &str, member: &str) -> Result<i64> {
    db.query_row(
        "SELECT COUNT(*) FROM inbox WHERE team=?1 AND state='open' AND substr(kind,1,15)='member_started:' AND substr(kind,16+instr(substr(kind,16),':'),length(?2)+1)=?2||':'",
        params![team, member],
        |r| r.get(0),
    )
    .map_err(storage)
}

pub(crate) fn hand_back(
    db: &Connection,
    team: &str,
    node: u32,
    actor: &str,
    kind: &str,
    text: &str,
    now: i64,
) -> Result<u64> {
    settle_decisions(db, team, Some(node), actor, false)?;
    set_hand_back(db, team, node, true)?;
    hand_back_item(db, team, node, actor, kind, text, now)
}
/// The lead's inbox item of a hand-back, of the cause's own kind, in the
/// node's current generation.
pub(crate) fn hand_back_item(
    db: &Connection,
    team: &str,
    node: u32,
    actor: &str,
    kind: &str,
    text: &str,
    now: i64,
) -> Result<u64> {
    let generation: u32 = db
        .query_row(
            "SELECT generation FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT generation FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1",
            params![team, node],
            |r| r.get(0),
        )
        .map_err(storage)?;
    db.execute(
        "INSERT INTO inbox(team,kind,node,generation,actor,text,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![team, kind, node, generation, actor, text, now],
    )
    .map_err(storage)?;
    Ok(db.last_insert_rowid() as u64)
}
/// The merge commit recorded for a decision, if a merge fact was written.
/// The commit of one acceptance's integration fact, if it has one. A
/// conflict fact stores NULL, so this reads the column as an Option.
pub(crate) fn integration_commit(
    db: &Connection,
    team: &str,
    node: u32,
    decision: u64,
) -> Result<Option<String>> {
    db.query_row(
        "SELECT commit_id FROM integration_facts WHERE team=?1 AND node=?2 AND decision=?3",
        params![team, node, decision],
        |r| r.get::<_, Option<String>>(0),
    )
    .optional()
    .map_err(storage)
    .map(|row| row.flatten())
}

pub(crate) fn merged_commit(db: &Connection, team: &str, decision: u64) -> Result<Option<String>> {
    db.query_row(
        "SELECT commit_id FROM integration_facts WHERE team=?1 AND decision=?2 AND commit_id IS NOT NULL LIMIT 1",
        params![team, decision],
        |r| r.get(0),
    )
    .optional()
    .map_err(storage)
}
/// One owner card and one lead inbox item when a merge fact survives a
/// rejected integration (zerus-63rru.25): the rejection did not undo the
/// merge, and only a revert on the integration branch removes it. Keyed by
/// the decision, so a replay opens nothing; the card takes acknowledged
/// alone. Nothing else is written.
pub(crate) fn rejected_merge(
    db: &Connection,
    team: &str,
    id: u32,
    decision: u64,
    branch: &str,
    actor: &str,
    now: i64,
) -> Result<bool> {
    let Some(commit) = merged_commit(db, team, decision)? else {
        return Ok(false);
    };
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM cards WHERE team=?1 AND kind='rejected_merge' AND node=?2 AND decision_seq=?3)",
            params![team, id, decision],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if exists {
        return Ok(false);
    }
    let n = node(db, team, id)?.ok_or_else(|| {
        Problem::new(code::INVALID_STATE, "The merged node is missing.")
    })?;
    let generation: u32 = db
        .query_row(
            "SELECT generation FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT generation FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1",
            params![team, id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let task = n
        .task
        .as_ref()
        .map(|task| format!("{}/{}", task.project, task.uuid))
        .unwrap_or_default();
    let facts = if task.is_empty() {
        format!("{} (node {id})", n.title)
    } else {
        format!("{} (task {task}, node {id})", n.title)
    };
    let text = format!(
        "{facts} was merged as {commit} into {branch} before this integration was rejected. The rejection did not undo the merge; only a revert on {branch} removes it."
    );
    db.execute(
        "INSERT INTO cards(team,kind,node,actor,text,created_ms,decision_seq) VALUES(?1,'rejected_merge',?2,?3,?4,?5,?6)",
        params![team, id, actor, text, now, decision],
    )
    .map_err(storage)?;
    // The inbox is unique by (team, kind, node, generation): the decision in
    // the kind lets two such decisions of one node and generation both land.
    db.execute(
        "INSERT INTO inbox(team,kind,node,generation,actor,text,created_ms) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![team, format!("rejected_merge:{decision}"), id, generation, actor, text, now],
    )
    .map_err(storage)?;
    Ok(true)
}

/// A node's settlement expires its integration cards; team settlement expires
/// every open card, including proposals without a node.
pub(crate) fn close_cards(
    db: &Connection,
    team: &str,
    node: Option<u32>,
    actor: &str,
) -> Result<()> {
    db.execute("UPDATE cards SET state='closed',decision='stale',decided_actor=?3 WHERE team=?1 AND state='open' AND (?2 IS NULL OR (kind='integration' AND node=?2))",params![team,node,actor]).map_err(storage)?;
    Ok(())
}
pub(crate) fn archived_leaf(db: &Connection, team: &str, id: u32) -> Result<Option<Node>> {
    db.query_row(
        &format!(
            "SELECT {} FROM node_archive WHERE team=?1 AND id=?2",
            node_select("node_archive")
        ),
        params![team, id],
        node_row,
    )
    .optional()
    .map_err(storage)
}

pub(crate) fn card_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Card> {
    let kind: String = r.get(1)?;
    Ok(Card {
        seq: r.get(0)?,
        kind: kind.clone(),
        node: r.get(2)?,
        actor: sql_json(r, 3)?,
        text: r.get(4)?,
        state: r.get(5)?,
        decision: r.get(6)?,
        parameters: r
            .get::<_, Option<String>>("parameters")?
            .map(|s| serde_json::from_str(&s).map_err(|e| rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e))))
            .transpose()?
            .unwrap_or_default(),
        decided_actor: r
            .get::<_, Option<String>>(7)?
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        created_ms: r.get(8)?,
        decisions: match r.get::<_, Option<String>>(9)? {
            Some(list) => serde_json::from_str(&list).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    9,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
            None => default_decisions(&kind),
        },
        notes: sql_json(r, 10)?,
    })
}
/// A card's decisions: the stored list, or by its kind. An integration card
/// and a pipeline acceptance card take approve and reject; any other card
/// opened without a list takes acknowledged alone.
pub(crate) fn card_decisions(kind: &str, stored: Option<&str>) -> Result<Vec<String>> {
    match stored {
        Some(list) => serde_json::from_str(list).map_err(storage),
        None => Ok(default_decisions(kind)),
    }
}
pub(crate) fn default_decisions(kind: &str) -> Vec<String> {
    if matches!(kind, "integration" | "pipeline_acceptance") {
        vec!["approve".into(), "reject".into()]
    } else {
        vec!["acknowledged".into()]
    }
}

/// A single indexed lookup; scope is checked before returning any card data.
pub(crate) const CARD_BY_SEQ_SQL: &str = "SELECT seq,kind,node,actor,text,state,decision,decided_actor,created_ms,decisions,notes,team,parameters FROM cards WHERE seq=?1";
pub(crate) fn card(db: &Connection, team: &str, seq: u64) -> Result<Option<Card>> {
    let row: Option<(String, Card)> = db
        .query_row(CARD_BY_SEQ_SQL, [seq], |r| Ok((r.get(11)?, card_row(r)?)))
        .optional()
        .map_err(storage)?;
    row.map(|(owner, card)| {
        if owner != team {
            Err(Problem::new(
                code::INVALID_STATE,
                "Card belongs to a different team.",
            ))
        } else {
            Ok(card)
        }
    })
    .transpose()
}

/// One bounded snapshot per node, independent of the archived plan projection.
pub(crate) fn observation(
    db: &Connection,
    team: &str,
    node: u32,
) -> Result<Option<TaskObservation>> {
    let saved: Option<TaskObservation> = db
        .query_row(
            "SELECT data FROM task_observations WHERE team=?1 AND node=?2",
            params![team, node],
            |r| sql_json(r, 0),
        )
        .optional()
        .map_err(storage)?;
    let mut observation = if let Some(saved) = saved {
        Some(saved)
    } else {
        legacy_observation(db, team, node)?
    };
    if let Some(o) = observation.as_mut() {
        o.previous_revision = o.revision.clone();
    }
    Ok(observation)
}
fn legacy_observation(db: &Connection, team: &str, node: u32) -> Result<Option<TaskObservation>> {
    let raw: Option<(String,String,bool,String,String)> = db.query_row(
        "SELECT task_revision,task_stage,task_cancelled,rank,data FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT task_revision,task_stage,task_cancelled,rank,data FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1",
        params![team,node], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,sql_json::<Node>(r,4)?.title))).optional().map_err(storage)?;
    raw.filter(|r| !r.0.is_empty())
        .map(|(revision, stage, cancelled, rank, title)| {
            Ok(TaskObservation {
                previous_revision: revision.clone(),
                revision,
                stage: serde_json::from_value(serde_json::Value::String(stage)).map_err(storage)?,
                cancelled,
                current_cancelled: false,
                rank,
                title,
                key: KeyState::Ours,
                attempt: 0,
                attempt_id: String::new(),
                attempt_conflict: false,
            })
        })
        .transpose()
}
pub(crate) fn released(db: &Connection, team: &str, node: u32) -> Result<bool> {
    Ok(db
        .query_row(
            "SELECT released FROM task_observations WHERE team=?1 AND node=?2",
            params![team, node],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?
        .unwrap_or(false))
}
pub(crate) fn record_observation(
    db: &Connection,
    team: &str,
    node: u32,
    o: &TaskObservation,
    released: bool,
) -> Result<()> {
    // The newest observation's attempt pair as it came, a conflict's empty ID
    // included. The attempt the node runs under is remembered apart, only
    // when the node takes an execution in (ops::timers::remember).
    db.execute("INSERT INTO task_observations(team,node,data,released,attempt,attempt_id,attempt_conflict) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(team,node) DO UPDATE SET data=excluded.data,released=MAX(task_observations.released,excluded.released),attempt=excluded.attempt,attempt_id=excluded.attempt_id,attempt_conflict=excluded.attempt_conflict",params![team,node,encode(o)?,released,o.attempt,o.attempt_id,o.attempt_conflict]).map_err(storage)?;
    Ok(())
}
