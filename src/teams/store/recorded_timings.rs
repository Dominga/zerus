//! Existing timing facts referenced by ONE journal page. No observation writer.
use super::*;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct EffectTiming {
    pub request_id: String,
    pub node: u32,
    pub generation: u32,
    pub member: String,
    pub kind: String,
    pub state: String,
    pub reserved_ms: i64,
    pub first_outcome_recorded_ms: Option<i64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct StageTiming {
    pub node: u32,
    pub target: u32,
    pub generation: u32,
    pub state: String,
    pub created_ms: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct RecordedFacts {
    pub effect: Option<EffectTiming>,
    pub stage: Option<StageTiming>,
    pub effect_unavailable: Option<&'static str>,
    pub stage_unavailable: Option<&'static str>,
}
#[derive(Clone, Debug, Serialize)]
pub struct RecordedEntry {
    #[serde(flatten)]
    pub event: JournalEntry,
    pub timing: RecordedFacts,
}
pub struct RecordedPage {
    pub page: Page<RecordedEntry>,
    pub revision: u64,
}

// At most two point lookups per event, with prepared statements and the
// existing UNIQUE effects(request_id) / PRIMARY KEY stage_tasks(team,node).
const EFFECT: &str = "SELECT e.request_id,e.node,e.generation,e.member,e.kind,e.state,e.created_ms,o.at_ms FROM effects e LEFT JOIN effect_outcomes o ON o.team=e.team AND o.request_id=e.request_id WHERE e.team=?1 AND e.request_id=?2";
const STAGE: &str =
    "SELECT node,target,generation,state,created_ms FROM stage_tasks WHERE team=?1 AND node=?2";

fn reference<'a>(
    refs: &'a [String],
    prefix: &str,
) -> std::result::Result<Option<&'a str>, &'static str> {
    if refs.len() > 16 {
        return Err("reference_limit");
    }
    let mut found = refs.iter().filter_map(|r| r.strip_prefix(prefix));
    let first = found.next();
    if found.next().is_some() {
        return Err("ambiguous_reference");
    }
    Ok(first)
}

fn selected_page(
    db: &Connection,
    team: &str,
    limit: u32,
    cursor: Option<&Cursor>,
    seqs: &[u64],
) -> Result<Page<JournalEntry>> {
    let invalid = || {
        Problem::new(
            code::INVALID_STATE,
            "Selected journal events changed or are unavailable; read the page again.",
        )
    };
    if !(1..=super::MAX_PAGE).contains(&limit) || seqs.len() > limit as usize {
        return Err(Problem::new(
            code::OVER_CAP,
            "Selected events exceed the page limit.",
        ));
    }
    if seqs
        .iter()
        .any(|id| *id == 0 || *id > 9_007_199_254_740_991)
        || seqs.windows(2).any(|ids| ids[0] >= ids[1])
        || cursor.is_some_and(|c| {
            c.scope != format!("{team}:journal")
                || c.after > c.ceiling
                || seqs.iter().any(|id| *id <= c.after || *id > c.ceiling)
        })
    {
        return Err(invalid());
    }
    let mut stmt = db.prepare("SELECT seq,at_ms,actor,kind,request_id,text,refs FROM journal WHERE team=?1 AND seq=?2 UNION ALL SELECT seq,at_ms,actor,kind,request_id,text,refs FROM journal_archive WHERE team=?1 AND seq=?2 LIMIT 2").map_err(storage)?;
    let mut rows = Vec::with_capacity(seqs.len());
    for id in seqs {
        let mut found = stmt
            .query_map(params![team, id], |r| {
                Ok(JournalEntry {
                    seq: r.get(0)?,
                    at_ms: r.get(1)?,
                    actor: sql_json(r, 2)?,
                    kind: r.get(3)?,
                    request_id: r.get(4)?,
                    text: r.get(5)?,
                    refs: sql_json(r, 6)?,
                })
            })
            .map_err(storage)?;
        let row = found
            .next()
            .transpose()
            .map_err(storage)?
            .ok_or_else(invalid)?;
        if found.next().transpose().map_err(storage)?.is_some() {
            return Err(invalid());
        }
        rows.push(row);
    }
    Ok(Page { rows, next: None })
}

impl Store {
    pub fn recorded_journal(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
    ) -> Result<RecordedPage> {
        self.recorded_journal_with(team, limit, cursor, || {})
    }

    fn recorded_journal_with(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
        after_page: impl FnOnce(),
    ) -> Result<RecordedPage> {
        self.recorded_with_selection(team, limit, cursor, None, after_page)
    }

    pub fn recorded_selected_journal(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
        seqs: &[u64],
    ) -> Result<RecordedPage> {
        self.recorded_with_selection(team, limit, cursor, Some(seqs), || {})
    }

    fn recorded_with_selection(
        &self,
        team: &str,
        limit: u32,
        cursor: Option<&Cursor>,
        seqs: Option<&[u64]>,
        after_page: impl FnOnce(),
    ) -> Result<RecordedPage> {
        self.active()?;
        let missing = || Problem::new(code::NOT_FOUND, "Team does not exist.");
        let db = self.db.as_ref().ok_or_else(missing)?;
        // Journal, revision and point joins share the same WAL read snapshot.
        // The cursor freezes event membership, not mutable outcome records on
        // later reads. The returned revision labels this read's fact snapshot.
        let tx = db.unchecked_transaction().map_err(storage)?;
        let page = if let Some(seqs) = seqs {
            selected_page(db, team, limit, cursor, seqs)?
        } else {
            self.journal(team, limit, cursor)?
        };
        let revision = db
            .query_row("SELECT revision FROM teams WHERE id=?1", [team], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?
            .ok_or_else(missing)?;
        after_page();
        let mut rows = Vec::with_capacity(page.rows.len());
        {
            let mut effects = db.prepare(EFFECT).map_err(storage)?;
            let mut stages = db.prepare(STAGE).map_err(storage)?;
            for event in page.rows {
                let mut facts = RecordedFacts {
                    effect: None,
                    stage: None,
                    effect_unavailable: None,
                    stage_unavailable: None,
                };
                match reference(&event.refs, "effect:") {
                    Ok(None) => {}
                    Ok(Some(id))
                        if uuid::Uuid::parse_str(id).is_ok_and(|v| v.to_string() == id) =>
                    {
                        facts.effect = effects
                            .query_row(params![team, id], |r| {
                                Ok(EffectTiming {
                                    request_id: r.get(0)?,
                                    node: r.get(1)?,
                                    generation: r.get(2)?,
                                    member: r.get(3)?,
                                    kind: r.get(4)?,
                                    state: r.get(5)?,
                                    reserved_ms: r.get(6)?,
                                    first_outcome_recorded_ms: r.get(7)?,
                                })
                            })
                            .optional()
                            .map_err(storage)?;
                        if facts.effect.is_none() {
                            facts.effect_unavailable = Some("record_missing");
                        }
                    }
                    Ok(Some(_)) => facts.effect_unavailable = Some("invalid_reference"),
                    Err(reason) => facts.effect_unavailable = Some(reason),
                }
                match reference(&event.refs, "node:") {
                    Ok(None) => {}
                    Ok(Some(id)) => match id
                        .parse::<u32>()
                        .ok()
                        .filter(|v| *v > 0 && v.to_string() == id)
                    {
                        Some(node) => {
                            facts.stage = stages
                                .query_row(params![team, node], |r| {
                                    Ok(StageTiming {
                                        node: r.get(0)?,
                                        target: r.get(1)?,
                                        generation: r.get(2)?,
                                        state: r.get(3)?,
                                        created_ms: r.get(4)?,
                                    })
                                })
                                .optional()
                                .map_err(storage)?;
                            // Most nodes are ordinary tasks. No stage record is
                            // inapplicable here, not a zero-duration stage.
                        }
                        None => facts.stage_unavailable = Some("invalid_reference"),
                    },
                    Err(reason) => facts.stage_unavailable = Some(reason),
                }
                rows.push(RecordedEntry {
                    event,
                    timing: facts,
                });
            }
        }
        tx.commit().map_err(storage)?;
        Ok(RecordedPage {
            page: Page {
                rows,
                next: page.next,
            },
            revision,
        })
    }
}

#[cfg(test)]
mod tests;
