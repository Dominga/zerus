//! The fixed, dormant original Claude producer's physical helper adapter.
//! No command, input permission, receipt, replay or all-writers proof is minted.
use super::*;
use crate::state::storage;
use serde_json::Value;
use std::{process::Child, time::Duration};
use zerus_client_core::run::writer_custody::{Failure, Report};

impl domain::OriginalDomain {
    /// The caller retains its actual native owner and existing appearance
    /// authority. Hashes are correlation only; this live domain is not rebuilt.
    pub(in crate::state) fn appearance(
        &self,
        writer: &storage::Guard,
        record: &Value,
        request: [&str; 3],
        companion: Arc<dyn Send + Sync>,
        until: Instant,
    ) -> Result<StartupAdmission> {
        self.matches(record)?;
        let [id, source, intent] = request;
        let request = admission::Request::new(id, Kind::Launch, source, intent)?;
        let reserved =
            admission::OriginalRequest::capture(writer, record, request, until)?.reserve(writer)?;
        self.matches(record)?;
        let admission = reserved
            .into_io_holding(writer, Some(companion))
            .map_err(|(_, why)| format!("Original appearance IO admission refused: {why:?}"))?;
        Ok(StartupAdmission(admission))
    }
}

pub(in crate::state) struct StartupAdmission(admission::SourceIoAdmission);
impl StartupAdmission {
    pub(in crate::state) fn attach(self, child: Child) -> StartupExchange {
        StartupExchange(self.0.attach(child))
    }
}
pub(in crate::state) struct StartupExchange(admission::SourceIoExchange);
impl StartupExchange {
    pub(in crate::state) fn close_empty_input(
        &mut self,
        writer: &storage::Guard,
        check: impl FnOnce(&mut Child, Instant) -> bool,
    ) -> std::result::Result<(), Failure> {
        self.0.close_empty_input(writer, check)
    }
    pub(in crate::state) fn cancel(&mut self) {
        self.0.cancel();
    }
    pub(in crate::state) fn poll(&mut self) -> Option<Result<Report>> {
        // This later, bounded ledger update is accounting after the SAME
        // wait/EOF/joins, never a renewal of the helper's effect deadline.
        self.0
            .poll()
            .map(|settled| settled.finish(Instant::now() + Duration::from_secs(1)))
    }
}
