use super::*;

fn commands(reference: &str) -> Vec<(Vec<String>, String)> {
    let message_id = "00000000-0000-4000-8000-000000000034";
    let specs: [(&str, &[&str], &str); 18] = [
        ("add", &["--project", "example", "--title", "\0"], ""),
        ("edit", &[reference], "\0"),
        ("rename", &[reference], "\0"),
        ("comment", &[reference], "\0"),
        ("move", &[reference, "--before", "T-missing"], ""),
        ("stage", &[reference, "done"], ""),
        ("target", &[reference, "new-session"], ""),
        ("send", &[reference, "--session", "missing-session"], ""),
        ("handoff", &[reference, "--session", "missing-session"], ""),
        ("start", &[reference], ""),
        (
            "worker",
            &[
                reference,
                "--subagent",
                "missing",
                "--request-id",
                message_id,
            ],
            "",
        ),
        ("report", &[reference], "\0"),
        (
            "report-attempt",
            &[reference, "--attempt", message_id],
            "\0",
        ),
        ("cancel", &[reference], ""),
        (
            "cancel-execution",
            &[reference],
            r#"{"schema":1,"dispatch_id":"fixture-dispatch","stage_at":1,"machine":"fixture-machine","session_id":"fixture-session","reason":"Cancel this task"}"#,
        ),
        (
            "message",
            &[reference, "--request-id", message_id, "--mode", "queue"],
            "\0",
        ),
        (
            "start-plan",
            &[reference],
            r#"{"schema":1,"id":"00000000-0000-4000-8000-000000000035"}"#,
        ),
        (
            "start-deliver",
            &[reference],
            r#"{"schema":1,"id":"00000000-0000-4000-8000-000000000035"}"#,
        ),
    ];
    specs
        .into_iter()
        .map(|(action, args, text)| {
            let mut out = vec![action.into()];
            out.extend(args.iter().map(|s| s.to_string()));
            out.push("--json".into());
            (out, text.into())
        })
        .collect()
}
fn run(f: &Fixture, args: &[String], input: &str, scenario: &str, ancestor: &str) -> Value {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    if scenario == "agent" {
        return f.unmarked_child(&args, input, ancestor);
    }
    let (child, path) = f.spawn(
        &args,
        input,
        None,
        &[
            ("HGS_TEAMS_TEST_CONTEXT", scenario.into()),
            (
                "HGS_TEAMS_TEST_PROOF_READS",
                f.h.temp.path().join("proof-reads").into_os_string(),
            ),
        ],
    );
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}
#[test]
fn every_ordinary_task_write_keeps_its_reply_and_caller_with_preview_on() {
    let mut f = Fixture::new();
    let ancestor = f.bind("m1");
    let task = f.run(
        &[
            "add",
            "--project",
            "example",
            "--title",
            "Ordinary task",
            "--json",
        ],
        "Ordinary text",
        None,
    )["value"]["task"]
        .clone();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let before = f.document(&task).revision();
    for scenario in ["agent", "namespace", "opaque"] {
        for (args, input) in commands(&reference) {
            std::fs::remove_file(f.h.temp.path().join("teams/preview")).unwrap();
            let off = run(&f, &args, &input, scenario, &ancestor);
            std::fs::write(f.h.temp.path().join("teams/preview"), b"on").unwrap();
            let on = run(&f, &args, &input, scenario, &ancestor);
            assert_eq!(on, off, "{scenario} {args:?}");
            assert_eq!(f.document(&task).revision(), before, "{scenario} {args:?}");
        }
    }
    assert!(
        !f.h.temp.path().join("proof-reads").exists(),
        "ordinary tasks consulted the Teams proof"
    );
}

#[test]
fn guarded_tasks_and_named_intake_keep_a_caller_refusal_before_every_write() {
    let mut f = Fixture::new();
    let ancestor = f.bind("m1");
    let task = f.add();
    let reference = format!("example/{}", task["id"].as_str().unwrap());
    let before = f.document(&task).revision();
    for scenario in ["agent", "namespace", "opaque"] {
        let expected = run(&f, &["prove-team-fixture".into()], "", scenario, &ancestor);
        assert_eq!(expected["ok"], false, "{scenario}: {expected}");
        for (mut args, input) in commands(&reference) {
            if args[0] == "add" {
                args.extend(["--team".into(), TEAM.into()]);
            }
            let out = run(&f, &args, &input, scenario, &ancestor);
            // An ordinary entry may refuse before it loads the team key.
            // Opaque/namespace contexts still require the strict Teams proof.
            assert!(
                out["error"] == expected["error"]
                    || scenario == "agent" && out["error"] == crate::tasks::person::REFUSED,
                "{scenario} {args:?}: {out}"
            );
            assert_eq!(f.document(&task).revision(), before);
        }
        let args = vec![
            "target".into(),
            reference.clone(),
            "team".into(),
            TEAM.into(),
            "--json".into(),
        ];
        assert_eq!(
            run(&f, &args, "", scenario, &ancestor)["error"],
            expected["error"]
        );
    }
}

#[test]
fn ordinary_tasks_accept_contexts_without_readable_agent_markers() {
    let mut f = Fixture::new();
    let ancestor = f.bind("m1");
    for scenario in ["namespace", "opaque"] {
        let args = vec![
            "add".into(),
            "--project".into(),
            "example".into(),
            "--title".into(),
            format!("Ordinary {scenario} task"),
            "--json".into(),
        ];
        let out = run(&f, &args, "An ordinary task prompt", scenario, &ancestor);
        assert_eq!(out["ok"], true, "{out}");
        let task = &out["value"]["task"];
        assert_eq!(task["author"]["kind"], "person");
        let args = vec![
            "comment".into(),
            format!("example/{}", task["id"].as_str().unwrap()),
            "--json".into(),
        ];
        let out = run(
            &f,
            &args,
            "An ordinary comment is saved",
            scenario,
            &ancestor,
        );
        assert_eq!(out["ok"], true, "{out}");
        let doc = f.document(task);
        let log = doc.view().unwrap().log;
        assert_eq!(log.last().unwrap().actor, Actor::Person);
        assert_eq!(log.last().unwrap().text, "An ordinary comment is saved");
    }
    assert!(!f.h.temp.path().join("proof-reads").exists());
}
