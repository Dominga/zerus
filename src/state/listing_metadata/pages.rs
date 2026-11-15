//! Immutable observed generations. No production listing/watch call site yet.
use super::*;
use serde::{Deserialize, Serialize};
const ROWS: usize = 128;
const BYTES: usize = 512 << 10;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    epoch: String,
    generation: i64,
    after: String,
}
pub(super) struct Page {
    pub rows: Vec<(String, Value)>,
    pub next: Option<Cursor>,
    pub fresh: bool,
}
impl Store {
    pub fn page(&self, cursor: Option<&Cursor>) -> anyhow::Result<Page> {
        self.verify()?;
        let (epoch, active, ready): (String, i64, bool) = self.db.query_row(
            "SELECT epoch,generation,ready FROM meta WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let generation = cursor.map_or(active, |c| c.generation);
        anyhow::ensure!(generation > 0, "Listing metadata is still rebuilding");
        if let Some(c) = cursor {
            anyhow::ensure!(
                c.epoch == epoch && c.after.len() <= 256,
                "Listing metadata cursor is stale"
            );
        }
        let (dirs, gap): (String, String) = self.db.query_row(
            "SELECT dirs,gap FROM generations WHERE generation=?1 AND complete=1",
            [generation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let fresh = ready
            && generation == active
            && repair::directories(self)? == dirs
            && repair::gap_stamp(self)? == gap;
        let after = cursor.map_or("", |c| c.after.as_str());
        let mut statement=self.db.prepare_cached("SELECT source,CASE WHEN length(CAST(seed AS BLOB))<=262144 THEN seed ELSE NULL END FROM snapshots WHERE generation=?1 AND source>?2 ORDER BY source LIMIT 128")?;
        let mut query = statement.query(params![generation, after])?;
        let mut rows = Vec::new();
        let mut bytes = 0;
        while let Some(row) = query.next()? {
            let source: String = row.get(0)?;
            let seed: String = row.get(1)?;
            anyhow::ensure!(
                source.len() <= 256 && seed.len() <= model::SEED_BYTES,
                "Invalid display page row"
            );
            if bytes + source.len() + seed.len() > BYTES {
                break;
            }
            bytes += source.len() + seed.len();
            let value = Seed::decode(&Key(source.clone()), &seed)?;
            rows.push((source, value));
        }
        anyhow::ensure!(
            rows.len() <= ROWS && bytes <= BYTES,
            "Display page exceeds its bound"
        );
        let last = rows.last().map(|r| r.0.as_str()).unwrap_or(after);
        let more: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM snapshots WHERE generation=?1 AND source>?2)",
            params![generation, last],
            |r| r.get(0),
        )?;
        anyhow::ensure!(!more || !rows.is_empty(), "Display page cannot advance");
        let next = more.then(|| Cursor {
            epoch,
            generation,
            after: last.into(),
        });
        self.verify()?;
        Ok(Page { rows, next, fresh })
    }
}
