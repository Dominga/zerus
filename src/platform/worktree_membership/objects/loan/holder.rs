//! Read-only checkout custody. No command here signals, schedules an effect or
//! supplies consent. The private owner must retain Source/Actions separately.
//! A phase is armed only from that owner's ORIGINAL guarded spawn; its join
//! message is emitted only after the owned mutator AND all I/O/loans settle.
use super::{peer::Peer, transport, Scope};
use std::{
    collections::BTreeSet,
    io,
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};
use zerus_client_core::{
    creation::canonical_uuid,
    worktree_programs::{
        custody::{Binding as Request, MAX_PHASES, RETENTION_SECONDS},
        stop::{Input, Source},
        Manifest,
    },
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "the original checkout loan phase is unavailable",
    )
}

struct Phase {
    request: Request,
    borrower: Peer,
    until: Instant,
    spent: bool,
    deadline_millis: u64,
}

pub(crate) struct Holder {
    scope: Scope,
    original: Manifest,
    pipeline: String,
    until: Instant,
    closed: bool,
    used: BTreeSet<String>,
    phase: Option<Phase>,
    source: Option<Source>,
    observations: usize,
    cleanup_used: bool,
}
impl Holder {
    /// The original Scope is MOVED here before confirmation; no serialized
    /// DirectoryId or later pathname reopen can construct its custody.
    pub(crate) fn new(
        scope: Scope,
        original: Manifest,
        pipeline: String,
        now: u64,
    ) -> io::Result<Self> {
        if !canonical_uuid(&pipeline)
            || pipeline == original.id
            || !original.valid(now)
            || scope.identity() != &original.checkout
            || scope.path != std::path::Path::new(&original.capture.root)
        {
            return Err(invalid());
        }
        let until = Instant::now()
            + Duration::from_secs(RETENTION_SECONDS)
                .min(Duration::from_secs(original.expires - now));
        scope.current(until)?;
        Ok(Self {
            scope,
            original,
            pipeline,
            until,
            closed: false,
            used: BTreeSet::new(),
            phase: None,
            source: None,
            observations: 0,
            cleanup_used: false,
        })
    }
    fn open(&mut self, until: Instant) -> io::Result<Instant> {
        if self.closed || Instant::now() >= self.until {
            self.closed = true;
            return Err(invalid());
        }
        let until = until.min(self.until);
        if Instant::now() >= until {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(until)
    }
    /// New read-only observations retain THIS scope, original target and fixed
    /// expiry. They may name new programs for review, never authorize stopping
    /// them. An unretired mutator blocks observation for Force, even after EOF.
    pub(crate) fn observe(&mut self, now: u64, until: Instant) -> io::Result<Manifest> {
        let until = self.open(until)?;
        if self.cleanup_used
            || self.phase.is_some()
            || self.observations >= MAX_PHASES
            || !self.original.valid(now)
        {
            return Err(invalid());
        }
        self.observations += 1;
        let programs = crate::platform::worktree_membership::programs(&self.scope, until)?;
        let mut current = self.original.clone();
        current.id = uuid::Uuid::new_v4().to_string();
        current.programs = programs;
        // Original issued/expires anchor custody, not a new permit. Fresh
        // rows are correlated by the observation nonce on the native stream.
        // NEVER extend original.expires or self.until while presenting a list.
        if !current.valid(now) || Instant::now() >= until {
            return Err(invalid());
        }
        self.scope.current(until)?;
        Ok(current)
    }
    /// DATA admission only, not normal-stop/Force authority. The caller must
    /// already hold its native confirmation and the original guarded helper.
    /// An old phase cannot be replaced merely because a socket/ACK returned.
    pub(crate) fn arm(
        &mut self,
        input: &Input,
        borrower: Peer,
        now: u64,
        until: Instant,
    ) -> io::Result<()> {
        let binding = Request::for_input(&self.pipeline, input);
        self.arm_bound(input, binding, borrower, now, until)
    }
    pub(crate) fn arm_cleanup(
        &mut self,
        input: &zerus_client_core::worktree_programs::cleanup::Input,
        borrower: Peer,
        now: u64,
        until: Instant,
    ) -> io::Result<()> {
        if !input.valid() || !self.used.contains(&input.stop.request_id) {
            return Err(invalid());
        }
        let binding = Request::for_cleanup(&self.pipeline, input);
        self.arm_bound(&input.stop, binding, borrower, now, until)?;
        // Terminal even if delivery/result is lost. No later stop, Force,
        // observation loan or new removal UUID can reopen this pipeline.
        self.cleanup_used = true;
        Ok(())
    }
    fn arm_bound(
        &mut self,
        input: &Input,
        binding: Request,
        borrower: Peer,
        now: u64,
        until: Instant,
    ) -> io::Result<()> {
        let until = self.open(until)?;
        let manifest = &input.manifest;
        if self.cleanup_used
            || self.phase.is_some()
            || self.used.len() >= MAX_PHASES
            || !input.valid()
            || !manifest.valid(now)
            || manifest.capture != self.original.capture
            || manifest.configuration != self.original.configuration
            || manifest.boot != self.original.boot
            || manifest.checkout != self.original.checkout
            || !self.original.contains_originals(&manifest.programs)
            || !binding.valid()
            || binding.pipeline != self.pipeline
            || binding.manifest != manifest.binding()
            || self.used.contains(&binding.request)
            || self.source.as_ref().is_some_and(|old| old != &input.source)
            || borrower.exited()?
        {
            return Err(invalid());
        }
        self.scope.current(until)?;
        let deadline_millis = zerus_client_core::worktree_programs::custody::clock::freeze(until)?;
        if self.source.is_none() {
            self.source = Some(input.source.clone());
        }
        self.used.insert(binding.request.clone());
        self.phase = Some(Phase {
            request: binding,
            borrower,
            until,
            spent: false,
            deadline_millis,
        });
        Ok(())
    }
    /// One transfer to the exact live native borrower. Spend BEFORE sendmsg:
    /// uncertain or partial delivery never permits a second loan or reconnect.
    pub(crate) fn lend(&mut self, stream: &UnixStream, bytes: &[u8]) -> io::Result<()> {
        let until = self.open(self.until)?;
        let request = Request::decode(bytes).map_err(io::Error::other)?;
        let phase = self.phase.as_mut().ok_or_else(invalid)?;
        let until = until.min(phase.until);
        if phase.spent || request != phase.request {
            return Err(invalid());
        }
        phase.borrower.current(stream, until)?;
        let description =
            super::transfer(&self.scope, &phase.request, phase.deadline_millis, until)?;
        phase.borrower.current(stream, until)?;
        phase.spent = true;
        transport::send(
            stream,
            &description,
            &self.scope.native_descriptors(),
            until,
        )
    }
    /// Authenticate the ORIGINAL native helper before waiting for its bounded
    /// request. Unrelated local sockets cannot reserve the control reader.
    pub(crate) fn lend_next(&mut self, stream: &UnixStream) -> io::Result<()> {
        let until = self.open(self.until)?;
        let phase = self.phase.as_ref().ok_or_else(invalid)?;
        let until = until.min(phase.until);
        if phase.spent {
            return Err(invalid());
        }
        phase.borrower.current(stream, until)?;
        let (bytes, fds) = transport::receive(stream, until)?;
        if !fds.is_empty() {
            return Err(invalid());
        }
        self.lend(stream, &bytes)
    }
    /// Called ONLY for the original owner's actual-join control message, after
    /// its owned child/workers/output readers/loans physically settle. Independently
    /// require kernel-confirmed exit of the original borrower. EOF, a release
    /// ACK or failed image observation cannot retire a still-live mutation.
    pub(crate) fn joined(&mut self, bytes: &[u8]) -> io::Result<()> {
        let request = Request::decode(bytes).map_err(io::Error::other)?;
        let phase = self.phase.as_ref().ok_or_else(invalid)?;
        if request != phase.request || !phase.borrower.exited()? {
            return Err(invalid());
        }
        self.phase.take();
        Ok(())
    }
    /// Cancellation/expiry stops admission but RETAINS every original charge.
    /// The owner still drains its actual original mutation before dropping us.
    pub(crate) fn close(&mut self) {
        self.closed = true;
    }
    pub(crate) fn settled(&self) -> bool {
        self.phase.is_none()
    }
}

#[cfg(test)]
mod tests;
