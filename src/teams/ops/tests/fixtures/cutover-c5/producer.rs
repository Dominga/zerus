// Append to the new ops/tests/cutover_delivery.rs module on production c5a34bbb6.
#[test]
fn wp166_generate_old_stores() {
    let output = std::path::PathBuf::from(std::env::var_os("WP166_LEGACY_OUT").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for case in ["open", "recovered", "generation"] {
        let mut h = Harness::new();
        let lead = h.bind("lead");
        h.bind("m1");
        h.bind("m2");
        let node = h.add(&person(), 1);
        let why = "Fence held. Nothing was sent.".to_owned();
        let old = h.intent(node, "m1");
        h.outcome(
            &old,
            if case == "generation" {
                HandoutOutcome::Uncertain {
                    reason: why.clone(),
                }
            } else {
                HandoutOutcome::CuttingOver {
                    reason: why.clone(),
                }
            },
        );
        if case == "open" {
            let other = h.add(&person(), 2);
            let unknown = h.intent(other, "m2");
            h.outcome(&unknown, HandoutOutcome::Uncertain { reason: why });
        } else {
            h.run(
                &lead,
                Operation::Reassign {
                    node,
                    member: "m2".into(),
                    reason: "Person chooses another member".into(),
                },
            )
            .unwrap();
            let reopen = h.reopen(node, "m2");
            h.reopen_outcome(
                &reopen,
                HandoutOutcome::Refused {
                    reason: "Task has no delivered conversation".into(),
                },
            );
            h.run(&lead, Operation::Release { node }).unwrap();
            let next = h.intent(node, "m2");
            h.outcome(
                &next,
                if case == "generation" {
                    HandoutOutcome::CuttingOver { reason: why }
                } else {
                    HandoutOutcome::Submitted {
                        receipt: "new accepted delivery".into(),
                    }
                },
            );
        }
        drop(h.store);
        std::fs::copy(
            h.temp.path().join("teams/teams-v1.sqlite3"),
            output.join(format!("{case}.sqlite3")),
        )
        .unwrap();
    }
}
