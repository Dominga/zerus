//! Bounded background hints only. A display seed never reaches an effect.
use super::*;

#[derive(Default)]
pub(super) struct Reconcile {
    after: String,
}
impl Reconcile {
    pub fn step(&mut self, root: &Path) -> anyhow::Result<()> {
        // Drop the RO SQLite transaction BEFORE taking the canonical writer.
        let candidates = {
            let store = Store::read(root)?;
            let ready: bool =
                store
                    .db
                    .query_row("SELECT ready FROM meta WHERE id=1", [], |row| row.get(0))?;
            if !ready {
                return Ok(());
            }
            let mut statement=store.db.prepare_cached("SELECT substr(source,1,257),substr(stamp,1,1025),CASE WHEN length(CAST(seed AS BLOB))<=262144 THEN seed ELSE NULL END FROM sources INDEXED BY sources_tombstone WHERE source>?1 AND tombstone=0 AND issue IS NULL ORDER BY source LIMIT 8")?;
            let candidates = statement
                .query_map([&self.after], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            candidates
        };
        if candidates.is_empty() {
            self.after.clear();
            return Ok(());
        }
        let until = Instant::now() + Duration::from_millis(250);
        let scope = storage::listing_scope::Scope::enter(until);
        let mut bytes = 0;
        let mut files = 0;
        for (source, stamp, seed) in candidates {
            if Instant::now() >= until {
                break;
            }
            self.after = source.clone();
            let key = Key(source.clone());
            key.validate()?;
            anyhow::ensure!(
                stamp.len() <= 1024 && seed.len() <= model::SEED_BYTES,
                "Invalid reconciliation hint"
            );
            let seed = Seed::decode(&key, &seed)?;
            if source.starts_with("dsh/")
                || source.starts_with("opencode/")
                || seed["record"]["paused"] == true
            {
                continue;
            }
            // Only likely completed bindings are scheduled; canonical guards
            // recheck every fact under the original state writer protocol.
            let hint = &seed["record"];
            if hint["phase"] != "ended" && hint["completion_reason"] != "clean_exit" {
                continue;
            }
            let Some(_guard) = checked(storage::writer_until(until))? else {
                break;
            };
            let record = listing::selected_record(
                root,
                &source,
                &json!({"native_stamp":stamp}),
                &mut bytes,
                &mut files,
                8,
                until,
            )?;
            let _route = storage::listing_scope::Route::verified(
                root.join(&source),
                string(&record, "name"),
            );
            let snapshot = checked(storage::live())?;
            // Fresh canonical input, not the advisory hint or its recipe.
            checked(archive::reconcile_selected_locked(&snapshot, record))?;
            scope.verify().map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }
}
