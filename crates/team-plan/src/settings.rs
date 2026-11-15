//! The settings bounds shared by the store and desktop pre-send check.
use crate::problem::code;
use crate::{Acceptance, Problem, Settings};

pub fn validate_settings(settings: &Settings) -> Result<(), Problem> {
    if settings.phase_acceptance == Acceptance::Pipeline {
        return Err(Problem::new(
            code::INVALID_STATE,
            "A phase cannot be accepted by a pipeline.",
        ));
    }
    if settings.caps.max_members == 0
        || settings.caps.max_members > 200
        || settings.caps.max_open_tasks == 0
        || settings.caps.max_open_tasks > 500
        || settings.caps.max_phase_tasks == 0
        || settings.caps.max_phase_tasks > 200
        || settings.caps.max_open_questions == 0
        || settings.caps.max_open_questions > 200
    {
        return Err(Problem::new(
            code::OVER_CAP,
            "Settings exceed the supported storage bounds.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_cap_boundaries_use_the_same_storage_rule() {
        for key in 0..4 {
            for value in [0, 1, 200, 201, 500, 501] {
                let mut settings = Settings::default();
                let cap = match key {
                    0 => &mut settings.caps.max_members,
                    1 => &mut settings.caps.max_phase_tasks,
                    2 => &mut settings.caps.max_open_tasks,
                    _ => &mut settings.caps.max_open_questions,
                };
                *cap = value;
                let maximum = if key == 2 { 500 } else { 200 };
                let result = validate_settings(&settings);
                assert_eq!(result.is_ok(), (1..=maximum).contains(&value));
                if let Err(problem) = result {
                    assert_eq!(problem.code, code::OVER_CAP);
                    assert_eq!(
                        problem.message,
                        "Settings exceed the supported storage bounds."
                    );
                }
            }
        }
    }

    #[test]
    fn a_phase_requires_a_person_or_lead_and_timers_keep_u32_semantics() {
        let mut settings = Settings::default();
        settings.timers.report_timeout_seconds = 0;
        settings.timers.resume_limit_per_hour = u32::MAX;
        for acceptance in [Acceptance::Lead, Acceptance::Person, Acceptance::Pipeline] {
            settings.phase_acceptance = acceptance;
            assert_eq!(
                validate_settings(&settings).is_ok(),
                acceptance != Acceptance::Pipeline
            );
        }
    }
}
