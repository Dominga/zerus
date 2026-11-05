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
        stop::Input,
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
}

pub(crate) struct Holder {
    scope: Scope,
    original: Manifest,
    pipeline: String,
    until: Instant,
    closed: bool,
    used: BTreeSet<String>,
    phase: Option<Phase>,
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
        let until = self.open(until)?;
        let manifest = &input.manifest;
        if self.phase.is_some()
            || self.used.len() >= MAX_PHASES
            || !input.valid()
            || !manifest.valid(now)
            || manifest.capture != self.original.capture
            || manifest.configuration != self.original.configuration
            || manifest.boot != self.original.boot
            || manifest.checkout != self.original.checkout
            || !self.original.contains_originals(&manifest.programs)
            || self.used.contains(&input.request_id)
            || borrower.exited()?
        {
            return Err(invalid());
        }
        self.scope.current(until)?;
        self.used.insert(input.request_id.clone());
        self.phase = Some(Phase {
            request: Request::for_input(&self.pipeline, input),
            borrower,
            until,
            spent: false,
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
        let description = self.scope.describe_loan(until)?;
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
