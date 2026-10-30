//! Optional raw older-peer mode of the existing supervisor. No client
//! projection, whole-listing cache, independent poller or effect authority.
use super::*;
use crate::{
    actions::Invocation,
    legacy::{self, listing, spool},
    run,
    source::Legacy,
};

impl Supervisor {
    pub(super) fn poll_source_legacy(&mut self, time: Duration) -> bool {
        let deadline = Instant::now() + time;
        let follows = legacy::Follows {
            activity: &[],
            processes: &[],
        };
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return false;
            }
            let now = Instant::now();
            if now >= deadline {
                self.source.as_ref().unwrap().resnapshot.close();
                return true;
            }
            let resnapshot = self
                .source
                .as_ref()
                .unwrap()
                .resnapshot
                .take(self.generation);
            let retry = self.retry.requested();
            let schedule = self
                .legacy
                .get_or_insert_with(|| legacy::Schedule::new(self.timing.legacy));
            if resnapshot || retry {
                self.retry.begin();
                schedule.refresh_listing_bounded(now);
            }
            let reads = schedule.due(now, &follows);
            if schedule.listing_retryable() {
                self.retry.waiting();
            } else {
                self.retry.begin();
            }
            for _ in reads {
                self.retry.begin();
                let ok = self.source_listing();
                let schedule = self.legacy.as_mut().unwrap();
                schedule.done(&legacy::Read::Listing, Instant::now());
                let (attempt, _) = schedule.listed(ok);
                if self.stop.load(Ordering::Relaxed) {
                    return false;
                }
                self.status(if ok {
                    ComputerStatus::Polled
                } else {
                    ComputerStatus::Reconnecting {
                        attempt: attempt.unwrap_or(1),
                    }
                });
            }
            let wake = self
                .legacy
                .as_ref()
                .and_then(legacy::Schedule::next)
                .map_or(deadline, |at| at.min(deadline));
            let left = wake
                .saturating_duration_since(Instant::now())
                .clamp(Duration::from_millis(1), Duration::from_millis(50));
            match self.rx.recv_timeout(left) {
                Ok(Input::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
                // Source mode has no GUI subscriptions or notifications. The
                // existing retry/resnapshot flags are checked on every turn.
                _ => {}
            }
        }
    }

    fn source_listing(&mut self) -> bool {
        let source = self.source.as_ref().unwrap().clone();
        source.resnapshot.close();
        let config = source.legacy_config().unwrap();
        // Refuse a retained pipe before creating another file/decoder for the
        // same alias. Reader ownership covers both pipes until their EOFs.
        let Some(reader) = source.reader() else {
            return false;
        };
        let reader = Arc::new(reader);
        let admission_until = Instant::now() + legacy::LISTING_TIMEOUT;
        let reservation = loop {
            if self.stop.load(Ordering::Relaxed) || Instant::now() >= admission_until {
                return false;
            }
            if let Some(slot) = config.pool.reserve() {
                break slot;
            }
            thread::sleep(Duration::from_millis(50));
        };
        self.connection = source.connect(&self.stop);
        if self.connection.is_none() {
            return false;
        }
        let result = self.stage_source_listing(&source, &config, reader, reservation);
        // Finished children do not occupy connection admission while replay
        // waits on consumer pressure. The private spool stays charged.
        self.connection = None;
        let Ok(staged) = result else {
            return false;
        };
        let Some(generation) = self.generation.checked_add(1) else {
            self.stop.store(true, Ordering::Relaxed);
            return false;
        };
        self.generation = generation;
        let observed_at = staged.observed_at();
        let until = Instant::now() + legacy::LISTING_TIMEOUT;
        let listing = staged.listing();
        source.resnapshot.open(generation);
        if !source.legacy(
            generation,
            Legacy::Begin {
                metadata: listing.metadata.clone(),
                rows: listing.rows,
                pages: listing.pages,
                input_bytes: listing.bytes,
            },
            observed_at,
            &self.stop,
            until,
        ) {
            source.resnapshot.close();
            return false;
        }
        let mut index = 0;
        let replay = staged.replay(until, &self.stop, |items| {
            if !source.legacy(
                generation,
                Legacy::Page {
                    index,
                    items: items.to_vec(),
                },
                observed_at,
                &self.stop,
                until,
            ) {
                return Err(io::Error::other("legacy source ingress refused"));
            }
            index += 1;
            Ok(())
        });
        // Never tag an earlier page as final: late storage/digest/EOF failure
        // must abandon the entire provisional replacement.
        let ok = replay.is_ok()
            && source.legacy(generation, Legacy::Complete, observed_at, &self.stop, until);
        if !ok {
            source.resnapshot.close();
        }
        ok
    }

    fn stage_source_listing(
        &self,
        source: &crate::source::Endpoint,
        config: &crate::source::LegacyConfig,
        reader: Arc<crate::source::Reader>,
        reservation: spool::Reservation,
    ) -> Result<spool::Staged, ()> {
        let storage = (config.factory)().map_err(|_| ())?;
        let observed_at = Instant::now();
        let mut args: Vec<String> = self
            .computer
            .alias()
            .map(|a| format!("@{a}"))
            .into_iter()
            .collect();
        args.extend(legacy::Read::Listing.args());
        let invocation = Invocation {
            args,
            stdin: None,
            timeout: legacy::LISTING_TIMEOUT,
            request_id: None,
        };
        run::run_decoded(
            &self.hgs,
            &invocation,
            &[],
            &self.stop,
            source.spawn_fn(),
            // Retained descendant pipes keep both alias-reader and worker
            // admission, even after this supervisor's bounded stop returns.
            Arc::new((reader, source.clone())),
            move |pipe, stop, until| {
                reservation.stage(
                    storage,
                    pipe,
                    listing::Limits::default(),
                    observed_at,
                    until,
                    stop,
                )
            },
        )
        .map_err(|_| ())
    }
}
