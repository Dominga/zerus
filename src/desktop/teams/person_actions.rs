//! Native Person actions and inert, bounded preparation data.
pub(crate) mod kernel;
mod plans;

pub(crate) fn required() -> zerus_team_plan::Problem {
    zerus_team_plan::Problem::new(
        "person_gesture_required",
        "Perform this team action in the Zerus window after native confirmation.",
    )
}
