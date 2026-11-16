//! Native Person actions and inert, bounded preparation data.
mod birth;
mod intake;
pub(crate) mod kernel;
#[cfg(all(test, unix))]
pub(crate) mod models;
mod original;
mod plans;
mod prepared;
mod runtime;
pub(in crate::desktop) use runtime::Runtime;

pub(crate) fn required() -> zerus_team_plan::Problem {
    zerus_team_plan::Problem::new(
        "person_gesture_required",
        "Perform this team action in the Zerus window after native confirmation.",
    )
}

/// Read-only seam for the existing Unix real-publication fixtures. It
/// constructs no native admission and grants no Person authority.
#[cfg(all(test, unix))]
pub(crate) fn phase_facts_fixture(
    config: &crate::config::Config,
    db: &rusqlite::Connection,
    plan: &zerus_client_core::desktop::team_person_actions::Plan,
) -> Result<String, zerus_team_plan::Problem> {
    prepared::Prepared::resolve(config, std::sync::Arc::new(plan.clone()))?
        .bound_facts(db, Some(config))
        .map(|(_, facts)| facts)
}
