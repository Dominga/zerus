//! Shared transition order. Portable controls model the observations; only the
//! native implementation can supply actual Caller/Grant/task/process custody.
use super::*;
use std::{thread, time::Duration};

pub(in crate::desktop) trait Session {
    type Started;
    fn phase(&mut self, phase: Budget) -> Result<()>;
    fn check(&mut self) -> Result<()>;
    fn save(&mut self, record: &Record) -> Result<()>;
    fn hold(&mut self) -> Result<()>;
    fn drained(&mut self) -> Result<bool>;
    fn pause(&mut self, request: &str);
    fn offer_stop(&mut self, record: &mut Record) -> Result<()>;
    fn stop(&mut self) -> Result<()>;
    fn stopped(&mut self) -> Result<bool>;
    fn start(&mut self) -> Result<Self::Started>;
    fn guid(started: &Self::Started) -> &str;
    fn ready(&mut self, started: &mut Self::Started) -> Result<Option<Ready>>;
    fn release(&mut self) -> Result<()>;
}

/// Consumes THIS new reservation once. Reopened originals have no execution
/// entry, even at Prepared; native routing can only look up their own UUID.
pub(in crate::desktop) fn run<S: Session>(
    session: &mut S,
    record: &mut Record,
    reserved: Reserved,
) -> Result<()> {
    ensure!(
        *record == reserved.record,
        "Fresh original refresh reservation changed."
    );
    let request = reserved
        .record
        .pending
        .as_ref()
        .context("Fresh refresh intent absent.")?
        .request_id
        .clone();
    ensure!(
        record
            .pending
            .as_ref()
            .is_some_and(|p| p.request_id == request
                && p.refresh.is_some()
                && p.phase == Phase::Prepared
                && p.review.is_none()),
        "Fresh original refresh reservation changed."
    );
    session.check()?;
    session.phase(Budget::Hold)?;
    session.check()?;
    session.hold()?;
    while !session.drained()? {
        session.check()?;
        thread::sleep(Duration::from_millis(10));
    }
    record.advance(Phase::Prepared, Phase::Held)?;
    session.save(record)?;
    session.check()?;
    session.phase(Budget::Stop)?;
    session.pause(&request);
    session.offer_stop(record)?;
    session.stop()?;
    while !session.stopped()? {
        session.check()?;
        thread::sleep(Duration::from_millis(10));
    }
    record.advance(Phase::StopOffered, Phase::Stopped)?;
    session.save(record)?;
    session.check()?;
    session.phase(Budget::Start)?;
    record.advance(Phase::Stopped, Phase::StartOffered)?;
    session.save(record)?;
    let mut started = session.start()?;
    record.pending.as_mut().expect("reserved").started = Some(S::guid(&started).into());
    session.save(record)?;
    session.check()?;
    session.phase(Budget::Readiness)?;
    let ready = loop {
        session.check()?;
        if let Some(ready) = session.ready(&mut started)? {
            break ready;
        }
        thread::sleep(Duration::from_millis(10));
    };
    record.advance(Phase::StartOffered, Phase::Ready)?;
    session.save(record)?;
    session.check()?;
    session.phase(Budget::Release)?;
    session.release()?;
    session.check()?;
    record.finish_refresh(&request, ready)?;
    session.save(record)
}

#[cfg(all(test, not(windows)))]
mod tests;
