//! Real decoded producer contexts with synthetic failure/retirement scheduling.
use super::*;
use crate::{
    actions::Exit,
    teams::{observed_timing::DEFAULT_WINDOW_MS, read::decode},
};
fn source() -> Value {
    serde_json::from_str(include_str!(
        "../../../../../../crates/team-plan/fixtures/cli-observed-phase.json"
    ))
    .unwrap()
}
fn checked(read: &TeamRead, value: Value) -> Result<Value, TeamError> {
    decode(
        read,
        Exit::Code {
            code: 0,
            stdout: serde_json::to_vec(&value).unwrap(),
            stderr: String::new(),
        },
    )
}
fn ready() -> (TeamsModel, Value, Instant) {
    let v = source();
    let now = Instant::now();
    let team = v["team"].as_str().unwrap();
    let mut m = TeamsModel::default();
    m.follow(
        TeamsFollow {
            team: Some(team.into()),
            ..Default::default()
        },
        now,
    );
    for k in ["status", "queue"] {
        let r = m.next(now).unwrap();
        m.apply(&r, checked(&r, v["context"][k].clone()), now);
    }
    m.follow_phases(team, Some(1), true, now).unwrap();
    for k in ["phases", "draft"] {
        let r = m.next(now).unwrap();
        m.apply(&r, checked(&r, v["context"][k].clone()), now);
    }
    m.follow_observed(team, 1, None, DEFAULT_WINDOW_MS, true, None, now)
        .unwrap();
    let r = m.next(now).unwrap();
    m.apply(&r, checked(&r, v.clone()), now);
    assert!(m.estimates().is_some());
    (m, v, now)
}
#[test]
fn window_phase_team_and_disable_retirement_fence_late_estimate_replies() {
    for mode in 0..4 {
        let (mut m, v, now) = ready();
        let team = v["team"].as_str().unwrap();
        assert!(m.page(&team_key("observed", team), None));
        let late = m.next(now).unwrap();
        match mode {
            0 => {
                m.follow_observed(team, 1, None, DEFAULT_WINDOW_MS / 2, true, None, now)
                    .unwrap();
            }
            1 => {
                m.follow_phases(team, Some(2), true, now).unwrap();
            }
            2 => {
                m.follow(
                    TeamsFollow {
                        team: Some("00000000-0000-4000-8000-00000000abcd".into()),
                        ..Default::default()
                    },
                    now,
                );
            }
            _ => {
                m.follow_observed(
                    team,
                    1,
                    None,
                    DEFAULT_WINDOW_MS,
                    false,
                    Some(m.observed_lease),
                    now,
                )
                .unwrap();
            }
        }
        assert!(m.estimates().is_none());
        m.apply(&late, checked(&late, v), now);
        assert!(m.estimates().is_none());
    }
}
#[test]
fn source_failure_and_a_different_selected_lease_drop_eligibility() {
    let (mut m, _, _) = ready();
    m.observed_lease += 1;
    assert!(m.estimates().is_none());
    let (mut m, _, now2) = ready();
    m.refresh(now2);
    let r = m.next(now2).unwrap();
    m.apply(
        &r,
        Err(TeamError::Failed {
            message: "Synthetic source failure".into(),
        }),
        now2,
    );
    assert!(m.estimates().is_none() && m.observed_page.is_none());
}
#[test]
fn changed_publication_or_generation_drops_the_old_cohort_without_history_substitution() {
    for mode in 0..2 {
        let (mut m, mut v, now) = ready();
        let team = v["team"].as_str().unwrap().to_owned();
        assert!(m.page(&team_key("observed", &team), None));
        let late = m.next(now).unwrap();
        if mode == 0 {
            v["context"]["draft"]["draft"]["publication"]["id"] =
                "00000000-0000-4000-8000-00000000abcd".into();
        } else {
            v["context"]["draft"]["draft"]["generation"] = 2.into();
            v["context"]["draft"]["draft"]["publication"]["generation"] = 2.into();
        }
        m.refresh(now);
        for k in ["status", "queue", "phases", "draft"] {
            let r = m.next(now).unwrap();
            m.apply(&r, checked(&r, v["context"][k].clone()), now);
        }
        assert!(m.estimates().is_none());
        m.apply(&late, checked(&late, source()), now);
        assert!(m.estimates().is_none());
    }
}
