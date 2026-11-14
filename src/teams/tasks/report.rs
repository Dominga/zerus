use super::*;

/// Current holder's ready report, bounded to 12,000 UTF-8 bytes. Neither an
/// old attempt's late report nor a person's acceptance is a holder report.
#[allow(dead_code)] // Consumed by the service package.
pub(crate) fn report(state: &Path, task: &TaskRef) -> Result<Option<String>> {
    use sha2::{Digest, Sha256};
    let directory = state
        .join("replica")
        .join(format!("{:x}", Sha256::digest(task.project.as_bytes())));
    if !directory.is_dir() {
        return Ok(None);
    }
    let tasks = ProjectStore::open(&state.join("replica"), &task.project).map_err(storage)?;
    let doc = load(&tasks, &task.uuid)?;
    let view = doc.view().map_err(storage)?;
    if view.execution.as_ref().is_some_and(|e| e.conflict) {
        return Ok(None);
    }
    let current = view.execution.as_ref().and_then(|e| e.current.as_ref());
    if view.execution.is_some() && current.is_none() {
        return Ok(None);
    }
    let legacy = current.is_none_or(|c| c.legacy);
    let holder = current
        .and_then(|c| c.executor.as_ref())
        .or(view.session.as_ref());
    let Some(holder) = holder.filter(|h| !h.session_id.is_empty()) else {
        return Ok(None);
    };
    // Legacy dispatches have no attempt UUID, including after their report
    // moved the task to Review. Bound their history by position, never time:
    // several boundaries can be written in the same millisecond.
    let boundary = legacy
        .then(|| {
            view.log.iter().rposition(|entry| {
                matches!(entry.kind.as_str(), "sent" | "resumed" | "started")
                    && entry.session.as_ref() == Some(holder)
            })
        })
        .flatten();
    if legacy && boundary.is_none() {
        return Ok(None);
    }
    let found = view
        .log
        .iter()
        .enumerate()
        .rev()
        .find(|(index, entry)| {
            entry.kind == "report"
                && entry.stale.is_none()
                && entry.actor == TaskActor::Agent(holder.clone())
                && if legacy {
                    *index > boundary.expect("legacy boundary") && entry.reported_attempt.is_none()
                } else {
                    entry.reported_attempt.as_deref() == current.map(|c| c.id.as_str())
                }
        })
        .map(|(_, entry)| entry);
    Ok(found.map(|entry| {
        let mut end = entry.text.len().min(12_000);
        while !entry.text.is_char_boundary(end) {
            end -= 1;
        }
        entry.text[..end].to_owned()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_replica::{
        execution::{NativeRequest, Origin, Receipt, ReportKind},
        Position, SessionKey, Settlement, Target,
    };

    #[test]
    fn a_missing_project_is_read_only() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            report(
                tmp.path(),
                &TaskRef {
                    project: "example".into(),
                    uuid: uuid::Uuid::new_v4().to_string()
                }
            )
            .unwrap(),
            None
        );
        assert!(!tmp.path().join("replica").exists());
    }

    #[test]
    fn a_new_legacy_dispatch_cannot_reuse_a_report_at_the_same_millisecond() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(&tmp.path().join("replica"), "example").unwrap();
        let holder = SessionKey {
            machine: "example-machine".into(),
            session_id: "launch_id:holder".into(),
            session: "codex/holder".into(),
        };
        let mut doc = store
            .new_task(
                "Work",
                Stage::Todo,
                Target::Any,
                TaskActor::Person,
                &Position::End,
                "example",
                1,
            )
            .unwrap();
        let reference = TaskRef {
            project: "example".into(),
            uuid: doc.uuid().into(),
        };
        doc.record_send(&holder, "first-dispatch", &TaskActor::Person, 2)
            .unwrap();
        doc.record_report("Old report", &TaskActor::Agent(holder.clone()), 2)
            .unwrap();
        store.save(&mut doc).unwrap();
        drop(store);
        assert_eq!(
            report(tmp.path(), &reference).unwrap().as_deref(),
            Some("Old report")
        );
        let store = ProjectStore::open(&tmp.path().join("replica"), "example").unwrap();
        doc.set_stage(Stage::Todo, &TaskActor::Person, 2).unwrap();
        doc.record_send(&holder, "second-dispatch", &TaskActor::Person, 2)
            .unwrap();
        store.save(&mut doc).unwrap();
        drop(store);
        assert_eq!(report(tmp.path(), &reference).unwrap(), None);
    }

    #[test]
    fn reads_only_the_current_holders_report_without_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(&tmp.path().join("replica"), "example").unwrap();
        let holder = SessionKey {
            machine: "example-machine".into(),
            session_id: "launch_id:holder".into(),
            session: "codex/holder".into(),
        };
        let mut doc = store
            .new_task(
                "Work",
                Stage::Todo,
                Target::Any,
                TaskActor::Person,
                &Position::End,
                "example",
                1,
            )
            .unwrap();
        let reference = TaskRef {
            project: "example".into(),
            uuid: doc.uuid().into(),
        };
        let send = |doc: &mut TaskDoc, at| {
            let id = uuid::Uuid::new_v4().to_string();
            let native = NativeRequest {
                machine: holder.machine.clone(),
                name: holder.session.clone(),
                run_id: "example-run".into(),
                conversation_id: String::new(),
                digest: "ab".repeat(32),
            };
            doc.reserve_attempt(
                &id,
                Origin::Send,
                &holder,
                native.clone(),
                &TaskActor::Person,
                at,
            )
            .unwrap();
            let receipt = Receipt {
                request_id: id.clone(),
                name: native.name,
                run_id: native.run_id,
                conversation_id: native.conversation_id,
                first_message: true,
                digest: native.digest,
                submitted: true,
            };
            doc.settle_attempt(
                &id,
                Settlement::Receipt(&receipt),
                &TaskActor::Person,
                at + 1,
            )
            .unwrap();
            id
        };
        let first = send(&mut doc, 2);
        store.save(&mut doc).unwrap();
        drop(store);
        assert_eq!(report(tmp.path(), &reference).unwrap(), None);
        let store = ProjectStore::open(&tmp.path().join("replica"), "example").unwrap();
        doc.report_attempt(
            &first,
            "Own report",
            ReportKind::Ready,
            None,
            &TaskActor::Agent(holder.clone()),
            5,
        )
        .unwrap();
        store.save(&mut doc).unwrap();
        drop(store);
        assert_eq!(
            report(tmp.path(), &reference).unwrap().as_deref(),
            Some("Own report")
        );
        let store = ProjectStore::open(&tmp.path().join("replica"), "example").unwrap();
        doc.set_stage(Stage::Todo, &TaskActor::Person, 6).unwrap();
        let second = send(&mut doc, 7);
        doc.report_attempt(
            &first,
            "Late old report",
            ReportKind::Ready,
            None,
            &TaskActor::Agent(holder.clone()),
            10,
        )
        .unwrap();
        store.save(&mut doc).unwrap();
        drop(store);
        assert_eq!(report(tmp.path(), &reference).unwrap(), None);
        let store = ProjectStore::open(&tmp.path().join("replica"), "example").unwrap();
        doc.report_attempt(
            &second,
            &"界".repeat(5000),
            ReportKind::Ready,
            None,
            &TaskActor::Agent(holder),
            11,
        )
        .unwrap();
        doc.report(
            "Person's text",
            ReportKind::Ready,
            None,
            &TaskActor::Person,
            12,
        )
        .unwrap();
        store.save(&mut doc).unwrap();
        let revision = doc.revision();
        drop(store);
        let text = report(tmp.path(), &reference).unwrap().unwrap();
        assert_eq!(text.len(), 12_000);
        assert_eq!(text, "界".repeat(4000));
        let store = ProjectStore::open(&tmp.path().join("replica"), "example").unwrap();
        assert_eq!(store.load(&reference.uuid).unwrap().revision(), revision);
    }
}
