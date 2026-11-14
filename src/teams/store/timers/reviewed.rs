//! The actor's reads before it finishes, merges or reopens a decided task:
//! whether the newest observation still shows what the decision was taken
//! for. Each reads one row of each table it names, on the ordinary store
//! path, whatever the node's history.
use super::*;

/// The node's current generation is the named one, its task is bound, and
/// the newest observation has the key ours (an Ours key is not serialized),
/// is not cancelled and has no conflict of attempts.
macro_rules! current {
    () => {
        "SELECT EXISTS(SELECT 1 FROM task_observations o JOIN live_tasks l ON l.team=o.team AND l.node=o.node WHERE o.team=?1 AND o.node=?2 AND COALESCE((SELECT n.generation FROM nodes n WHERE n.team=?1 AND n.id=?2),(SELECT a.generation FROM node_archive a WHERE a.team=?1 AND a.id=?2))=?3 AND json_extract(o.data,'$.key') IS NULL AND NOT json_extract(o.data,'$.cancelled') AND NOT o.attempt_conflict AND "
    };
}
/// The current attempt is the execution the node took in: the remembered
/// attempt, or, for a row that holds no attempt at all, the legacy pair on
/// both sides.
macro_rules! took_in {
    () => {
        "((o.attempt_id<>'' AND o.attempt_id=o.last_attempt_id) OR (o.attempt=0 AND o.attempt_id='' AND o.last_attempt_id=''))"
    };
}
/// In review under the execution the node took in.
macro_rules! reviewed {
    () => {
        concat!(
            "(json_extract(o.data,'$.stage')='in_review' AND ",
            took_in!(),
            ")"
        )
    };
}
pub(crate) mod sql {
    pub const REVIEWED: &str = concat!(current!(), reviewed!(), ")");
    /// At any stage: no current attempt, or the one the node took in.
    pub const OWN: &str = concat!(current!(), "(o.attempt_id='' OR ", took_in!(), "))");
    pub const ACCEPTED: &str = concat!(
        current!(),
        "(json_extract(o.data,'$.stage')='done' OR ",
        reviewed!(),
        "))"
    );
}

impl Store {
    /// True only when the node's current generation is `generation`, its
    /// task is bound, and the newest observation of that task has the key
    /// ours, the stage In review, is not cancelled, has no conflict of
    /// attempts, and its current attempt is the execution the node took in
    /// (the remembered attempt, or the legacy pair on both sides). False in
    /// every other case, without an observation or a bound task included.
    pub fn reviewed_task_current(&self, team: &str, node: u32, generation: u32) -> Result<bool> {
        self.read(|db| {
            db.query_row(sql::REVIEWED, params![team, node, generation], |r| r.get(0))
                .map_err(storage)
        })
    }
    /// For a reopen and a hand-over, whether the newest observation leaves
    /// the execution the node took in uncontradicted: the node's current
    /// generation is `generation`, its task is bound, and the observation
    /// has the key ours, is not cancelled, has no conflict of attempts, and
    /// its current attempt is empty or the one the node took in, at any
    /// stage; the stage is the command's business. False otherwise.
    pub fn own_task_current(&self, team: &str, node: u32, generation: u32) -> Result<bool> {
        self.read(|db| {
            db.query_row(sql::OWN, params![team, node, generation], |r| r.get(0))
                .map_err(storage)
        })
    }
    /// Whether the newest observation agrees with an acceptance: as for
    /// `reviewed_task_current`, or the task is Done, whoever executed it.
    /// The conflict flag counts on Done too.
    pub fn accepted_task_current(&self, team: &str, node: u32, generation: u32) -> Result<bool> {
        self.read(|db| {
            db.query_row(sql::ACCEPTED, params![team, node, generation], |r| r.get(0))
                .map_err(storage)
        })
    }
}
