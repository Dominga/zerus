//! A failing verdict for an implementer who was removed from the team.
use super::*;

fn open_kinds(h: &Harness, node: u32) -> Vec<String> {
    h.store
        .inbox(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .filter(|i| i.node == node && i.state == "open")
        .map(|i| i.kind)
        .collect()
}
fn reopens(h: &Harness) -> Vec<u64> {
    h.store
        .pending_reopens(TEAM, 200, None)
        .unwrap()
        .rows
        .into_iter()
        .map(|d| d.seq)
        .collect()
}
fn failed_verdicts(h: &Harness, stage: u32) -> u32 {
    h.store
        .db
        .as_ref()
        .unwrap()
        .query_row(
            "SELECT count(*) FROM verdicts WHERE team=?1 AND node=?2 AND NOT pass",
            rusqlite::params![TEAM, stage],
            |r| r.get(0),
        )
        .unwrap()
}

/// The verdict stays recorded, and with its implementer removed the parent
/// goes to the lead instead of a Return that could never be carried out;
/// with the implementer present the pipeline returns the task as before. A
/// replay changes nothing.
#[test]
fn a_failing_verdict_for_a_removed_implementer_goes_to_the_lead_instead_of_a_return() {
    for gone in [true, false] {
        let (mut h, parent, _) = wanted();
        let lead = h.bind("lead");
        h.bind("m2");
        let reviewer = bound(&h, "m3");
        let stage = stage_attempt(&mut h, parent, "m3");
        if gone {
            h.run(
                &person(),
                Operation::ArchiveMember {
                    member: "m1".into(),
                },
            )
            .unwrap();
        }
        let rev = h.store.team(TEAM).unwrap().unwrap().revision;
        let r = request(rev, verdict(stage, 1, false));
        let first = crate::teams::ops::apply(&mut h.store, &reviewer, &r, 1234).unwrap();
        assert_eq!(failed_verdicts(&h, stage), 1);
        let decisions = h.store.decisions(TEAM, 200, None).unwrap().rows;
        let kind = "verdict_holder_removed:2".to_owned();
        if gone {
            assert!(decisions
                .iter()
                .all(|d| d.node != parent || d.state == DecisionState::Settled));
            assert!(reopens(&h).is_empty());
            assert!(h.node(parent).hold);
            let handed: Vec<_> = h
                .store
                .inbox(TEAM, 200, None)
                .unwrap()
                .rows
                .into_iter()
                .filter(|i| i.node == parent && i.kind == kind && i.state == "open")
                .collect();
            assert_eq!(handed.len(), 1);
            assert!(handed[0].text.contains("Member m1 was removed"));
            assert!(handed[0].text.contains("failing verdict"));
            assert_eq!(first.item, Some(handed[0].seq));
        } else {
            let ret = decisions
                .iter()
                .rev()
                .find(|d| d.node == parent)
                .unwrap()
                .clone();
            assert_eq!(
                (ret.kind.as_str(), ret.state),
                ("return", DecisionState::Pending)
            );
            assert_eq!(reopens(&h), vec![ret.seq]);
            assert!(!open_kinds(&h, parent).contains(&kind));
        }
        let snapshot = (
            h.store.decisions(TEAM, 200, None).unwrap().rows,
            h.store.inbox(TEAM, 200, None).unwrap().rows,
            h.store.journal(TEAM, 200, None).unwrap().rows,
        );
        let again = crate::teams::ops::apply(&mut h.store, &reviewer, &r, 1234).unwrap();
        assert_eq!(again, first);
        assert_eq!(
            (
                h.store.decisions(TEAM, 200, None).unwrap().rows,
                h.store.inbox(TEAM, 200, None).unwrap().rows,
                h.store.journal(TEAM, 200, None).unwrap().rows,
            ),
            snapshot
        );
        if gone {
            h.run(
                &lead,
                Operation::Reassign {
                    node: parent,
                    member: "m2".into(),
                    reason: "m1 was removed".into(),
                },
            )
            .unwrap();
            let next = h
                .store
                .decisions(TEAM, 200, None)
                .unwrap()
                .rows
                .into_iter()
                .rev()
                .find(|d| d.node == parent)
                .unwrap();
            assert_eq!(
                (next.kind.as_str(), next.state),
                ("reassign", DecisionState::Pending)
            );
            assert_eq!(reopens(&h), vec![next.seq]);
        }
    }
}
