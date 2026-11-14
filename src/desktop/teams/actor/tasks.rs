//! Task creation repeats only its stored identity. Clearing is idempotent;
//! both commands retain their known outcome until its store fact is saved.
use super::*;
use ports::TaskReply;
use serde::{Deserialize, Serialize};
pub(super) mod settle;
#[derive(Serialize, Deserialize)]
struct Command {
    node: u32,
    task: TaskRef,
    creation: bool,
    original: String,
    reply: Option<TaskReply>,
    receipt: Option<Request>,
}
impl Runtime {
    pub(super) fn stage_tasks(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let mut budget = 4;
        self.recover_commands(store, ports, team, now, &mut budget)?;
        if ports.stopped()
            || !store
                .team(&team.id)?
                .is_some_and(|t| t.status == TeamStatus::Running)
        {
            return Ok(());
        }
        self.wanted_stages(store, ports, team, now, &mut budget)?;
        self.released_stages(store, ports, team, now, &mut budget)
    }
    fn recover_commands(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let after = self
            .load::<String>(&team.id, "task_command_cursor")?
            .unwrap_or_default();
        let rows = self.ledger.prefix_page(&team.id, "task_command:", &after)?;
        let next = if rows.len() == 200 {
            rows.last().expect("full page").key.clone()
        } else {
            String::new()
        };
        for row in rows {
            if ports.stopped() {
                break;
            }
            self.task_row(store, ports, team, &row.key, now, budget)?;
        }
        self.put(&team.id, "task_command_cursor", &next).map(|_| ())
    }
    fn wanted_stages(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let cursor = self.load::<crate::teams::store::Cursor>(&team.id, "stage_cursor")?;
        let page = store.stage_tasks(&team.id, 16, cursor.as_ref())?;
        for stage in page.rows {
            if *budget == 0 || ports.stopped() {
                break;
            }
            let key = format!("task_command:stage:{}", stage.node);
            if self.ledger.get(&team.id, &key)?.is_some()
                || !self.due(store, team, &key, "", now)?
            {
                continue;
            }
            let result = match stage_reference(store, team, &stage, now) {
                Ok(task) => {
                    self.enqueue_command(store, ports, team, stage.node, task, true, now, budget)
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                self.failure(store, team, &key, "", &error, now)?;
            }
        }
        if let Some(next) = page.next {
            self.put(&team.id, "stage_cursor", &next)?;
        } else {
            self.remove_task_row(&team.id, "stage_cursor")?;
        }
        Ok(())
    }
    fn released_stages(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let after = self
            .load::<u32>(&team.id, "stage_clear_cursor")?
            .unwrap_or(0);
        let page = store.released_stages(&team.id, 16, after)?;
        let next = if page.len() == 16 {
            page.last().expect("full page").node
        } else {
            0
        };
        for keyed in page {
            if *budget == 0 || ports.stopped() {
                break;
            }
            let key = format!("task_command:clear:{}", keyed.node);
            if self.ledger.get(&team.id, &key)?.is_some()
                || !self.due(store, team, &key, "", now)?
            {
                continue;
            }
            if let Err(error) = self.enqueue_command(
                store, ports, team, keyed.node, keyed.task, false, now, budget,
            ) {
                self.failure(store, team, &key, "", &error, now)?;
            }
        }
        self.put(&team.id, "stage_clear_cursor", &next).map(|_| ())
    }
    #[allow(clippy::too_many_arguments)]
    fn enqueue_command(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        node: u32,
        task: TaskRef,
        creation: bool,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let key = format!(
            "task_command:{}:{node}",
            if creation { "stage" } else { "clear" }
        );
        let row = Command {
            node,
            task,
            creation,
            original: String::new(),
            reply: None,
            receipt: None,
        };
        self.ledger.reserve(
            &team.id,
            &key,
            serde_json::to_value(&row).map_err(crate::teams::store::storage)?,
        )?;
        boundary("reserved");
        self.task_row(store, ports, team, &key, now, budget)
    }
    fn task_row(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        key: &str,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        if !self.due(store, team, key, "", now)? {
            return Ok(());
        }
        if let Err(error) = self.advance_command(store, ports, team, key, now, budget) {
            self.failure(store, team, key, "", &error, now)?;
        }
        Ok(())
    }
    pub(super) fn advance_command(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        key: &str,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let Some(mut row) = self.load::<Command>(&team.id, key)? else {
            return Ok(());
        };
        if row.reply.is_some() {
            return self.settle_command(store, team, key, &mut row, now);
        }
        if *budget == 0
            || ports.stopped()
            || !store
                .team(&team.id)?
                .is_some_and(|t| t.status == TeamStatus::Running)
        {
            return Ok(());
        }
        let effect = if row.creation {
            store.stage_task_effect(&team.id, row.node, &row.task.uuid)?
        } else {
            None
        };
        if row.creation && effect.is_none() {
            return self.command_settled(store, team, key, now);
        }
        self.invoke_command(store, ports, team, (key, &mut row), effect, now, budget)
    }
    #[allow(clippy::too_many_arguments)]
    fn invoke_command(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        entry: (&str, &mut Command),
        effect: Option<crate::teams::store::StageTaskEffect>,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let (key, row) = entry;
        row.original = uuid::Uuid::new_v4().to_string();
        self.put(&team.id, key, row)?;
        *budget -= 1;
        let reply = if let Some(effect) = effect {
            ports.stage(team, &effect, &row.original)
        } else {
            ports.clear_stage(
                team,
                &crate::teams::store::timers::KeyedTask {
                    node: row.node,
                    task: row.task.clone(),
                },
                &row.original,
            )
        }?;
        boundary("effect");
        match &reply {
            TaskReply::Created if row.creation => (),
            TaskReply::Cleared if !row.creation => (),
            TaskReply::Refused { .. } => (),
            TaskReply::CuttingOver { reason } => {
                return Err(problem(&super::cancel::reason::spoken(reason)))
            }
            _ => return Err(problem("The task command answered another kind of effect")),
        }
        row.reply = Some(reply);
        self.put(&team.id, key, row)?;
        boundary("outcome");
        self.settle_command(store, team, key, row, now)
    }
    fn remove_task_row(&mut self, team: &str, key: &str) -> Result<()> {
        if let Some(row) = self.ledger.get(team, key)? {
            self.ledger.remove(&row)?;
        }
        Ok(())
    }
    fn command_settled(
        &mut self,
        store: &mut Store,
        team: &Team,
        key: &str,
        now: i64,
    ) -> Result<()> {
        self.recovered(store, team, key, now)?;
        self.remove_task_row(&team.id, key)
    }
}
fn stage_reference(
    store: &mut Store,
    team: &Team,
    stage: &crate::teams::store::StageTask,
    now: i64,
) -> Result<TaskRef> {
    let id = if stage.state == "wanted" {
        let id = uuid::Uuid::new_v4().to_string();
        boundary("before_intent");
        write(
            store,
            &team.id,
            Operation::StageTaskIntent {
                node: stage.node,
                effect_request_id: id.clone(),
            },
            now,
        )?;
        boundary("intent");
        id
    } else {
        stage
            .request_id
            .clone()
            .ok_or_else(|| problem("Reserved stage has no creation identity"))?
    };
    Ok(TaskRef {
        project: team.project.clone(),
        uuid: id,
    })
}
#[cfg(test)]
thread_local! {pub(super) static CRASH:std::cell::Cell<Option<&'static str>>=const{std::cell::Cell::new(None)};}
fn boundary(point: &str) {
    #[cfg(test)]
    CRASH.with(|slot| {
        if slot.get() == Some(point) {
            slot.set(None);
            panic!("fixture restart at task command {point}")
        }
    });
    #[cfg(not(test))]
    let _ = point;
}
