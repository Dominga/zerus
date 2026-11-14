use super::*;
impl Runtime {
    pub(super) fn reports(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let mut cursor = None;
        for _ in 0..4 {
            let page = store.nodes_by_state(&team.id, NodeState::Reported, 200, cursor.as_ref())?;
            for node in page.rows {
                if ports.stopped() {
                    return Ok(());
                }
                let key = format!("report:{}", node.id);
                let revision = store
                    .task_observation(&team.id, node.id)?
                    .map_or(String::new(), |o| o.revision);
                if !self.due(store, team, &key, &revision, now)? {
                    continue;
                }
                if let Err(error) = self.report(store, ports, team, &node, now) {
                    self.failure(store, team, &key, &revision, &error, now)?;
                }
            }
            cursor = page.next;
            if cursor.is_none() {
                return Ok(());
            }
        }
        Err(problem("Report page bound exceeded"))
    }
    fn report(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        node: &Node,
        now: i64,
    ) -> Result<()> {
        let key = format!("report:{}", node.id);
        let Some(observation) = store.task_observation(&team.id, node.id)? else {
            return Ok(());
        };
        if observation.stage != ops::TaskStage::InReview {
            return Ok(());
        }
        let Some(generation) = store.generation(&team.id, node.id)? else {
            return Ok(());
        };
        if store.report(&team.id, node.id, generation)?.is_some() {
            return self.recovered(store, team, &key, now);
        }
        // Its holder was removed, which took the task back (zerus-63rru.77):
        // the report is not the team's. The lead is told once, and the step
        // ends here: no retry, no failure, no card.
        if let Some(member) = store.removed_reporter(&team.id, node.id, generation)? {
            if !store.late_report_told(&team.id, node.id, generation)? {
                write(
                    store,
                    &team.id,
                    Operation::RecordLateReport {
                        node: node.id,
                        generation,
                        member,
                    },
                    now,
                )?;
            }
            return self.recovered(store, team, &key, now);
        }
        let Some(task) = node.task.as_ref() else {
            return Ok(());
        };
        let Some(text) = ports.report(task)? else {
            return self.cooldown(
                store,
                team,
                &key,
                &observation.revision,
                now.saturating_add(60_000),
            );
        };
        let effect = self
            .open_effects(store, &team.id)?
            .into_iter()
            .filter(|e| e.node == node.id && e.generation == generation)
            .max_by_key(|e| e.seq)
            .ok_or_else(|| problem("Reviewed task has no current holder"))?;
        let member = store
            .member(&team.id, &effect.member)?
            .ok_or_else(|| problem("Report holder disappeared"))?;
        let head = ports.reported(team, &member)?;
        boundary("read");
        // The member's own words: they keep their line feeds and tabs and lose
        // every other control character before they are bounded and recorded.
        let text = crate::teams::clean::lines(&text);
        let bounded = cut(&text, 15_000);
        // The service's own statements stand beside the member's text, not
        // inside it: the stored text is exactly the member's bounded words.
        let mut notes = Vec::new();
        if bounded.len() < text.len() {
            notes.push(crate::teams::ops::ReportNote::new("text_shortened"));
        }
        if head.dirty {
            notes.push(crate::teams::ops::ReportNote::new("worktree_dirty"));
        }
        // The report and its receipt share the store transaction and the
        // (team,node,generation) primary key. Retrying this read cannot add a
        // second report after a lost result; no session effect is involved.
        write(
            store,
            &team.id,
            Operation::RecordReport {
                node: node.id,
                generation,
                member: effect.member,
                text: bounded,
                commit: head.commit,
                notes,
            },
            now,
        )?;
        boundary("recorded");
        self.recovered(store, team, &key, now)
    }
}

#[cfg(test)]
thread_local! {pub(super) static CRASH:std::cell::Cell<Option<&'static str>>=const {std::cell::Cell::new(None)};}
fn boundary(name: &str) {
    #[cfg(test)]
    CRASH.with(|c| {
        if c.get() == Some(name) {
            c.set(None);
            panic!("fixture restart at report {name}");
        }
    });
    #[cfg(not(test))]
    let _ = name;
}
