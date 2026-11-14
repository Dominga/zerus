use super::*;
use ports::Finish;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
struct Hold {
    node: u32,
    stamp: String,
    reason: String,
}
impl Runtime {
    pub(super) fn finish(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let mut cursor = None;
        let mut budget = 8;
        for _ in 0..4 {
            let page = store.pending_accepts(&team.id, 200, cursor.as_ref())?;
            for decision in page.rows {
                if ports.stopped() {
                    return Ok(());
                }
                let key = format!("finish:{}", decision.node);
                if let Err(error) = self.finish_one(store, ports, team, &decision, now, &mut budget)
                {
                    self.failure(store, team, &key, "", &error, now)?;
                }
            }
            cursor = page.next;
            if cursor.is_none() {
                break;
            }
        }
        if cursor.is_some() {
            return Err(problem("Finish page bound exceeded"));
        }
        self.finish_cleanup(store, team, now)
    }
    pub(super) fn finish_one(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        d: &crate::teams::store::Decision,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let Some(n) = store.node(&team.id, d.node)? else {
            return Ok(());
        };
        let Some(o) = store.task_observation(&team.id, d.node)? else {
            return Ok(());
        };
        if o.stage == ops::TaskStage::Done
            || store.generation(&team.id, d.node)? != Some(d.generation)
        {
            return Ok(());
        }
        if team.settings.integration == Integration::Branch
            && n.kind != NodeKind::Stage
            && !n.integrated
        {
            return Ok(());
        }
        let ready = store.reviewed_task_current(&team.id, d.node, d.generation)?;
        if !self.held_step(
            store,
            team,
            ("finish", d.node, d.generation, d.seq, d.kind.as_str()),
            ready,
            now,
        )? {
            return Ok(());
        }
        let stamp = serde_json::to_string(&(d.seq, d.generation, &o.revision))
            .map_err(crate::teams::store::storage)?;
        let held = format!("finish_hold:{}", d.node);
        let refusal = format!("finish_refused:{}", d.node);
        let key = format!("finish:{}", d.node);
        if let Some(h) = self.load::<Hold>(&team.id, &held)? {
            if h.stamp == stamp {
                if self.due(store, team, &refusal, &stamp, now)? {
                    self.force_failure(store, team, &refusal, &stamp, &problem(&h.reason), now)?;
                }
                return Ok(());
            }
            self.recovered(store, team, &refusal, now)?;
            if let Some(row) = self.ledger.get(&team.id, &held)? {
                self.ledger.remove(&row)?;
            }
        }
        if *budget == 0 || !self.due(store, team, &key, "", now)? {
            return Ok(());
        }
        *budget -= 1;
        self.finish_request(store, ports, team, (&n, d), &stamp, now)
    }
    fn finish_request(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        target: (&Node, &crate::teams::store::Decision),
        stamp: &str,
        now: i64,
    ) -> Result<()> {
        let (n, d) = target;
        let held = format!("finish_hold:{}", n.id);
        let refusal = format!("finish_refused:{}", n.id);
        let key = format!("finish:{}", n.id);
        let id = uuid::Uuid::new_v4().to_string();
        boundary("before");
        let reply = ports.finish(team, n, d, &id);
        boundary("after");
        match reply {
            Ok(Finish::Done) => self.cooldown(store, team, &key, stamp, now.saturating_add(60_000)),
            Ok(Finish::Stale) => self.defer_finish(store, ports, team, target, now),
            Ok(Finish::Refused(reason)) => {
                let reason = super::cancel::reason::spoken(&reason);
                self.put(
                    &team.id,
                    &held,
                    &Hold {
                        node: n.id,
                        stamp: stamp.into(),
                        reason: cut(&reason, 2048),
                    },
                )?;
                self.force_failure(store, team, &refusal, stamp, &problem(&reason), now)
            }
            Ok(Finish::CuttingOver(reason)) => {
                let reason = super::cancel::reason::spoken(&reason);
                self.failure(store, team, &key, stamp, &problem(&reason), now)
            }
            Err(error) => self.failure(store, team, &key, stamp, &error, now),
        }
    }
    fn defer_finish(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        target: (&Node, &crate::teams::store::Decision),
        now: i64,
    ) -> Result<()> {
        let (n, d) = target;
        let task = n
            .task
            .as_ref()
            .ok_or_else(|| problem("Stale finish has no task"))?;
        ports.observe(store, task)?;
        let observed = store
            .task_observation(&team.id, n.id)?
            .ok_or_else(|| problem("Stale finish observation disappeared"))?;
        let fresh = serde_json::to_string(&(d.seq, d.generation, &observed.revision))
            .map_err(crate::teams::store::storage)?;
        self.defer(store, team, &format!("finish:{}", n.id), &fresh, now)
    }
    fn finish_cleanup(&mut self, store: &mut Store, team: &Team, now: i64) -> Result<()> {
        for row in self.ledger.page(&team.id, "finish_hold:")? {
            if !row.key.starts_with("finish_hold:") {
                break;
            }
            let hold: Hold =
                serde_json::from_value(row.data.clone()).map_err(crate::teams::store::storage)?;
            if store
                .task_observation(&team.id, hold.node)?
                .is_some_and(|o| o.stage == ops::TaskStage::Done || o.key != ops::KeyState::Ours)
            {
                self.recovered(store, team, &format!("finish_refused:{}", hold.node), now)?;
                self.ledger.remove(&row)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
thread_local! {pub(super) static CRASH:std::cell::Cell<Option<&'static str>>=const {std::cell::Cell::new(None)};}
fn boundary(name: &str) {
    #[cfg(test)]
    CRASH.with(|c| {
        if c.get() == Some(name) {
            c.set(None);
            panic!("fixture restart at finish {name}");
        }
    });
    #[cfg(not(test))]
    let _ = name;
}
