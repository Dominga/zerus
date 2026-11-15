//! Valid rows are CLI fixtures pinned by the root product-writer contract.
//! Tests named `handwritten_*` deliberately corrupt that evidence.
use super::*;
use serde_json::json;

const TEAM: &str = "00000000-0000-4000-8000-00000000beef";
fn asked(kind: DrawerKind) -> DrawerRead {
    DrawerRead {
        team: TEAM.into(),
        kind,
        after: None,
    }
}
fn fixture(kind: DrawerKind) -> Value {
    serde_json::from_str(match kind {
        DrawerKind::Feed => {
            include_str!("../../../../../crates/team-plan/fixtures/cli-journal.json")
        }
        DrawerKind::Inbox => {
            include_str!("../../../../../crates/team-plan/fixtures/cli-inbox.json")
        }
        DrawerKind::Cards => {
            include_str!("../../../../../crates/team-plan/fixtures/cli-cards.json")
        }
    })
    .unwrap()
}
fn exit(value: Value) -> Exit {
    Exit::Code {
        code: 0,
        stdout: serde_json::to_vec(&value).unwrap(),
        stderr: String::new(),
    }
}

#[test]
fn product_written_drawer_pages_reach_the_client() {
    let mut pages = serde_json::Map::new();
    for kind in [DrawerKind::Feed, DrawerKind::Inbox, DrawerKind::Cards] {
        let raw = fixture(kind);
        let page = decode(&asked(kind), exit(raw.clone())).unwrap();
        assert_eq!(page.team, TEAM);
        assert_eq!(page.kind, kind);
        assert_eq!(page.rows.len(), raw["rows"].as_array().unwrap().len());
        assert_eq!(page.rows[0].text, raw["rows"][0]["text"]);
        if kind == DrawerKind::Cards {
            assert_eq!(page.rows[0].card.as_ref().unwrap().team, TEAM);
        }
        pages.insert(
            match kind {
                DrawerKind::Feed => "feed",
                DrawerKind::Inbox => "inbox",
                DrawerKind::Cards => "cards",
            }
            .into(),
            serde_json::to_value(page).unwrap(),
        );
    }
    let pinned: Value = serde_json::from_str(include_str!("../fixtures/drawers.json")).unwrap();
    assert_eq!(
        Value::Object(pages),
        pinned,
        "the UI uses the core's product-written projection"
    );
}

#[test]
fn each_drawer_uses_the_existing_page_and_child_bounds() {
    for kind in [DrawerKind::Feed, DrawerKind::Inbox, DrawerKind::Cards] {
        let mut read = asked(kind);
        read.after = Some("t1.example".into());
        let call = invocation(&read).unwrap();
        assert_eq!(
            call.args,
            [
                "team",
                kind.command(),
                "--team",
                TEAM,
                "--limit",
                "200",
                "--after",
                "t1.example",
                "--json"
            ]
        );
        assert_eq!(call.stdin, None);
        assert_eq!(call.request_id, None);
        assert_eq!(call.timeout, std::time::Duration::from_secs(15));
        read.team = "--other".into();
        assert!(invocation(&read).is_err());
        read.team = TEAM.into();
        read.after = Some("--option".into());
        assert!(invocation(&read).is_err());
    }
}

#[test]
fn handwritten_foreign_malformed_and_oversized_pages_never_become_empty_success() {
    for kind in [DrawerKind::Feed, DrawerKind::Inbox, DrawerKind::Cards] {
        let raw = fixture(kind);
        let mut foreign = raw.clone();
        foreign["team"] = json!("other");
        let mut missing = raw.clone();
        missing["rows"][0].as_object_mut().unwrap().remove("seq");
        let mut too_many = raw.clone();
        too_many["rows"] = json!(vec![raw["rows"][0].clone(); PAGE as usize + 1]);
        let mut bad_next = raw.clone();
        bad_next["next"] = json!("x".repeat(CURSOR_BYTES + 1));
        for broken in [foreign, missing, too_many, bad_next] {
            assert!(decode(&asked(kind), exit(broken)).is_err());
        }
    }
}

#[test]
fn handwritten_long_text_is_marked_and_the_cursor_is_preserved_exactly() {
    let mut read = asked(DrawerKind::Inbox);
    read.after = Some("t1.original".into());
    let mut raw = fixture(DrawerKind::Inbox);
    raw["next"] = json!("t1.next");
    raw["rows"][0]["text"] = json!("я".repeat(TEXT_CHARS + 1));
    let page = decode(&read, exit(raw)).unwrap();
    assert_eq!(page.next.as_deref(), Some("t1.next"));
    assert_eq!(page.after.as_deref(), Some("t1.original"));
    assert_eq!(page.rows[0].text.chars().count(), TEXT_CHARS);
    assert!(page.rows[0].text_truncated);
}
