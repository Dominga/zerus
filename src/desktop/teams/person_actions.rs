//! Native Person actions and inert, bounded preparation data.
pub(crate) mod kernel;
mod birth;
mod original;
mod intake;
mod plans;
mod prepared;
mod runtime;
#[cfg(all(test, unix))]
pub(crate) mod models;
pub(in crate::desktop) use runtime::Runtime;

pub(crate) fn required() -> zerus_team_plan::Problem {
    zerus_team_plan::Problem::new(
        "person_gesture_required",
        "Perform this team action in the Zerus window after native confirmation.",
    )
}
