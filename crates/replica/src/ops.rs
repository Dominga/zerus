//! The task operations that need their project: where a task goes in its
//! list and whether its prompt could be sent. hgs and the mobile apps use
//! these, so the rules hold the same everywhere.
use crate::render::{task_prompt, Tools};
use crate::schema::{Actor, Stage, Target};
use crate::store::{Position, ProjectStore};
use crate::task::{check_new_stage, NewTask, TaskDoc};
use crate::Result;

impl ProjectStore {
    /// Refuses a task whose prompt could not be sent: the rendered prompt
    /// follows hgs's input limits. `project_name` names the project in it.
    pub fn check_prompt(&self, task: &TaskDoc, project_name: &str) -> Result<()> {
        let settings = self.project()?.settings();
        task_prompt(
            &settings.task_template,
            &task.view()?,
            project_name,
            Tools::Cli,
        )
        .map(|_| ())
    }

    /// A new task at `position` of its stage, To do or Backlog; unsaved.
    #[allow(clippy::too_many_arguments)]
    pub fn new_task(
        &self,
        text: &str,
        stage: Stage,
        target: Target,
        author: Actor,
        position: &Position,
        project_name: &str,
        now: i64,
    ) -> Result<TaskDoc> {
        // Before the rank: placing it in another stage could renumber that stage.
        check_new_stage(stage)?;
        let rank = self.rank_for(stage, position, None)?;
        let id = self.unused_short_id()?;
        let task = TaskDoc::create(NewTask {
            project: self.project_id(),
            id: &id,
            text,
            stage,
            target,
            author,
            rank: &rank,
            now,
        })?;
        self.check_prompt(&task, project_name)?;
        Ok(task)
    }

    /// Named-card creation is idempotent under its caller-reserved UUID. The
    /// project lock is held by this store for the lookup and final save.
    #[allow(clippy::too_many_arguments)]
    pub fn new_named_task(
        &self,
        title: &str,
        text: &str,
        files: &[crate::TaskAttachment],
        stage: Stage,
        target: Target,
        author: Actor,
        position: &Position,
        project_name: &str,
        now: i64,
        request: Option<&str>,
        start: bool,
    ) -> Result<(TaskDoc, bool)> {
        use sha2::{Digest, Sha256};
        fn identities(value: &mut serde_json::Value) {
            if let Some(object) = value.as_object_mut() {
                if object
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| !id.is_empty())
                {
                    object.remove("session");
                }
                for child in object.values_mut() {
                    identities(child);
                }
            }
        }
        check_new_stage(stage)?;
        crate::task::check_title(title)?;
        if !text.is_empty() {
            crate::schema::check_text(text, "description")?;
        }
        crate::attachment::check_attachments(files)?;
        let mut intent = serde_json::json!({"project":self.project_id(), "title":title,"text":text,"stage":stage,"target":target,"author":author,"position":format!("{position:?}"),"start":start,
            "files":files.iter().map(|file| file.metadata()).collect::<Result<Vec<_>>>()?});
        identities(&mut intent);
        let fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(&intent)?));
        if let Some(uuid) = request {
            if !crate::ids::is_uuid(uuid) {
                return Err(crate::Error::new("invalid task creation identity"));
            }
            if self.snapshot_path(uuid).exists() || self.updates_path(uuid).exists() {
                let existing = self.find(uuid)?;
                if existing.creation_fingerprint().as_deref() != Some(fingerprint.as_str()) {
                    return Err(crate::Error::new("this task creation identity belongs to another input; inspect the existing card"));
                }
                return Ok((existing, false));
            }
        }
        let rank = self.rank_for(stage, position, None)?;
        let id = self.unused_short_id()?;
        let mut task = TaskDoc::create_named(
            NewTask {
                project: self.project_id(),
                id: &id,
                text,
                stage,
                target,
                author: author.clone(),
                rank: &rank,
                now,
            },
            title,
            request.map(|uuid| (uuid, fingerprint.as_str())),
        )?;
        if !files.is_empty() {
            task.set_attachments(files, &author, now)?;
        }
        if start {
            task.record_start(&author, now)?;
        }
        self.check_prompt(&task, project_name)?;
        Ok((task, true))
    }

    /// Replaces the prompt of a task in To do or Backlog.
    pub fn edit_task(
        &self,
        task: &mut TaskDoc,
        text: &str,
        actor: &Actor,
        project_name: &str,
        now: i64,
    ) -> Result<()> {
        task.edit_text(text, actor, now)?;
        self.check_prompt(task, project_name)
    }

    /// Moves a task to stage `to`; a task that changes list goes to the top
    /// of its new list.
    pub fn change_stage(
        &self,
        task: &mut TaskDoc,
        to: Stage,
        actor: &Actor,
        now: i64,
    ) -> Result<()> {
        let from = task.stage()?;
        task.set_stage(to, actor, now)?;
        if from != to && matches!(to, Stage::Todo | Stage::Backlog) {
            let rank = self.rank_for(to, &Position::Top, Some(task.uuid()))?;
            task.set_rank(&rank)?;
        }
        Ok(())
    }

    /// Puts a task at `position` within its current stage, without changing execution.
    pub fn reorder(&self, task: &mut TaskDoc, position: &Position) -> Result<()> {
        task.writable()?;
        let rank = self.rank_for(task.stage()?, position, Some(task.uuid()))?;
        task.set_rank(&rank)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Actor, ErrorKind, Position, ProjectStore, Stage, Target, MAX_TEXT};

    fn open() -> (tempfile::TempDir, ProjectStore) {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "p1").unwrap();
        (root, store)
    }

    fn file(store: &ProjectStore, text: &str, stage: Stage, position: Position) -> String {
        let mut task = store
            .new_task(
                text,
                stage,
                Target::Any,
                Actor::Person,
                &position,
                "orchard",
                1,
            )
            .unwrap();
        store.save(&mut task).unwrap();
        task.uuid().to_owned()
    }

    fn texts(store: &ProjectStore, stage: Stage) -> Vec<String> {
        store
            .ordered(stage)
            .unwrap()
            .iter()
            .map(|t| t.view().unwrap().text)
            .collect()
    }

    #[test]
    fn named_card_recovery_never_reverts_later_content_or_stage() {
        let (_root, store) = open();
        let request = "aaaaaaaa-aaaa-4aaa-baaa-aaaaaaaaaaaa";
        let (mut task, created) = store
            .new_named_task(
                "Initial title",
                "",
                &[],
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &Position::End,
                "orchard",
                1,
                Some(request),
                false,
            )
            .unwrap();
        assert!(created);
        assert_eq!(task.uuid(), request);
        assert_eq!(task.schema(), 4);
        assert_eq!(task.view().unwrap().text, "");
        store.save(&mut task).unwrap();
        task.edit_text("Detailed requirement", &Actor::Person, 2)
            .unwrap();
        task.set_title("Renamed card", &Actor::Person, 3).unwrap();
        task.set_stage(Stage::Backlog, &Actor::Person, 4).unwrap();
        store.save(&mut task).unwrap();
        let before = task.view().unwrap();
        let (recovered, created) = store
            .new_named_task(
                "Initial title",
                "",
                &[],
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &Position::End,
                "orchard",
                5,
                Some(request),
                false,
            )
            .unwrap();
        assert!(!created);
        assert_eq!(recovered.view().unwrap(), before);
        assert!(store
            .new_named_task(
                "Other title",
                "",
                &[],
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &Position::End,
                "orchard",
                6,
                Some(request),
                false
            )
            .is_err());
        assert!(store
            .new_named_task(
                "Initial title",
                "",
                &[],
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &Position::Top,
                "orchard",
                6,
                Some(request),
                false
            )
            .is_err());
        assert!(store
            .new_named_task(
                "Initial title",
                "",
                &[],
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &Position::End,
                "orchard",
                6,
                Some(request),
                true
            )
            .is_err());
        assert_eq!(store.ordered(Stage::Backlog).unwrap().len(), 1);
    }

    #[test]
    fn named_card_title_and_description_remain_independent() {
        let (_root, store) = open();
        for title in ["", " ", "one\ntwo", "one\ttwo", &"x".repeat(513)] {
            assert!(store
                .new_named_task(
                    title,
                    "",
                    &[],
                    Stage::Todo,
                    Target::Any,
                    Actor::Person,
                    &Position::End,
                    "orchard",
                    1,
                    None,
                    false
                )
                .is_err());
        }
        let (mut task, _) = store
            .new_named_task(
                "Title only",
                "",
                &[],
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &Position::End,
                "orchard",
                1,
                None,
                false,
            )
            .unwrap();
        let actor = Actor::Agent(crate::SessionKey {
            machine: "fixture".into(),
            session: "codex/work".into(),
            session_id: "launch_id:fixture".into(),
        });
        task.record_start(&actor, 2).unwrap();
        let before = task.view().unwrap();
        task.set_title("Renamed during work", &Actor::Person, 3)
            .unwrap();
        let after = task.view().unwrap();
        assert_eq!(after.stage, before.stage);
        assert_eq!(after.session, before.session);
        assert_eq!(after.text, "");
        assert_eq!(after.log.last().unwrap().kind, "title");
        task.set_title("Renamed during work", &Actor::Person, 4)
            .unwrap();
        assert_eq!(task.view().unwrap().log, after.log);
    }

    #[test]
    fn a_new_task_goes_where_asked_and_only_to_todo_or_backlog() {
        let (_root, store) = open();
        file(&store, "Second", Stage::Todo, Position::End);
        file(&store, "First", Stage::Todo, Position::Top);
        file(&store, "Later", Stage::Backlog, Position::End);
        assert_eq!(texts(&store, Stage::Todo), ["First", "Second"]);
        assert_eq!(texts(&store, Stage::Backlog), ["Later"]);
        for stage in [Stage::Active, Stage::Review, Stage::Done] {
            let error = store
                .new_task(
                    "Nope",
                    stage,
                    Target::Any,
                    Actor::Person,
                    &Position::End,
                    "orchard",
                    1,
                )
                .err()
                .unwrap();
            assert_eq!(error.kind(), ErrorKind::Refused);
            assert_eq!(error.to_string(), "a new task goes to Backlog or To do");
        }
    }

    #[test]
    fn a_prompt_that_could_not_be_sent_is_refused() {
        let (_root, store) = open();
        let text = "x".repeat(MAX_TEXT);
        let error = store
            .new_task(
                &text,
                Stage::Todo,
                Target::Any,
                Actor::Person,
                &Position::End,
                "orchard",
                1,
            )
            .err()
            .unwrap();
        assert_eq!(error.kind(), ErrorKind::Refused);
        let uuid = file(&store, "Short", Stage::Todo, Position::End);
        let mut task = store.load(&uuid).unwrap();
        assert!(store
            .edit_task(&mut task, &text, &Actor::Person, "orchard", 2)
            .is_err());
        store
            .edit_task(&mut task, "Still short", &Actor::Person, "orchard", 2)
            .unwrap();
        assert_eq!(task.view().unwrap().text, "Still short");
    }

    #[test]
    fn a_task_that_changes_list_goes_to_its_top() {
        let (_root, store) = open();
        file(&store, "Old backlog", Stage::Backlog, Position::End);
        let uuid = file(&store, "Moved", Stage::Todo, Position::End);
        let mut task = store.load(&uuid).unwrap();
        store
            .change_stage(&mut task, Stage::Backlog, &Actor::Person, 2)
            .unwrap();
        store.save(&mut task).unwrap();
        assert_eq!(texts(&store, Stage::Backlog), ["Moved", "Old backlog"]);
    }

    #[test]
    fn reorder_moves_a_task_within_its_list() {
        let (_root, store) = open();
        file(&store, "One", Stage::Todo, Position::End);
        file(&store, "Two", Stage::Todo, Position::End);
        let uuid = file(&store, "Three", Stage::Todo, Position::End);
        let mut task = store.load(&uuid).unwrap();
        store.reorder(&mut task, &Position::Index(0)).unwrap();
        store.save(&mut task).unwrap();
        assert_eq!(texts(&store, Stage::Todo), ["Three", "One", "Two"]);
    }
    #[test]
    fn every_stage_can_be_reordered_without_changing_execution_or_history() {
        use crate::SessionKey;
        for stage in [
            Stage::Backlog,
            Stage::Todo,
            Stage::Active,
            Stage::Review,
            Stage::Done,
        ] {
            let (_root, store) = open();
            let owner = SessionKey {
                machine: "m1".into(),
                session: "codex/api".into(),
                session_id: "launch_id:worker".into(),
            };
            let mut ids = Vec::new();
            for text in ["One", "Two", "Three"] {
                let uuid = file(
                    &store,
                    text,
                    if stage == Stage::Backlog {
                        stage
                    } else {
                        Stage::Todo
                    },
                    Position::End,
                );
                let mut task = store.load(&uuid).unwrap();
                if matches!(stage, Stage::Active | Stage::Review | Stage::Done) {
                    task.record_send(&owner, &format!("request-{text}"), &Actor::Person, 2)
                        .unwrap();
                    if matches!(stage, Stage::Review | Stage::Done) {
                        task.record_report("Result", &Actor::Agent(owner.clone()), 3)
                            .unwrap();
                    }
                    if stage == Stage::Done {
                        task.set_stage(stage, &Actor::Person, 4).unwrap();
                    }
                    store.save(&mut task).unwrap();
                }
                let mut ordered = store.load(&uuid).unwrap();
                ordered.set_rank(["A", "B", "C"][ids.len()]).unwrap();
                store.save(&mut ordered).unwrap();
                ids.push(uuid);
            }
            let mut task = store.load(&ids[2]).unwrap();
            let before = serde_json::to_value(task.view().unwrap()).unwrap();
            store
                .reorder(&mut task, &Position::Before(ids[0].clone()))
                .unwrap();
            store.save(&mut task).unwrap();
            assert_eq!(texts(&store, stage), ["Three", "One", "Two"]);
            let mut after =
                serde_json::to_value(store.load(&ids[2]).unwrap().view().unwrap()).unwrap();
            after["rank"] = before["rank"].clone();
            assert_eq!(
                after, before,
                "a board reorder changed execution in {stage:?}"
            );
            // An anchor in another stage must still be refused.
            let other = file(
                &store,
                "Other stage",
                if stage == Stage::Backlog {
                    Stage::Todo
                } else {
                    Stage::Backlog
                },
                Position::End,
            );
            assert!(store.reorder(&mut task, &Position::After(other)).is_err());
        }
    }
}
