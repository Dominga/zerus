//! Immutable observed generations. No production listing/watch call site yet.
use super::*;
use serde::{Deserialize, Serialize};
const ROWS: usize = 128;
const BYTES: usize = 512 << 10;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Cursor {
    epoch: String,
    generation: i64,
    high_water: i64,
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
        let (dirs, gap, high_water): (String, String, i64) = self.db.query_row(
            "SELECT dirs,gap,high_water FROM generations WHERE generation=?1 AND complete=1",
            [generation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if let Some(c) = cursor {
            anyhow::ensure!(c.high_water == high_water, "Display snapshot cut changed");
        }
        let mut fresh = ready
            && generation == active
            && repair::directories(self)? == dirs
            && repair::gap_stamp(self)? == gap;
        let after = cursor.map_or("", |c| c.after.as_str());
        let mut statement=self.db.prepare_cached("SELECT source,CASE WHEN length(CAST(seed AS BLOB))<=262144 THEN seed ELSE NULL END,substr(stamp,1,1025) FROM snapshots WHERE generation=?1 AND source>?2 ORDER BY source LIMIT 128")?;
        let mut query = statement.query(params![generation, after])?;
        let mut rows = Vec::new();
        let mut bytes = 0;
        let until = Instant::now() + Duration::from_millis(250);
        while let Some(row) = query.next()? {
            anyhow::ensure!(
                Instant::now() < until,
                "Display page verification timed out"
            );
            let source: String = row.get(0)?;
            let seed: String = row.get(1)?;
            let stamp: String = row.get(2)?;
            anyhow::ensure!(
                source.len() <= 256 && seed.len() <= model::SEED_BYTES && stamp.len() <= 1024,
                "Invalid display page row"
            );
            if bytes + source.len() + seed.len() + stamp.len() > BYTES {
                break;
            }
            bytes += source.len() + seed.len() + stamp.len();
            let key = Key(source.clone());
            let mut value = Seed::decode(&key, &seed)?;
            value["native_stamp"] = json!(stamp);
            fresh &= self.source_stamp(&key).ok().flatten().as_deref() == Some(stamp.as_str());
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
            high_water,
            after: last.into(),
        });
        self.verify()?;
        Ok(Page { rows, next, fresh })
    }
}

/// The public reader surface intentionally exposes no reservation, settlement,
/// recovery or canonical effect method.
pub(super) struct Reader(Store);
impl Reader {
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        Ok(Self(Store::read(root)?))
    }
    pub fn page(&self, cursor: Option<&Cursor>) -> anyhow::Result<Page> {
        let mut page = self.0.page(cursor)?;
        // The retained RO transaction pins generation data, not current readiness.
        // Read current metadata separately without taking the publisher gate.
        let check = || -> anyhow::Result<bool> {
            let pinned: (String, i64, i64) = self.0.db.query_row(
                "SELECT epoch,generation,writes FROM meta WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let current = Store::read(&self.0.root.path)?;
            let (epoch, generation, writes, ready): (String, i64, i64, bool) =
                current.db.query_row(
                    "SELECT epoch,generation,writes,ready FROM meta WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
            Ok(ready && pinned == (epoch, generation, writes))
        };
        page.fresh &= check().unwrap_or(false);
        Ok(page)
    }
}
