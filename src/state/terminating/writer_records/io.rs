//! Inactive bridge from the original durable row to its actual helper I/O.
//! The future host still verifies its original request/source and guarded spawn;
//! no executable, PID, input permission or operation result is selected here.
//! This does not replace Root's process-group runner or its cleanup semantics.
//! The host must still retain cross-process inherited crash custody and account
//! for any writer/descendant outside these pipes before retiring its row. This
//! leaf installs no such controller or cross-process completion protocol.
use super::*;
use std::process::Child;
use zerus_client_core::run::writer_custody::{Admission, Exchange, Failure, Report};

/// Reserved before the host's original guarded spawn. A refusal returns the
/// SAME reservation; only a definite zero-effect host may retire it. There is
/// no retry, new UUID, storage initialization or adoption inside this bridge.
pub(super) struct IoAdmission {
    original: Reservation,
    admitted: Admission,
}
impl IoAdmission {
    pub(super) fn reserve(
        original: Reservation,
    ) -> std::result::Result<Self, (Box<Reservation>, Failure)> {
        Self::reserve_holding(original, None)
    }

    /// Keep an additional original source companion in the SAME Core charge,
    /// including retirement after Drop. This data supplies no input authority.
    pub(super) fn reserve_holding(
        original: Reservation,
        companion: Option<Arc<dyn Send + Sync>>,
    ) -> std::result::Result<Self, (Box<Reservation>, Failure)> {
        if Instant::now() >= original.until {
            return Err((Box::new(original), Failure::Expired));
        }
        let source = match original.resources() {
            Ok(source) => source,
            Err(_) => return Err((Box::new(original), Failure::SourceChanged)),
        };
        let source: Arc<dyn Send + Sync> = match companion {
            Some(companion) => Arc::new((source, companion)),
            None => source,
        };
        match Admission::reserve(source, original.until) {
            Ok(admitted) => Ok(Self { original, admitted }),
            Err(reason) => Err((Box::new(original), reason)),
        }
    }

    /// Consume only the SAME Child from that host's guarded spawn. The input
    /// stays withheld until the original row and host's native/source check.
    pub(super) fn attach(self, child: Child) -> IoExchange {
        IoExchange {
            original: Some(self.original),
            exchange: self.admitted.attach(child),
        }
    }

    /// Guarded spawn explicitly refused before ANY input. The underlying
    /// constructor is permanently failed and can never release private input.
    pub(super) fn attach_withheld(self, child: Child) -> IoExchange {
        IoExchange {
            original: Some(self.original),
            exchange: self.admitted.attach_withheld(child),
        }
    }
}

/// Dropping this bridge keeps the durable row. The underlying exchange retains
/// the original Child/pipes/workers AND the same borrowed native lease during
/// retirement, even when this local owner and its main lease have gone away.
pub(super) struct IoExchange {
    original: Option<Reservation>,
    exchange: Exchange,
}
impl IoExchange {
    pub(super) fn send(
        &mut self,
        input: Vec<u8>,
        check: impl FnOnce(&mut Child, Instant) -> bool,
    ) -> std::result::Result<(), Failure> {
        let original = self.original.as_ref().ok_or(Failure::Invalid)?;
        self.exchange.send(input, |child, until| {
            original.resources().is_ok() && check(child, until) && original.resources().is_ok()
        })
    }

    /// Only for an already guarded empty-input helper, not zero-input proof
    /// for a failed write. This preserves the original row/source check.
    pub(super) fn close_empty_input(
        &mut self,
        check: impl FnOnce(&mut Child, Instant) -> bool,
    ) -> std::result::Result<(), Failure> {
        let original = self.original.as_ref().ok_or(Failure::Invalid)?;
        self.exchange.close_empty_input(|child, until| {
            original.resources().is_ok() && check(child, until) && original.resources().is_ok()
        })
    }

    /// Stops only the underlying owned helper. This is not settlement, an
    /// agent signal, a group stop or an operation/cleanup authorization.
    pub(super) fn cancel(&mut self) {
        self.exchange.cancel();
    }

    pub(super) fn failure(&self) -> Option<Failure> {
        self.exchange.failure()
    }

    /// A physical report is exposed only after the underlying SAME Child's
    /// wait, stdin closure/join and both actual output EOFs/joins. The exact
    /// reservation remains private until then, so an adapter cannot retire
    /// this original row merely because the helper exited or a cap elapsed.
    pub(super) fn poll(&mut self) -> Option<Settled> {
        let report = self.exchange.poll()?;
        Some(Settled {
            original: self.original.take()?,
            report,
        })
    }
}

/// Physical custody data only. A failed or successful helper report neither
/// confirms the native operation nor authorizes replay, Terminate or cleanup.
/// Lost/drop custody never produces this value or retires the pending row.
pub(super) struct Settled {
    original: Reservation,
    report: Report,
}
impl Settled {
    pub(super) fn report(&self) -> &Report {
        &self.report
    }

    /// Retire only this original physical writer row, with the existing live
    /// owner, exact row and remaining-loan checks. This bounded journal update
    /// does not change the captured helper deadline or any operation receipt.
    /// The host must first prove settlement of any additional cross-process
    /// writer custody; this token alone covers only the original exchange.
    pub(super) fn retire(&mut self, until: Instant) -> Result<()> {
        self.original.retire(until)
    }
}

#[cfg(test)]
mod tests;
