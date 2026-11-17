//! Strict read-only observation projection. No continuous work is measured.
use serde::Serialize;
use serde_json::Value;
pub use zerus_team_plan::observed_timing::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservedPane {
    pub epoch: u64,
    pub page: ObservedTimingPage,
    pub coverage: Vec<UnitCoverage>,
    pub after: Option<String>,
}

pub fn decode(
    value: &Value,
    team: &str,
    phase: u32,
    node: Option<u32>,
    window: u64,
) -> Result<ObservedTimingPage, String> {
    if value.get("ok").and_then(Value::as_bool) != Some(true)
        || value.get("team").and_then(Value::as_str) != Some(team)
    {
        return Err("The observed source scope is unavailable.".into());
    }
    let raw = value
        .get("observed")
        .ok_or("The source has no observation page.")?;
    if serde_json::to_vec(raw).map_err(|e| e.to_string())?.len() > MAX_PAGE_BYTES {
        return Err("The observation page exceeds its bound.".into());
    }
    let page: ObservedTimingPage =
        serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
    if !validate::page(&page)
        || page.scope.team != team
        || page.scope.phase != phase
        || page.scope.node != node
        || page.scope.window.window_ms != window
    {
        return Err("The observation page is unsupported or off scope.".into());
    }
    Ok(page)
}

pub fn project(page: ObservedTimingPage, epoch: u64, after: Option<String>) -> ObservedPane {
    let coverage = page
        .cohort
        .units
        .iter()
        .map(|unit| {
            let rows: Vec<_> = page.rows.iter().filter(|f| f.unit == unit.id).collect();
            let mut reasons = page.reasons.clone();
            if rows.is_empty() {
                reasons.push(Reason::OffPage)
            }
            let mut categories = Vec::new();
            for row in &rows {
                for reason in &row.reasons {
                    if !reasons.contains(reason) {
                        reasons.push(*reason)
                    }
                }
                if !categories.contains(&row.category) {
                    categories.push(row.category)
                }
            }
            // Points never prove continuous duration or a complete implementation
            // window. Gap/clock/restart precedence cannot be filled by task state.
            let unavailable = rows.is_empty()
                || rows.iter().any(|f| {
                    f.category == Category::Gap
                        || f.reasons.iter().any(|r| {
                            matches!(
                                r,
                                Reason::ClockDiscontinuity
                                    | Reason::ObserverRestart
                                    | Reason::SourceUnavailable
                            )
                        })
                });
            UnitCoverage {
                key: unit.key.clone(),
                window: page.scope.window.clone(),
                last_verified_ms: rows.iter().filter_map(|f| f.last_verified_ms).max(),
                coverage: if unavailable {
                    CoverageKind::Unavailable
                } else {
                    CoverageKind::Sampled
                },
                reasons,
                supported_categories: categories,
                implementation_observed_ms: None,
            }
        })
        .collect();
    ObservedPane {
        epoch,
        page,
        coverage,
        after,
    }
}

// The fork keeps upstream's team client unwired (merge 2026-11-17): these
// tests drive it through the client-wide Change pipeline or the removed UI
// fixtures, which carry the fork's own teams.
#[cfg(any())]
mod tests;
