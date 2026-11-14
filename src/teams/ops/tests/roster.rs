use super::*;

fn refused_host(agent: &str, lead: bool) {
    let mut h = Harness::empty();
    let mut op = create();
    if let Operation::Create { bundle, .. } = &mut op {
        if lead {
            bundle.template.lead.agent = agent.into();
        } else {
            let role = &bundle.template.roster[0].role;
            bundle
                .roles
                .iter_mut()
                .find(|r| &r.id == role)
                .unwrap()
                .agent = agent.into();
        }
    }
    let problem = h.run(&person(), op).unwrap_err();
    assert_eq!(problem.code, code::INVALID_STATE);
    assert!(problem.message.contains(agent), "{problem:?}");
    assert!(problem.message.contains("served by a resident agent host"));
    assert!(problem
        .message
        .contains("cannot be a team lead or member in this milestone"));
    assert!(h.store.db.is_some(), "the refusal saved a receipt");
}

/// The roster refusal is a saved decision on a computer that has no store
/// yet, as it is on one that has a store: the same receipt, not a lost
/// outcome and not a silent refusal (zerus-63rru.35).
#[test]
fn a_host_served_roster_refusal_leaves_the_same_receipt_with_or_without_a_store() {
    let mut op = create();
    if let Operation::Create { bundle, .. } = &mut op {
        bundle.template.lead.agent = "opencode".into();
    }
    let mut fresh = Harness::empty();
    let fresh_problem = fresh.run(&person(), op.clone()).unwrap_err();
    assert_eq!(fresh_problem.code, code::INVALID_STATE);
    let fresh_receipt: String = fresh
        .store
        .db
        .as_ref()
        .expect("the refused create created the store to save its receipt")
        .query_row("SELECT result FROM requests", [], |r| r.get(0))
        .unwrap();
    assert!(fresh_receipt.contains("Refused"), "{fresh_receipt}");
    let mut existing = Harness::new();
    let existing_problem = existing.run_for(TEAM_B, &person(), op).unwrap_err();
    assert_eq!(existing_problem, fresh_problem);
    let existing_receipt: String = existing
        .store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT result FROM requests WHERE team=?1",
            [TEAM_B],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(existing_receipt, fresh_receipt);
}
#[test]
fn a_host_served_lead_is_refused_at_creation() {
    refused_host("opencode", true);
}
#[test]
fn a_host_served_member_is_refused_at_creation() {
    refused_host("opencode", false);
}

#[test]
fn deepseek_seats_are_created_without_contacting_or_starting_a_host() {
    for lead in [false, true] {
        let mut h = Harness::empty();
        let mut op = create();
        if let Operation::Create { bundle, .. } = &mut op {
            if lead { bundle.template.lead.agent = "dsh".into(); }
            else {
                let role = &bundle.template.roster[0].role;
                bundle.roles.iter_mut().find(|r| &r.id == role).unwrap().agent = "dsh".into();
            }
        }
        h.run(&person(), op).unwrap();
        assert!(!h.temp.path().join("dsh").exists());
    }
}
