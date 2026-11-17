//! Additive pause attribution. Existing engines retain the authoritative
//! Phase.paused bit; this companion alone never admits or suppresses work.
use super::*;
use zerus_team_plan::phase_control::*;

mod prestart;
pub(crate) mod task_custody;
mod view;
pub(crate) use prestart::unstarted;
pub(crate) use view::view;

const OBJECTS: &[(&str, &str)] = &[
    ("phase_controls_layout", "CREATE TABLE phase_controls_layout (singleton INTEGER PRIMARY KEY CHECK(singleton=1), format TEXT NOT NULL CHECK(format='phase_pause_episodes'))"),
    ("phase_pause_episodes", "CREATE TABLE phase_pause_episodes (id TEXT PRIMARY KEY, team TEXT NOT NULL, phase INTEGER NOT NULL, owner TEXT NOT NULL CHECK(length(CAST(owner AS BLOB))<=4096), reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB)) BETWEEN 1 AND 2048), created_ms INTEGER NOT NULL, active INTEGER NOT NULL CHECK(active IN (0,1)), FOREIGN KEY(team,phase) REFERENCES phases(team,n))"),
    ("phase_pause_current", "CREATE UNIQUE INDEX phase_pause_current ON phase_pause_episodes(team,phase) WHERE active=1"),
    ("phase_pause_history", "CREATE INDEX phase_pause_history ON phase_pause_episodes(team,phase,id)"),
    ("phase_pause_identity", "CREATE TRIGGER phase_pause_identity BEFORE UPDATE OF id,team,phase,owner,reason,created_ms ON phase_pause_episodes BEGIN SELECT RAISE(ABORT,'the original phase pause identity and reason are immutable'); END"),
    ("stage_tasks_target_history", "CREATE INDEX stage_tasks_target_history ON stage_tasks(team,target,node)"),
];

/// Complete control facts may exclude only the one exact reserved NEW
/// publication of an open correction. Every earlier publication and node
/// remains part of this structural proof; completion/acceptance is separate.
pub(crate) fn prerequisites(db: &Connection, team: &Team, p: &Phase) -> Result<Availability> {
    let blocked = |s: &str| Availability {
        allowed: false,
        reason: Some(s.into()),
    };
    if p.n == 0
        || p.kind != PhaseKind::Plan
        || p.mode != PhaseMode::Standard
        || !matches!(
            team.status,
            TeamStatus::Planning | TeamStatus::Running | TeamStatus::Paused
        )
        || !matches!(
            p.state,
            PhaseState::Draft
                | PhaseState::Rejected
                | PhaseState::Approved
                | PhaseState::AwaitingAcceptance
        )
    {
        return Ok(blocked(
            "Only a nonterminal planned phase of an active team can change controls.",
        ));
    }
    let identity = scope(db, &team.id, p.n)?;
    match identity {
        PhaseControlScope::Draft { .. } => {
            if !matches!(p.state, PhaseState::Draft | PhaseState::Rejected)
                || drafts::nodes(db, &team.id, p.n)?.is_empty()
            {
                return Ok(blocked(
                    "The selected phase has no complete current draft scope.",
                ));
            }
        }
        PhaseControlScope::Published {
            publication: id, ..
        } => {
            let current = publication::by_id(db, &team.id, &id)?
                .ok_or_else(|| storage("The control publication disappeared."))?;
            if !publication_input(db, &team.id, p.n, &current)? {
                return Ok(blocked(
                    "The selected phase has no complete frozen publication scope.",
                ));
            }
            if current.state == "reserved" {
                if p.state != PhaseState::Draft {
                    return Ok(blocked("The phase state conflicts with its publication."));
                }
            } else {
                if current.state != "published"
                    || !matches!(
                        p.state,
                        PhaseState::Approved | PhaseState::AwaitingAcceptance
                    )
                {
                    return Ok(blocked("The phase state conflicts with its publication."));
                }
                let episode = rework::open(db, &team.id, p.n)?;
                if let Some(row) = &episode {
                    if row
                        .header
                        .as_ref()
                        .is_some_and(|h| !crate::teams::ops::rework::controls(p, h))
                    {
                        return Err(storage(
                            "The retained correction controls conflict with the phase.",
                        ));
                    }
                    if let Some(new) = &row.approval {
                        let reserved = publication::by_id(db, &team.id, new)?
                            .ok_or_else(|| storage("The correction approval is missing."))?;
                        if row.state != "publishing"
                            || reserved.state != "reserved"
                            || Some(reserved.generation) != row.generation
                            || !publication_input(db, &team.id, p.n, &reserved)?
                        {
                            return Err(storage("The reserved correction publication conflicts with its exact episode."));
                        }
                    }
                }
                let excluded = episode.as_ref().and_then(|r| r.approval.as_deref());
                let retained =
                    phase_snapshot::load_except(db, &team.id, p.n, team.revision, excluded)?;
                if !retained.complete || retained.nodes.is_empty() {
                    return Ok(blocked(
                        "The complete published phase membership is unavailable.",
                    ));
                }
            }
        }
    }
    Ok(Availability {
        allowed: true,
        reason: None,
    })
}

fn publication_input(
    db: &Connection,
    team: &str,
    phase: u32,
    p: &publication::Publication,
) -> Result<bool> {
    let belongs: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM phase_publications WHERE id=?1 AND team=?2 AND phase=?3 AND generation=?4)", params![p.id,team,phase,p.generation],|r|r.get(0)).map_err(storage)?;
    if !belongs || p.total == 0 || p.total > 200 {
        return Ok(false);
    }
    let frozen = publication::snapshot(db, team, &p.id)?;
    let ids: std::collections::BTreeSet<_> = frozen.iter().map(|n| n.id).collect();
    if frozen.len() != p.total as usize || ids.len() != frozen.len() || ids.contains(&0) {
        return Ok(false);
    }
    let mut stmt = db.prepare("SELECT team,node FROM phase_publication_nodes WHERE publication=?1 ORDER BY node LIMIT 201").map_err(storage)?;
    let rows = stmt
        .query_map([&p.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
        })
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    Ok(rows.len() == ids.len()
        && rows
            .iter()
            .all(|(home, id)| home == team && ids.contains(id)))
}

fn normalized(sql: &str) -> String {
    sql.trim_end_matches(';')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Absence is a legacy store. Partial, altered or newer shapes are refused.
pub(crate) fn installed(db: &Connection) -> Result<bool> {
    let mut found = 0;
    for (name, expected) in OBJECTS {
        let sql: Option<String> = db
            .query_row("SELECT sql FROM sqlite_master WHERE name=?1", [name], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?;
        if let Some(sql) = sql {
            found += 1;
            if normalized(&sql) != normalized(expected) {
                return Err(storage("The phase control layout is unknown or damaged."));
            }
        }
    }
    if found == 0 {
        return Ok(false);
    }
    if found != OBJECTS.len() {
        return Err(storage("The phase control layout is incomplete."));
    }
    let valid: bool = db
        .query_row(
            "SELECT count(*)=1 AND min(singleton)=1 AND min(format)='phase_pause_episodes' FROM phase_controls_layout",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if !valid {
        return Err(storage("The phase control layout mark is unknown."));
    }
    Ok(true)
}

pub(super) fn extend(db: &Connection) -> Result<()> {
    if installed(db)? {
        return Ok(());
    }
    let own = db.is_autocommit();
    db.execute_batch(if own {
        "BEGIN IMMEDIATE"
    } else {
        "SAVEPOINT phase_controls"
    })
    .map_err(begin_error)?;
    let result = execution_fence(db)
        .and_then(|()| timers::fence(db))
        .and_then(|()| {
            if installed(db)? {
                return Ok(());
            }
            for (_, sql) in OBJECTS {
                db.execute_batch(sql).map_err(storage)?;
            }
            db.execute(
                "INSERT INTO phase_controls_layout VALUES(1,'phase_pause_episodes')",
                [],
            )
            .map_err(storage)?;
            installed(db).map(|_| ())
        });
    let end = match (&result, own) {
        (Ok(()), true) => "COMMIT",
        (Ok(()), false) => "RELEASE phase_controls",
        (Err(_), true) => "ROLLBACK",
        (Err(_), false) => "ROLLBACK TO phase_controls; RELEASE phase_controls",
    };
    db.execute_batch(end).map_err(storage)?;
    result
}

pub(crate) fn phase(db: &Connection, team: &str, n: u32) -> Result<Phase> {
    let text: Option<Option<String>> = db.query_row(
        "SELECT CASE WHEN length(CAST(data AS BLOB))<=4096 THEN data END FROM phases WHERE team=?1 AND n=?2",
        params![team,n], |r| r.get(0),
    ).optional().map_err(storage)?;
    match text {
        Some(Some(text)) => {
            let p: Phase = decode(&text)?;
            if p.n != n {
                return Err(storage("The phase header conflicts with its identity."));
            }
            Ok(p)
        }
        Some(None) => Err(Problem::new(
            code::OVER_CAP,
            "The phase header exceeds its byte bound.",
        )),
        None => Err(Problem::new(code::NOT_FOUND, "Phase does not exist.")),
    }
}

pub(crate) fn phases(db: &Connection, team: &str) -> Result<Vec<Phase>> {
    let mut stmt = db
        .prepare("SELECT n FROM phases WHERE team=?1 ORDER BY n LIMIT 201")
        .map_err(storage)?;
    let ids = stmt
        .query_map([team], |r| r.get::<_, u32>(0))
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)?;
    if ids.len() > 200 {
        return Err(Problem::new(
            code::OVER_CAP,
            "The phase header scope exceeds its bound.",
        ));
    }
    ids.into_iter().map(|n| phase(db, team, n)).collect()
}

pub(crate) fn scope(db: &Connection, team: &str, n: u32) -> Result<PhaseControlScope> {
    let episode = rework::open(db, team, n)?;
    let publication = if let Some(row) = &episode {
        publication::by_id(db, team, &row.base_publication)?
    } else {
        publication::publication(db, team, n)?
    };
    if let Some(p) = publication {
        if p.generation == 0 || p.total == 0 || p.total > 200 {
            return Err(storage(
                "The current phase publication has incomplete identity.",
            ));
        }
        let matches: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM phase_publications WHERE id=?1 AND team=?2 AND phase=?3)", params![&p.id,team,n],|r|r.get(0)).map_err(storage)?;
        let result = PhaseControlScope::Published {
            phase: n,
            generation: p.generation,
            publication: p.id,
            rework: episode.map(|r| CorrectionScope {
                id: r.id,
                generation: r.generation,
                approval: r.approval,
            }),
        };
        if !matches || !result.valid() {
            return Err(storage(
                "The current publication or correction has a conflicting phase identity.",
            ));
        }
        Ok(result)
    } else {
        if episode.is_some() {
            return Err(storage("The correction has no retained base publication."));
        }
        let generation: Option<u64> = db
            .query_row(
                "SELECT generation FROM phase_drafts WHERE team=?1 AND phase=?2",
                params![team, n],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        match generation {
            Some(generation)
                if (PhaseControlScope::Draft {
                    phase: n,
                    generation,
                })
                .valid() =>
            {
                Ok(PhaseControlScope::Draft {
                    phase: n,
                    generation,
                })
            }
            _ => Err(storage(
                "The selected phase has no current draft generation.",
            )),
        }
    }
}

pub(crate) fn pause(db: &Connection, team: &str, p: &Phase) -> Result<PhasePause> {
    pause_in_layout(db, team, p, installed(db)?)
}

/// Called by the one readiness loader used at intent and final admission.
/// Check the format once, then only indexed current rows for <=200 phases.
pub(crate) fn validate_pauses(db: &Connection, team: &str, phases: &[Phase]) -> Result<()> {
    let present = installed(db)?;
    for p in phases {
        pause_in_layout(db, team, p, present)?;
    }
    Ok(())
}

fn pause_in_layout(db: &Connection, team: &str, p: &Phase, present: bool) -> Result<PhasePause> {
    let episode: Option<(String, Option<String>, Option<String>, i64)> = if present {
        db.query_row(
            "SELECT id,CASE WHEN length(CAST(owner AS BLOB))<=4096 THEN owner END,CASE WHEN length(CAST(reason AS BLOB))<=2048 THEN reason END,created_ms FROM phase_pause_episodes WHERE team=?1 AND phase=?2 AND active=1",
            params![team,p.n], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)),
        ).optional().map_err(storage)?
    } else {
        None
    };
    if !p.paused {
        if episode.is_some() {
            return Err(storage(
                "The pause episode conflicts with the phase pause bit.",
            ));
        }
        return Ok(PhasePause::Running);
    }
    if episode.is_none() && present {
        let attributed: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM phase_pause_episodes INDEXED BY phase_pause_history WHERE team=?1 AND phase=?2)",
            params![team, p.n], |r| r.get(0),
        ).map_err(storage)?;
        if attributed {
            return Err(storage(
                "The paused phase has attributed history but no current episode.",
            ));
        }
    }
    let episode = episode
        .map(|(id, owner, reason, created_ms)| -> Result<PauseEpisode> {
            let owner: PauseOwner =
                decode(&owner.ok_or_else(|| storage("The phase pause owner exceeds its bound."))?)?;
            let reason =
                reason.ok_or_else(|| storage("The phase pause reason exceeds its bound."))?;
            if uuid::Uuid::parse_str(&id)
                .map(|u| u.to_string() != id)
                .unwrap_or(true)
                || reason.trim().is_empty()
                || zerus_team_plan::forbidden_char(&reason).is_some()
                || created_ms < 0
            {
                return Err(storage("The saved pause identity or reason is invalid."));
            }
            if !owner.valid() {
                return Err(storage("The saved phase pause owner is invalid."));
            }
            Ok(PauseEpisode {
                id,
                owner,
                reason,
                created_ms,
            })
        })
        .transpose()?;
    Ok(PhasePause::Paused { episode })
}
