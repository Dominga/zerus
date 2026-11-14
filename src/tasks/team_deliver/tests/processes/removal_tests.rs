//! zerus-63rru.77: a member removed while the real `hgs task team-deliver`
//! child for its hand-out runs. The child checks its permit twice, the
//! second time under the team store's write lock just before it reserves the
//! task attempt; after that reservation nothing stops it from typing.
use super::*;

fn removal(f: &mut Fixture) {
    f.h.run(
        &person(),
        ops::Operation::ArchiveMember {
            member: "m1".into(),
        },
    )
    .unwrap();
}
fn wait_for(f: &Fixture, prefix: &str) {
    let until = Instant::now() + Duration::from_secs(8);
    while !std::fs::read_dir(&f.config.state)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with(prefix))
    {
        assert!(Instant::now() < until, "the child never reached {prefix}");
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn effect_state(f: &Fixture) -> String {
    let rows = f.h.store.effects(TEAM, 200, None).unwrap().rows;
    rows.into_iter()
        .find(|e| e.request_id == f.body.id)
        .unwrap()
        .state
}

/// The removal lands after the child's first check and before its locked
/// one: the child refuses in its own words, reserves no attempt and types
/// nothing.
#[test]
fn a_removal_before_the_childs_locked_check_is_refused_and_nothing_is_typed() {
    let mut f = Fixture::new();
    let (child, path) = f.spawn(
        "submitted",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        true,
    );
    wait_for(&f, "ready-");
    removal(&mut f);
    assert_eq!(effect_state(&f), "superseded");
    std::fs::write(f.config.state.join("release"), "go").unwrap();
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "refused", "{out}");
    assert_eq!(
        out["reply"]["error"], "Effect is closed or belongs to another generation.",
        "{out}"
    );
    assert_eq!(f.count(), 0, "the child typed");
    assert!(f.document().attempt(&f.body.id).unwrap().is_none());
}

/// The removal lands after the child reserved the attempt and saved it:
/// nothing stops the child; it types and answers submitted. That is why the
/// removal cannot say the prompt never arrived.
#[test]
fn a_removal_after_the_childs_reservation_does_not_stop_its_typing() {
    let mut f = Fixture::new();
    let (child, path) = f.spawn(
        "type-after-pause",
        &f.args(false),
        &serde_json::to_string(&f.body).unwrap(),
        false,
    );
    wait_for(&f, "typing-");
    assert!(f.document().attempt(&f.body.id).unwrap().is_some());
    removal(&mut f);
    assert_eq!(effect_state(&f), "superseded");
    std::fs::write(f.config.state.join("release-typing"), "go").unwrap();
    let out = f.finish(child, path);
    assert_eq!(out["reply"]["state"], "submitted", "{out}");
    assert_eq!(f.count(), 1, "the child typed once");
}

/// What the person's Cancel of a started stage of a removed reviewer does,
/// with the real commands (zerus-63rru.77): the stage is started by m3 with
/// the real hand-out and `start`, m3 is removed, the person cancels, and the
/// real `hgs task team-cancel` runs; `record` is the actor's own reading and
/// recording of the child's reply. Then the real observation of the stage's
/// task, and every fact the lead and the person could see.
#[derive(Debug)]
pub(crate) struct RealCancel {
    /// The child's reply as it printed it, and the notices it sent.
    pub reply: Value,
    pub notices: Option<String>,
    /// The stage's newest decision: kind and state.
    pub cancel: (String, String),
    /// The stage's state, whether it is still live, and its hold columns.
    pub stage: (String, bool, (bool, Option<String>, bool)),
    /// The stage task's own stage and whether it is cancelled.
    pub task: (String, bool),
    /// The parent's state and hold columns, and its pipeline run's state.
    pub parent: (String, (bool, Option<String>, bool)),
    pub run: String,
    /// Every inbox item: node, kind, state and text.
    pub items: Vec<(u32, String, String, String)>,
}
pub(crate) fn removed_stage_cancel(
    session_gone: bool,
    record: impl FnOnce(&mut crate::teams::store::Store, &[u8], u32),
) -> RealCancel {
    use sha2::{Digest, Sha256};
    let mut f = stage_tests::fixture();
    let stage = f.body.node;
    let parent = f.h.node(stage).stage_of.unwrap();
    let task = stage_tests::created(&mut f);
    crate::teams::tasks::observe(&f.config.state, &task).unwrap();
    f.task = task;
    f.body.id = f.h.intent(stage, "m3");
    assert_eq!(f.run("submitted", false)["reply"]["state"], "submitted");
    f.h.outcome(
        &f.body.id,
        ops::HandoutOutcome::Submitted {
            receipt: "fixture".into(),
        },
    );
    let args = vec![
        "start".into(),
        f.task.uuid.clone(),
        "--project".into(),
        f.task.project.clone(),
        "--json".into(),
    ];
    let (child, path) = f.spawn("holder", &args, "", false);
    assert_eq!(f.finish(child, path)["ok"], true);
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    assert_eq!(
        f.h.node(stage).state,
        zerus_team_plan::model::NodeState::InProgress
    );
    removal(&mut f);
    f.h.run(
        &person(),
        ops::Operation::Cancel {
            node: stage,
            reason: "Not needed".into(),
        },
    )
    .unwrap();
    if session_gone {
        let name = "codex/deliver-m3";
        let file = f
            .config
            .state
            .join(format!("{:x}.json", Sha256::digest(name.as_bytes())));
        std::fs::remove_file(file).unwrap();
    }
    let mut args = vec!["team-cancel".into()];
    args.extend(f.args(false));
    let body = json!({"schema":1,"team":TEAM,"node":stage});
    let (child, path) = f.spawn("cancel-command", &args, &body.to_string(), false);
    let out = f.finish(child, path);
    let reply = out["reply"].clone();
    record(&mut f.h.store, reply.to_string().as_bytes(), stage);
    let notices = std::fs::read_to_string(f.config.state.join("cancel-notices")).ok();
    crate::teams::tasks::observe(&f.config.state, &f.task).unwrap();
    let db = f.h.store.db.as_ref().unwrap();
    let node_row = |id: u32| -> (String, bool, (bool, Option<String>, bool)) {
        db.query_row(
            "SELECT node_state,1,hold,hold_owner,hand_back FROM nodes WHERE team=?1 AND id=?2 UNION ALL SELECT node_state,0,hold,hold_owner,hand_back FROM node_archive WHERE team=?1 AND id=?2 LIMIT 1",
            params![TEAM, id],
            |r| Ok((r.get(0)?, r.get::<_, i64>(1)? == 1, (r.get(2)?, r.get(3)?, r.get(4)?))),
        )
        .unwrap()
    };
    let decision =
        f.h.store
            .decisions(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .rev()
            .find(|d| d.node == stage)
            .unwrap();
    let view = f.document().view().unwrap();
    let (parent_state, _, parent_hold) = node_row(parent);
    RealCancel {
        reply,
        notices,
        cancel: (decision.kind, format!("{:?}", decision.state)),
        stage: node_row(stage),
        task: (view.stage.as_str().to_owned(), view.cancelled_at.is_some()),
        parent: (parent_state, parent_hold),
        run: db
            .query_row(
                "SELECT state FROM pipeline_runs WHERE team=?1 AND node=?2",
                params![TEAM, parent],
                |r| r.get(0),
            )
            .unwrap(),
        items: f
            .h
            .store
            .inbox(TEAM, 200, None)
            .unwrap()
            .rows
            .into_iter()
            .map(|i| (i.node, i.kind, i.state, i.text))
            .collect(),
    }
}
