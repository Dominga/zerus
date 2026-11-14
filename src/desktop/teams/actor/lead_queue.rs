//! Persist the exact reconcile request across an uncertain store write.
use super::*;
const SCOPE: &str = "lead_queue";
impl Runtime {
    pub(super) fn lead_queue(&mut self, store: &mut Store, team: &Team, now: i64) -> Result<()> {
        let saved = self.load::<Request>(&team.id, SCOPE)?;
        if saved.is_none() && !store.lead_queue_changed(team)? {
            return Ok(());
        }
        if !self.due(store, team, SCOPE, "", now)? {
            return Ok(());
        }
        let request = match saved {
            Some(request) => request,
            None => {
                let (_, request) = request(store, &team.id, Operation::ReconcileLeadQueue)?;
                self.put(&team.id, SCOPE, &request)?;
                request
            }
        };
        let actor = Actor::Service {
            person: team.owner.clone(),
            team: team.id.clone(),
        };
        match ops::apply(store, &actor, &request, now) {
            Ok(_) => self.recovered(store, team, SCOPE, now)?,
            Err(e) if e.code == ops::lead_queue::UNCHANGED || e.code == code::STALE_REVISION => {}
            Err(e) => return self.failure(store, team, SCOPE, "", &e, now),
        }
        if let Some(row) = self.ledger.get(&team.id, SCOPE)? {
            self.ledger.remove(&row)?;
        }
        Ok(())
    }
}
