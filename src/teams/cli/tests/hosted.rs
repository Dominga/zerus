//! A proven DeepSeek session is only a session key (zerus-63rru.28). While
//! no DeepSeek seat exists, it belongs to no team: every command answers it
//! exactly as it answers an unbound Claude session, and no person-only
//! operation accepts it.
use super::harness::*;
use crate::teams::caller::hosted_fixture::{isolated, Host, NAME};
use crate::teams::cli::actor::Caller;
use serde_json::{json, Value};
use zerus_team_plan::model::SessionKey;

#[test]
fn a_proven_hosted_session_outside_every_team_is_any_unbound_session() {
    isolated("teams::cli::tests::hosted::outside_fixture");
}

/// One request envelope, as the write tests build it.
fn ask(harness: &Harness, step: u32, extra: Value) -> String {
    let mut fields = match extra {
        Value::Object(fields) => fields,
        _ => panic!("an operation's fields are an object"),
    };
    fields.insert("request_id".into(), json!(uid(0x7000 + step)));
    fields.insert("team".into(), json!(TEAM));
    fields.insert("expected_revision".into(), json!(harness.revision()));
    Value::Object(fields).to_string()
}

/// A reply with one session's ID in place of another's, so two callers'
/// replies compare.
fn named(reply: &Value, session: &str) -> String {
    reply.to_string().replace(session, "<session>")
}

#[test]
#[ignore = "run by its isolated wrapper"]
fn outside_fixture() {
    let h = Harness::new();
    crate::teams::caller::hosted_fixture::catalog(&h.config.dir);
    h.create();
    h.bind("lead", LEAD_SESSION);
    h.bind("m1", M1_SESSION);
    h.add(1);
    let host = Host::start();
    let member = host.launched(NAME, Some("00000000-0000-4000-8000-0000000000e1"));
    host.call(&member);
    let proven = crate::teams::caller::caller(&h.config);
    let Caller::Session(hosted) = &proven else {
        panic!("{proven:?}")
    };
    let claude = SessionKey {
        machine: hosted.machine.clone(),
        session_id: "launch_id:00000000-0000-4000-8000-0000000000c1".into(),
    };
    let unbound = Caller::Session(claude.clone());
    let revision = h.revision();
    let create = ask(
        &h,
        1,
        json!({"project":"example-project","name":"Other","home":MACHINE,
            "integration_branch":"zerus/team/other","base_commit":COMMIT}),
    );
    let commands = [
        ("current", String::new()),
        ("list", String::new()),
        ("waiting", String::new()),
        (
            "status --team 00000000-0000-4000-8000-00000000beef",
            String::new(),
        ),
        (
            "inbox --team 00000000-0000-4000-8000-00000000beef",
            String::new(),
        ),
        (
            "journal --team 00000000-0000-4000-8000-00000000beef",
            String::new(),
        ),
        ("template list", String::new()),
        ("create --template bugfix", create),
        ("pause", ask(&h, 2, json!({}))),
        ("stop", ask(&h, 3, json!({}))),
        ("settings", ask(&h, 4, json!({"settings":{}}))),
        (
            "cancel",
            ask(&h, 5, json!({"node":1,"reason":"Not needed"})),
        ),
        ("ask", ask(&h, 6, json!({"text":"Which branch?"}))),
        ("reply", ask(&h, 7, json!({"item":1,"text":"main"}))),
        ("template save", json!({"id":"other"}).to_string()),
    ];
    for (line, input) in &commands {
        let (theirs, exit) = h.cli(&proven, line, input);
        let (ours, unbound_exit) = h.cli(&unbound, line, input);
        assert_eq!(exit, unbound_exit, "{line}: {theirs} {ours}");
        assert_eq!(
            named(&theirs, &hosted.session_id),
            named(&ours, &claude.session_id),
            "{line}"
        );
    }
    // No person-only operation accepts it, and no lead's or member's either.
    for line in [
        "waiting",
        "template list",
        "create --template bugfix",
        "pause",
        "stop",
        "settings",
        "cancel",
        "template save",
        "current",
        "inbox --team 00000000-0000-4000-8000-00000000beef",
        "ask",
        "reply",
    ] {
        let input = &commands.iter().find(|(l, _)| *l == line).unwrap().1;
        let (reply, exit) = h.cli(&proven, line, input);
        assert_eq!((code(&reply), exit), ("not_allowed", 1), "{line}: {reply}");
    }
    for line in ["list", "status --team 00000000-0000-4000-8000-00000000beef"] {
        let (reply, exit) = h.cli(&proven, line, "");
        assert_eq!(exit, 0, "{line}: {reply}");
        assert_eq!(
            reply["actor"],
            json!({"kind":"other","person":"local","session_id":hosted.session_id}),
            "{line}"
        );
    }
    for action in ["on", "off"] {
        let (reply, exit) = crate::teams::preview(h.state(), action, || proven.clone());
        assert_eq!(
            (code(&reply), exit),
            ("not_allowed", 1),
            "{action}: {reply}"
        );
    }
    assert_eq!(h.revision(), revision, "nothing changed");
    assert!(crate::teams::enabled(h.state()));
}
