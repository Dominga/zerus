//! Cloning a repository, against `tray/src/RepositoryClone.cpp` and
//! `HgsClient::repositoryOperation`. Names and validity are the Qt client's
//! own answers, recorded by a harness running its code on Qt 6.12.
use serde_json::{json, Value};
use std::time::Duration;
use zerus_client_core::actions::Exit;
use zerus_client_core::paths::Style;
use zerus_client_core::repository::{self, *};
use zerus_client_core::ComputerId;

#[test]
fn suggested_names_answer_as_qt_does() {
    for (url, name) in [
        ("https://example.com/org/repo.git", "repo"),
        ("git@example.com:org/repo.git", "repo"),
        (" https://example.com/a/b/ ", "b"),
        ("repo", "repo"),
        ("example.com:repo", "repo"),
        ("https://example.com/my repo!!x.git", "my-repo-x"),
        ("https://example.com/Ünïcode_ñ-1.2", "Ünïcode_ñ-1.2"),
        ("https://example.com/..", ""),
        ("https://example.com/.git", ""),
        ("https://example.com/a+b@c", "a-b-c"),
        ("https://example.com/x.git.git", "x.git"),
        ("https://example.com/日本語", "日本語"),
        ("", ""),
        ("https://example.com/a%20b", "a-20b"),
        ("https://example.com/-x-", "-x-"),
    ] {
        assert_eq!(suggested_name(url), name, "{url:?}");
    }
    assert_eq!(
        suggested_name(&format!("https://example.com/{}", "a".repeat(92))),
        "a".repeat(80)
    );
}

#[test]
fn destination_names_answer_as_qt_does() {
    for (name, valid) in [
        ("repo", true),
        ("", false),
        (".", false),
        ("..", false),
        ("a/b", false),
        ("a\\b", false),
        ("a:b", false),
        ("a*b", false),
        ("a?b", false),
        ("a\"b", false),
        ("a<b", false),
        ("a|b", false),
        ("tab\tx", false),
        ("del\u{7f}x", false),
        ("ok.name-1_2", true),
        (" lead", true),
    ] {
        assert_eq!(valid_name(name), valid, "{name:?}");
    }
    assert_eq!(destination("/srv/", "r"), "/srv/r");
    assert_eq!(destination("/", "r"), "/r");
    assert_eq!(destination("~", "r"), "~/r");
}

#[test]
fn calls_carry_qt_arguments() {
    let peer = ComputerId::from("box");
    let check = repository::invocation(&peer, Action::Check, "u", "/srv", "r", "id", "");
    assert_eq!(
        check.args,
        [
            "@box",
            "repository",
            "check",
            "--json",
            "--url",
            "u",
            "--parent",
            "/srv",
            "--name",
            "r"
        ]
    );
    assert_eq!(check.timeout, Duration::from_secs(30));
    assert_eq!(check.stdin, None);
    let clone = repository::invocation(&peer, Action::Clone, "u", "/srv", "r", "id", "m-1");
    assert_eq!(
        clone.args,
        [
            "@box",
            "--expect-machine",
            "m-1",
            "repository",
            "clone",
            "--json",
            "--url",
            "u",
            "--parent",
            "/srv",
            "--name",
            "r",
            "--request-id",
            "id"
        ]
    );
    assert_eq!(clone.timeout, Duration::from_secs(650));
    let status = repository::invocation(
        &ComputerId::local(),
        Action::Status,
        "u",
        "/srv",
        "r",
        "id",
        "",
    );
    assert_eq!(
        status.args,
        ["repository", "status", "--json", "--request-id", "id"]
    );
    let help = help_invocation(&peer, "m-1");
    assert_eq!(help.args, ["@box", "--expect-machine", "m-1", "--help"]);
    assert_eq!(help.timeout, Duration::from_secs(15));
}

fn ok(value: Value) -> Exit {
    Exit::Code {
        code: 0,
        stdout: value.to_string().into_bytes(),
        stderr: String::new(),
    }
}

#[test]
fn only_a_cli_with_the_marker_clones() {
    let help = |text: &str| Exit::Code {
        code: 0,
        stdout: text.as_bytes().to_vec(),
        stderr: String::new(),
    };
    assert_eq!(supported(help("x repository-clone-v1 y"), "box"), Ok(()));
    assert_eq!(
        supported(help("usage"), "box"),
        Err("Update hgs on box to clone repositories.".into())
    );
    assert_eq!(
        supported(help("usage"), ""),
        Err("Update hgs on this computer to clone repositories.".into())
    );
}

#[test]
fn replies_are_receipts_of_this_request() {
    let local = ComputerId::local();
    let ready = json!({"status": "ready", "name": "r", "parent": "/srv", "path": "/srv/r"});
    assert!(reply(ok(ready.clone()), &local, Action::Check, "r", "").is_ok());
    // Another name or path is not this check's receipt.
    let mut other = ready.clone();
    other["path"] = json!("/srv/x");
    assert_eq!(
        reply(ok(other), &local, Action::Check, "r", ""),
        Err(INVALID_REPLY.into())
    );
    // A local Windows parent compares in Windows form.
    let windows = json!({"status": "ready", "name": "r", "parent": "C:\\src", "path": "c:/SRC/r"});
    assert!(reply(ok(windows.clone()), &local, Action::Check, "r", "").is_ok());
    assert!(reply(
        ok(windows),
        &ComputerId::from("box"),
        Action::Check,
        "r",
        ""
    )
    .is_err());
    // A clone or status names the same request with a known status.
    for status in ["cloned", "failed", "unconfirmed"] {
        let data = json!({"status": status, "request_id": "id", "parent": "/srv", "path": "/srv/r", "name": "r"});
        assert!(
            reply(ok(data), &local, Action::Clone, "r", "id").is_ok(),
            "{status}"
        );
    }
    let foreign =
        json!({"status": "cloned", "request_id": "other", "parent": "/srv", "path": "/srv/r"});
    assert!(reply(ok(foreign), &local, Action::Status, "r", "id").is_err());
    let relative =
        json!({"status": "cloned", "request_id": "id", "parent": "srv", "path": "srv/r"});
    assert!(reply(ok(relative), &local, Action::Clone, "r", "id").is_err());
    // A failed call reports its stderr, or that it is unconfirmed.
    let failed = |stderr: &str| Exit::Code {
        code: 1,
        stdout: Vec::new(),
        stderr: stderr.into(),
    };
    assert_eq!(
        reply(failed(" denied\n"), &local, Action::Clone, "r", "id"),
        Err("denied".into())
    );
    assert_eq!(
        reply(failed(""), &local, Action::Clone, "r", "id"),
        Err(UNCONFIRMED_OPERATION.into())
    );
    assert_eq!(
        reply(Exit::TimedOut, &local, Action::Clone, "r", "id"),
        Err(UNCONFIRMED_OPERATION.into())
    );
}

fn filled() -> Clone {
    let mut clone = Clone::new(Style::Posix, None, None);
    assert!(clone.set_source("https://example.com/r.git ", "/srv/", "r"));
    clone
}

fn receipt(status: &str) -> Value {
    json!({"status": status, "request_id": "id-1", "name": "r", "parent": "/srv", "path": "/srv/r", "detail": "no space"})
}

#[test]
fn start_checks_saves_the_record_and_only_then_clones() {
    let mut clone = filled();
    assert_eq!(clone.parent(), "/srv/");
    assert!(clone.can_submit());
    let next = clone.begin();
    let (check, action) = next.run.unwrap();
    assert_eq!(action, Action::Check);
    assert_eq!(
        clone.status_text(),
        "Checking repository access and destination…"
    );
    assert!(clone.busy() && !clone.cloning() && clone.fields_locked());
    assert_eq!(
        clone
            .invocation(&ComputerId::local(), Action::Check, "")
            .args[3..5],
        ["--url", "https://example.com/r.git"]
    );

    let next = clone.finished(
        check,
        Ok(json!({"parent": "/srv", "path": "/srv/r"})),
        "id-1",
    );
    assert_eq!(
        next.store,
        Some(Store::Pending(Pending {
            request_id: "id-1".into(),
            url: "https://example.com/r.git".into(),
            parent: "/srv".into(),
            name: "r".into(),
            path: "/srv/r".into(),
        }))
    );
    assert!(
        next.run.is_none(),
        "nothing clones before the record is saved"
    );
    assert!(clone.busy());
    let next = clone.saved(true);
    let (ticket, action) = next.run.unwrap();
    assert_eq!(action, Action::Clone);
    assert!(clone.cloning() && clone.uncertain());
    assert_eq!(clone.status_text(), "Cloning repository…");
    assert_eq!(
        clone
            .invocation(&ComputerId::local(), Action::Clone, "")
            .args
            .last()
            .map(String::as_str),
        Some("id-1")
    );
    // A second answer to the save does nothing.
    assert_eq!(clone.saved(true), Next::default());
    // Cancel never stops a clone.
    clone.cancel_check();
    assert!(clone.cloning());

    let next = clone.finished(ticket, Ok(receipt("cloned")), "unused");
    assert_eq!(
        next.store,
        Some(Store::Settled {
            parent: "/srv".into()
        })
    );
    assert_eq!(next.prepared.as_deref(), Some("/srv/r"));
    assert!(!clone.uncertain() && !clone.busy());
    assert_eq!(clone.created_path(), "/srv/r");
    // Use checkout and start: the checkout is used without cloning again.
    let next = clone.begin();
    assert_eq!(next.prepared.as_deref(), Some("/srv/r"));
    assert!(next.run.is_none());
}

#[test]
fn a_record_that_is_not_saved_never_clones() {
    let mut clone = filled();
    let (check, _) = clone.begin().run.unwrap();
    clone.finished(
        check,
        Ok(json!({"parent": "/srv", "path": "/srv/r"})),
        "id-1",
    );
    assert_eq!(clone.saved(false), Next::default());
    assert_eq!(
        clone.status_text(),
        "Could not save clone recovery information."
    );
    assert!(!clone.uncertain() && !clone.busy() && clone.can_submit());
}

#[test]
fn an_unconfirmed_destination_stops_before_saving() {
    let mut clone = filled();
    let (check, _) = clone.begin().run.unwrap();
    let next = clone.finished(
        check,
        Ok(json!({"parent": "/srv", "path": "/srv/other"})),
        "id-1",
    );
    assert_eq!(next, Next::default());
    assert_eq!(
        clone.status_text(),
        "The computer did not confirm this destination."
    );
    assert!(clone.can_submit());
    // A failed check is not uncertain; a failed clone is.
    let (check, _) = clone.begin().run.unwrap();
    clone.finished(check, Err("offline".into()), "id-1");
    assert!(!clone.uncertain());
    assert_eq!(clone.status_text(), "offline");
}

fn cloning() -> (Clone, u64) {
    let mut clone = filled();
    let (check, _) = clone.begin().run.unwrap();
    clone.finished(
        check,
        Ok(json!({"parent": "/srv", "path": "/srv/r"})),
        "id-1",
    );
    let (ticket, _) = clone.saved(true).run.unwrap();
    (clone, ticket)
}

#[test]
fn a_lost_clone_stays_uncertain_and_is_only_read_again() {
    let (mut clone, ticket) = cloning();
    assert_eq!(
        clone.finished(ticket, Err("connection lost".into()), ""),
        Next::default()
    );
    assert!(clone.uncertain() && !clone.can_submit() && clone.fields_locked());
    assert_eq!(clone.begin(), Next::default(), "never clones again");
    let (status, action) = clone.check_result().run.unwrap();
    assert_eq!(action, Action::Status);
    assert_eq!(clone.status_text(), "Checking the original clone result…");
    // Still running: uncertain, and the operation is not repeated.
    clone.finished(status, Ok(receipt("unconfirmed")), "");
    assert!(clone.uncertain());
    assert_eq!(
        clone.status_text(),
        "The original clone is still running or its completion is unknown. Check again; the operation will not be repeated."
    );
    // A status read that finds the checkout asks before it is used.
    let (status, _) = clone.check_result().run.unwrap();
    let next = clone.finished(status, Ok(receipt("cloned")), "");
    assert_eq!(
        next.store,
        Some(Store::Settled {
            parent: "/srv".into()
        })
    );
    assert_eq!(next.prepared, None);
    assert_eq!(clone.status_text(), "Checkout is ready. Confirm to use it.");
    assert_eq!(clone.created_path(), "/srv/r");
}

#[test]
fn a_failed_or_mismatched_clone_keeps_its_files() {
    let (mut clone, ticket) = cloning();
    let next = clone.finished(ticket, Ok(receipt("failed")), "");
    assert_eq!(
        next.store,
        Some(Store::Settled {
            parent: "/srv".into()
        })
    );
    assert!(!clone.uncertain());
    assert_eq!(
        clone.status_text(),
        "no space\nAny remaining checkout files are kept. Choose another destination or use the existing folder."
    );
    let (mut clone, ticket) = cloning();
    let mut moved = receipt("cloned");
    moved["path"] = json!("/srv/elsewhere");
    assert_eq!(clone.finished(ticket, Ok(moved), ""), Next::default());
    assert!(clone.uncertain());
    assert_eq!(
        clone.status_text(),
        "Clone result does not match this destination. Inspect the folder before continuing."
    );
}

#[test]
fn a_saved_record_resumes_uncertain_and_can_be_retired() {
    let pending = Pending {
        request_id: "id-1".into(),
        url: "u".into(),
        parent: "/srv".into(),
        name: "r".into(),
        path: "/srv/r".into(),
    };
    let mut clone = Clone::new(Style::Posix, Some("/home/user"), Some(pending));
    assert!(clone.uncertain());
    assert_eq!(
        clone.status_text(),
        "A previous clone is unconfirmed. Check its original result before continuing."
    );
    assert_eq!(clone.destination(), "/srv/r");
    assert!(!clone.set_source("v", "/x", "y"), "fields stay locked");
    let next = clone.choose_another_destination();
    assert_eq!(next.store, Some(Store::Retired));
    assert!(!clone.uncertain());
    assert_eq!(clone.name(), "");
    assert_eq!(
        clone.status_text(),
        "The original checkout is kept. Choose a different destination for a new clone."
    );
    // Without a record the remembered parent is used, `~` by default.
    assert_eq!(
        Clone::new(Style::Posix, Some("/home/user"), None).parent(),
        "/home/user"
    );
    assert_eq!(Clone::new(Style::Posix, None, None).parent(), "~");
}

#[test]
fn a_cancelled_or_stale_answer_changes_nothing() {
    let mut clone = filled();
    let (check, _) = clone.begin().run.unwrap();
    clone.cancel_check();
    assert!(!clone.busy());
    assert_eq!(
        clone.finished(
            check,
            Ok(json!({"parent": "/srv", "path": "/srv/r"})),
            "id-1"
        ),
        Next::default()
    );
    let (again, _) = clone.begin().run.unwrap();
    assert_ne!(again, check);
    assert_eq!(
        clone.finished(check, Err("late".into()), ""),
        Next::default()
    );
    assert!(clone.busy());
}
