//! Coverage of one recorded journal page, never a work-time measurement.
use super::drawers::DrawerRow;
use super::read::{TeamError, PAGE};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecordedCoverage {
    pub count: usize,
    pub limit: u64,
    pub first_seq: Option<u64>,
    pub last_seq: Option<u64>,
    pub earlier_page: bool,
    pub later_page: bool,
    pub clock_reversed: bool,
}

pub(super) fn coverage(
    rows: &[DrawerRow],
    after: Option<&str>,
    next: Option<&str>,
) -> Result<RecordedCoverage, TeamError> {
    // Sequence order remains authoritative even when the recorded clock goes
    // backwards. Refuse ambiguous sequence identities, never sort by time.
    if rows.len() > PAGE as usize
        || rows
            .iter()
            .any(|row| row.seq == 0 || row.seq > 9_007_199_254_740_991)
        || rows.windows(2).any(|pair| pair[0].seq >= pair[1].seq)
    {
        return Err(TeamError::Invalid {
            message: "Invalid recorded journal order or page size.".into(),
        });
    }
    Ok(RecordedCoverage {
        count: rows.len(),
        limit: PAGE,
        first_seq: rows.first().map(|row| row.seq),
        last_seq: rows.last().map(|row| row.seq),
        earlier_page: after.is_some(),
        later_page: next.is_some(),
        clock_reversed: rows.windows(2).any(|pair| pair[0].at_ms > pair[1].at_ms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Exit;
    use crate::teams::drawers::{decode, DrawerKind, DrawerRead};
    use serde_json::{json, Value};

    fn raw() -> Value {
        serde_json::from_str(include_str!(
            "../../../../crates/team-plan/fixtures/cli-journal.json"
        ))
        .unwrap()
    }
    fn page(
        value: Value,
        after: Option<&str>,
    ) -> Result<super::super::drawers::DrawerPage, TeamError> {
        decode(
            &DrawerRead {
                team: value["team"].as_str().unwrap().into(),
                kind: DrawerKind::Feed,
                after: after.map(str::to_owned),
                recorded_timings: false,
                timing_seqs: None,
                timing_next: None,
            },
            Exit::Code {
                code: 0,
                stdout: serde_json::to_vec(&value).unwrap(),
                stderr: String::new(),
            },
        )
    }

    #[test]
    fn product_written_events_keep_sequence_text_and_recorded_coverage() {
        let raw = raw();
        let page = page(raw.clone(), None).unwrap();
        let covered = page.timeline.unwrap();
        assert_eq!(covered.count, raw["rows"].as_array().unwrap().len());
        assert_eq!(covered.first_seq, page.rows.first().map(|row| row.seq));
        assert_eq!(covered.last_seq, page.rows.last().map(|row| row.seq));
        assert_eq!(covered.limit, 200);
        assert!(!covered.earlier_page && !covered.later_page && !covered.clock_reversed);
        for (actual, original) in page.rows.iter().zip(raw["rows"].as_array().unwrap()) {
            assert_eq!(actual.seq, original["seq"]);
            assert_eq!(actual.at_ms, original["at_ms"]);
            assert_eq!(actual.text, original["text"]);
        }
    }

    #[test]
    fn handwritten_clock_reversal_keeps_sequence_and_both_cursor_directions() {
        let mut raw = raw();
        raw["rows"][1]["at_ms"] = json!(0);
        raw["next"] = json!("t1.original-ceiling");
        let page = page(raw, Some("t1.original-after")).unwrap();
        assert_eq!(page.after.as_deref(), Some("t1.original-after"));
        assert_eq!(page.next.as_deref(), Some("t1.original-ceiling"));
        assert_eq!(page.rows[0].seq, 1);
        assert_eq!(page.rows[1].at_ms, 0);
        let covered = page.timeline.unwrap();
        assert!(covered.clock_reversed && covered.earlier_page && covered.later_page);
    }

    #[test]
    fn handwritten_ambiguous_order_and_over_cap_are_refused() {
        for seq in [0_u64, 1, 9_007_199_254_740_992] {
            let mut raw = raw();
            raw["rows"][1]["seq"] = json!(seq);
            assert!(page(raw, None).is_err());
        }
        let mut raw = raw();
        raw["rows"] = json!(vec![raw["rows"][0].clone(); 201]);
        assert!(page(raw, None).is_err());
    }

    #[test]
    fn handwritten_empty_last_page_is_not_a_whole_team_claim() {
        let mut raw = raw();
        raw["rows"] = json!([]);
        let page = page(raw, Some("t1.last")).unwrap();
        let covered = page.timeline.unwrap();
        assert_eq!(covered.first_seq, None);
        assert_eq!(covered.last_seq, None);
        assert_eq!(covered.count, 0);
        assert!(covered.earlier_page);
        assert!(!covered.later_page);
    }
}
