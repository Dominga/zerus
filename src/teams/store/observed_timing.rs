//! One immutable selected observation page; no task/session discovery or repair.
use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
pub(crate) use zerus_team_plan::observed_timing::*;

pub(crate) fn present(db: &Connection) -> Result<bool> {
    let tables:u32=db.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('observed_timing_stream','observed_timing_facts','observed_timing_units')",[],|r|r.get(0)).map_err(storage)?;
    if tables == 0 {
        return Ok(false);
    }
    if tables != 3 {
        return Err(storage(
            "The optional observation schema is partial; no repair was attempted.",
        ));
    }
    let objects:u32=db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('observed_timing_page','observed_timing_unit','observed_timing_effect','observed_timing_cohort','observed_timing_cohort_node','observed_timing_unit_page','observed_timing_verified','observed_original_once','observed_task_latest','observed_timing_kept','observed_timing_immutable','observed_timing_stream_kept','observed_timing_stream_immutable','observed_timing_units_kept','observed_timing_units_immutable')",[],|r|r.get(0)).map_err(storage)?;
    if objects != 15 {
        return Err(storage(
            "The optional observation indexes/immutability guards are unavailable.",
        ));
    }
    Ok(true)
}
pub(crate) fn extend(db: &Connection) -> Result<String> {
    if present(db)? {
        return stream(db);
    }
    db.execute_batch(include_str!("observed-timing.sql"))
        .map_err(storage)?;
    db.execute(
        "INSERT INTO observed_timing_stream(id,identity) VALUES(1,?1)",
        [stream_identity()],
    )
    .map_err(storage)?;
    stream(db)
}

#[cfg(test)]
thread_local! {static FIXTURE_STREAM:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
#[cfg(test)]
pub(crate) struct FixtureStream(bool);
#[cfg(test)]
impl Drop for FixtureStream {
    fn drop(&mut self) {
        FIXTURE_STREAM.set(self.0)
    }
}
#[cfg(test)]
#[must_use]
pub(crate) fn fixture_stream() -> FixtureStream {
    FixtureStream(FIXTURE_STREAM.replace(true))
}
fn stream_identity() -> String {
    #[cfg(test)]
    if FIXTURE_STREAM.get() {
        return "00000000-0000-4000-8000-000000001733".into();
    }
    uuid::Uuid::new_v4().to_string()
}

fn stream(db: &Connection) -> Result<String> {
    db.query_row(
        "SELECT identity FROM observed_timing_stream WHERE id=1",
        [],
        |r| r.get(0),
    )
    .map_err(storage)
}

pub(crate) fn fact(db: &Connection, seq: u64) -> Result<TimingFact> {
    let size: usize = db
        .query_row(
            "SELECT length(CAST(body AS BLOB)) FROM observed_timing_facts WHERE seq=?1",
            [seq],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if size > MAX_FACT_BYTES {
        return Err(Problem::new(
            code::OVER_CAP,
            "The observation exceeds its byte bound.",
        ));
    }
    let body: String = db
        .query_row(
            "SELECT body FROM observed_timing_facts WHERE seq=?1",
            [seq],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let row: TimingFact = decode(&body)?;
    if !validate::fact(&row)
        || row.seq != seq
        || row.schema != SCHEMA
        || row.implementation_observed_ms.is_some()
        || row.snapshot.native_model.is_some()
        || row
            .key
            .binding
            .as_ref()
            .is_some_and(|b| b.native_incarnation.is_some())
    {
        return Err(Problem::new(
            code::INVALID_STATE,
            "The observation schema is unavailable.",
        ));
    }
    Ok(row)
}
pub(crate) fn latest_task(db: &Connection, team: &str, node: u32) -> Result<Option<TimingFact>> {
    if !present(db)? {
        return Ok(None);
    }
    let seq = db.query_row("SELECT seq FROM observed_timing_facts INDEXED BY observed_task_latest WHERE team=?1 AND node=?2 AND json_extract(body,'$.source.kind')='task_document' ORDER BY seq DESC LIMIT 1", params![team,node], |r|r.get::<_,u64>(0)).optional().map_err(storage)?;
    seq.map(|seq| fact(db, seq)).transpose()
}
pub(crate) fn reserved(db: &Connection, team: &str, effect: &str) -> Result<Option<TimingFact>> {
    if !present(db)? {
        return Ok(None);
    }
    let seq = db.query_row("SELECT seq FROM observed_timing_facts WHERE team=?1 AND effect=?2 AND json_extract(body,'$.category')='delivery_reserved' ORDER BY seq LIMIT 1", params![team,effect], |r|r.get::<_,u64>(0)).optional().map_err(storage)?;
    seq.map(|seq| fact(db, seq)).transpose()
}
pub(crate) fn append(db: &Connection, fact: &TimingFact) -> Result<()> {
    use sha2::{Digest, Sha256};
    let key = encode(&fact.key)?;
    if key.len() > MAX_UNIT_KEY_BYTES {
        return Err(Problem::new(
            code::OVER_CAP,
            "The exact observation key exceeds its bound.",
        ));
    }
    let digest = format!("{:x}", Sha256::digest(key.as_bytes()));
    db.execute("INSERT OR IGNORE INTO observed_timing_units(id,team,phase,node,digest,key) VALUES(?1,?2,?3,?4,?5,?6)",params![fact.seq,fact.key.team,fact.key.phase,fact.key.node,digest,key]).map_err(storage)?;
    let (id, original): (u64, String) = db
        .query_row(
            "SELECT id,key FROM observed_timing_units WHERE team=?1 AND digest=?2",
            params![fact.key.team, digest],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage)?;
    if original != key {
        return Err(storage("The immutable observation key changed."));
    }
    let mut fact = fact.clone();
    fact.unit = id;
    if !validate::fact(&fact) {
        return Err(Problem::new(
            code::INVALID_STATE,
            "Unsupported observation source claims.",
        ));
    }
    let body = encode(&fact)?;
    if body.len() > MAX_FACT_BYTES {
        return Err(Problem::new(
            code::OVER_CAP,
            "The observation exceeds its byte bound.",
        ));
    }
    let original_request = if fact.source.kind == SourceKind::TeamJournal {
        fact.source.event_id.as_deref()
    } else {
        None
    };
    let verified = fact.category != Category::Gap
        && !fact.reasons.contains(&Reason::ClockDiscontinuity)
        && fact.last_verified_ms == Some(fact.observed_at_ms);
    db.execute("INSERT INTO observed_timing_facts(seq,unit,team,phase,node,generation,effect,original_request,observed_ms,anchor_ms,last_verified_ms,verified,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![fact.seq,id,fact.key.team,fact.key.phase,fact.key.node,fact.key.node_generation,fact.key.effect,original_request,fact.observed_at_ms,if verified {Some(fact.observed_at_ms)}else{fact.last_verified_ms},fact.last_verified_ms,verified,body]).map_err(storage)?;
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    schema: u32,
    scope: ObservedScope,
    units: Vec<String>,
    cohort_more: bool,
    stream_id: Option<String>,
    ceiling: u64,
    after: u64,
    revision: u64,
    last_verified_ms: Option<i64>,
    reasons: Vec<Reason>,
}
fn invalid_cursor() -> Problem {
    Problem::new(
        code::INVALID_STATE,
        "Observation scope changed; select a fresh page.",
    )
}
fn parse(value: &str) -> Result<Position> {
    if value.len() > MAX_CURSOR_BYTES {
        return Err(invalid_cursor());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value.strip_prefix("ot1.").ok_or_else(invalid_cursor)?)
        .map_err(|_| invalid_cursor())?;
    let p: Position = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    if p.schema != SCHEMA || p.after > p.ceiling {
        return Err(invalid_cursor());
    }
    Ok(p)
}
fn cursor(p: &Position) -> Result<String> {
    Ok(format!(
        "ot1.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(p).map_err(storage)?)
    ))
}

fn selected_units(db: &Connection, p: &Position) -> Result<ObservedCohort> {
    if p.units.len() > MAX_FACTS {
        return Err(invalid_cursor());
    }
    let mut units = Vec::with_capacity(p.units.len());
    let mut seen = std::collections::BTreeSet::new();
    for token in &p.units {
        if token.len() != 16
            || !token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(invalid_cursor());
        }
        let id = u64::from_str_radix(token, 16).map_err(|_| invalid_cursor())?;
        if id == 0 || id > p.ceiling || !seen.insert(id) {
            return Err(invalid_cursor());
        }
        let size:usize=db.query_row("SELECT length(CAST(key AS BLOB)) FROM observed_timing_units WHERE id=?1 AND team=?2 AND phase=?3",params![id,p.scope.team,p.scope.phase],|r|r.get(0)).map_err(storage)?;
        if size > MAX_UNIT_KEY_BYTES {
            return Err(Problem::new(
                code::OVER_CAP,
                "The selected observation key exceeds its bound.",
            ));
        }
        let raw: String = db
            .query_row(
                "SELECT key FROM observed_timing_units WHERE id=?1 AND team=?2 AND phase=?3",
                params![id, p.scope.team, p.scope.phase],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let key: ObservedUnitKey = decode(&raw)?;
        if key.team != p.scope.team
            || key.home != p.scope.home
            || key.phase != p.scope.phase
            || p.scope.node.is_some_and(|n| n != key.node)
        {
            return Err(invalid_cursor());
        }
        units.push(ObservedUnitRef { id, key });
    }
    Ok(ObservedCohort {
        units,
        more: p.cohort_more,
    })
}
fn verify_position(
    db: &Connection,
    p: &Position,
    cohort: &ObservedCohort,
    revision: u64,
) -> Result<()> {
    if p.revision > revision || !p.reasons.contains(&Reason::NativeUnsupported) {
        return Err(invalid_cursor());
    }
    if p.ceiling == 0 {
        if !cohort.units.is_empty() || p.last_verified_ms.is_some() {
            return Err(invalid_cursor());
        }
        return Ok(());
    }
    let end = fact(db, p.ceiling)?;
    if !cohort
        .units
        .iter()
        .any(|u| u.id == end.unit && u.key == end.key)
        || end.source.team_revision > p.revision
    {
        return Err(invalid_cursor());
    }
    let mut verified: (u64, Option<i64>) = (0, None);
    let mut stmt=db.prepare("SELECT seq,last_verified_ms FROM observed_timing_facts INDEXED BY observed_timing_verified WHERE team=?1 AND phase=?2 AND unit=?3 AND seq<=?4 AND verified ORDER BY seq DESC LIMIT 1").map_err(storage)?;
    for unit in &cohort.units {
        let point: Option<(u64, Option<i64>)> = stmt
            .query_row(
                params![p.scope.team, p.scope.phase, unit.id, p.ceiling],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some(point) = point {
            if point.0 > verified.0 {
                verified = point;
            }
        }
    }
    if verified.1 != p.last_verified_ms
        || p.scope.window.to_ms != verified.1
        || p.scope.window.from_ms
            != verified
                .1
                .map(|t| t.saturating_sub(p.scope.window.window_ms as i64).max(0))
    {
        return Err(invalid_cursor());
    }
    Ok(())
}
impl Store {
    pub fn observed_timing(
        &self,
        team: &str,
        phase: u32,
        node: Option<u32>,
        window_ms: u64,
        limit: u32,
        after: Option<&str>,
    ) -> Result<ObservedTimingPage> {
        self.active()?;
        if window_ms == 0
            || window_ms > MAX_WINDOW_MS
            || limit == 0
            || limit as usize > MAX_FACTS
            || node == Some(0)
        {
            return Err(Problem::new(
                code::OVER_CAP,
                "The selected observation window exceeds its bounds.",
            ));
        }
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team does not exist."))?;
        let tx = db.unchecked_transaction().map_err(storage)?;
        let header = rows::team(db, team)?
            .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team does not exist."))?;
        let phase_exists:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM phases WHERE team=?1 AND n=?2)",params![team,phase],|r|r.get(0)).map_err(storage)?;
        if !phase_exists {return Err(Problem::new(code::NOT_FOUND,"The selected observation phase is unavailable."));}
        let available = present(db)?;
        let identity = if available { Some(stream(db)?) } else { None };
        let position = if let Some(after) = after {
            let p = parse(after)?;
            if p.scope.team != team
                || p.scope.home != header.home
                || p.scope.phase != phase
                || p.scope.node != node
                || p.scope.window.window_ms != window_ms
                || p.stream_id != identity
            {
                return Err(invalid_cursor());
            }
            p
        } else {
            let global = if available {
                db.query_row("SELECT COALESCE((SELECT seq FROM observed_timing_facts WHERE team=?1 AND phase=?2 ORDER BY seq DESC LIMIT 1),0)",params![team,phase],|r|r.get::<_,u64>(0)).map_err(storage)?
            } else {
                0
            };
            let mut ids = Vec::new();
            let mut more = false;
            if available {
                let sql = if node.is_some() {
                    "SELECT id FROM observed_timing_units INDEXED BY observed_timing_cohort_node WHERE team=?1 AND phase=?2 AND node=?4 AND id<=?3 ORDER BY id LIMIT 201"
                } else {
                    "SELECT id FROM observed_timing_units INDEXED BY observed_timing_cohort WHERE team=?1 AND phase=?2 AND id<=?3 AND (?4 IS NULL) ORDER BY id LIMIT 201"
                };
                let mut stmt = db.prepare(sql).map_err(storage)?;
                for row in stmt
                    .query_map(params![team, phase, global, node], |r| r.get::<_, u64>(0))
                    .map_err(storage)?
                {
                    if ids.len() == MAX_FACTS {
                        more = true;
                        break;
                    }
                    ids.push(row.map_err(storage)?);
                }
            }
            let mut ceiling = 0;
            let mut verified: (u64, Option<i64>) = (0, None);
            if available {
                let mut newest=db.prepare("SELECT seq FROM observed_timing_facts INDEXED BY observed_timing_unit_page WHERE team=?1 AND phase=?2 AND unit=?3 AND seq<=?4 ORDER BY seq DESC LIMIT 1").map_err(storage)?;
                let mut valid=db.prepare("SELECT seq,last_verified_ms FROM observed_timing_facts INDEXED BY observed_timing_verified WHERE team=?1 AND phase=?2 AND unit=?3 AND seq<=?4 AND verified ORDER BY seq DESC LIMIT 1").map_err(storage)?;
                for id in &ids {
                    ceiling = ceiling.max(
                        newest
                            .query_row(params![team, phase, id, global], |r| r.get::<_, u64>(0))
                            .optional()
                            .map_err(storage)?
                            .unwrap_or(0),
                    );
                    if let Some(point) = valid
                        .query_row(params![team, phase, id, global], |r| {
                            Ok((r.get::<_, u64>(0)?, r.get::<_, Option<i64>>(1)?))
                        })
                        .optional()
                        .map_err(storage)?
                    {
                        if point.0 > verified.0 {
                            verified = point;
                        }
                    }
                }
            }
            let publication =
                super::publication::publication(db, team, phase)?.map(|p| PublicationKey {
                    id: p.id,
                    draft_generation: p.generation,
                });
            let mut reasons = vec![Reason::NativeUnsupported];
            if ceiling == 0 {
                reasons.push(Reason::NoObservations)
            } else {
                for reason in fact(db, ceiling)?.reasons {
                    if !reasons.contains(&reason) {
                        reasons.push(reason)
                    }
                }
            }
            if more {
                reasons.push(Reason::CohortCap);
            }
            if matches!(
                header.status,
                TeamStatus::Stopped | TeamStatus::Archived | TeamStatus::Finished
            ) {
                reasons.push(Reason::TerminalTeam);
            }
            Position {
                schema: SCHEMA,
                scope: ObservedScope {
                    team: team.into(),
                    home: header.home,
                    phase,
                    node,
                    publication,
                    window: ObservedWindow {
                        from_ms: verified
                            .1
                            .map(|n| n.saturating_sub(window_ms as i64).max(0)),
                        to_ms: verified.1,
                        window_ms,
                    },
                },
                units: ids.iter().map(|id| format!("{id:016x}")).collect(),
                cohort_more: more,
                stream_id: identity,
                ceiling,
                after: 0,
                revision: header.revision,
                last_verified_ms: verified.1,
                reasons,
            }
        };
        let cohort = if available {
            selected_units(db, &position)?
        } else {
            ObservedCohort {
                units: vec![],
                more: false,
            }
        };
        if available {
            verify_position(db, &position, &cohort, header.revision)?;
        }
        let mut page = ObservedTimingPage {
            schema: SCHEMA,
            scope: position.scope.clone(),
            cohort,
            stream_id: position.stream_id.clone(),
            source_revision: position.revision,
            fact_ceiling: position.ceiling,
            last_verified_ms: position.last_verified_ms,
            as_of_ms: position.last_verified_ms,
            capabilities: Capabilities::default(),
            rows: vec![],
            next: None,
            more: false,
            truncated: position.cohort_more,
            reasons: position.reasons.clone(),
        };
        if available && position.ceiling > 0 && position.scope.window.to_ms.is_some() {
            // LIMIT applies BEFORE the time-window filter. Clock reversals
            // and long-lived units must not turn a small page into a historical
            // scan. The latest finite sequence sample is explicitly partial.
            let mut metadata = std::collections::BTreeMap::new();
            let upper = if position.after == 0 {
                position.ceiling
            } else {
                position.after - 1
            };
            let mut stmt=db.prepare("SELECT seq,length(CAST(body AS BLOB)),anchor_ms FROM observed_timing_facts INDEXED BY observed_timing_unit_page WHERE team=?1 AND phase=?2 AND unit=?3 AND seq<=?4 ORDER BY seq DESC LIMIT ?5").map_err(storage)?;
            for unit in &page.cohort.units {
                for row in stmt
                    .query_map(params![team, phase, unit.id, upper, limit + 1], |r| {
                        Ok((
                            r.get::<_, u64>(0)?,
                            r.get::<_, usize>(1)?,
                            r.get::<_, Option<i64>>(2)?,
                        ))
                    })
                    .map_err(storage)?
                {
                    let (seq, size, anchor) = row.map_err(storage)?;
                    metadata.insert(seq, (size, anchor));
                    if metadata.len() > limit as usize + 1 {
                        metadata.pop_first();
                    }
                }
            }
            let mut bytes = 16 * 1024 + encode(&page.cohort)?.len();
            let mut last = position.after;
            for (inspected, (seq, (size, anchor))) in metadata.into_iter().rev().enumerate() {
                if inspected >= limit as usize {
                    page.more = true;
                    break;
                }
                if anchor.is_none_or(|at| {
                    Some(at) < position.scope.window.from_ms
                        || Some(at) > position.scope.window.to_ms
                }) {
                    page.truncated = true;
                    if !page.reasons.contains(&Reason::PartialPage) {
                        page.reasons.push(Reason::PartialPage);
                    }
                    last = seq;
                    continue;
                }
                if size > MAX_FACT_BYTES || bytes + size > MAX_PAGE_BYTES {
                    page.truncated = true;
                    page.reasons.push(Reason::ByteCap);
                    page.more = !page.rows.is_empty();
                    break;
                }
                let row = fact(db, seq)?;
                if !page
                    .cohort
                    .units
                    .iter()
                    .any(|unit| unit.id == row.unit && unit.key == row.key)
                {
                    return Err(invalid_cursor());
                }
                bytes += size;
                last = seq;
                page.rows.push(row);
            }
            page.rows.reverse();
            if page.more {
                let mut next = position.clone();
                next.after = last;
                page.next = Some(cursor(&next)?);
                page.reasons.push(Reason::PartialPage);
            }
        }
        if encode(&page)?.len() > MAX_PAGE_BYTES {
            return Err(Problem::new(
                code::OVER_CAP,
                "The observation page exceeds its byte bound.",
            ));
        }
        tx.commit().map_err(storage)?;
        Ok(page)
    }
}
