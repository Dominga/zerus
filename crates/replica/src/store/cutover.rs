//! Task-local prepare and seal custody, under the existing store admission.
use super::*;
use crate::{Capture, CutoverSeal};

impl Admitted<'_> {
    pub fn cutover_ready(&self, task: &TaskDoc) -> Result<()> {
        if self.store.phone {
            return Err(Error::new(
                "a phone projection is not native task admission",
            ));
        }
        task.validate_cutover_history()?;
        self.store.check_cutover_pin(task)?;
        if task.cutover_seal()?.is_none() || self.store.prepared(task.uuid())?.is_some() {
            return Err(Error::new("the original task cutover is still pending; automatic sync must finish it before a new write or start"));
        }
        Ok(())
    }
    /// The C2 caller holds native catalog/project admission. Read the task
    /// here and write only its first floor; a caller DTO cannot add edits.
    pub fn write_first_floor(&mut self, uuid: &str) -> Result<TaskDoc> {
        if self.store.phone {
            return Err(Error::new(
                "a phone projection cannot create a native floor",
            ));
        }
        if self.store.prepared(uuid)?.is_some() {
            return Err(Error::new("the original task freeze is pending; automatic catch-up must finish before a native floor write"));
        }
        let mut task = self.store.counted(uuid)?;
        if task.execution_managed() || task.cutover_seal()?.is_some() {
            return Err(Error::new("this task already has an execution root"));
        }
        task.floor()?;
        task.validate_cutover_history()?;
        self.store.save_admitted(&mut task)?;
        self.store.durable(uuid, task.current_vv());
        Ok(task)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    task: String,
    project: String,
    capture: Capture,
    #[serde(default)]
    proposal: Option<Proposal>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    seal: String,
    digest: String,
    version: crate::Version,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    format: u64,
    store: String,
    seal: CutoverSeal,
}

impl ProjectStore {
    /// Read-only native producer evidence; no snapshot/DTO can mint it.
    pub fn admitted_cutover_identity(&self, uuid: &str) -> Result<Option<String>> {
        if self.phone {
            return Ok(None);
        }
        let _turn = self.admission.enter();
        let Some(pin) = self.pinned_cutover(uuid)? else {
            return Ok(None);
        };
        let task = self.counted(uuid)?;
        task.validate_cutover_history()?;
        self.check_cutover_pin(&task)?;
        Ok(Some(pin.identity()))
    }
    /// Literal pre-upgrade writer fixture. Never available in production.
    #[cfg(any(test, feature = "test-support"))]
    pub fn save_legacy_for_test(&self, task: &mut TaskDoc) -> Result<()> {
        let _turn = self.admission.enter();
        self.save_admitted(task)?;
        self.durable(task.uuid(), task.current_vv());
        Ok(())
    }
    pub fn retain_cutover_proposal(&self, bytes: &[u8], own: &Capture) -> Result<()> {
        let _turn = self.admission.enter();
        self.retain_cutover_proposal_locked(bytes, own)
    }

    fn retain_cutover_proposal_locked(&self, bytes: &[u8], own: &Capture) -> Result<()> {
        if bytes.len() > crate::MAX_TASK_DOCUMENT {
            return Err(Error::new(
                "the original task cutover exceeds its byte bound",
            ));
        }
        let task = TaskDoc::from_bytes(bytes)?;
        task.validate_cutover_history()?;
        if task.view()?.project != self.project {
            return Err(Error::new("the proposal belongs to another project"));
        }
        let seal = task
            .cutover_seal()?
            .ok_or_else(|| Error::new("the proposal has no seal"))?;
        let mut prepared = self
            .prepared(task.uuid())?
            .filter(|prepared| &prepared.capture == own)
            .ok_or_else(|| Error::new("the proposal has no matching original native freeze"))?;
        if let Some(original) = &prepared.proposal {
            if original.seal != seal.identity()
                || original.digest != format!("{:x}", Sha256::digest(bytes))
            {
                return Err(Error::new(
                    "the original uncertain cutover cannot be replaced by a new proposal",
                ));
            }
            let original = bounded(
                &self.proposal_path(task.uuid()),
                crate::MAX_TASK_DOCUMENT as u64,
            )?;
            let retained = TaskDoc::from_bytes(&original)?;
            if retained.applied_version() != task.applied_version() {
                return Err(Error::new(
                    "the original proposal's exact version cannot be replaced",
                ));
            }
            return Ok(());
        }
        match bounded(&self.proposal_path(task.uuid()), crate::MAX_TASK_DOCUMENT as u64) {
            Ok(original) if original != bytes => return Err(Error::new("an original cutover was retained before its custody pointer; it cannot be overwritten")),
            Ok(_) => {},
            Err(error) if error.kind()==ErrorKind::NotFound => write_atomic(&self.proposal_path(task.uuid()), bytes)?,
            Err(error) => return Err(error.into()),
        }
        prepared.proposal = Some(Proposal {
            seal: seal.identity(),
            digest: format!("{:x}", Sha256::digest(bytes)),
            version: task
                .applied_version()
                .ok_or_else(|| Error::new("the proposal has unapplied changes"))?,
        });
        write_atomic(
            &self.prepare_path(task.uuid()),
            &serde_json::to_vec(&prepared)?,
        )?;
        Ok(())
    }

    pub fn original_cutover_proposal(&self, uuid: &str) -> Result<Option<Vec<u8>>> {
        if self.phone {
            return Err(Error::new(
                "a phone projection cannot restore native custody",
            ));
        }
        let _turn = self.admission.enter();
        let Some(mut prepared) = self.prepared(uuid)? else {
            return Ok(None);
        };
        if prepared.proposal.is_none() {
            let bytes = match bounded(&self.proposal_path(uuid), crate::MAX_TASK_DOCUMENT as u64) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            // Restoring the original data pointer grants no adoption: the
            // participant/source checks still run before any pin/save.
            self.retain_cutover_proposal_locked(&bytes, &prepared.capture)?;
            prepared = self
                .prepared(uuid)?
                .ok_or_else(|| Error::new("the original prepare disappeared"))?;
        }
        let expected = prepared
            .proposal
            .ok_or_else(|| Error::new("the original custody pointer is still unavailable"))?;
        let bytes = bounded(&self.proposal_path(uuid), crate::MAX_TASK_DOCUMENT as u64)?;
        let task = TaskDoc::from_bytes(&bytes)?;
        task.validate_cutover_history()?;
        if task.uuid() != uuid
            || task.view()?.project != self.project
            || task
                .cutover_seal()?
                .as_ref()
                .map(CutoverSeal::identity)
                .as_ref()
                != Some(&expected.seal)
            || format!("{:x}", Sha256::digest(&bytes)) != expected.digest
            || task.applied_version().as_ref() != Some(&expected.version)
        {
            return Err(Error::new(
                "the retained original cutover proposal cannot be verified",
            ));
        }
        Ok(Some(bytes))
    }

    pub fn pending_cutover_capture(&self, uuid: &str) -> Result<Option<Capture>> {
        let _turn = self.admission.enter();
        Ok(self.prepared(uuid)?.map(|prepared| prepared.capture))
    }

    /// Confirm a saved original after a cut before custody cleanup. It adds
    /// no CRDT operation and cannot turn a pin-only partial save into success.
    pub fn settle_cutover(&self, uuid: &str) -> Result<bool> {
        if self.phone {
            return Err(Error::new(
                "a phone projection cannot settle native task custody",
            ));
        }
        let _turn = self.admission.enter();
        let Some(prepared) = self.prepared(uuid)? else {
            return Ok(false);
        };
        let Some(proposal) = &prepared.proposal else {
            return Ok(false);
        };
        let Some(pin) = self.pinned_cutover(uuid)? else {
            return Ok(false);
        };
        if pin.identity() != proposal.seal {
            return Err(Error::new("the original cutover pin changed"));
        }
        let task = match self.counted_with(uuid, read_regular) {
            Ok(task) => task,
            Err(_) => return Ok(false),
        };
        task.validate_cutover_history()?;
        if task.cutover_seal()?.as_ref() != Some(&pin)
            || !crate::version_includes(
                &task.applied_version().unwrap_or_default(),
                &proposal.version,
            )
        {
            return Ok(false);
        }
        self.finish_cutover(uuid)?;
        Ok(true)
    }

    fn finish_cutover(&self, uuid: &str) -> Result<()> {
        match fs::remove_file(self.proposal_path(uuid)) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        fs::remove_file(self.prepare_path(uuid))?;
        #[cfg(unix)]
        File::open(self.dir.join("tasks"))?.sync_all()?;
        Ok(())
    }

    pub(crate) fn cutover_path(&self, uuid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{uuid}.cutover"))
    }
    fn prepare_path(&self, uuid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{uuid}.prepare"))
    }
    fn proposal_path(&self, uuid: &str) -> PathBuf {
        self.dir.join("tasks").join(format!("{uuid}.proposal"))
    }
    fn prepared(&self, uuid: &str) -> Result<Option<Prepared>> {
        match bounded(&self.prepare_path(uuid), 1 << 20) {
            Ok(bytes) if bytes.len() <= 1024 * 1024 => {
                let prepared: Prepared = serde_json::from_slice(&bytes)?;
                if prepared.task != uuid || prepared.project != self.project {
                    return Err(Error::new(
                        "the task prepare belongs to another task or project",
                    ));
                }
                Ok(Some(prepared))
            }
            Ok(_) => Err(Error::new("the task prepare is too large")),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    pub(crate) fn pinned_cutover(&self, uuid: &str) -> Result<Option<CutoverSeal>> {
        Ok(self.cutover_pin(uuid)?.map(|pin| pin.seal))
    }
    fn cutover_pin(&self, uuid: &str) -> Result<Option<Pin>> {
        match bounded(&self.cutover_path(uuid), 1 << 20) {
            Ok(bytes) if bytes.len() <= 1024 * 1024 => {
                let pin: Pin = serde_json::from_slice(&bytes)?;
                if pin.format != 1
                    || !ids::is_uuid(&pin.store)
                    || pin.seal.task != uuid
                    || pin.seal.project != self.project
                {
                    return Err(Error::new(
                        "the admitted seal belongs to another task or project",
                    ));
                }
                Ok(Some(pin))
            }
            Ok(_) => Err(Error::new("the admitted task seal is too large")),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
    pub(crate) fn check_cutover_pin(&self, task: &TaskDoc) -> Result<()> {
        if task.cutover_seal()? != self.pinned_cutover(task.uuid())? {
            return Err(Error::new("this task has not adopted the admitted attempts cutover here; automatic sync must catch it up before any task write or start"));
        }
        if let Some(pin) = self.cutover_pin(task.uuid())? {
            if pin.store != Self::store_id(self.dir.parent().expect("project parent"))? {
                return Err(Error::new("this replica store changed identity; automatic sync must re-adopt the current task seal before any write or start"));
            }
        }
        Ok(())
    }
    fn write_cutover_pin(&self, seal: &CutoverSeal) -> Result<()> {
        let pin = Pin {
            format: 1,
            store: Self::store_id(self.dir.parent().expect("project parent"))?,
            seal: seal.clone(),
        };
        write_atomic(&self.cutover_path(&seal.task), &serde_json::to_vec(&pin)?)
    }
    pub(crate) fn check_cutover_save(&self, task: &mut TaskDoc, received: bool) -> Result<()> {
        if self.phone {
            if self.pinned_cutover(task.uuid())?.is_some() || self.prepared(task.uuid())?.is_some()
            {
                return Err(Error::new(
                    "a phone cache cannot replace native task custody",
                ));
            }
            if task.schema() >= crate::task::cutover_schema() {
                if !received {
                    return Err(Error::new("This sealed task is read-only on the phone; use its computer to change or start it"));
                }
                task.projection_only = true;
            }
            return Ok(());
        }
        if self.prepared(task.uuid())?.is_some() {
            return Err(Error::new("this task is preparing its attempts cutover; automatic sync must finish that exact prepare before any new write or start"));
        }
        let fresh = task.created_here
            && task.saved.is_none()
            && !self.snapshot_path(task.uuid()).exists()
            && !self.updates_path(task.uuid()).exists()
            && !self.was_managed(task.uuid());
        if fresh && !received {
            let seal = task.seal_genesis()?;
            self.write_cutover_pin(&seal)?;
        }
        if received {
            task.validate_import_history()?;
        } else {
            task.validate_cutover_history()?;
        }
        self.check_cutover_pin(task)?;
        if !received && task.cutover_seal()?.is_none() {
            return Err(Error::new("this task still uses the old attempts protocol here; automatic sync must adopt its current cutover before any task write or start"));
        }
        Ok(())
    }
    /// Freeze under native store admission. Repetition returns the original
    /// token and version; it grants no native dispatch authority.
    pub fn prepare_cutover(&self, uuid: &str, node: &str) -> Result<(Capture, Vec<u8>)> {
        if !ids::is_uuid(uuid) || !ids::is_uuid(node) {
            return Err(Error::new(
                "a task prepare needs exact task and computer identities",
            ));
        }
        if self.phone {
            return Err(Error::new(
                "a phone projection cannot prepare, adopt or export native task authority",
            ));
        }
        let _turn = self.admission.enter();
        let own_store = Self::store_id(self.dir.parent().expect("project parent"))?;
        let stored = self.snapshot_path(uuid).exists()
            || self.updates_path(uuid).exists()
            || self.was_managed(uuid);
        let task = stored
            .then(|| self.counted_with(uuid, read_regular))
            .transpose()?;
        if let Some(task) = &task {
            task.validate_import_history()?;
            // A clean copy imported by an older reader may contain an
            // unadmitted seal. Freeze it as candidate data only; export and
            // every write still require the native pin or qualified catch-up.
            if let Some(known) = self.pinned_cutover(uuid)? {
                if task.cutover_seal()?.as_ref() != Some(&known) {
                    return Err(Error::new(
                        "the task candidate has another already known root",
                    ));
                }
            }
        }
        let version = task
            .as_ref()
            .map(|task| {
                task.applied_version()
                    .ok_or_else(|| Error::new("the task is not applied"))
            })
            .transpose()?
            .unwrap_or_default();
        let bytes = task
            .as_ref()
            .map(TaskDoc::snapshot)
            .transpose()?
            .unwrap_or_default();
        if let Some(prepared) = self.prepared(uuid)? {
            if prepared.capture.node != node
                || prepared.capture.store != own_store
                || prepared.capture.version != version
            {
                return Err(Error::new("the original task prepare changed here; its bytes are kept and cannot be rebased"));
            }
            return Ok((prepared.capture, bytes));
        }
        let capture = Capture {
            node: node.to_owned(),
            store: own_store,
            version,
            token: ids::new_uuid(),
        };
        write_atomic(
            &self.prepare_path(uuid),
            &serde_json::to_vec(&Prepared {
                task: uuid.to_owned(),
                project: self.project.clone(),
                capture: capture.clone(),
                proposal: None,
            })?,
        )?;
        Ok((capture, bytes))
    }
    /// Explicit bootstrap: only the proposal containing this native store's
    /// retained capture can adopt. Ordinary update import cannot call this.
    pub fn adopt_cutover(&self, bytes: &[u8], own: Option<&Capture>) -> Result<CutoverSeal> {
        if self.phone {
            return Err(Error::new(
                "a phone projection cannot prepare, adopt or export native task authority",
            ));
        }
        let _turn = self.admission.enter();
        self.adopt_cutover_locked(bytes, own, false)
    }
    /// The root transport must first bind a read-only admitted-seal export
    /// to its freshly verified current source node/store. The receiver still
    /// needs its own freeze and independently recomputed historical root.
    pub fn catch_up_cutover(
        &self,
        bytes: &[u8],
        own: &Capture,
        source_node: &str,
        source_store: &str,
        expected_seal: &str,
    ) -> Result<CutoverSeal> {
        if !ids::is_uuid(source_node) || !ids::is_uuid(source_store) {
            return Err(Error::new(
                "cutover catch-up has no verified source identity",
            ));
        }
        if self.phone {
            return Err(Error::new(
                "a phone projection cannot prepare, adopt or export native task authority",
            ));
        }
        let _turn = self.admission.enter();
        let task = TaskDoc::from_bytes(bytes)?;
        if task
            .cutover_seal()?
            .as_ref()
            .map(CutoverSeal::identity)
            .as_deref()
            != Some(expected_seal)
        {
            return Err(Error::new(
                "the source answered with another admitted task seal",
            ));
        }
        self.adopt_cutover_locked(bytes, Some(own), true)
    }
    fn adopt_cutover_locked(
        &self,
        bytes: &[u8],
        own: Option<&Capture>,
        catchup: bool,
    ) -> Result<CutoverSeal> {
        let mut task = TaskDoc::from_bytes(bytes)?;
        if task.view()?.project != self.project {
            return Err(Error::new("the cutover belongs to another project"));
        }
        task.validate_cutover_history()?;
        let seal = task
            .cutover_seal()?
            .ok_or_else(|| Error::new("the task has no seal"))?;
        let pinned = self.pinned_cutover(task.uuid())?;
        let native_pinned = self.cutover_pin(task.uuid())?.is_some_and(|pin| {
            Self::store_id(self.dir.parent().expect("project parent"))
                .is_ok_and(|own| own == pin.store)
        });
        if pinned.as_ref().is_some_and(|pinned| pinned != &seal) {
            return Err(Error::new(
                "a different admitted task root or seal cannot replace the original here",
            ));
        }
        let prepared = self
            .prepared(task.uuid())?
            .ok_or_else(|| Error::new("this store has no original task prepare"))?;
        let own = own
            .filter(|own| *own == &prepared.capture)
            .ok_or_else(|| Error::new("the adoption does not match the original task prepare"))?;
        if own.store != Self::store_id(self.dir.parent().expect("project parent"))? {
            return Err(Error::new("the original task freeze belongs to another replica-store incarnation; it is retained and cannot be retargeted"));
        }
        if prepared.proposal.as_ref().is_some_and(|original| {
            original.seal != seal.identity()
                || original.digest != format!("{:x}", Sha256::digest(bytes))
        }) {
            return Err(Error::new(
                "the original uncertain cutover proposal cannot be retargeted",
            ));
        }
        if !catchup && !native_pinned && seal.captures.get(&own.node) != Some(own) {
            return Err(Error::new(
                "the proposal does not include this store's original frozen capture",
            ));
        }
        if !crate::version_includes(
            &task
                .applied_version()
                .ok_or_else(|| Error::new("the proposal has unapplied changes"))?,
            &own.version,
        ) {
            return Err(Error::new("this returning task has extra old local changes; its original bytes are kept for review and are never replayed"));
        }
        // Re-read intact bytes under the same admission before pin/save.
        if self.snapshot_path(task.uuid()).exists() {
            let local = self.counted_with(task.uuid(), read_regular)?;
            if native_pinned
                && pinned.as_ref() == Some(&seal)
                && local.cutover_seal()?.as_ref() == Some(&seal)
                && local.current_vv().includes_vv(&task.current_vv())
            {
                // Exact original already saved: finish custody, no import,
                // new operation, acknowledgement or native action.
                self.finish_cutover(task.uuid())?;
                return Ok(seal);
            }
            if local.applied_version().as_ref() != Some(&own.version) {
                return Err(Error::new(
                    "the task changed after its original freeze; its bytes are kept",
                ));
            }
            let local_floor = local.floor_record();
            if seal
                .root
                .as_ref()
                .zip(local_floor.as_ref())
                .is_some_and(|(root, local)| local != root)
            {
                return Err(Error::new("the proposal has another historical task floor"));
            }
        }
        // Pin first. A cut before the canonical save leaves a held copy.
        self.retain_cutover_proposal_locked(bytes, own)?;
        self.write_cutover_pin(&seal)?;
        task.saved = None;
        self.save_admitted(&mut task)?;
        self.durable(task.uuid(), task.current_vv());
        self.finish_cutover(task.uuid())?;
        Ok(seal)
    }
    /// Read-only: serving a snapshot cannot mint, repair or rebase a seal.
    pub fn export_cutover(&self, uuid: &str) -> Result<(CutoverSeal, Vec<u8>)> {
        if self.phone {
            return Err(Error::new(
                "a phone projection cannot prepare, adopt or export native task authority",
            ));
        }
        let _turn = self.admission.enter();
        let task = self.counted(uuid)?;
        task.validate_cutover_history()?;
        self.check_cutover_pin(&task)?;
        let seal = self
            .pinned_cutover(uuid)?
            .ok_or_else(|| Error::new("this store has no admitted task seal to export"))?;
        Ok((seal, task.snapshot()?))
    }
}

fn bounded(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    open_regular(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::other(
            "task cutover custody exceeds its byte bound",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, NewTask, Stage, Target};
    const NODE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

    fn legacy(store: &ProjectStore) -> TaskDoc {
        let mut task = TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-a1b2c3",
            text: "original",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        task.floor().unwrap();
        // Literal pre-upgrade writer fixture: it has no new prepare/seal gate.
        store.save_admitted(&mut task).unwrap();
        task
    }

    #[test]
    fn a_frozen_clean_prefix_cannot_mint_a_competing_first_floor() {
        let root = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(root.path(), "project-1").unwrap();
        let mut task = TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-a1b2c3",
            text: "Clean original prefix",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        store.save_legacy_for_test(&mut task).unwrap();
        store.prepare_cutover(task.uuid(), NODE).unwrap();
        let before = fs::read(store.snapshot_path(task.uuid())).unwrap();
        let mut native = store
            .admit_until(Instant::now() + Duration::from_secs(1))
            .unwrap();
        assert!(native.write_first_floor(task.uuid()).is_err());
        assert_eq!(fs::read(store.snapshot_path(task.uuid())).unwrap(), before);
    }

    #[test]
    fn a_phone_receives_new_and_updated_sealed_tasks_without_native_authority() {
        let source_root = tempfile::tempdir().unwrap();
        let source = ProjectStore::open(source_root.path(), "project-1").unwrap();
        let mut task = TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-a1b2c3",
            text: "Current task",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        source.save(&mut task).unwrap();
        let phone_root = tempfile::tempdir().unwrap();
        let phone = ProjectStore::open_phone(phone_root.path(), "project-1").unwrap();
        let mut exchange = crate::phone::Exchange::new();
        for updated in [false, true] {
            if updated {
                task.edit_text("Updated current task", &Actor::Person, 2)
                    .unwrap();
                source.save(&mut task).unwrap();
            }
            let request:crate::phone::Request=serde_json::from_value(serde_json::json!({"schema":1,"project":"project-1","inventory":{"project":{},"tasks":{}}})).unwrap();
            exchange
                .accept(&phone, crate::phone::answer(&source, request).unwrap())
                .unwrap();
            let mut view = phone
                .load(task.uuid())
                .expect("the board keeps receiving task state");
            assert_eq!(
                view.view().unwrap().text,
                if updated {
                    "Updated current task"
                } else {
                    "Current task"
                }
            );
            assert!(!view.view().unwrap().writable);
            assert!(view.comment("No authority", &Actor::Person, 3).is_err());
            assert!(phone.save(&mut view).is_err());
            assert!(phone.pinned_cutover(task.uuid()).unwrap().is_none());
            assert!(phone.export_cutover(task.uuid()).is_err());
            assert!(phone.prepare_cutover(task.uuid(), NODE).is_err());
        }
        drop(phone);
        let native = ProjectStore::open(phone_root.path(), "project-1").unwrap();
        assert!(
            native.load(task.uuid()).is_err(),
            "read-only candidate bytes are not a native admitted copy"
        );
    }

    #[test]
    fn an_old_local_copy_cannot_save_a_new_write_before_catching_up() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let task = legacy(&store);
        let mut old = store.load(task.uuid()).unwrap();
        old.comment("must remain unsaved", &Actor::Person, 2)
            .unwrap();
        assert!(store.save(&mut old).is_err());
        assert!(!store
            .load(task.uuid())
            .unwrap()
            .view()
            .unwrap()
            .log
            .iter()
            .any(|entry| entry.text == "must remain unsaved"));
    }

    #[test]
    fn prepare_is_idempotent_and_blocks_a_stale_handle_save() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut task = legacy(&store);
        let (first, _) = store.prepare_cutover(task.uuid(), NODE).unwrap();
        let (again, _) = store.prepare_cutover(task.uuid(), NODE).unwrap();
        assert_eq!(first, again);
        task.comment("racing mutation", &Actor::Person, 2).unwrap();
        assert!(store.save(&mut task).is_err());
    }

    #[test]
    fn adoption_requires_the_original_freeze_and_keeps_stale_extra_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut task = legacy(&store);
        let (capture, _) = store.prepare_cutover(task.uuid(), NODE).unwrap();
        task.seal_cutover(
            BTreeMap::from([(NODE.into(), capture.clone())]),
            BTreeMap::new(),
        )
        .unwrap();
        let mut wrong = capture;
        wrong.token = ids::new_uuid();
        let before = read_regular(&store.snapshot_path(task.uuid())).unwrap();
        assert!(store
            .adopt_cutover(&task.snapshot().unwrap(), Some(&wrong))
            .is_err());
        assert_eq!(
            read_regular(&store.snapshot_path(task.uuid())).unwrap(),
            before
        );
    }

    #[test]
    fn a_fresh_ordinary_import_cannot_pin_a_relays_claimed_seal() {
        let temp = tempfile::tempdir().unwrap();
        let sender = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut task = legacy(&sender);
        task.seal_cutover(BTreeMap::new(), BTreeMap::new()).unwrap();
        let target = tempfile::tempdir().unwrap();
        let receiver = ProjectStore::open(target.path(), "project-1").unwrap();
        let result = receiver
            .import_updates(vec![crate::Update {
                document: crate::Document::Task(task.uuid().to_owned()),
                bytes: task.snapshot().unwrap(),
            }])
            .unwrap();
        assert_eq!(result.refused.len(), 1);
        assert!(!receiver.snapshot_path(task.uuid()).exists());
    }

    #[test]
    fn the_original_proposal_is_retained_before_any_peer_can_commit_it() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut task = legacy(&store);
        let (capture, _) = store.prepare_cutover(task.uuid(), NODE).unwrap();
        task.seal_cutover(
            BTreeMap::from([(NODE.into(), capture.clone())]),
            BTreeMap::new(),
        )
        .unwrap();
        let bytes = task.snapshot().unwrap();
        store.retain_cutover_proposal(&bytes, &capture).unwrap();
        assert_eq!(
            store.original_cutover_proposal(task.uuid()).unwrap(),
            Some(bytes)
        );
    }

    #[test]
    fn a_cut_between_retaining_bytes_and_the_pointer_keeps_the_same_original() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut task = legacy(&store);
        let (capture, _) = store.prepare_cutover(task.uuid(), NODE).unwrap();
        let original_prepare = bounded(&store.prepare_path(task.uuid()), 1 << 20).unwrap();
        task.seal_cutover(
            BTreeMap::from([(NODE.into(), capture.clone())]),
            BTreeMap::new(),
        )
        .unwrap();
        let bytes = task.snapshot().unwrap();
        store.retain_cutover_proposal(&bytes, &capture).unwrap();
        write_atomic(&store.prepare_path(task.uuid()), &original_prepare).unwrap();
        assert_eq!(
            store.original_cutover_proposal(task.uuid()).unwrap(),
            Some(bytes.clone())
        );
        assert_eq!(
            bounded(
                &store.proposal_path(task.uuid()),
                crate::MAX_TASK_DOCUMENT as u64
            )
            .unwrap(),
            bytes
        );
        assert!(store.pinned_cutover(task.uuid()).unwrap().is_none());
    }

    #[test]
    fn a_cut_after_canonical_save_only_settles_the_original_prepare() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut task = legacy(&store);
        let (capture, _) = store.prepare_cutover(task.uuid(), NODE).unwrap();
        let prepared = read_regular(&store.prepare_path(task.uuid())).unwrap();
        task.seal_cutover(
            BTreeMap::from([(NODE.into(), capture.clone())]),
            BTreeMap::new(),
        )
        .unwrap();
        let bytes = task.snapshot().unwrap();
        store.adopt_cutover(&bytes, Some(&capture)).unwrap();
        write_atomic(&store.prepare_path(task.uuid()), &prepared).unwrap();
        let before = store.load(task.uuid()).unwrap().version();
        assert!(store.adopt_cutover(&bytes, Some(&capture)).is_ok());
        assert_eq!(store.load(task.uuid()).unwrap().version(), before);
        assert!(!store.prepare_path(task.uuid()).exists());
    }

    #[test]
    fn a_changed_native_store_cannot_reuse_an_old_adoption_pin() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "project-1").unwrap();
        let mut task = TaskDoc::create(NewTask {
            project: "project-1",
            id: "T-a1b2c3",
            text: "native",
            stage: Stage::Todo,
            target: Target::Any,
            author: Actor::Person,
            rank: "V",
            now: 1,
        })
        .unwrap();
        store.save(&mut task).unwrap();
        let _ = ProjectStore::store_id(temp.path()).unwrap();
        write_atomic(
            &temp.path().join("store-id"),
            format!("{}\n", ids::new_uuid()).as_bytes(),
        )
        .unwrap();
        assert!(store.export_cutover(task.uuid()).is_err());
        task.comment("new identity", &Actor::Person, 2).unwrap();
        assert!(store.save(&mut task).is_err());
    }
}
