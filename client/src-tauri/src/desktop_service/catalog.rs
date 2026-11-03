//! One serial baseline/delta walker. Only its initial membership set retains keys.
use super::*;

pub(super) async fn catalog_loop(
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    configuration: String,
) -> Result<(), &'static str> {
    let mut failures = BTreeMap::<String, (u64, u8)>::new();
    let mut last = None::<String>;
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let work = {
            let mut dirty = shared.dirty.lock().unwrap_or_else(PoisonError::into_inner);
            failures.retain(|computer, _| dirty.sources.contains_key(computer));
            // One walker, with round-robin admission so a failed baseline's
            // single retry cannot repeatedly jump ahead of another computer.
            let next = last
                .as_ref()
                .and_then(|last| {
                    dirty
                        .pending
                        .range::<String, _>((
                            std::ops::Bound::Excluded(last),
                            std::ops::Bound::Unbounded,
                        ))
                        .next()
                        .cloned()
                })
                .or_else(|| dirty.pending.first().cloned());
            next.and_then(|computer| {
                dirty.pending.remove(&computer);
                dirty.running = Some(computer.clone());
                dirty.sources.get(&computer).cloned().map(|source| {
                    let since = dirty.progress.get(&computer).copied();
                    (computer, source, since)
                })
            })
        };
        if let Some((computer, source, since)) = work {
            last = Some(computer.clone());
            if failures
                .get(&computer)
                .is_some_and(|(serial, _)| *serial != source.serial)
            {
                failures.remove(&computer);
            }
            let epoch = shared
                .snapshot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .epoch
                .clone()
                .ok_or("service_unavailable")?;
            if shared.park_until_source_identity(&computer, &source) {
                shared
                    .dirty
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .running = None;
                row(
                    &shared,
                    &computer,
                    &source,
                    &epoch,
                    &stop,
                    Change::Computer {
                        computer: ComputerId::from(computer.as_str()),
                        status: ComputerStatus::Reconnecting { attempt: 0 },
                    },
                )
                .await?;
                continue;
            }
            let result = walk(
                &shared,
                &computer,
                source.clone(),
                &epoch,
                since,
                stop.clone(),
                &configuration,
            )
            .await;
            shared
                .dirty
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .running = None;
            match result {
                Ok(()) => {
                    failures.remove(&computer);
                }
                Err(_)
                    if shared
                        .dirty
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .sources
                        .get(&computer)
                        .is_some_and(|current| current.serial == source.serial)
                        && !stop.load(Ordering::Acquire) =>
                {
                    // A one-use cursor's unknown outcome is never replayed.
                    // Try one fresh baseline. Persistent failure fences only
                    // this computer; its rows remain last known until Retry
                    // or an actual source scope change, never a global EOF.
                    let count = failures.get(&computer).map_or(1, |(_, count)| count + 1);
                    let current = {
                        let mut dirty = shared.dirty.lock().unwrap_or_else(PoisonError::into_inner);
                        // Retry/removal/hint can race the completed RPC.
                        if !dirty
                            .sources
                            .get(&computer)
                            .is_some_and(|current| current.serial == source.serial)
                        {
                            continue;
                        }
                        let current = dirty.restart(&computer)?.ok_or("catalog_invalidated")?;
                        if count > 1 {
                            dirty.pending.remove(&computer);
                            dirty.halted.insert(computer.clone());
                        }
                        current
                    };
                    failures.insert(computer.clone(), (current.serial, count));
                    row(
                        &shared,
                        &computer,
                        &current,
                        &epoch,
                        &stop,
                        Change::Computer {
                            computer: ComputerId::from(computer.as_str()),
                            status: ComputerStatus::Reconnecting {
                                attempt: count.into(),
                            },
                        },
                    )
                    .await?;
                }
                Err(_) => (),
            }
        } else {
            shared.changed.notified().await;
        }
    }
}

fn insert_key(
    keys: &mut BTreeSet<String>,
    bytes: &mut usize,
    key: String,
) -> Result<(), &'static str> {
    if keys.contains(&key) {
        return Ok(());
    }
    if *bytes + key.len() > KEY_BYTES || keys.len() >= CATALOG_KEYS {
        return Err("catalog_capacity");
    }
    *bytes += key.len();
    keys.insert(key);
    Ok(())
}

impl Shared {
    /// Park only this local baseline; hold neither a relay nor an RPC slot.
    /// Hello application takes the same hub/dirty lock order and wakes it.
    fn park_until_source_identity(&self, computer: &str, source: &SourceVersion) -> bool {
        if !computer.is_empty()
            || !self
                .snapshot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .details_available
        {
            return false;
        }
        let hub = self.details.lock().unwrap_or_else(PoisonError::into_inner);
        if hub.local_identity_ready(source.generation) {
            return false;
        }
        let mut dirty = self.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        if dirty
            .sources
            .get(computer)
            .is_some_and(|current| current.serial == source.serial)
        {
            dirty.pending.remove(computer);
            dirty.halted.insert(computer.into());
        }
        true
    }
    pub(super) fn catalog_identity_ready(&self, computer: &str, generation: u64) {
        let mut dirty = self.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        if dirty
            .sources
            .get(computer)
            .is_some_and(|source| source.generation == generation)
            && !dirty.progress.contains_key(computer)
        {
            dirty.halted.remove(computer);
            dirty.pending.insert(computer.into());
            self.changed.notify_one();
        }
    }
    fn page_source(
        &self,
        computer: &str,
        old: &SourceVersion,
        page: &CatalogPage,
    ) -> Option<SourceVersion> {
        let mut dirty = self.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        let current = dirty.sources.get(computer)?;
        if !old.matches(current) {
            return None;
        }
        if current.identity() != old.identity() && page.identity != current.identity() {
            // A hint bound the previously unknown projection while this first
            // page was in flight. Its older response must not replace that hint.
            return None;
        }
        if old.incarnation != page.identity.incarnation
            || old.generation != page.identity.generation
            || current
                .projection
                .as_ref()
                .is_some_and(|p| Some(p) != page.identity.projection.as_ref())
            || old.online != page.online
        {
            drop(dirty);
            let source = SourceVersion {
                incarnation: page.identity.incarnation,
                generation: page.identity.generation,
                projection: page.identity.projection.clone(),
                online: page.online,
                ..SourceVersion::default()
            };
            let _ = self.catalog_hint(computer, source, page.revision, true);
            return None;
        }
        // Initial status has source identities but no projection. Bind the
        // FIRST page before emitting any provisional row, under the hint lock.
        let mut source = current.clone();
        source.projection = page.identity.projection.clone();
        dirty.sources.insert(computer.into(), source.clone());
        if let Some(head) = page.revision {
            dirty
                .heads
                .entry(computer.into())
                .and_modify(|old| *old = (*old).max(head))
                .or_insert(head);
        }
        Some(source)
    }
    fn catalog_head(&self, computer: &str, source: &SourceVersion, head: Revision) {
        let mut dirty = self.dirty.lock().unwrap_or_else(PoisonError::into_inner);
        if dirty.sources.get(computer) == Some(source) {
            dirty
                .heads
                .entry(computer.into())
                .and_modify(|old| *old = (*old).max(head))
                .or_insert(head);
        }
    }
}

async fn deliver(
    shared: &Arc<Shared>,
    computer: &str,
    source: &SourceVersion,
    epoch: &str,
    stop: &Arc<AtomicBool>,
    value: CatalogValue,
) -> Result<(), &'static str> {
    emit_data(
        shared,
        Data::Catalog(CatalogDelivery {
            shared: Arc::downgrade(shared),
            computer: computer.into(),
            epoch: epoch.into(),
            source: source.clone(),
            stopped: stop.clone(),
            value,
        }),
        stop,
    )
    .await
}
async fn row(
    shared: &Arc<Shared>,
    computer: &str,
    source: &SourceVersion,
    epoch: &str,
    stop: &Arc<AtomicBool>,
    change: Change,
) -> Result<(), &'static str> {
    deliver(
        shared,
        computer,
        source,
        epoch,
        stop,
        CatalogValue::Row(Box::new(change)),
    )
    .await
}

/// One journal revision: the changes of one native watch delta frame (the
/// journal records one revision per frame), held until the revision ends,
/// even across change pages, so it is bounded by that frame. Its upserts go
/// first, so a view batch boundary never leaves selection on a removed run.
/// A service that journals the native removal reason supplies it, including
/// for a replacement paged into another frame (zerus-ic13). An older service
/// omits it; each such removal then learns which names this frame lists
/// again: the native watch's `replaced` (zerus-uutz). The walk's key
/// bookkeeping keeps the journal's order.
#[derive(Default)]
struct Frame {
    upserts: Vec<Change>,
    removals: Vec<(String, Option<String>)>,
    names: BTreeSet<String>,
}
/// The removal reasons `Change::Remove` keeps; another one is no reason.
const REASONS: [&str; 4] = [
    zerus_watch_protocol::reason::REPLACED,
    zerus_watch_protocol::reason::ARCHIVED,
    zerus_watch_protocol::reason::FORGOTTEN,
    zerus_watch_protocol::reason::ENDED,
];
impl Frame {
    async fn deliver(
        &mut self,
        shared: &Arc<Shared>,
        computer: &str,
        source: &SourceVersion,
        epoch: &str,
        stop: &Arc<AtomicBool>,
    ) -> Result<(), &'static str> {
        let listed = Arc::new(std::mem::take(&mut self.names));
        for change in std::mem::take(&mut self.upserts) {
            row(shared, computer, source, epoch, stop, change).await?;
        }
        for (key, reason) in std::mem::take(&mut self.removals) {
            let value = match reason {
                Some(reason) => CatalogValue::Row(Box::new(Change::Remove {
                    computer: ComputerId::from(computer),
                    key,
                    reason: Some(reason),
                })),
                None => CatalogValue::Removal {
                    key,
                    listed: listed.clone(),
                },
            };
            deliver(shared, computer, source, epoch, stop, value).await?;
        }
        Ok(())
    }
}

fn project(
    shared: &Shared,
    computer: &str,
    source: &SourceVersion,
    value: &Value,
) -> zerus_client_core::ViewRow {
    shared
        .details
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .project_row(&ComputerId::from(computer), source.generation, value)
}

async fn walk(
    shared: &Arc<Shared>,
    computer: &str,
    mut source: SourceVersion,
    epoch: &str,
    since: Option<Revision>,
    stop: Arc<AtomicBool>,
    configuration: &str,
) -> Result<(), &'static str> {
    let mut cursor = None::<String>;
    let mut relay = catalog_relay::Relay::default();
    let result = async {
        if since.is_none() {
            row(shared, computer, &source, epoch, &stop, Change::Computer { computer: ComputerId::from(computer), status: ComputerStatus::Reconnecting { attempt: 0 } }).await?;
        }
        let mut keys = since.is_none().then(BTreeSet::new);
        let mut key_bytes = 0;
        let mut start = since;
        if since.is_none() {
            let mut first = true;
            loop {
                if stop.load(Ordering::Acquire) || !shared.current(computer, &source, epoch) { return Ok(()); }
                let id = uuid::Uuid::new_v4().to_string();
                let value = relay.request(shared, configuration, json!({"op":"catalog_page","request_id":id,"computer":computer,"cursor":cursor}), &stop).await?;
                let page = CatalogPage::checked(value, &id, epoch, computer)?;
                cursor = page.next_cursor.clone();
                if !shared.current(computer, &source, epoch) { return Ok(()); }
                if first {
                    let Some(bound) = shared.page_source(computer, &source, &page) else { return Ok(()); };
                    source = bound; start = page.start_revision; first = false;
                } else if page.identity != source.identity() || page.online != source.online || page.start_revision != start {
                    return Err("catalog_scope_changed");
                }
                if let Some(head) = page.revision { shared.catalog_head(computer, &source, head); }
                for item in page.items {
                    let keys = keys.as_mut().expect("baseline owns membership");
                    if keys.contains(&item.key) { return Err("catalog_duplicate"); }
                    insert_key(keys, &mut key_bytes, item.key.clone())?;
                    row(shared, computer, &source, epoch, &stop, Change::Upsert { computer: ComputerId::from(computer),
                        key: item.key, row: project(shared, computer, &source, &item.row) }).await?;
                }
                if cursor.is_none() { break; }
            }
        }
        let through = if let Some(start) = start {
            let mut through = None;
            let mut previous: Option<service::ChangeItem> = None;
            let mut frame = Frame::default();
            loop {
                if stop.load(Ordering::Acquire) || !shared.current(computer, &source, epoch) { return Ok(()); }
                let id = uuid::Uuid::new_v4().to_string();
                let value = relay.request(shared, configuration, json!({"op":"catalog_changes","request_id":id,"epoch":epoch,"computer":computer,
                    "incarnation":source.incarnation,"generation":source.generation,"projection":source.projection,
                    "since_revision":start,"cursor":cursor}), &stop).await?;
                let page = ChangePage::checked(value, &id, epoch, computer)?;
                cursor = page.next_cursor.clone();
                if !shared.current(computer, &source, epoch) { return Ok(()); }
                if page.identity != source.identity() || page.since_revision != start || through.is_some_and(|high| high != page.through_revision) {
                    return Err("catalog_scope_changed");
                }
                through = Some(page.through_revision);
                shared.catalog_head(computer, &source, page.revision);
                for item in page.items {
                    if let Some(old) = &previous {
                        let order = (item.revision, &item.key).cmp(&(old.revision, &old.key));
                        if order.is_lt() || (order.is_eq() && old != &item) { return Err("catalog_changes_order"); }
                        if order.is_eq() { continue; }
                        if old.revision != item.revision { frame.deliver(shared, computer, &source, epoch, &stop).await?; }
                    }
                    if let Some(value) = &item.row {
                        if let Some(keys) = keys.as_mut() { insert_key(keys, &mut key_bytes, item.key.clone())?; }
                        let row = project(shared, computer, &source, value);
                        frame.names.insert(row.name.clone());
                        frame.upserts.push(Change::Upsert { computer: ComputerId::from(computer), key: item.key.clone(), row });
                    } else {
                        if let Some(keys) = keys.as_mut() { if keys.remove(&item.key) { key_bytes -= item.key.len(); } }
                        let reason = item.reason.clone().filter(|r| REASONS.contains(&r.as_str()));
                        frame.removals.push((item.key.clone(), reason));
                    }
                    previous = Some(item);
                }
                if cursor.is_none() { break; }
            }
            frame.deliver(shared, computer, &source, epoch, &stop).await?;
            through
        } else { None };
        relay.close().await?;
        let (done, joined) = oneshot::channel();
        deliver(shared, computer, &source, epoch, &stop, CatalogValue::Finish { through, keys, done }).await?;
        tokio::time::timeout(RPC_WAIT, joined).await.map_err(|_| "catalog_view_timeout")?.map_err(|_| "catalog_invalidated")?;
        Ok(())
    }.await;
    let drained = relay.close().await;
    if let Some(cursor) = cursor {
        let _ = request(shared, json!({"op":"catalog_close","request_id":uuid::Uuid::new_v4().to_string(),"computer":computer,"cursor":cursor}), Arc::new(AtomicBool::new(false))).await;
    }
    result.and(drained)
}
