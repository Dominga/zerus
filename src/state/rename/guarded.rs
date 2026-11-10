//! Normal rename recovery with retained source data and explicit original-owner
//! effects. No ambient session selection or ordinary writer/backend fallback.
use super::super::guarded_recovery::{Effects, View};
use super::*;

const FILES: usize = 4096;

fn value(view: &mut View<'_>, path: &Path) -> Result<Value> {
    view.json(path, storage::ID_RECORD_BYTES)?
        .ok_or("Original rename source disappeared".into())
}

fn commit_in_place(view: &mut View<'_>, transaction: &Value, old: &str, new: &str) -> Result<()> {
    let record = &transaction["record"];
    let id = string(record, "session_id");
    let path = storage::id_record_path(id).ok_or("Rename record has no canonical session ID")?;
    let raw = value(view, &path)?;
    let mut current = view.record(&path, string(&raw, "name"))?;
    if current["run_id"] != record["run_id"] || !storage::in_id_layout(&current) {
        return Err("rename source run changed; refusing to replace its binding".into());
    }
    if string(&current, "name") != new {
        labels::add_guarded(view, new, id)?;
        recovery::rename_job_in_place_guarded(view, &current, new)?;
        anchor(&mut current);
        current["name"] = json!(new);
        current["renamed_at"] = json!(now());
        moved_terminal(transaction, &mut current);
        view.write_record(&mut current)?;
    }
    labels::remove_guarded(view, old, id)?;
    rename_archives(view, old, new, &current["run_id"], false)
}

fn rename_archives(
    view: &mut View<'_>,
    old: &str,
    new: &str,
    run: &Value,
    clear_shadow: bool,
) -> Result<()> {
    for path in view.files(&root().join("archives"), FILES)? {
        let mut archived = value(view, &path)?;
        if archived["run_id"] == *run && string(&archived, "name") == old {
            anchor(&mut archived);
            archived["name"] = json!(new);
            if clear_shadow {
                archived
                    .as_object_mut()
                    .ok_or("Invalid rename archive")?
                    .remove("rename_shadow");
                archived
                    .as_object_mut()
                    .ok_or("Invalid rename archive")?
                    .remove("rename_target");
            }
            view.atomic_record(&path, &mut archived)?;
        }
    }
    Ok(())
}

fn commit(view: &mut View<'_>, transaction: &Value) -> Result<()> {
    let (old, new) = (string(transaction, "old"), string(transaction, "new"));
    validate(old, new)?;
    if transaction["layout"] == "id" && transaction["record"].is_object() {
        return commit_in_place(view, transaction, old, new);
    }
    let Some(mut record) = transaction["record"]
        .as_object()
        .map(|_| transaction["record"].clone())
    else {
        return Ok(());
    };
    let source = source_path(transaction);
    if let Some(current) = view.json(&source, storage::ID_RECORD_BYTES)? {
        if current["run_id"] != record["run_id"] {
            return Err("rename source run changed; refusing to replace its binding".into());
        }
        if current["rename_shadow"] != true {
            record = current;
        }
    }
    let destination = view.label_path(new)?;
    if view.read(&destination, storage::ID_RECORD_BYTES)?.is_some() {
        let current = view.record(&destination, new)?;
        if current["run_id"] != record["run_id"] {
            return Err("rename destination was reused; refusing to replace its binding".into());
        }
    }
    anchor(&mut record);
    record["name"] = json!(new);
    record["renamed_at"] = json!(now());
    moved_terminal(transaction, &mut record);
    if transaction["legacy"] == true {
        record["legacy_shadow"] = json!({"name":old,"run_id":record["run_id"]});
    }
    recovery::rename_job_guarded(view, old, new, string(&record, "run_id"))?;
    keep_warm::rename_guarded(view, old, new)?;
    view.write_record(&mut record)?;
    if transaction["legacy"] == true {
        view.atomic(
            &storage::route_path(old),
            format!(
                "{}\n",
                json!({"version":1,"name":old,"run_id":record["run_id"]})
            )
            .as_bytes(),
        )?;
        let mut shadow = record.clone();
        shadow["name"] = json!(old);
        shadow["rename_target"] = json!(new);
        shadow["rename_shadow"] = json!(true);
        view.atomic_record(&storage::legacy_record_path(old), &mut shadow)?;
    } else {
        view.remove(&source)?;
    }
    rename_archives(view, old, new, &record["run_id"], true)
}

pub(in crate::state) fn recover_locked(effects: &dyn Effects) -> Result<()> {
    let paths = View::new(effects).files(&root().join("renames"), FILES)?;
    for path in paths {
        let mut view = View::new(effects);
        let transaction = value(&mut view, &path)?;
        if transaction["version"] != 1 {
            return Err("invalid rename transaction".into());
        }
        let (old, new) = (string(&transaction, "old"), string(&transaction, "new"));
        validate(old, new)?;
        if let Some(id) = transaction["session_id"].as_str() {
            let from = match string(&transaction, "terminal") {
                "" => old,
                name => name,
            };
            if let Some(current) = view.session_name(id)? {
                if current == from {
                    view.remove(&path)?;
                    continue;
                }
                if current != new {
                    return Err(backend::RENAME_RECOVERY_CHANGED.text().into());
                }
            }
        }
        commit(&mut view, &transaction)?;
        view.remove(&path)?;
    }
    effects.check()
}

pub(in crate::state) fn mirror_legacy(view: &mut View<'_>, record: &Value) -> Result<()> {
    let legacy = &record["legacy_shadow"];
    let old = string(legacy, "name");
    if old.is_empty() || legacy["run_id"] != record["run_id"] {
        return Ok(());
    }
    let path = storage::legacy_record_path(old);
    let Some(current) = view.json(&path, storage::ID_RECORD_BYTES)? else {
        return Ok(());
    };
    if current["run_id"] != record["run_id"] {
        return Err("legacy rename shadow has a different run identity".into());
    }
    let mut shadow = record.clone();
    shadow["name"] = json!(old);
    shadow["rename_target"] = record["name"].clone();
    shadow["rename_shadow"] = json!(true);
    view.atomic_record(&path, &mut shadow)
}

pub(in crate::state) fn sync_legacy_locked(effects: &dyn Effects) -> Result<()> {
    for path in View::new(effects).files(&root().join("name_routes"), FILES)? {
        let mut view = View::new(effects);
        let route = value(&mut view, &path)?;
        let old = string(&route, "name");
        let Some(shadow) =
            view.json(&storage::legacy_record_path(old), storage::ID_RECORD_BYTES)?
        else {
            continue;
        };
        if shadow["rename_shadow"] != true || shadow["run_id"] != route["run_id"] {
            return Err("invalid legacy rename shadow identity".into());
        }
        let name = string(&shadow, "rename_target");
        if name.is_empty() {
            continue;
        }
        let record_path = view.label_path(name)?;
        if view.read(&record_path, storage::ID_RECORD_BYTES)?.is_none() {
            continue;
        }
        let mut record = view.record(&record_path, name)?;
        if record["run_id"] != shadow["run_id"] {
            continue;
        }
        if record["supervisor"] != shadow["supervisor"] {
            return Err("legacy supervisor completion identity changed".into());
        }
        let completed =
            shadow["exited_at"].as_f64().unwrap_or(0.) > record["exited_at"].as_f64().unwrap_or(0.);
        let spawned = shadow["updated"].as_f64().unwrap_or(0.)
            > record["updated"].as_f64().unwrap_or(0.)
            && (record["pid"] != shadow["pid"]
                || record["process_start"] != shadow["process_start"]);
        if completed || spawned {
            if shadow["pid"].as_u64().unwrap_or(0) == 0 {
                return Err("legacy supervisor published an invalid process identity".into());
            }
            record["pid"] = shadow["pid"].clone();
            record["process_start"] = shadow["process_start"].clone();
            if completed {
                for key in [
                    "exit_code",
                    "termination_signal",
                    "exited_at",
                    "completion_source",
                    "completion_reason",
                ] {
                    if let Some(value) = shadow.get(key) {
                        record[key] = value.clone();
                    }
                }
            }
            view.write_record(&mut record)?;
        }
    }
    effects.check()
}
