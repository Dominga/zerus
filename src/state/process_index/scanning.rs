//! Bounded background scan controller. Data coverage stays explicitly partial.
use super::*;
pub(crate) const STEP_ENTRIES: usize = 256;
pub(crate) const STEP_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const STEP_TIME: Duration = Duration::from_millis(50);
pub(crate) enum Read {
    Entry { bytes: usize, row: Option<Snapshot> },
    Yield,
    End,
}
pub(crate) trait Source {
    /// At most one directory entry and at most `bytes` of source I/O. Hold an
    /// oversized-for-this-step entry until the next step; do not skip it.
    fn next(&mut self, store: &mut Store, bytes: usize, until: Instant) -> Result<Read>;
}
#[derive(Default, Debug)]
pub(crate) struct Step {
    pub entries: usize,
    pub bytes: usize,
    pub rows: usize,
    pub done: bool,
    pub yielded: bool,
}
pub(crate) struct Scan<S> {
    pub source: S,
    pub build: Build,
    page: Vec<Snapshot>,
    page_bytes: usize,
    pending: Option<Snapshot>,
    cursor: u64,
    ended: bool,
    done: bool,
}
impl<S: Source> Scan<S> {
    pub(crate) fn new(store: &mut Store, source: S) -> Result<Self> {
        Ok(Self {
            source,
            build: store.rebuild()?,
            page: Vec::new(),
            page_bytes: 0,
            pending: None,
            cursor: 0,
            ended: false,
            done: false,
        })
    }
    pub(crate) fn step(
        &mut self,
        store: &mut Store,
        mut now: impl FnMut() -> Instant,
    ) -> Result<Step> {
        let until = now() + STEP_TIME;
        self.step_until(store, now, until)
    }
    pub(crate) fn step_until(
        &mut self,
        store: &mut Store,
        mut now: impl FnMut() -> Instant,
        until: Instant,
    ) -> Result<Step> {
        let mut step = Step::default();
        if self.done {
            step.done = true;
            return Ok(step);
        }
        while step.entries < STEP_ENTRIES
            && step.bytes < STEP_BYTES
            && self.page.len() < PAGE_ROWS
            && now() < until
        {
            let row = if let Some(row) = self.pending.take() {
                Some(row)
            } else {
                if self.ended {
                    break;
                }
                match self.source.next(store, STEP_BYTES - step.bytes, until)? {
                    Read::Entry { bytes, row } => {
                        ensure!(
                            bytes <= STEP_BYTES - step.bytes,
                            "Scan source exceeded its I/O allowance"
                        );
                        step.entries += 1;
                        step.bytes += bytes;
                        row
                    }
                    Read::Yield => {
                        step.yielded = true;
                        break;
                    }
                    Read::End => {
                        self.ended = true;
                        break;
                    }
                }
            };
            if let Some(row) = row {
                row.validate()?;
                let bytes = serde_json::to_vec(&row)?.len();
                ensure!(bytes <= PAGE_BYTES, "Scan snapshot exceeds its page");
                if self.page_bytes + bytes > PAGE_BYTES {
                    self.pending = Some(row);
                    break;
                }
                self.page_bytes += bytes;
                self.page.push(row);
            }
        }
        // A blocking read may overrun its admission deadline. Retain at most
        // one bounded page and defer every new SQLite operation to a fresh step.
        if !self.page.is_empty() && now() < until {
            self.cursor = self.cursor.checked_add(1).context("Scan cursor overflow")?;
            store.seed(
                &self.build,
                &format!("page-{:020}", self.cursor),
                &self.page,
            )?;
            step.rows = self.page.len();
            self.page.clear();
            self.page_bytes = 0;
        }
        if self.ended && self.pending.is_none() && self.page.is_empty() && now() < until {
            store.scan_done(&self.build)?;
            self.done = true;
        }
        step.done = self.done;
        Ok(step)
    }
}

#[cfg(test)]
mod tests;
