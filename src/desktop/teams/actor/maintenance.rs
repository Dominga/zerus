use super::*;
impl Runtime {
    /// A standing team does not retain a retry row for each completed task.
    /// The key cursor makes every row reachable even while earlier waits stay.
    pub(super) fn cleanup_metadata(
        &mut self,
        store: &mut Store,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let after = self
            .load::<String>(&team.id, "metadata_cursor")?
            .unwrap_or_default();
        let rows = self.ledger.prefix_page(&team.id, "gate:", &after)?;
        let next = if rows.len() == 200 {
            rows.last().expect("full page").key.clone()
        } else {
            String::new()
        };
        let merging = self.ledger.merge_pending(&team.id)?;
        let (reopens, complete) = self.metadata_reopens(store, team, &rows, now)?;
        for row in rows {
            // Without the whole pending set the rows that read it stay.
            if complete || !reads_reopens(&row.key) {
                self.cleanup_gate(store, team, &row, &reopens, merging, now)?;
            }
        }
        self.put(&team.id, "metadata_cursor", &next)?;
        Ok(())
    }
    /// The pending reopens that the page's rows read, and whether the read
    /// is whole. Its page bound is a row failure of this cleanup, with the
    /// usual backoff: the rows that read the set stay, the pass goes on.
    fn metadata_reopens(
        &mut self,
        store: &mut Store,
        team: &Team,
        rows: &[Row],
        now: i64,
    ) -> Result<(std::collections::BTreeSet<u64>, bool)> {
        let mut pending = std::collections::BTreeSet::new();
        if !rows.iter().any(|r| reads_reopens(&r.key)) {
            return Ok((pending, true));
        }
        if !self.due(store, team, REOPEN_BOUND, "", now)? {
            return Ok((pending, false));
        }
        let mut cursor = None;
        for _ in 0..4 {
            let page = store.pending_reopens(&team.id, 200, cursor.as_ref())?;
            pending.extend(page.rows.into_iter().map(|d| d.seq));
            cursor = page.next;
            if cursor.is_none() {
                self.recovered(store, team, REOPEN_BOUND, now)?;
                return Ok((pending, true));
            }
        }
        let error = problem("Reopen cleanup page bound exceeded");
        self.failure(store, team, REOPEN_BOUND, "", &error, now)?;
        Ok((pending, false))
    }
    fn cleanup_gate(
        &mut self,
        store: &mut Store,
        team: &Team,
        row: &Row,
        reopens: &std::collections::BTreeSet<u64>,
        merging: bool,
        now: i64,
    ) -> Result<()> {
        let Some(key) = row.key.strip_prefix("gate:") else {
            return Ok(());
        };
        if let Some(decision) = key
            .strip_prefix("reopen:")
            .and_then(|s| s.parse::<u64>().ok())
        {
            if !reopens.contains(&decision) {
                self.recovered(store, team, key, now)?;
            }
            return Ok(());
        }
        let held = key.strip_prefix("held:");
        let Some((kind, value)) = held.unwrap_or(key).split_once(':') else {
            return Ok(());
        };
        if if held.is_some() {
            !matches!(kind, "finish" | "merge" | "reopen")
        } else {
            !matches!(
                kind,
                "observe" | "report" | "handout" | "finish" | "finish_refused" | "integration"
            )
        } {
            return Ok(());
        }
        if held.is_none() && kind == "integration" && merging {
            return Ok(());
        }
        let Ok(node) = value.parse::<u32>() else {
            return Ok(());
        };
        if held.is_some() && self.held_retired(store, team, row, kind, node, reopens)? {
            self.recovered(store, team, key, now)?;
            return Ok(());
        }
        if !store.node_live(&team.id, node)?
            || store
                .node(&team.id, node)?
                .is_none_or(|n| matches!(n.state, NodeState::Accepted | NodeState::Cancelled))
        {
            self.recovered(store, team, key, now)?;
        }
        Ok(())
    }
}

const REOPEN_BOUND: &str = "cleanup:reopens";
fn reads_reopens(key: &str) -> bool {
    key.starts_with("gate:reopen:") || key.starts_with("gate:held:reopen:")
}

impl Runtime {
    pub(super) fn prune_terminal_rows(&mut self, store: &Store) -> Result<bool> {
        let (id, after) = self
            .maintenance_team
            .clone()
            .expect("selected terminal team");
        let team = store.team(&id)?;
        let Some(team) = team.filter(|t| {
            matches!(
                t.status,
                TeamStatus::Stopped | TeamStatus::Archived | TeamStatus::Finished
            )
        }) else {
            self.maintenance_team = None;
            return Ok(true);
        };
        if team.status != TeamStatus::Finished
            && (!store.pending_cancels(&id, 1, 0)?.is_empty()
                || !store.keyed_tasks(&id, 0, 1)?.is_empty()
                || self
                    .open_effects(store, &id)?
                    .iter()
                    .any(|e| e.outcome.is_none()))
        {
            // Keep work frontiers while terminal work is pending. Pruning
            // their cursors here could strand rows behind a blocked page.
            self.maintenance_team = None;
            return Ok(true);
        }
        let rows = self.ledger.page(&id, &after)?;
        for row in &rows {
            let evidence = ["delivery:", "merge:"]
                .iter()
                .any(|prefix| row.key.starts_with(prefix));
            if team.status == TeamStatus::Finished || !evidence {
                self.ledger.remove(row)?;
            }
        }
        if rows.len() == 200 {
            self.maintenance_team = Some((id, rows.last().unwrap().key.clone()));
            Ok(false)
        } else {
            // The team's rows are reclaimed: its service items go with them.
            self.ledger.clear_team_items(&id)?;
            self.maintenance_team = None;
            Ok(true)
        }
    }
}

/// Service items checked in one pass, against the rows they mirror.
const ITEM_PAGE: u32 = 64;
impl Runtime {
    /// One page of the service items, in key order: an item whose mirrored
    /// row is gone is removed, so a writer that missed its clear leaves no
    /// item that reads "as of" for ever. The cursor wraps; every item is
    /// checked within `rows / 64` passes.
    pub(super) fn prune_items(&mut self, store: &Store) -> Result<()> {
        let keys = self.ledger.item_page(&self.items_after, ITEM_PAGE)?;
        self.items_after = if keys.len() == ITEM_PAGE as usize {
            keys.last().expect("full page").clone()
        } else {
            Default::default()
        };
        for key in keys {
            if !self.item_mirrored(store, &key)? {
                self.ledger.clear_item(&key.team, &key.kind, &key.subject)?;
            }
        }
        Ok(())
    }
    /// Whether the row an item mirrors is still there: the cancel row of its
    /// decision, the terminal page's retry record, a key of its team that
    /// still waits for a final record, or, for the full table's marker, a
    /// table without room. An unknown kind is kept.
    fn item_mirrored(&self, store: &Store, key: &items::Key) -> Result<bool> {
        Ok(match key.kind.as_str() {
            CANCEL_RETRY | CANCEL_HALTED => self
                .ledger
                .has_row(&key.team, &format!("cancel:{}", key.subject))?,
            TERMINAL_PAGE => self.ledger.terminal_retry()?.is_some(),
            KEY_CLEAR => !store.keyed_tasks(&key.team, 0, 1)?.is_empty(),
            items::UNLISTED => !self.ledger.item_room()?,
            _ => true,
        })
    }
}
use super::super::items::{
    self,
    kind::{CANCEL_HALTED, CANCEL_RETRY, KEY_CLEAR, TERMINAL_PAGE},
};
