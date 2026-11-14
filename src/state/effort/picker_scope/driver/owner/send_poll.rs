//! Claude Send alone may lend Root to hooks, retaining the original object,
//! SID exclusion, namespaces, receipts, frame, helper custody and deadline.
use super::*;

impl Owner {
    pub(in crate::state::effort::picker_scope::driver) fn root_held(&self) -> Result<bool> {
        self.actor()?;
        let phase = self
            .phase
            .lock()
            .map_err(|_| "Original Send source unavailable")?;
        Ok(!matches!(&*phase, Phase::Native(scope) if matches!(scope.writer, Writer::Polling)))
    }

    /// The fixed read helper forms may poll; ordinary Scope operations cannot.
    pub(in crate::state::effort::picker_scope::driver) fn read_snapshot(
        &self,
    ) -> Result<(Value, Value)> {
        self.actor()?;
        self.check()?;
        let (snapshot, polling) = {
            let phase = self
                .phase
                .lock()
                .map_err(|_| "Original Send source unavailable")?;
            match &*phase {
                Phase::Native(scope) if matches!(scope.writer, Writer::Polling) => {
                    let (original, record) = scope.poll_current()?;
                    ((record, original.version), true)
                }
                Phase::Native(scope) => ((scope.current()?, scope.record.version.clone()), false),
                Phase::Root(_) => return Err("Original Send SID scope unavailable".into()),
            }
        };
        if polling {
            send::claude::check_marker(self, &snapshot.0)?;
        }
        Ok(snapshot)
    }

    pub(in crate::state::effort::picker_scope::driver) fn release_send_root(&self) -> Result<()> {
        self.actor()?;
        self.check()?;
        if self.helpers.load(Ordering::Acquire) != 0 {
            return Err("Original Send helpers have not physically settled".into());
        }
        self.with_scope(|scope| {
            let record = scope.current()?;
            send::claude::check_marker(self, &record)?;
            if !matches!(scope.writer, Writer::Owned(_)) {
                return Err("Only an owned original Send may lend Root".into());
            }
            self.invalidate();
            let Writer::Owned(writer) = std::mem::replace(&mut scope.writer, Writer::Polling)
            else {
                unreachable!("checked owned writer")
            };
            // A failed unlock is unknown, never a reason to reopen or replace it.
            writer.release().map_err(|e| e.to_string())
        })
    }

    pub(in crate::state::effort::picker_scope::driver) fn retake_send_root(
        &self,
        deadline: Instant,
    ) -> Result<()> {
        self.actor()?;
        let deadline = deadline.min(self.until);
        self.check()?;
        if self.helpers.load(Ordering::Acquire) != 0 {
            return Err("Original Send helpers have not physically settled".into());
        }
        let mut phase = self
            .phase
            .lock()
            .map_err(|_| "Original Send source unavailable")?;
        let Phase::Native(scope) = &mut *phase else {
            return Err("Original Send SID scope unavailable".into());
        };
        let (_, record) = scope.poll_current()?;
        send::claude::check_marker(self, &record)?;
        // Duplicate only the independently pinned original object's descriptor.
        // There is no pathname lock acquisition, recovery or new SID admission.
        let file = scope
            .writer_file
            .file
            .try_clone()
            .map_err(|e| e.to_string())?;
        loop {
            self.control.check()?;
            check_time(deadline)?;
            scope.sources()?;
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => break,
                Err(error) if crate::platform::lock_contended(&error) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        let writer = Guard::held(file);
        check_time(deadline)?;
        self.control.check()?;
        let (original, record) = scope.poll_current()?;
        send::claude::check_marker(self, &record)?;
        terminating::check_mutation(&record)?;
        if writer.identity().map_err(|e| e.to_string())?
            != crate::platform::FileIdentity::of(&scope.writer_file.file)
                .map_err(|e| e.to_string())?
        {
            return Err("Original Send Root changed while reacquiring".into());
        }
        self.invalidate();
        scope.writer = Writer::Owned(writer);
        scope.record = original;
        scope.current()?;
        send::claude::adopt_version(self, &record, &scope.record.version)
    }
}
