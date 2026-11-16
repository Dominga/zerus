use super::*;

fn blocked(reason: &str) -> Availability {
    Availability {
        allowed: false,
        reason: Some(reason.into()),
    }
}

pub(crate) fn view(
    db: &Connection,
    team: &Team,
    p: &Phase,
    custody: Option<&Result<task_custody::TaskCustody>>,
) -> Result<Option<PhaseControls>> {
    if p.n == 0 || p.kind != PhaseKind::Plan || p.mode != PhaseMode::Standard {
        return Ok(None);
    }
    if !installed(db)? {
        return Ok(None);
    }
    let capability = prerequisites(db, team, p)?;
    let active = capability.allowed;
    let scope_ref = scope(db, &team.id, p.n)?;
    let pause = pause(db, &team.id, p)?;
    let prestart = if active {
        task_custody::availability(custody, db, team, p)?
    } else {
        capability.clone()
    };
    let mut all = phases(db, &team.id)?;
    all.retain(|p| p.n != 0);
    all.sort_by_key(|p| (if p.order == 0 { p.n } else { p.order }, p.n));
    let index = all
        .iter()
        .position(|candidate| candidate.n == p.n)
        .ok_or_else(|| storage("The selected phase is missing from its header scope."))?;
    let neighbor = |p: Option<&Phase>| -> Result<PhaseMove> {
        let Some(p) = p else {
            return Ok(PhaseMove {
                target: None,
                availability: blocked("There is no neighboring planned phase in that direction."),
            });
        };
        let target = Some(scope(db, &team.id, p.n)?);
        let availability = if !prestart.allowed {
            prestart.clone()
        } else {
            let current = prerequisites(db, team, p)?;
            if current.allowed {
                task_custody::availability(custody, db, team, p)?
            } else {
                current
            }
        };
        Ok(PhaseMove {
            target,
            availability,
        })
    };
    let earlier = neighbor(index.checked_sub(1).and_then(|i| all.get(i)))?;
    let later = neighbor(all.get(index + 1))?;
    let resume = if !active {
        capability.clone()
    } else if matches!(pause, PhasePause::Paused { .. }) {
        capability.clone()
    } else {
        blocked("The phase is not paused.")
    };
    Ok(Some(PhaseControls {
        version: 1,
        scope: scope_ref,
        pause,
        unstarted: prestart.clone(),
        barrier: capability.clone(),
        pause_change: capability,
        resume,
        acceptance: prestart,
        earlier,
        later,
    }))
}
