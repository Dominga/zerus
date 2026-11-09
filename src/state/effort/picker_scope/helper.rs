//! Dormant original-helper custody; no picker controller calls this.
//!
//! The host still supplies its SAME guarded spawn and native/image/receipt
//! checks. No executable, PID, process group, operation or recovery is selected
//! here. An argv-only helper may have acted before it reads stdin, so possible
//! delivery is recorded before the spawn callback, not after an output report.
//!
//! This places cancellation and physical I/O retention inside the current
//! process. It does not establish cross-process crash recovery, inherited
//! custody, a hard callback-return bound or the complete live-writer floor.
use super::*;
use std::{
    process::Child,
    sync::{Arc, Mutex},
};
use zerus_client_core::run::{
    writer_custody::{Admission, Exchange, Failure, Report},
    GuardedChild,
};

type Source = Arc<Mutex<Scope<'static>>>;

/// Capacity and all original resources are reserved BEFORE a helper can exist.
/// Dropping this before spawn creates no child and offers no native input.
pub(super) struct Prepared {
    source: Source,
    admission: Admission,
}
impl Prepared {
    pub(super) fn new(
        writer: Guard,
        expected: &Value,
        receipts: Receipts,
        until: Instant,
    ) -> Result<Self> {
        let scope = Scope::owned(writer, expected, receipts, until)?;
        let source = Arc::new(Mutex::new(scope));
        let admission = Admission::reserve(source.clone(), until)
            .map_err(|failure| format!("Original picker helper custody refused: {failure:?}"))?;
        Ok(Self { source, admission })
    }

    /// The trusted host MUST return the same Child once one exists, including
    /// post-spawn refusals as Withheld. Err means no Child was created. It must
    /// not detach pipes, reap elsewhere, panic after spawn or replace the Child.
    /// A callback must observe this one supplied deadline; no nested renewal.
    ///
    /// Both Ready and Withheld count as possibly effectful: withholding stdin
    /// does not prove that an argv-only command never acted. No error here or
    /// in its physical report supplies a no-start/delivery receipt or retry.
    pub(super) fn spawn(
        self,
        mut native: impl FnMut(&Value) -> Result<()>,
        spawn: impl FnOnce(&Value, Instant) -> std::io::Result<GuardedChild>,
    ) -> Result<Helper> {
        let Self { source, admission } = self;
        let (record, until) = {
            let mut scope = source
                .lock()
                .map_err(|_| "Original picker scope is unavailable")?;
            let record = scope.before(|record| native(record))?;
            // A native argv effect can happen before the trusted spawn returns.
            scope.may_have_sent = true;
            (record, scope.until)
        };
        let child = spawn(&record, until).map_err(|error| error.to_string())?;
        let (mut exchange, withheld) = match child {
            GuardedChild::Ready(child) => (admission.attach(child), false),
            GuardedChild::Withheld(child) => (admission.attach_withheld(child), true),
        };
        if !withheld {
            // This adapter offers no private bytes. Empty EOF follows another
            // fresh native/original barrier; it is never image or input consent.
            let _ = exchange.close_empty_input(|_child: &mut Child, deadline| {
                let Ok(mut scope) = source.lock() else {
                    return false;
                };
                if deadline != scope.until {
                    let _ = scope.retain::<()>(Err("Original picker deadline changed".into()));
                    return false;
                }
                scope.before(|record| native(record)).is_ok()
            });
        }
        Ok(Helper {
            exchange,
            source: Some(source),
            report: None,
        })
    }
}

/// Until physical settlement this exposes no Scope, record commit or new key.
/// Dropping it retires the SAME original exchange; its source charge retains
/// the owned Root writer, input exclusion and pinned objects until actual wait
/// and every original pipe EOF/worker join. Lost wait evidence keeps custody.
pub(super) struct Helper {
    exchange: Exchange,
    source: Option<Source>,
    report: Option<Report>,
}

pub(super) struct Drained {
    pub(super) scope: Scope<'static>,
    /// Physical exchange data only. Existing native result/receipt policy and
    /// the Scope's per-effect and final native barriers still decide authority.
    pub(super) report: Report,
}

impl Helper {
    pub(super) fn cancel(&mut self) {
        self.exchange.cancel();
    }

    /// Provisional failure data while originals may still be draining.
    /// This never grants another operation, a no-start result or a retry.
    pub(super) fn failure(&self) -> Option<Failure> {
        self.exchange.failure()
    }

    /// Never waits for a worker, joins a live thread or substitutes a deadline
    /// for EOF. The original Scope can return only with an actual Report.
    pub(super) fn poll(&mut self) -> Option<Drained> {
        if self.report.is_none() {
            self.report = self.exchange.poll();
        }
        self.report.as_ref()?;
        let source = self.source.take()?;
        let mut scope = match Arc::try_unwrap(source) {
            Ok(source) => match source.into_inner() {
                Ok(scope) => scope,
                Err(poisoned) => {
                    let mut scope = poisoned.into_inner();
                    let _ = scope.retain::<()>(Err("Original picker scope is unavailable".into()));
                    scope
                }
            },
            Err(source) => {
                // No external source Arc is exposed. If original custody still
                // holds one, retain it and its real report rather than release.
                self.source = Some(source);
                return None;
            }
        };
        let report = self.report.take()?;
        if report.failure.is_some() || !report.exit.success() || !report.input_written {
            let _ = scope.retain::<()>(Err("The original picker helper did not complete".into()));
        }
        let current = scope.current().map(drop);
        let _ = scope.retain(current);
        Some(Drained { scope, report })
    }
}
