//! Historical original completion, NOT physical drain or stop consent. The
//! native worker supplies the actual selected config.state; checkout paths
//! never select a lifecycle journal. Missing/uncertain evidence cannot retry.
use super::*;
use crate::state::repositories::directory::Directory;
use zerus_client_core::worktree_programs::stop::Input;

const ARCHIVE_BYTES: u64 = 1 << 20;
pub(super) struct Completed {
    input: Input,
    archive: Value,
    ownership: Value,
}
fn check(until: Instant) -> Result<()> {
    if Instant::now() >= until {
        Err(INVALID.into())
    } else {
        Ok(())
    }
}
fn receipt_bound(input: &Input, receipt: &Value) -> Result<()> {
    let capture = &input.manifest.capture;
    let target = &capture.target;
    let request = input.termination_id()?;
    let result = &receipt["result_target"];
    if !input.valid()
        || receipt["status"] != "completed"
        || receipt["request_id"] != request
        || receipt["name"] != target.name
        || receipt["run_id"] != target.run_id
        || receipt["conversation_id"] != target.conversation_id
        || result["archive_id"] != input.archive_id
        || result["name"] != target.name
        || result["run_id"] != target.run_id
        || result["conversation_id"] != target.conversation_id
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn bind(input: &Input, receipt: &Value, archive: &Value, ownership: &Value) -> Result<()> {
    receipt_bound(input, receipt)?;
    let capture = &input.manifest.capture;
    let target = &capture.target;
    if archive["version"] != 1
        || archive["archive_id"] != input.archive_id
        || archive["name"] != target.name
        || archive["run_id"] != target.run_id
        || archive["conversation_id"] != target.conversation_id
        || archive["worktree_owner"] != *ownership
        || ownership["id"] != capture.owner_id
        || ownership["path"] != capture.root
        || ownership["common_dir"] != capture.common_dir
    {
        return Err(INVALID.into());
    }
    Ok(())
}
pub(super) fn observe(state: &Path, input: &Input, until: Instant) -> Result<Completed> {
    check(until)?;
    if !input.valid() || !state.is_absolute() {
        return Err(INVALID.into());
    }
    let directory = Directory::open(state)?;
    if !directory.private()? {
        return Err(INVALID.into());
    }
    // Reuse the original receipt owner's hash/UUID/name/run/conversation
    // checks, passing the EXACT original stdin, without reconstructing it.
    let receipt = super::super::observe_lifecycle_receipt(
        state,
        &input.manifest.capture.target.name,
        input.original.as_bytes(),
    )?;
    check(until)?;
    // Unavailable/uncertain is not a reason to inspect a guessed archive or
    // checkout. The existing owner validated the original request hash first.
    receipt_bound(input, &receipt)?;
    let archives = directory.child("archives")?;
    if !archives.private()? {
        return Err(INVALID.into());
    }
    let file = archives.read_private(&format!("{}.json", input.archive_id))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > ARCHIVE_BYTES {
        return Err(INVALID.into());
    }
    let mut bytes = Vec::new();
    file.take(ARCHIVE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > ARCHIVE_BYTES {
        return Err(INVALID.into());
    }
    let archive: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    check(until)?;
    let ownership = super::owner(&input.manifest.capture)?;
    bind(input, &receipt, &archive, &ownership)?;
    directory.verify()?;
    archives.verify()?;
    check(until)?;
    Ok(Completed {
        input: input.clone(),
        archive,
        ownership,
    })
}
impl Completed {
    /// Freshly refuse replacement/corruption while retaining the original
    /// correlation. Success still supplies NO current process/permission fact.
    pub(super) fn recheck(&self, state: &Path, until: Instant) -> Result<()> {
        let current = observe(state, &self.input, until)?;
        if current.archive != self.archive || current.ownership != self.ownership {
            return Err(INVALID.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn data() -> (Input, Value, Value, Value) {
        let target = zerus_client_core::Target {
            computer: String::new(),
            name: "codex/example/original".into(),
            run_id: "original-run".into(),
            conversation_id: "original-conversation".into(),
        };
        let capture = Capture {
            target: target.clone(),
            machine: uuid::Uuid::new_v4().to_string(),
            root: "/srv/example/work".into(),
            common_dir: "/srv/example/main/.git".into(),
            owner_id: uuid::Uuid::new_v4().to_string(),
        };
        let invocation = capture
            .invocation(&uuid::Uuid::new_v4().to_string())
            .unwrap();
        let input = Input {
            request_id: uuid::Uuid::new_v4().to_string(),
            confirmation_id: uuid::Uuid::new_v4().to_string(),
            source: zerus_client_core::worktree_programs::stop::Source::Service {
                epoch: uuid::Uuid::new_v4().to_string(),
            },
            manifest: wire::Manifest {
                abi: wire::ABI.into(),
                id: uuid::Uuid::new_v4().to_string(),
                capture,
                configuration: "a".repeat(64),
                boot: uuid::Uuid::new_v4().to_string(),
                checkout: wire::DirectoryId {
                    device: 1,
                    inode: 2,
                    mount: "fixture-object".into(),
                },
                issued: 1000,
                expires: 1300,
                programs: Vec::new(),
            },
            original: String::from_utf8(invocation.stdin.unwrap()).unwrap(),
            archive_id: uuid::Uuid::new_v4().to_string(),
            stop: zerus_client_core::worktree_programs::stop::Kind::Normal,
        };
        let ownership = json!({"id":input.manifest.capture.owner_id,"path":input.manifest.capture.root,
            "common_dir":input.manifest.capture.common_dir});
        let archive = json!({"version":1,"archive_id":input.archive_id,"name":target.name,"run_id":target.run_id,
            "conversation_id":target.conversation_id,"worktree_owner":ownership});
        let receipt = json!({"status":"completed","request_id":input.termination_id().unwrap(),
            "name":target.name,"run_id":target.run_id,"conversation_id":target.conversation_id,
            "result_target":{"name":target.name,"run_id":target.run_id,"conversation_id":target.conversation_id,"archive_id":input.archive_id}});
        assert!(input.valid());
        (input, receipt, archive, ownership)
    }
    #[test]
    fn only_exact_original_done_archive_and_launch_ownership_bind() {
        let (input, receipt, archive, ownership) = data();
        assert!(bind(&input, &receipt, &archive, &ownership).is_ok());
        for status in ["claimed", "uncertain", "failed", "unavailable", ""] {
            let mut changed = receipt.clone();
            changed["status"] = status.into();
            assert!(bind(&input, &changed, &archive, &ownership).is_err());
        }
        for key in ["request_id", "name", "run_id", "conversation_id"] {
            let mut changed = receipt.clone();
            changed[key] = "different".into();
            assert!(
                bind(&input, &changed, &archive, &ownership).is_err(),
                "{key}"
            );
        }
        for key in ["archive_id", "name", "run_id", "conversation_id"] {
            let mut changed = receipt.clone();
            changed["result_target"][key] = "different".into();
            assert!(
                bind(&input, &changed, &archive, &ownership).is_err(),
                "{key}"
            );
        }
        for key in [
            "archive_id",
            "name",
            "run_id",
            "conversation_id",
            "worktree_owner",
        ] {
            let mut changed = archive.clone();
            changed[key] = Value::Null;
            assert!(
                bind(&input, &receipt, &changed, &ownership).is_err(),
                "{key}"
            );
        }
        for key in ["id", "path", "common_dir"] {
            let mut changed = ownership.clone();
            changed[key] = "different".into();
            assert!(bind(&input, &receipt, &archive, &changed).is_err(), "{key}");
        }
    }
}
