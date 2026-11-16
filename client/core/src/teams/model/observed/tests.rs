//! Scope scheduling controls use synthetic tokens and no fabricated rows.
use super::*;
#[test]
fn synthetic_observed_tokens_do_not_accumulate_after_team_scope_changes() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    for n in 1..=500 {
        let team = format!("00000000-0000-4000-8000-{n:012x}");
        model.follow(
            TeamsFollow {
                team: Some(team.clone()),
                ..Default::default()
            },
            now,
        );
        model.follow_phases(&team, Some(0), true, now).unwrap();
        model
            .follow_observed(
                &team,
                0,
                None,
                observed_timing::DEFAULT_WINDOW_MS,
                true,
                None,
                now,
            )
            .unwrap();
        assert!(model.page(&team_key("observed", &team), Some("synthetic-token")));
        assert!(
            model.selected.len() <= 1,
            "only one selected observation cursor may remain, found {}",
            model.selected.len()
        );
    }
    model.follow(TeamsFollow::default(), now);
    assert!(model.selected.is_empty());
    assert!(model.observed_page.is_none() && model.observed_selection.is_none());
}
