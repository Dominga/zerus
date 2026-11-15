//! Planning facts come from the creation, launch and binding writers.
use super::*;
use crate::teams::ops::tests::person;
use zerus_team_plan::model::TeamStatus;

#[test]
fn a_planning_leads_brief_keeps_the_existing_claim_and_unknown_rules() {
    for unknown in [false, true] {
        let mut world = World::from_harness(crate::desktop::teams::tests::bound161_fixture());
        world.unknown = unknown;
        world.pass();
        assert_eq!(world.sends.len(), 1);
        assert_eq!(world.sends[0].kind, Kind::Brief);
        assert!(world.sends[0].text.contains("hgs team current"));
        world.restart();
        world.pass();
        world.pass();
        assert_eq!(world.sends.len(), 1);
        assert_eq!(world.records(), 1);
        assert_eq!(
            world.h.store.team(TEAM).unwrap().unwrap().status,
            TeamStatus::Planning
        );
    }
}

#[test]
fn planning_pause_refuses_standing_effects_without_consuming_the_brief() {
    let mut world = World::from_harness(crate::desktop::teams::tests::bound161_fixture());
    world.h.run(&person(), Operation::Pause).unwrap();
    world.pass();
    assert!(world.sends.is_empty());
    assert_eq!(world.records(), 0);
    world.h.run(&person(), Operation::Resume).unwrap();
    world.pass();
    assert_eq!(world.sends.len(), 1);
    assert_eq!(world.records(), 1);
    assert_eq!(
        world.h.store.team(TEAM).unwrap().unwrap().status,
        TeamStatus::Planning
    );
}
