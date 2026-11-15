//! Export only the already displayed bounded page. No CLI read or path here.
use super::read::{valid_team, CURSOR_BYTES, PAGE};
use super::recorded_timings::{valid_view, RecordedFacts, TimingSnapshot};
use super::timeline::RecordedCoverage;
use super::view::{NAME_CHARS, TEXT_CHARS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Write};

pub const EXPORT_BYTES: usize = 8 << 20;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: u8,
    kind: String,
    source: String,
    scope: String,
    computer: Option<String>,
    phase: Option<u32>,
    history_complete: bool,
    team: String,
    after: Option<String>,
    next: Option<String>,
    timeline: RecordedCoverage,
    timings: Option<TimingSnapshot>,
    rows: Vec<Event>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Event {
    seq: u64,
    at_ms: i64,
    kind: String,
    by: String,
    text: String,
    text_truncated: bool,
    timing: Option<RecordedFacts>,
}
fn bounded(s: &str, cap: usize) -> bool {
    s.len() <= cap * 4 && s.chars().count() <= cap
}
fn cursor(value: &Option<String>) -> bool {
    value.as_ref().is_none_or(|s| {
        !s.is_empty()
            && s.len() <= CURSOR_BYTES
            && !s.starts_with('-')
            && !s.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}
struct Limited(Vec<u8>);
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > EXPORT_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other(
                "The recorded-page export exceeds its byte limit.",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn encode(value: Value) -> Result<Vec<u8>, String> {
    if !value["rows"]
        .as_array()
        .is_some_and(|rows| rows.len() <= PAGE as usize)
    {
        return Err("The export is not one bounded journal page.".into());
    }
    let page: Snapshot = serde_json::from_value(value)
        .map_err(|_| "The recorded-page export shape is unsupported.")?;
    let valid_rows = page.rows.iter().all(|row| {
        row.seq > 0
            && row.seq <= 9_007_199_254_740_991
            && row.at_ms.unsigned_abs() <= 9_007_199_254_740_991
            && bounded(&row.kind, NAME_CHARS)
            && bounded(&row.by, NAME_CHARS)
            && bounded(&row.text, TEXT_CHARS)
            && row.timing.as_ref().is_none_or(|facts| {
                valid_view(facts)
                    && facts.effect.as_ref().is_none_or(|e| {
                        e.record.reserved_ms.unsigned_abs() <= 9_007_199_254_740_991
                            && e.record
                                .first_outcome_recorded_ms
                                .is_none_or(|t| t.unsigned_abs() <= 9_007_199_254_740_991)
                    })
                    && facts
                        .stage
                        .as_ref()
                        .is_none_or(|s| s.created_ms.unsigned_abs() <= 9_007_199_254_740_991)
            })
    });
    let covered = &page.timeline;
    if page.schema != 1
        || page.kind != "recorded_team_events"
        || page.source != "local_hgs_displayed_page"
        || page.scope != "journal_page"
        || page.computer.is_some()
        || page.phase.is_some()
        || page.history_complete
        || !valid_team(&page.team)
        || !cursor(&page.after)
        || !cursor(&page.next)
        || !valid_rows
        || page.rows.windows(2).any(|rows| rows[0].seq >= rows[1].seq)
        || covered.count != page.rows.len()
        || covered.limit != PAGE
        || covered.first_seq != page.rows.first().map(|row| row.seq)
        || covered.last_seq != page.rows.last().map(|row| row.seq)
        || covered.earlier_page != page.after.is_some()
        || covered.later_page != page.next.is_some()
        || covered.clock_reversed
            != page
                .rows
                .windows(2)
                .any(|rows| rows[0].at_ms > rows[1].at_ms)
        || page
            .timings
            .as_ref()
            .is_some_and(|v| v.schema != 1 || v.revision == 0 || v.revision > 9_007_199_254_740_991)
        || page
            .rows
            .iter()
            .any(|row| row.timing.is_some() != page.timings.is_some())
    {
        return Err("The export does not match its selected page coverage.".into());
    }
    let mut bytes = Limited(Vec::new());
    serde_json::to_writer_pretty(&mut bytes, &page).map_err(|e| e.to_string())?;
    Ok(bytes.0)
}

#[cfg(test)]
mod tests;
