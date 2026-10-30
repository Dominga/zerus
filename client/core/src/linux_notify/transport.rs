//! Optional method transport. No worker, detector, ledger, queue or retry loop.
//!
//! G1 must fence/admit/reserve each exact job before calling this port. Namespace
//! data and a successful method reply confer no lease or settlement authority.
//! The caller retains its execution charge until `shutdown` reports `Drained`.
//! Signals and durable native-ID/token routing belong to a later adapter slice.
use super::{escape_body, Namespace};
use futures_lite::future;
use std::collections::BTreeMap;
use std::future::Future;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use zbus::zvariant::Value;
use zbus::{Connection, Executor, Message};

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const BUS: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
pub const MAX_BUDGET: Duration = Duration::from_secs(5);
pub const TEXT_LIMIT: usize = 16 * 1024;
const POLL: Duration = Duration::from_millis(10);

/// All refusals precede the native method call. No daemon error text is exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    InvalidBudget,
    Cancelled,
    Deadline,
    Connection,
    Lookup,
    Unavailable,
    NamespaceChanged,
    TextLimit,
    Closed,
    Runtime,
}

/// Any failure after the native call might have issued is terminal Unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unknown {
    Deadline,
    Cancelled,
    Transport,
    Protocol,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    Refused(Refusal),
    Delivered(T),
    Unknown(Unknown),
}

/// Native identity data only, not a verified G1 receipt or activation ticket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeId {
    pub namespace: Namespace,
    pub id: NonZeroU32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drain {
    Pending,
    Drained,
}

/// Owns one connection, driven on the caller's thread. zbus's internal executor
/// thread is disabled, and no connection/reader handle escapes this type.
/// The optional feature does not construct a transport or install a caller.
pub struct Transport {
    connection: Option<Connection>,
    executor: Executor<'static>,
    closing: bool,
}
impl Transport {
    /// Explicit address only: the port never chooses an inherited session bus.
    /// Connection authentication and Hello are inside this single deadline.
    pub fn connect(address: &str, budget: Duration, cancel: &AtomicBool) -> Result<Self, Refusal> {
        outside_tokio()?;
        let deadline = deadline(budget)?;
        before(cancel, deadline)?;
        if address.len() > TEXT_LIMIT {
            return Err(Refusal::Connection);
        }
        let builder = zbus::connection::Builder::address(address)
            .map_err(|_| Refusal::Connection)?
            .internal_executor(false)
            .max_queued(1)
            .method_timeout(MAX_BUDGET);
        let connection = drive(None, deadline, cancel, builder.build())
            .map_err(Refusal::from)?
            .map_err(|_| Refusal::Connection)?;
        let executor = connection.executor().clone();
        Ok(Self {
            connection: Some(connection),
            executor,
            closing: false,
        })
    }

    /// A fresh read of the authenticated bus ID and exact unique daemon owner.
    /// Absence is an explicit pre-call refusal; this never activates a daemon.
    pub fn namespace(
        &mut self,
        budget: Duration,
        cancel: &AtomicBool,
    ) -> Result<Namespace, Refusal> {
        self.lookup(deadline(budget)?, cancel)
    }

    pub fn post(
        &mut self,
        expected: &Namespace,
        title: &str,
        body: &str,
        budget: Duration,
        cancel: &AtomicBool,
    ) -> Outcome<NativeId> {
        let deadline = match deadline(budget) {
            Ok(deadline) => deadline,
            Err(error) => return Outcome::Refused(error),
        };
        if title.len() > TEXT_LIMIT || body.len() > TEXT_LIMIT {
            return Outcome::Refused(Refusal::TextLimit);
        }
        if let Err(error) = self.preflight(expected, deadline, cancel) {
            return Outcome::Refused(error);
        }
        let hints = BTreeMap::from([
            ("desktop-entry", Value::from("hgs-tray")),
            ("urgency", Value::from(1_u8)),
            ("category", Value::from("im.received")),
        ]);
        let escaped = escape_body(body);
        let payload = (
            "Zerus",
            0_u32,
            "hgs-zerus-swarm-symbolic",
            title,
            escaped.as_str(),
            vec!["default", "Open session", "open-session", "Open session"],
            hints,
            -1_i32,
        );
        let connection = self.connection.as_ref().unwrap();
        let reply = self.method(
            expected,
            deadline,
            cancel,
            connection.call_method(
                Some(expected.owner.as_str()),
                PATH,
                Some(SERVICE),
                "Notify",
                &payload,
            ),
        );
        match reply {
            Outcome::Delivered(message) => match message.body().deserialize::<u32>() {
                Ok(id) if id > 0 => Outcome::Delivered(NativeId {
                    namespace: expected.clone(),
                    id: NonZeroU32::new(id).unwrap(),
                }),
                _ => Outcome::Unknown(Unknown::Protocol),
            },
            Outcome::Unknown(error) => Outcome::Unknown(error),
            Outcome::Refused(error) => Outcome::Refused(error),
        }
    }

    /// G1 must durably tombstone activation before proposing this exact close.
    pub fn withdraw(
        &mut self,
        native: &NativeId,
        budget: Duration,
        cancel: &AtomicBool,
    ) -> Outcome<()> {
        let deadline = match deadline(budget) {
            Ok(deadline) => deadline,
            Err(error) => return Outcome::Refused(error),
        };
        if let Err(error) = self.preflight(&native.namespace, deadline, cancel) {
            return Outcome::Refused(error);
        }
        let connection = self.connection.as_ref().unwrap();
        let reply = self.method(
            &native.namespace,
            deadline,
            cancel,
            connection.call_method(
                Some(native.namespace.owner.as_str()),
                PATH,
                Some(SERVICE),
                "CloseNotification",
                &(native.id.get(),),
            ),
        );
        match reply {
            Outcome::Delivered(message) => match message.body().deserialize::<()>() {
                Ok(()) => Outcome::Delivered(()),
                Err(_) => Outcome::Unknown(Unknown::Protocol),
            },
            Outcome::Unknown(error) => Outcome::Unknown(error),
            Outcome::Refused(error) => Outcome::Refused(error),
        }
    }

    /// A bounded shutdown attempt. Pending keeps this transport and its executor
    /// owned; the caller must retain admission and call again. No timed join or
    /// close return is treated as drain. Drained means the connection is gone and
    /// its owned executor has no unfinished reader/connection tasks.
    pub fn shutdown(&mut self, budget: Duration) -> Drain {
        self.closing = true;
        if self.drained() {
            return Drain::Drained;
        }
        if outside_tokio().is_err() {
            return Drain::Pending;
        }
        let Ok(deadline) = deadline(budget) else {
            return Drain::Pending;
        };
        let cancel = AtomicBool::new(false);
        if let Some(connection) = &self.connection {
            let result = drive(
                Some(&self.executor),
                deadline,
                &cancel,
                connection.clone().close(),
            );
            if !matches!(result, Ok(Ok(()))) && !connection.is_closed() {
                return Drain::Pending;
            }
            self.connection.take();
        }
        let finished = async {
            while !self.executor.is_empty() {
                async_io::Timer::after(POLL).await;
            }
        };
        let _ = drive(Some(&self.executor), deadline, &cancel, finished);
        if self.drained() {
            Drain::Drained
        } else {
            Drain::Pending
        }
    }

    fn drained(&self) -> bool {
        self.connection.is_none() && self.executor.is_empty()
    }

    fn preflight(
        &self,
        expected: &Namespace,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<(), Refusal> {
        if self.lookup(deadline, cancel)? != *expected {
            return Err(Refusal::NamespaceChanged);
        }
        before(cancel, deadline)
    }

    fn lookup(&self, deadline: Instant, cancel: &AtomicBool) -> Result<Namespace, Refusal> {
        outside_tokio()?;
        before(cancel, deadline)?;
        let connection = self
            .connection
            .as_ref()
            .filter(|_| !self.closing)
            .ok_or(Refusal::Closed)?;
        let read = async {
            let bus = connection
                .call_method(Some(BUS), BUS_PATH, Some(BUS), "GetId", &())
                .await
                .map_err(|_| Refusal::Lookup)?;
            if bus.header().sender().map(|name| name.as_str()) != Some(BUS) {
                return Err(Refusal::Lookup);
            }
            let bus_id: String = bus.body().deserialize().map_err(|_| Refusal::Lookup)?;
            if bus_id.len() != 32 || !bus_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Refusal::Lookup);
            }
            let owner = connection.call_method(Some(BUS), BUS_PATH, Some(BUS), "GetNameOwner", &(SERVICE,)).await.map_err(|error| {
                if matches!(error, zbus::Error::MethodError(ref name, _, _) if name.as_str() == "org.freedesktop.DBus.Error.NameHasNoOwner") { Refusal::Unavailable } else { Refusal::Lookup }
            })?;
            if owner.header().sender().map(|name| name.as_str()) != Some(BUS) {
                return Err(Refusal::Lookup);
            }
            let owner: String = owner.body().deserialize().map_err(|_| Refusal::Lookup)?;
            zbus::names::UniqueName::try_from(owner.as_str()).map_err(|_| Refusal::Lookup)?;
            Ok(Namespace { bus_id, owner })
        };
        drive(Some(&self.executor), deadline, cancel, read).map_err(Refusal::from)?
    }

    fn method(
        &self,
        expected: &Namespace,
        deadline: Instant,
        cancel: &AtomicBool,
        call: impl Future<Output = zbus::Result<Message>>,
    ) -> Outcome<Message> {
        if let Err(error) = outside_tokio() {
            return Outcome::Refused(error);
        }
        if let Err(error) = before(cancel, deadline) {
            return Outcome::Refused(error);
        }
        // Entering this future is the uncertain-effect boundary. No error beyond
        // it, including an explicit daemon MethodError, grants a retry.
        match drive(Some(&self.executor), deadline, cancel, call) {
            Ok(Ok(reply))
                if reply.header().sender().map(|name| name.as_str())
                    == Some(expected.owner.as_str()) =>
            {
                Outcome::Delivered(reply)
            }
            Ok(Ok(_)) => Outcome::Unknown(Unknown::Protocol),
            Ok(Err(_)) => Outcome::Unknown(Unknown::Transport),
            Err(Interrupted::Cancelled) => Outcome::Unknown(Unknown::Cancelled),
            Err(Interrupted::Deadline) => Outcome::Unknown(Unknown::Deadline),
        }
    }
}

#[derive(Clone, Copy)]
enum Interrupted {
    Cancelled,
    Deadline,
}
impl From<Interrupted> for Refusal {
    fn from(value: Interrupted) -> Self {
        match value {
            Interrupted::Cancelled => Self::Cancelled,
            Interrupted::Deadline => Self::Deadline,
        }
    }
}
fn outside_tokio() -> Result<(), Refusal> {
    if tokio::runtime::Handle::try_current().is_ok() {
        Err(Refusal::Runtime)
    } else {
        Ok(())
    }
}
fn deadline(budget: Duration) -> Result<Instant, Refusal> {
    if budget.is_zero() || budget > MAX_BUDGET {
        return Err(Refusal::InvalidBudget);
    }
    Ok(Instant::now() + budget)
}
fn before(cancel: &AtomicBool, deadline: Instant) -> Result<(), Refusal> {
    if cancel.load(Ordering::Acquire) {
        Err(Refusal::Cancelled)
    } else if Instant::now() >= deadline {
        Err(Refusal::Deadline)
    } else {
        Ok(())
    }
}
fn drive<T>(
    executor: Option<&Executor<'static>>,
    deadline: Instant,
    cancel: &AtomicBool,
    work: impl Future<Output = T>,
) -> Result<T, Interrupted> {
    if cancel.load(Ordering::Acquire) {
        return Err(Interrupted::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(Interrupted::Deadline);
    }
    let monitor = async {
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(Interrupted::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Interrupted::Deadline);
            }
            let timer = async {
                async_io::Timer::at(deadline.min(Instant::now() + POLL)).await;
            };
            let tick = async {
                match executor {
                    Some(executor) => executor.tick().await,
                    None => std::future::pending::<()>().await,
                }
            };
            future::or(timer, tick).await;
        }
    };
    future::block_on(future::or(async { Ok(work.await) }, monitor))
}

#[cfg(test)]
mod tests;
