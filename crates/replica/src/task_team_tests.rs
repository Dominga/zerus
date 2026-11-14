use super::*;

const TEAM: &str = "00000000-0000-4000-8000-000000000001";
const HOME: &str = "00000000-0000-4000-8000-000000000002";

#[cfg(not(feature = "test-without-team"))]
fn task() -> TaskDoc {
    TaskDoc::create(NewTask {
        project: "example-project",
        id: "T-abcdef",
        text: "An ordinary task",
        stage: Stage::Todo,
        target: Target::Any,
        author: Actor::Person,
        rank: "V",
        now: 1,
    })
    .unwrap()
}

#[test]
fn team_key_survives_an_edit_without_its_reader() {
    // This test also runs with test-without-team: all the key's production
    // code, including its view field, is excluded from that build.
    let mut original =
        TaskDoc::from_bytes(include_bytes!("../tests/vectors/task-team-current.loro")).unwrap();
    let raw = string(&original.fields(), "team").unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&raw).unwrap(),
        serde_json::json!({"team": TEAM, "home": HOME})
    );
    assert!(original
        .view()
        .unwrap()
        .log
        .iter()
        .any(|entry| entry.text == "Sent to team Example team"));
    let mut older = TaskDoc::from_bytes(&original.snapshot().unwrap()).unwrap();
    assert!(older.view().unwrap().writable);
    assert_eq!(older.schema(), SCHEMA);
    #[cfg(feature = "test-without-team")]
    assert!(serde_json::to_value(older.view().unwrap())
        .unwrap()
        .get("team")
        .is_none());
    older
        .edit_text("Edited by a reader that ignores the key", &Actor::Person, 2)
        .unwrap();
    original.import(&older.snapshot().unwrap()).unwrap();
    assert_eq!(
        string(&original.fields(), "team").as_deref(),
        Some(raw.as_str())
    );
    assert_eq!(original.schema(), SCHEMA);
    assert_eq!(
        original.view().unwrap().text,
        "Edited by a reader that ignores the key"
    );
}

#[cfg(not(feature = "test-without-team"))]
#[test]
fn team_key_sync_round_trip_never_raises_schema_or_changes_target() {
    let mut original = task();
    let mut peer = TaskDoc::from_bytes(&original.snapshot().unwrap()).unwrap();
    let team = crate::TaskTeam {
        team: TEAM.into(),
        home: HOME.into(),
    };
    original.set_team(Some(&team), &Actor::Person, 2).unwrap();
    peer.import(&original.snapshot().unwrap()).unwrap();
    assert_eq!(peer.team().unwrap(), Some(team.clone()));
    assert_eq!(peer.schema(), SCHEMA);
    assert_eq!(peer.view().unwrap().target, Target::Any);
    assert_eq!(peer.view().unwrap().team, Some(team.clone()));
    let revision = peer.revision();
    peer.set_team(Some(&team), &Actor::Person, 3).unwrap();
    assert_eq!(peer.revision(), revision);
    peer.set_team(None, &Actor::Person, 4).unwrap();
    original.import(&peer.snapshot().unwrap()).unwrap();
    assert_eq!(original.team().unwrap(), None);
    assert_eq!(original.schema(), SCHEMA);
}

#[cfg(not(feature = "test-without-team"))]
#[test]
fn team_key_validates_identity_and_respects_read_only_documents() {
    let mut task = task();
    let valid = crate::TaskTeam {
        team: TEAM.into(),
        home: HOME.into(),
    };
    for invalid in [
        crate::TaskTeam {
            team: "label".into(),
            ..valid.clone()
        },
        crate::TaskTeam {
            home: "host".into(),
            ..valid.clone()
        },
    ] {
        let before = task.revision();
        assert!(task.set_team(Some(&invalid), &Actor::Person, 2).is_err());
        assert_eq!(task.revision(), before);
    }
    task.set_raw_field("team", "{\"team\":false}");
    assert_eq!(task.team().unwrap(), None);
    assert!(task.view().unwrap().team_invalid);
    task.meta().insert("schema", TASK_SCHEMA + 1).unwrap();
    assert!(task.set_team(Some(&valid), &Actor::Person, 2).is_err());
}

#[cfg(not(feature = "test-without-team"))]
#[test]
fn team_key_invalid_data_never_damages_load_or_listing() {
    for (raw, invalid) in [
        (
            serde_json::json!({"team":TEAM,"home":HOME,"future":"field"}).to_string(),
            false,
        ),
        ("not json".into(), true),
        ("false".into(), true),
        (
            serde_json::json!({"team":TEAM,"home":HOME,"future":"x".repeat(2049)}).to_string(),
            true,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let store = crate::ProjectStore::open(temp.path(), "example-project").unwrap();
        let mut document = task();
        document.set_raw_field("team", &raw);
        assert_eq!(document.view().unwrap().team_invalid, invalid);
        store.save(&mut document).unwrap();
        assert_eq!(
            store
                .load(document.uuid())
                .unwrap()
                .view()
                .unwrap()
                .team_invalid,
            invalid
        );
        let (listed, damaged) = store.listing().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(damaged.is_empty());
        assert_eq!(listed[0].view().unwrap().team_invalid, invalid);
    }
    let wrong = task();
    wrong.fields().insert("team", true).unwrap();
    assert!(wrong.view().unwrap().team_invalid);
}

#[cfg(not(feature = "test-without-team"))]
#[test]
fn team_key_fixture_and_history_are_readable_in_older_clients() {
    let mut document = task();
    document
        .set_team_named(
            Some(&crate::TaskTeam {
                team: TEAM.into(),
                home: HOME.into(),
            }),
            "Example team",
            &Actor::Person,
            2,
        )
        .unwrap();
    assert_eq!(
        document.view().unwrap().log.last().unwrap().text,
        "Sent to team Example team"
    );
    if std::env::var_os("ZERUS_BLESS").is_some() {
        std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/vectors/task-team-current.loro"),
            document.snapshot().unwrap(),
        )
        .unwrap();
    }
    document
        .set_team_named(None, "Example team", &Actor::Person, 3)
        .unwrap();
    assert_eq!(
        document.view().unwrap().log.last().unwrap().text,
        "Removed from team Example team"
    );
}
