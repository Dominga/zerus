//! Reuse the maintained named-pipe process/image proof. It retains the actual
//! client handle and kernel image-section match, never PID-then-path trust.
use crate::desktop::transport::Stream;
use std::{io, time::Instant};
use zerus_client_core::desktop::windows::{self, process::Original, Peer as NativePeer, Server};

const ANCESTORS: usize = 64;
const COMMAND_BYTES: usize = 16 << 10;
fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "The original task-control peer cannot be verified",
    )
}
fn budget(until: Instant) -> io::Result<()> {
    if Instant::now() >= until {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Ok(())
    }
}

pub(super) struct Peer {
    native: NativePeer,
    process: Original,
    arguments: Vec<String>,
    ancestry: Vec<Original>,
}
impl Peer {
    pub fn arguments(&self, until: Instant) -> io::Result<Vec<String>> {
        self.current(until)?;
        Ok(self.arguments.clone())
    }
    pub fn capture(stream: &Stream, until: Instant) -> io::Result<Self> {
        budget(until)?;
        let pipe = stream.verification_handle()?;
        let Server::Verified(native) = windows::verify_own_client_until(&pipe, until) else {
            return Err(refused());
        };
        let process = Original::open(native.pid)?;
        if !process.own || process.birth.to_string() != native.birth || !native.current() {
            return Err(refused());
        }
        let arguments = process.arguments()?;
        if arguments.len() > 64 || arguments.iter().map(String::len).sum::<usize>() > COMMAND_BYTES
        {
            return Err(refused());
        }
        let peer = Self {
            native,
            process,
            arguments,
            ancestry: vec![],
        };
        peer.current(until)?;
        Ok(peer)
    }
    pub fn command(&self, expected: &[&str], until: Instant) -> io::Result<()> {
        self.current(until)?;
        if self
            .arguments
            .get(1..)
            .is_none_or(|args| args.iter().map(String::as_str).ne(expected.iter().copied()))
        {
            return Err(refused());
        }
        Ok(())
    }
    pub fn executor(&mut self, pid: u32, birth: &str, until: Instant) -> io::Result<()> {
        self.current(until)?;
        if !self.ancestry.is_empty() {
            return Err(refused());
        }
        let expected = birth.parse::<u64>().map_err(|_| refused())?;
        let mut child_pid = self.process.pid;
        let mut child_parent = self.process.parent;
        let mut child_birth = self.process.birth;
        let mut seen = std::collections::BTreeSet::from([child_pid]);
        for _ in 0..ANCESTORS {
            if child_pid == pid {
                return if child_birth == expected {
                    self.current(until)
                } else {
                    Err(refused())
                };
            }
            budget(until)?;
            if child_parent == 0 || !seen.insert(child_parent) {
                return Err(refused());
            }
            let parent = Original::open(child_parent)?;
            if !parent.own || parent.birth > child_birth {
                return Err(refused());
            }
            child_pid = parent.pid;
            child_parent = parent.parent;
            child_birth = parent.birth;
            self.ancestry.push(parent);
        }
        Err(refused())
    }
    pub fn current(&self, until: Instant) -> io::Result<()> {
        budget(until)?;
        if !self.native.current()
            || !self.process.current()
            || self.native.pid != self.process.pid
            || self.native.birth != self.process.birth.to_string()
            || self.process.arguments()? != self.arguments
        {
            return Err(refused());
        }
        let mut child = &self.process;
        for parent in &self.ancestry {
            budget(until)?;
            if !parent.current()
                || !parent.own
                || child.parent != parent.pid
                || parent.birth > child.birth
            {
                return Err(refused());
            }
            child = parent;
        }
        if !self.native.current() || !self.process.current() {
            return Err(refused());
        }
        budget(until)
    }
}

pub(super) struct Resident {
    agent: Original,
    supervisor: Original,
    agent_image: windows::image::PinnedImage,
    trusted: windows::image::PinnedImage,
    arguments: Vec<String>,
}
impl Resident {
    pub fn capture(
        agent: u32,
        agent_birth: &str,
        supervisor: u32,
        supervisor_start: &str,
        supervisor_birth: &str,
        until: Instant,
    ) -> io::Result<Self> {
        budget(until)?;
        if agent == supervisor {
            return Err(refused());
        }
        let agent = Original::open(agent)?;
        let supervisor = Original::open(supervisor)?;
        if !agent.own
            || !supervisor.own
            || agent.birth.to_string() != agent_birth
            || supervisor.birth.to_string() != supervisor_start
            || supervisor.birth.to_string() != supervisor_birth
            || agent.parent != supervisor.pid
            || supervisor.birth > agent.birth
        {
            return Err(refused());
        }
        let own = Original::open(std::process::id())?;
        let trusted = windows::image::PinnedImage::for_process(&own.handle())?;
        trusted.matches_process(&supervisor.handle())?;
        let agent_image = windows::image::PinnedImage::for_process(&agent.handle())?;
        let arguments = supervisor.arguments()?;
        if arguments.len() > 64 || arguments.iter().map(String::len).sum::<usize>() > COMMAND_BYTES
        {
            return Err(refused());
        }
        let resident = Self {
            agent,
            supervisor,
            agent_image,
            trusted,
            arguments,
        };
        resident.current(until)?;
        Ok(resident)
    }
    pub fn current(&self, until: Instant) -> io::Result<()> {
        budget(until)?;
        if !self.agent.current()
            || !self.supervisor.current()
            || self.supervisor.arguments()? != self.arguments
        {
            return Err(refused());
        }
        self.trusted.matches_process(&self.supervisor.handle())?;
        self.agent_image.matches_process(&self.agent.handle())?;
        budget(until)
    }
    pub fn arguments(&self, until: Instant) -> io::Result<Vec<String>> {
        self.current(until)?;
        Ok(self.arguments.clone())
    }
}
