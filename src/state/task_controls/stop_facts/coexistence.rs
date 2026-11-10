//! Inactive selected-capture support and original data custody. This
//! is not an actor/effect proof; the runtime gate stays off.
//! Outside CurrentLease it holds only data/object custody.
#![cfg(windows)]
#![allow(dead_code)] // Inactive support only, never a global lint allowance.
use super::*;
use crate::state::{
    repositories::directory::{stop_capture, Directory},
    task_controls::writer::Writer,
};
use std::{cell::Cell, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    OriginalWriter,
    Publisher,
    Fact,
    Snapshot,
    WriterObject,
    Namespace,
    Keeper,
    Retirement,
    Conversion,
    Handoff,
    Lease,
    Final,
}
#[cfg(test)]
thread_local! { static CUT: Cell<Option<Stage>> = const { Cell::new(None) }; }
fn checkpoint(stage: Stage) -> std::result::Result<(), Stage> {
    #[cfg(test)]
    if CUT.with(|cut| cut.get() == Some(stage)) {
        return Err(stage);
    }
    let _ = stage;
    Ok(())
}
struct Custody {
    native: Option<Writer>,
    root: Option<Directory>,
    directory: Option<Directory>,
    aliases: Vec<Directory>,
    namespace: Option<stop_capture::Namespace>,
    fact_file: Option<File>,
    writer_file: Option<File>,
    expected: Executor,
    name: String,
    snapshot: Option<Snapshot>,
    identity: Option<platform::FileIdentity>,
    writer: Option<platform::FileIdentity>,
    bytes: usize,
    raw: Vec<u8>, // Bounded acquired bytes remain in FAILED data custody too.
    digest: [u8; 32],
    until: Instant,
    detail: Option<&'static str>,
}
impl Custody {
    fn clock(&self) -> std::result::Result<(), Stage> {
        if Instant::now() >= self.until {
            Err(Stage::Final)
        } else {
            Ok(())
        }
    }
    fn prepare(
        &mut self,
        new_fixture: bool,
        extra_aliases: usize,
    ) -> std::result::Result<(), Stage> {
        checkpoint(Stage::OriginalWriter)?;
        self.clock()?;
        if extra_aliases > 2 {
            return Err(Stage::Namespace);
        }
        let native = self.native.as_ref().ok_or(Stage::OriginalWriter)?;
        native.verify().map_err(|_| Stage::OriginalWriter)?;
        let publisher = Publisher::open_enabled(
            native.root().map_err(|_| Stage::OriginalWriter)?,
            native.guard().map_err(|_| Stage::OriginalWriter)?,
            &self.expected,
            new_fixture,
        )
        .map_err(|_| Stage::Publisher)?;
        #[cfg(test)]
        if new_fixture {
            // Synthetic fixture initialization only; the dormant non-test
            // factory always requests existing history and publishes nothing.
            publisher
                .publish(&Fact {
                    id: "00000000-0000-4000-8000-000000000002".into(),
                    executor: self.expected.clone(),
                    source: Source::VerifiedExternalInterrupt,
                    turn: Some("original-turn".into()),
                })
                .map_err(|_| Stage::Publisher)?;
        }
        // Consume only the independent Publisher's owned directories. The
        // native Writer remains owned by this Custody until explicit retirement.
        let Publisher {
            root,
            directory,
            name,
            ..
        } = publisher;
        self.root = Some(root);
        self.directory = Some(directory);
        self.name = name;
        checkpoint(Stage::Publisher)?;
        let root = self.root.as_ref().ok_or(Stage::Publisher)?;
        for _ in 0..extra_aliases {
            // These are real, internally acquired aliases, never a caller's
            // boolean/count substituted for a checked handle inventory.
            self.aliases
                .push(Directory::open(&root.path).map_err(|_| Stage::Namespace)?);
        }
        self.fact_file = Some(
            self.directory
                .as_ref()
                .ok_or(Stage::Fact)?
                .read_private(&self.name)
                .map_err(|_| Stage::Fact)?,
        );
        checkpoint(Stage::Fact)?;
        let fact = self.fact_file.as_ref().ok_or(Stage::Fact)?;
        self.identity = Some(platform::FileIdentity::of(fact).map_err(|_| Stage::Fact)?);
        fact.take(BYTES as u64 + 1)
            .read_to_end(&mut self.raw)
            .map_err(|_| Stage::Snapshot)?;
        self.bytes = self.raw.len();
        self.digest = Sha256::digest(&self.raw).into();
        if self.raw.len() > BYTES {
            return Err(Stage::Snapshot);
        }
        self.snapshot = Some(serde_json::from_slice(&self.raw).map_err(|_| Stage::Snapshot)?);
        let snapshot = self.snapshot.as_ref().ok_or(Stage::Snapshot)?;
        snapshot
            .validate(&self.expected)
            .map_err(|_| Stage::Snapshot)?;
        if snapshot.held_unknown() {
            return Err(Stage::Snapshot);
        }
        checkpoint(Stage::Snapshot)?;
        let native = self.native.as_ref().ok_or(Stage::OriginalWriter)?;
        self.writer = Some(
            native
                .guard()
                .map_err(|_| Stage::WriterObject)?
                .identity()
                .map_err(|_| Stage::WriterObject)?,
        );
        self.writer_file = Some(
            self.root
                .as_ref()
                .ok_or(Stage::WriterObject)?
                .read_private(".lock")
                .map_err(|_| Stage::WriterObject)?,
        );
        if platform::FileIdentity::of(self.writer_file.as_ref().ok_or(Stage::WriterObject)?)
            .map_err(|_| Stage::WriterObject)?
            != self.writer.ok_or(Stage::WriterObject)?
        {
            return Err(Stage::WriterObject);
        }
        checkpoint(Stage::WriterObject)?;
        self.root
            .as_ref()
            .ok_or(Stage::Namespace)?
            .verify()
            .map_err(|_| Stage::Namespace)?;
        self.directory
            .as_ref()
            .ok_or(Stage::Namespace)?
            .verify()
            .map_err(|_| Stage::Namespace)?;
        // Move both originals as one closed ownership transition. Even an
        // unexpected incomplete state is restored to Failed custody.
        let (root, directory) = match (self.root.take(), self.directory.take()) {
            (Some(root), Some(directory)) => (root, directory),
            (root, directory) => {
                self.root = root;
                self.directory = directory;
                return Err(Stage::Namespace);
            }
        };
        self.namespace = Some(stop_capture::Namespace::own(
            root,
            directory,
            std::mem::take(&mut self.aliases),
        ));
        checkpoint(Stage::Namespace)?;
        let qualification = {
            let ns = self.namespace.as_mut().ok_or(Stage::Namespace)?;
            ns.initialize(self.until).and_then(|()| {
                ns.qualify_file(
                    self.fact_file.as_ref().ok_or("no retained fact")?,
                    self.until,
                )?;
                ns.qualify_file(
                    self.writer_file.as_ref().ok_or("no retained writer")?,
                    self.until,
                )
            })
        };
        if let Err(reason) = qualification {
            self.detail = Some(reason);
            return Err(Stage::Keeper);
        }
        checkpoint(Stage::Keeper)?;
        self.native
            .as_ref()
            .ok_or(Stage::Retirement)?
            .verify()
            .map_err(|_| Stage::Retirement)?;
        checkpoint(Stage::Retirement)?;
        // Low-level conversion itself consumes this owned Writer (including
        // its opaque R Directory). No caller-released boolean can authorize it.
        self.clock()?;
        if let Err(reason) = self
            .namespace
            .as_mut()
            .ok_or(Stage::Conversion)?
            .convert(&mut self.native, self.until)
        {
            self.detail = Some(reason);
            return Err(Stage::Conversion);
        }
        checkpoint(Stage::Conversion)?;
        let current = self
            .namespace
            .as_ref()
            .ok_or(Stage::Handoff)?
            .lease(self.until)
            .map_err(|_| Stage::Handoff)?;
        self.check(&current)?;
        checkpoint(Stage::Handoff)?;
        Ok(())
    }
    fn check(&self, lease: &stop_capture::ReadLease<'_>) -> std::result::Result<(), Stage> {
        self.clock()?;
        if self.native.is_some() {
            return Err(Stage::Retirement);
        }
        lease.verify(self.until).map_err(|_| Stage::Lease)?;
        let fact = self.fact_file.as_ref().ok_or(Stage::Fact)?;
        let writer = self.writer_file.as_ref().ok_or(Stage::WriterObject)?;
        let ns = self.namespace.as_ref().ok_or(Stage::Namespace)?;
        for file in [fact, writer] {
            if !platform::verify_private_file(file).map_err(|_| Stage::Lease)? {
                return Err(Stage::Lease);
            }
            ns.qualify_file(file, self.until)
                .map_err(|_| Stage::Lease)?;
        }
        if Some(platform::FileIdentity::of(fact).map_err(|_| Stage::Fact)?) != self.identity
            || Some(platform::FileIdentity::of(writer).map_err(|_| Stage::WriterObject)?)
                != self.writer
        {
            return Err(Stage::Lease);
        }
        lease
            .with_files(&self.name, self.until, |current_writer, current_fact| {
                ns.qualify_file(current_writer, self.until)?;
                ns.qualify_file(current_fact, self.until)?;
                if Some(
                    platform::FileIdentity::of(current_writer)
                        .map_err(|_| "current writer identity")?,
                ) != self.writer
                    || Some(
                        platform::FileIdentity::of(current_fact)
                            .map_err(|_| "current fact identity")?,
                    ) != self.identity
                {
                    return Err("current original file binding changed");
                }
                let mut bytes = Vec::new();
                (current_fact)
                    .take(BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "bounded fact read")?;
                if bytes.len() > BYTES
                    || bytes.len() != self.bytes
                    || <[u8; 32]>::from(Sha256::digest(&bytes)) != self.digest
                {
                    return Err("complete original fact bytes changed");
                }
                let parsed: Snapshot =
                    serde_json::from_slice(&bytes).map_err(|_| "fact framing changed")?;
                parsed
                    .validate(&self.expected)
                    .map_err(|_| "fact scope changed")?;
                if parsed.held_unknown() || Some(&parsed) != self.snapshot.as_ref() {
                    return Err("snapshot changed or overflowed");
                }
                for f in [fact, writer, current_fact, current_writer] {
                    if !platform::verify_private_file(f).map_err(|_| "final held-file privacy")? {
                        return Err("held-file privacy changed");
                    }
                }
                Ok(())
            })
            .map_err(|_| Stage::Lease)?;
        lease.verify(self.until).map_err(|_| Stage::Lease)?;
        self.clock()
    }
}

#[must_use = "Retain failed data custody until explicit retirement"]
pub(super) struct Failed {
    stage: Stage,
    custody: Box<Custody>,
}
impl Failed {
    pub fn stage(&self) -> Stage {
        self.stage
    }
    /// Closed reason text only, no path, volume, GUID, SID, ACL or raw content.
    pub fn detail(&self) -> Option<&'static str> {
        self.custody.detail
    }
    pub fn retire(self) {
        drop(self);
    }
}
#[must_use = "A seal is data-only; a live CurrentLease is required for use"]
pub(super) struct Seal {
    custody: Custody,
    failed: Cell<bool>,
}
/// Selected handoff: take the SAME already-owned original Writer and its
/// stored budget. No root, later Instant, new mint or released assertion.
pub(super) fn from_owned(owned: Writer, expected: Executor) -> std::result::Result<Seal, Failed> {
    let until = owned.deadline();
    build(owned, expected, until, false, 0)
}

/// Closed pending owner: create the native Writer with this SAME deadline,
/// then move it by value into custody. No caller can pair an opaque Writer
/// with a later deadline after its private original budget has been erased.
/// No runtime/default factory selects this dormant entry.
pub(super) fn seal(
    root: &Path,
    expected: Executor,
    until: Instant,
) -> std::result::Result<Seal, Failed> {
    let native = crate::state::task_controls::writer::open(root, until);
    begin(native, expected, until, false, 0)
}
fn begin(
    native: std::result::Result<Option<Writer>, String>,
    expected: Executor,
    until: Instant,
    new_fixture: bool,
    aliases: usize,
) -> std::result::Result<Seal, Failed> {
    match native {
        Ok(Some(writer)) => build(writer, expected, until, new_fixture, aliases),
        _ => Err(Failed {
            stage: Stage::OriginalWriter,
            custody: Box::new(empty(expected, until)),
        }),
    }
}
fn empty(expected: Executor, until: Instant) -> Custody {
    Custody {
        native: None,
        root: None,
        directory: None,
        aliases: vec![],
        namespace: None,
        fact_file: None,
        writer_file: None,
        expected,
        name: String::new(),
        snapshot: None,
        identity: None,
        writer: None,
        bytes: 0,
        raw: Vec::new(),
        digest: [0; 32],
        until,
        detail: None,
    }
}

fn build(
    writer: Writer,
    expected: Executor,
    until: Instant,
    new_fixture: bool,
    aliases: usize,
) -> std::result::Result<Seal, Failed> {
    let mut custody = empty(expected, until);
    custody.native = Some(writer);
    match custody.prepare(new_fixture, aliases) {
        Ok(()) => Ok(Seal {
            custody,
            failed: Cell::new(false),
        }),
        Err(stage) => Err(Failed {
            stage,
            custody: Box::new(custody),
        }),
    }
}
impl Seal {
    pub fn lease<'a>(
        &'a self,
        expected: &Executor,
    ) -> std::result::Result<CurrentLease<'a>, Stage> {
        if self.failed.get() || expected != &self.custody.expected {
            self.failed.set(true);
            return Err(Stage::Lease);
        }
        let result = (|| {
            checkpoint(Stage::Lease)?;
            let namespace = self
                .custody
                .namespace
                .as_ref()
                .ok_or(Stage::Namespace)?
                .lease(self.custody.until)
                .map_err(|_| Stage::Lease)?;
            self.custody.check(&namespace)?;
            Ok(CurrentLease {
                owner: self,
                namespace,
                failed: Cell::new(false),
            })
        })();
        if result.is_err() {
            self.failed.set(true);
        }
        result
    }
    pub fn retire(self) {
        drop(self);
    }
}
/// This value retains strict original/current namespace pins for its ENTIRE
/// lifetime. No raw namespace guard, File, mutable Directory or Actor conversion
/// escapes. Transaction users retain the complete scope through COMMIT.
pub(super) struct CurrentLease<'a> {
    owner: &'a Seal,
    namespace: stop_capture::ReadLease<'a>,
    failed: Cell<bool>,
}
impl CurrentLease<'_> {
    fn check(&self) -> std::result::Result<(), Stage> {
        if self.failed.get() || self.owner.failed.get() {
            return Err(Stage::Lease);
        }
        let result = self
            .owner
            .custody
            .check(&self.namespace)
            .and_then(|()| checkpoint(Stage::Final));
        if result.is_err() {
            self.failed.set(true);
            self.owner.failed.set(true);
        }
        result
    }
    /// A bounded read use retains this exact typed namespace lease through
    /// the final recheck. The returned value is data only, never an actor or
    /// transaction receipt; callers must retain the scope through COMMIT.
    pub fn read<T>(&self, read: impl FnOnce(&[Fact]) -> T) -> std::result::Result<T, Stage> {
        self.read_scoped(|_, facts| read(facts))
    }
    /// Both references are original snapshot DATA, never an Actor grant.
    /// The current namespace lease and original deadline span the complete
    /// callback and final recheck, with the existing failure/panic poisoning.
    pub fn read_scoped<T>(
        &self,
        read: impl FnOnce(&Executor, &[Fact]) -> T,
    ) -> std::result::Result<T, Stage> {
        let snapshot = self
            .owner
            .custody
            .snapshot
            .as_ref()
            .ok_or(Stage::Snapshot)?;
        let result = checked_read(
            || self.check(),
            || read(&snapshot.executor, &snapshot.facts),
        );
        if result.is_err() {
            self.failed.set(true);
            self.owner.failed.set(true);
        }
        result
    }
    pub fn final_check(&self) -> std::result::Result<(), Stage> {
        self.check()
    }
}

#[cfg(test)]
pub(super) fn fixture_seal(
    root: &Path,
    expected: Executor,
    until: Instant,
    fresh: bool,
    aliases: usize,
) -> std::result::Result<Seal, Failed> {
    begin(
        crate::state::task_controls::writer::test_open(root, until).map(Some),
        expected,
        until,
        fresh,
        aliases,
    )
}
#[cfg(test)]
pub(super) fn fault(stage: Option<Stage>) {
    CUT.with(|cut| cut.set(stage));
}
#[cfg(test)]
pub(super) fn retained(failed: &Failed) -> (bool, bool, bool, bool) {
    (
        failed.custody.native.is_some(),
        failed.custody.fact_file.is_some(),
        failed.custody.writer_file.is_some(),
        failed.custody.namespace.is_some(),
    )
}

// Pure actual-source flow: final failure never returns the read value. This
// tests no namespace/kernel/actor authority and is portable for extraction.
fn checked_read<T>(
    mut verify: impl FnMut() -> std::result::Result<(), Stage>,
    read: impl FnOnce() -> T,
) -> std::result::Result<T, Stage> {
    verify()?;
    let data =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(read)).map_err(|_| Stage::Final)?;
    verify()?;
    Ok(data)
}
#[cfg(test)]
mod read_flow_controls {
    use super::*;
    #[test]
    fn rejected_entry_does_not_call_the_reader() {
        let called = Cell::new(false);
        let result = checked_read(|| Err(Stage::Lease), || called.set(true));
        assert_eq!(result, Err(Stage::Lease));
        assert!(!called.get());
    }
    #[test]
    fn final_refusal_cannot_return_the_previously_read_data() {
        let calls = Cell::new(0);
        let reads = Cell::new(0);
        let result = checked_read(
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    Err(Stage::Final)
                } else {
                    Ok(())
                }
            },
            || {
                reads.set(reads.get() + 1);
                "private-read-value"
            },
        );
        assert_eq!(result, Err(Stage::Final));
        assert_eq!(reads.get(), 1);
        assert_eq!(calls.get(), 2);
    }
    #[test]
    fn complete_read_keeps_the_same_verifier_alive_across_the_whole_use() {
        let calls = Cell::new(0);
        let result = checked_read(
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
            || {
                assert_eq!(calls.get(), 1);
                42
            },
        );
        assert_eq!(result, Ok(42));
        assert_eq!(calls.get(), 2);
    }
}

#[cfg(test)]
mod read_panic_control {
    use super::*;
    #[test]
    fn a_panicking_read_never_returns_data_or_skips_as_a_success() {
        let checks = Cell::new(0);
        let result: std::result::Result<(), Stage> = checked_read(
            || {
                checks.set(checks.get() + 1);
                Ok(())
            },
            || panic!("private read fixture"),
        );
        assert_eq!(result, Err(Stage::Final));
        assert_eq!(checks.get(), 1);
    }
}
