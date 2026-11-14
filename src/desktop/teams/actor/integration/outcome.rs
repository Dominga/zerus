//! Reserve the entire possible receipt before Git; a certain large conflict
//! must fit both the private ledger row and the store's 200-entry fact.
use super::*;
const MAX_ROW: usize = 16_384;
fn size(row: &MergeRow) -> Result<usize> {
    serde_json::to_vec(row)
        .map(|v| v.len())
        .map_err(crate::teams::store::storage)
}
fn future(row: &MergeRow, outcome: MergeResult) -> MergeRow {
    let mut next = row.clone();
    next.padding.clear();
    next.outcome = Some(outcome);
    next.fact_id = Some("00000000-0000-4000-8000-000000000001".into());
    next.fact_revision = Some(u64::MAX);
    next
}
pub(super) fn reserve(row: &mut MergeRow) -> Result<()> {
    let worst = future(
        row,
        MergeResult::Unknown {
            reason: "x".repeat(2048),
        },
    );
    if size(&worst)? > MAX_ROW {
        return Err(problem("Integration plan leaves no room for its outcome"));
    }
    // Charge the complete future receipt, not only the small initial intent.
    row.padding = "x".into();
    row.padding = " ".repeat(MAX_ROW - size(row)? + 1);
    Ok(())
}
fn conflicts(row: &MergeRow, files: Vec<String>, total: usize) -> Result<Vec<String>> {
    let mut kept = Vec::new();
    for name in files.into_iter().take(199) {
        kept.push(render_path(&name));
        let mut candidate = kept.clone();
        if total > kept.len() {
            candidate.push(format!("and {} more files", total - kept.len()));
        }
        if size(&future(row, MergeResult::Conflict { files: candidate }))? > MAX_ROW {
            kept.pop();
            break;
        }
    }
    if total > kept.len() {
        kept.push(format!("and {} more files", total - kept.len()));
    }
    Ok(kept)
}
pub(super) fn save(row: &mut MergeRow, reply: MergeResult) -> Result<()> {
    row.padding.clear();
    row.outcome = Some(match reply {
        MergeResult::Conflict { files } => MergeResult::Conflict {
            files: {
                let total = files.len();
                conflicts(row, files, total)?
            },
        },
        MergeResult::Unknown { reason } => MergeResult::Unknown {
            reason: cut(&reason, 2048),
        },
        MergeResult::Unavailable { reason } => MergeResult::Unavailable {
            reason: cut(&reason, 2048),
        },
        merged => merged,
    });
    Ok(())
}

fn render_path(name: &str) -> String {
    crate::teams::clean::path(name, 4096)
}
pub(super) fn omit_refused_names(row: &mut MergeRow) -> Result<bool> {
    let Some(MergeResult::Conflict { files }) = &row.outcome else {
        return Ok(false);
    };
    let total = files.len().max(1);
    let mut good: Vec<String> = files
        .iter()
        .filter(|s| !s.trim().is_empty() && !s.chars().any(char::is_control) && s.len() <= 4096)
        .cloned()
        .collect();
    if good.len() == files.len() {
        good.clear();
    }
    let kept = conflicts(row, good, total)?;
    if &kept == files {
        return Ok(false);
    }
    row.outcome = Some(MergeResult::Conflict { files: kept });
    row.fact_id = None;
    row.fact_revision = None;
    Ok(true)
}
