//! Visible row keys are display interests, never renderer-supplied paths.
//! Resolve them only against the original current service catalog/Hello.
use super::*;
use zerus_client_core::git::{self, RowInterest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    row: String,
    serial: u64,
    source: (String, u64, u64),
}

impl Hub {
    pub(super) fn git_current(&self, key: &Key, pump: &crate::pump::Pump) -> bool {
        let Some(source) = self.sources.get(&key.computer).filter(|s| s.online) else {
            return false;
        };
        self.git_bindings.get(key).is_some_and(|bindings| {
            bindings.iter().any(|b| {
                b.serial == source.serial
                    && b.source.2 == source.generation
                    && self
                        .lease
                        .as_ref()
                        .is_some_and(|lease| lease.epoch == b.source.0)
                    && pump.git_scope(&key.computer, &b.row).is_some_and(|row| {
                        row.source.as_ref() == Some(&b.source) && row.path == key.key
                    })
            })
        })
    }
}

impl Service {
    pub fn git_rows(&self, rows: Vec<RowInterest>) -> Result<(), String> {
        if rows.len() > git::VISIBLE_FOLDERS || rows.iter().any(|r| !r.valid()) {
            return Err("Invalid visible Git rows.".into());
        }
        self.shared
            .details
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .git_rows = rows;
        reconcile(&self.shared);
        self.shared.details_changed.notify_one();
        Ok(())
    }
}

pub(super) fn reconcile(shared: &Arc<Shared>) {
    let wanted = {
        let hub = shared
            .details
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if hub.git_rows.is_empty() && hub.git_bindings.is_empty() {
            return;
        }
        hub.git_rows.clone()
    };
    // Never nest the source-admission locks under the detail lock. Catalog
    // provenance on each native mirror row excludes older baseline rows.
    let scopes = {
        let snapshot = shared.snapshot.lock();
        let dirty = admission(&shared.dirty);
        wanted
            .iter()
            .filter_map(|row| {
                // A cached/staging baseline is not current checkout evidence.
                if !dirty.progress.contains_key(&row.computer) {
                    return None;
                }
                let source = dirty.sources.get(&row.computer).filter(|s| s.online)?;
                Some((
                    row.computer.clone(),
                    (
                        snapshot.epoch.clone()?,
                        source.incarnation,
                        source.generation,
                    ),
                ))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let mut hub = shared
        .details
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if wanted != hub.git_rows {
        return;
    }
    let mut desired = BTreeMap::<Key, Vec<Binding>>::new();
    let mut per_computer = BTreeMap::<String, usize>::new();
    for interest in &wanted {
        let Some(scope) = scopes.get(&interest.computer) else {
            continue;
        };
        let Some(source) = hub
            .sources
            .get(&interest.computer)
            .filter(|s| s.online && s.generation == scope.2)
        else {
            continue;
        };
        let cap = source
            .state
            .hello()
            .filter(|hello| hello.topics.iter().any(|t| t == topic::GIT))
            .map_or(0, |hello| hello.limits.git_subscriptions.min(git::FOLDERS));
        let Some(row) = shared
            .pump
            .git_scope(&interest.computer, &interest.key)
            .filter(|row| row.source.as_ref() == Some(scope))
        else {
            continue;
        };
        let path = row.path;
        let key = Key {
            computer: interest.computer.clone(),
            topic: topic::GIT.into(),
            key: path,
        };
        if !desired.contains_key(&key) {
            let count = per_computer.entry(key.computer.clone()).or_default();
            if *count >= cap {
                continue;
            }
            *count += 1;
        }
        desired.entry(key).or_default().push(Binding {
            row: interest.key.clone(),
            serial: source.serial,
            source: scope.clone(),
        });
    }
    let removals = hub
        .routes
        .keys()
        .filter(|key| key.topic == topic::GIT && !desired.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    for key in removals {
        hub.unfollow(&key);
        shared.pump.push(Change::Git {
            computer: ComputerId(key.computer),
            key: key.key,
            update: zerus_client_core::GitUpdate::Unavailable {
                code: "git_interest_retired".into(),
            },
        });
    }
    let replaced = desired
        .iter()
        .filter(|(key, bindings)| {
            hub.git_bindings.get(*key).is_some_and(|old| {
                old.first()
                    .zip(bindings.first())
                    .is_some_and(|(a, b)| a.serial != b.serial)
            })
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    for key in replaced {
        hub.unfollow(&key);
    }
    hub.git_bindings = desired.clone();
    for key in desired.keys() {
        // A terminal refusal stays halted until the row leaves the interest
        // set or the original source is replaced. No automatic busy retry.
        if hub.routes.contains_key(key) {
            continue;
        }
        if hub.capacity() >= protocol::DEMANDS {
            break;
        }
        let _ = hub.follow(
            &key.computer,
            Subscription {
                topic: topic::GIT.into(),
                path: Some(key.key.clone()),
                ..Default::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::ViewRow;
    fn lease() -> Lease {
        Lease {
            abi: protocol::ABI.into(),
            epoch: uuid::Uuid::new_v4().to_string(),
            lease: uuid::Uuid::new_v4().to_string(),
        }
    }
    #[test]
    fn git_admission_needs_the_original_native_row_source_and_path() {
        let mut hub = Hub {
            lease: Some(lease()),
            ..Default::default()
        };
        hub.source("peer", 7).unwrap();
        let source = (hub.lease.as_ref().unwrap().epoch.clone(), 2, 7);
        let key = Key {
            computer: "peer".into(),
            topic: topic::GIT.into(),
            key: "/fixture".into(),
        };
        hub.git_bindings.insert(
            key.clone(),
            vec![Binding {
                row: "run:a".into(),
                serial: hub.sources["peer"].serial,
                source: source.clone(),
            }],
        );
        let pump = crate::pump::Pump::default();
        let original = ViewRow {
            name: "codex/fixture/a".into(),
            cwd: "/fixture".into(),
            catalog_source: Some(source.clone()),
            ..Default::default()
        };
        let push = |row| {
            pump.push(Change::Upsert {
                computer: ComputerId("peer".into()),
                key: "run:a".into(),
                row,
            })
        };
        assert!(!hub.git_current(&key, &pump));
        push(original.clone());
        assert!(hub.git_current(&key, &pump));
        let mut changed = original.clone();
        changed.cwd = "/another-checkout".into();
        push(changed);
        assert!(!hub.git_current(&key, &pump));
        let mut stale = original.clone();
        stale.catalog_source.as_mut().unwrap().2 = 6;
        push(stale);
        assert!(!hub.git_current(&key, &pump));
        let mut foreign = original.clone();
        foreign.catalog_source.as_mut().unwrap().0 = uuid::Uuid::new_v4().to_string();
        push(foreign);
        assert!(!hub.git_current(&key, &pump));
        push(original);
        hub.sources.get_mut("peer").unwrap().online = false;
        assert!(!hub.git_current(&key, &pump));
        hub.sources.get_mut("peer").unwrap().online = true;
        let mut local = key.clone();
        local.computer.clear();
        assert!(!hub.git_current(&local, &pump));
        pump.push(Change::Remove {
            computer: ComputerId("peer".into()),
            key: "run:a".into(),
            reason: None,
        });
        assert!(!hub.git_current(&key, &pump));
    }
    #[test]
    fn renderer_or_cached_json_cannot_mint_native_catalog_provenance() {
        let row: ViewRow =
            serde_json::from_value(json!({"name":"codex/fixture/a","cwd":"/fixture",
            "catalog_source":["forged",2,7]}))
            .unwrap();
        assert!(row.catalog_source.is_none());
        assert!(serde_json::from_value::<RowInterest>(
            json!({"computer":"","key":"run:a","path":"/forged"})
        )
        .is_err());
    }
    #[test]
    fn refused_git_route_retires_once_and_does_not_recover_by_itself() {
        let mut hub = Hub {
            lease: Some(lease()),
            ..Default::default()
        };
        let spec = Subscription {
            topic: topic::GIT.into(),
            path: Some("/fixture".into()),
            ..Default::default()
        };
        hub.follow("", spec.clone()).unwrap();
        let key = Key::new("", &spec).unwrap();
        let (_, Work::Follow { id, .. }) = hub.next().unwrap() else {
            panic!("one original follow");
        };
        assert!(hub
            .recover(&key, &id, "git_status_failed", false)
            .unwrap()
            .is_some());
        assert!(matches!(hub.next(), Some((_, Work::Unfollow(original))) if original == id));
        assert!(hub.next().is_none());
        hub.retired(&id).unwrap();
        assert!(hub.next().is_none());
        assert!(hub.routes[&key].halted);
        hub.unfollow(&key);
        assert!(hub.routes.is_empty());
    }
}
