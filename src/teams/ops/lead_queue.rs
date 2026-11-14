//! One service-owned inbox episode per waiting category, never per task.
use super::*;

const PREFIXES: [&str; 2] = ["lead_admission:", "lead_routing:"];
const ENDED: &str = "The service ended this item: no tasks wait in this category.";
pub const UNCHANGED: &str = "lead_queue_unchanged";

#[derive(Debug, PartialEq, Eq)]
struct Episode {
    seq: u64,
    category: usize,
    text: String,
}
fn counts(db: &Connection, team: &Team) -> Result<[u32; 2]> {
    let mut counts = [0, 0];
    // Both queue partitions have enforced caps. Read one extra rather than
    // allowing a corrupt store to turn this pass into an unbounded scan.
    for (category, admitted, cap) in [(0, false, 100), (1, true, 500)] {
        if category == 1 && team.settings.routing != Routing::Lead {
            continue;
        }
        let sql = if admitted {
            "SELECT count(*) FROM (SELECT id FROM nodes WHERE team=?1 AND admitted=1 AND routed=0 AND node_state IN ('waiting','pending') LIMIT ?2)"
        } else {
            "SELECT count(*) FROM (SELECT id FROM nodes WHERE team=?1 AND admitted=0 AND node_state IN ('waiting','pending') LIMIT ?2)"
        };
        let count: u32 = db
            .query_row(sql, params![team.id, cap + 1], |r| r.get(0))
            .map_err(storage)?;
        if count > cap {
            return Err(Problem::new(
                code::OVER_CAP,
                "The waiting queue exceeds its bounded partition.",
            ));
        }
        counts[category] = count;
    }
    Ok(counts)
}
fn episodes(db: &Connection, team: &str) -> Result<Vec<Episode>> {
    let rows = db.prepare("SELECT seq,kind,text FROM inbox WHERE team=?1 AND node=0 AND reply IS NULL AND (kind LIKE 'lead_admission:%' OR kind LIKE 'lead_routing:%') ORDER BY seq LIMIT 3")
        .and_then(|mut s| s.query_map([team], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(storage)?;
    let mut seen = [false; 2];
    let mut result = Vec::new();
    for (seq, kind, text) in rows {
        let category = usize::from(kind.starts_with(PREFIXES[1]));
        if seen[category] {
            return Err(Problem::new(
                code::OVER_CAP,
                "More than one waiting-queue episode per category.",
            ));
        }
        seen[category] = true;
        result.push(Episode {
            seq,
            category,
            text,
        });
    }
    Ok(result)
}
fn text(category: usize, count: u32) -> String {
    let action = if category == 0 {
        "your admission"
    } else {
        "your routing"
    };
    let waiting = if count == 1 {
        "1 task waits".to_owned()
    } else {
        format!("{count} tasks wait")
    };
    format!("{waiting} for {action}. Read the queue with: hgs team queue")
}
fn changed(db: &Connection, team: &Team) -> Result<bool> {
    let counts = counts(db, team)?;
    let episodes = episodes(db, &team.id)?;
    Ok((0..2).any(
        |category| match episodes.iter().find(|e| e.category == category) {
            Some(e) => counts[category] == 0 || e.text != text(category, counts[category]),
            None => counts[category] != 0,
        },
    ))
}
impl Store {
    /// Bounded comparison only; the operation recomputes under its writer lock.
    pub fn lead_queue_changed(&self, team: &Team) -> Result<bool> {
        self.active()?;
        let db = self.db.as_ref().ok_or_else(missing)?;
        let tx = db.unchecked_transaction().map_err(storage)?;
        let result = changed(&tx, team)?;
        tx.commit().map_err(storage)?;
        Ok(result)
    }
}
pub(super) fn reconcile(ctx: &mut Context<'_>) -> Result<()> {
    if ctx.team.status != TeamStatus::Running {
        return Err(invalid("Waiting-queue notices require a running team."));
    }
    if !changed(ctx.db, ctx.team)? {
        return Err(Problem::new(UNCHANGED, "No waiting-queue episode changed."));
    }
    let counts = counts(ctx.db, ctx.team)?;
    let episodes = episodes(ctx.db, &ctx.team.id)?;
    for (category, count) in counts.into_iter().enumerate() {
        match episodes.iter().find(|e| e.category == category) {
            Some(e) if count == 0 => {
                ctx.db.execute("UPDATE inbox SET state='closed',reply=?3,reply_actor=?4 WHERE team=?1 AND seq=?2 AND reply IS NULL", params![ctx.team.id,e.seq,ENDED,encode(ctx.actor)?]).map_err(storage)?;
            }
            Some(e) => {
                // Keep the item and its doorbell mark when only the count changes.
                ctx.db
                    .execute(
                        "UPDATE inbox SET text=?3 WHERE team=?1 AND seq=?2",
                        params![ctx.team.id, e.seq, text(category, count)],
                    )
                    .map_err(storage)?;
            }
            None if count > 0 => {
                let kind = format!(
                    "{}{}",
                    PREFIXES[category],
                    ctx.team
                        .revision
                        .checked_add(1)
                        .ok_or_else(|| over("Revision is exhausted."))?
                );
                ctx.db.execute("INSERT INTO inbox(team,kind,node,generation,actor,text,notes,created_ms) VALUES(?1,?2,0,0,?3,?4,'[]',?5)", params![ctx.team.id,kind,encode(ctx.actor)?,text(category,count),ctx.now]).map_err(storage)?;
            }
            None => {}
        }
    }
    Ok(())
}
