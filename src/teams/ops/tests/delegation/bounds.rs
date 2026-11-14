use super::*;

fn extra_open(h: &Harness, lead: &Actor, count: u32) {
    let db = h.store.db.as_ref().unwrap();
    let tx = db.unchecked_transaction().unwrap();
    let actor = encode(lead).unwrap();
    for index in 0..count {
        let id = 50_000 + index;
        tx.execute("INSERT INTO node_identity(team,id,task_project,task_uuid) VALUES(?1,?2,'example-project',?3)",params![TEAM,id,format!("00000000-0000-4000-8000-{id:012x}")]).unwrap();
        tx.execute("INSERT INTO decisions(team,node,generation,actor,kind,text,state) VALUES(?1,?2,1,?3,'return','Earlier open decision',?4)",params![TEAM,id,actor,DecisionState::Pending]).unwrap();
    }
    tx.commit().unwrap();
}

#[test]
fn r7_the_open_decision_bound_accepts_600_and_rolls_back_601() {
    for extra in [599, 600] {
        let mut h = Harness::new();
        let (node, lead) = h.reported();
        h.run(&lead, Operation::Accept { node }).unwrap();
        let seq = latest(&h, node);
        extra_open(&h, &lead, extra);
        let before = h.store.team(TEAM).unwrap().unwrap();
        let journal = h.store.journal(TEAM, 200, None).unwrap().rows;
        let mut settings = before.settings.clone();
        settings.delegations.merge_accepted = false;
        let result = h.run(&person(), Operation::ChangeSettings { settings });
        if extra == 599 {
            result.unwrap();
            assert_eq!(state(&h, seq), DecisionState::AwaitingOwner);
            assert_eq!(open(&h).len(), 1);
        } else {
            refused(result, code::OVER_CAP);
            assert_eq!(h.store.team(TEAM).unwrap().unwrap(), before);
            assert_eq!(state(&h, seq), DecisionState::Ready);
            assert!(open(&h).is_empty());
            assert_eq!(h.store.journal(TEAM, 200, None).unwrap().rows, journal);
            h.run(&person(), Operation::Pause).unwrap();
            assert_eq!(
                h.store.team(TEAM).unwrap().unwrap().status,
                TeamStatus::Paused
            );
        }
    }
}
