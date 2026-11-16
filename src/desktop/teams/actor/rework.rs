//! Review and Auto use the exact correction episode and ordinary retry gate.
use super::*;

impl Runtime {
    pub(super) fn correction_review(
        &mut self,
        store: &mut Store,
        team: &Team,
        phase: u32,
        now: i64,
    ) -> Result<()> {
        let Some(row) = store.db.as_ref().map_or(Ok(None), |db| {
            crate::teams::store::rework::open(db, &team.id, phase)
        })?
        else {
            return Ok(());
        };
        let key = format!("correction-auto:{}", row.id);
        if row.state != "review" || row.approval.is_some() {
            return self.recovered(store, team, &key, now);
        }
        let (Some(generation), Some(header)) = (row.generation, row.header) else {
            return Ok(());
        };
        if team.status == TeamStatus::Running
            && team.settings.plan_approval == Approval::Auto
            && header.approval == Approval::Auto
        {
            let stamp = format!("{generation}:{}", team.revision);
            if !self.due(store, team, &key, &stamp, now)? {
                return Ok(());
            }
            match write(
                store,
                &team.id,
                Operation::ApprovePhaseCorrections {
                    phase,
                    publication: row.base_publication,
                    rework: row.id,
                    generation,
                },
                now,
            ) {
                Ok(_) => self.recovered(store, team, &key, now),
                Err(mut error) => {
                    error.message = format!(
                        "Phase {phase} corrections could not be approved: {}",
                        error.message
                    );
                    self.failure(store, team, &key, &stamp, &error, now)
                }
            }
        } else {
            self.recovered(store, team, &key, now)?;
            if store.phase_review_generation(&team.id, phase)? == Some(generation) {
                return Ok(());
            }
            write(
                store,
                &team.id,
                Operation::ReconcileCorrectionReview {
                    phase,
                    publication: row.base_publication,
                    rework: row.id,
                    generation,
                },
                now,
            )?;
            Ok(())
        }
    }
}
