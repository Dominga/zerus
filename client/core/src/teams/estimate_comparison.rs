//! A derived join to one admitted observation page. No history or actual work
//! is inferred, and estimate membership never certifies observation coverage.
use super::{
    estimate_projection as estimates, observed_timing as observed, view::phases::PhaseDraft,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonUnknown {
    EstimateUnavailable,
    OutsideCohort,
    NotImplementation,
    OriginalPublicationUnavailable,
    DifferentPublication,
    DifferentTask,
    PartialCoverage,
    ImplementationUnsupported,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EstimateUnit {
    pub unit: u64,
    pub node: u32,
    pub estimate_minutes: Option<u32>,
    pub coverage: observed::UnitCoverage,
    pub unknown: ComparisonUnknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EstimatePane {
    #[serde(flatten)]
    pub estimates: estimates::PhaseEstimates,
    /// The selected observation lease, not the phase epoch or retirement counter.
    pub epoch: u64,
    pub window: observed::ObservedWindow,
    pub units: Vec<EstimateUnit>,
}

pub(crate) fn project(
    context: &estimates::EstimateContext,
    draft: &PhaseDraft,
    pane: &observed::ObservedPane,
) -> Option<EstimatePane> {
    let page = &pane.page;
    let publication = draft
        .publication
        .as_ref()
        .map(|p| observed::PublicationKey {
            id: p.id.clone(),
            draft_generation: p.generation,
        });
    if page.scope.team != context.team
        || page.scope.home != context.home
        || page.scope.phase != draft.phase.n
        || page.scope.publication != publication
        || page.source_revision != context.revision
        || !observed::validate::page(page)
        || pane.coverage.len() != page.cohort.units.len()
        || pane
            .coverage
            .iter()
            .any(|c| c.implementation_observed_ms.is_some())
    {
        return None;
    }
    let estimates = estimates::project(context, draft, None);
    let preview = estimates
        .scope
        .as_ref()
        .is_some_and(|s| s.basis == estimates::EstimateBasis::UnapprovedPreview);
    let mut units = Vec::new();
    // An unapproved preview has no actual comparison, including to older facts.
    if !preview {
        for unit in &page.cohort.units {
            let coverage = pane
                .coverage
                .iter()
                .find(|c| c.key == unit.key && c.window == page.scope.window)?;
            let row = estimates.rows.iter().find(|r| r.node == unit.key.node);
            let unknown = if !estimates.cohort_complete {
                ComparisonUnknown::EstimateUnavailable
            } else if row.is_none() {
                ComparisonUnknown::OutsideCohort
            } else if unit.key.kind != observed::UnitKind::Implementation {
                ComparisonUnknown::NotImplementation
            } else if unit.key.publication.is_none() {
                ComparisonUnknown::OriginalPublicationUnavailable
            } else if unit.key.publication != publication {
                ComparisonUnknown::DifferentPublication
            } else if row.is_some_and(|r| r.task.as_ref() != Some(&unit.key.task)) {
                ComparisonUnknown::DifferentTask
            } else if row.is_some_and(|r| r.estimate_minutes.is_none()) {
                ComparisonUnknown::EstimateUnavailable
            } else if page.more
                || page.truncated
                || page.cohort.more
                || coverage.coverage != observed::CoverageKind::Sampled
            {
                ComparisonUnknown::PartialCoverage
            } else {
                ComparisonUnknown::ImplementationUnsupported
            };
            units.push(EstimateUnit {
                unit: unit.id,
                node: unit.key.node,
                estimate_minutes: (unknown == ComparisonUnknown::ImplementationUnsupported)
                    .then(|| row.and_then(|r| r.estimate_minutes))
                    .flatten(),
                coverage: coverage.clone(),
                unknown,
            });
        }
    }
    Some(EstimatePane {
        estimates,
        epoch: pane.epoch,
        window: page.scope.window.clone(),
        units,
    })
}

#[cfg(test)]
mod tests;
