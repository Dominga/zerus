use super::super::cross as reads;
use super::*;
use rusqlite::params;

fn team_id(n: u32) -> String {
    format!("00000000-0000-4000-8000-{n:012x}")
}
fn add_team(db: &Connection, n: u32, status: TeamStatus) -> String {
    let id = team_id(n);
    db.execute("INSERT INTO teams(id,project,name,goal,home,owner,status,lead,settings,bundle,integration_branch,base_commit,revision) SELECT ?1,project,name,goal,home,owner,?2,lead,settings,bundle,integration_branch,base_commit,revision FROM teams WHERE id=?3",params![id,encode(&status).unwrap(),TEAM]).unwrap();
    id
}
fn add_card(db: &Connection, team: &str, state: &str) -> u64 {
    db.execute("INSERT INTO cards(team,kind,actor,text,state,created_ms) VALUES(?1,'proposal',?2,'Team proposal',?3,0)",params![team,encode(&person()).unwrap(),state]).unwrap();
    db.last_insert_rowid() as u64
}
fn team_page(h: &Harness, live: bool, limit: u32, cursor: Option<&Cursor>) -> Page<TeamRow> {
    if live {
        h.store.live_teams(limit, cursor).unwrap()
    } else {
        h.store.teams(limit, cursor).unwrap()
    }
}

#[test]
fn team_lists_keep_id_order_and_exclude_additions_between_pages() {
    for live in [false, true] {
        let h = Harness::new();
        let db = h.store.db.as_ref().unwrap();
        for n in [1, 3, 5] {
            add_team(db, n, TeamStatus::Running);
        }
        let first = team_page(&h, live, 2, None);
        assert_eq!(
            first.rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            vec![team_id(1), team_id(3)]
        );
        for n in [2, 4, 6] {
            add_team(db, n, TeamStatus::Paused);
        }
        let mut rows = first.rows;
        let mut cursor = first.next;
        while let Some(c) = cursor {
            let page = team_page(&h, live, 2, Some(&c));
            assert!(page.rows.len() <= 2);
            rows.extend(page.rows);
            cursor = page.next;
        }
        assert_eq!(
            rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            vec![team_id(1), team_id(3), team_id(5), TEAM.into()]
        );
        let original = h.store.team(TEAM).unwrap().unwrap();
        let row = rows.last().unwrap();
        assert_eq!(
            (
                &row.project,
                &row.name,
                row.status,
                &row.lead,
                &row.home,
                row.revision
            ),
            (
                &original.project,
                &original.name,
                original.status,
                &original.lead,
                &original.home,
                original.revision
            )
        );
        assert_eq!(team_page(&h, live, 200, None).rows.len(), 7);
    }
}

#[test]
fn live_teams_return_running_and_paused_metadata_without_decoding_bundles() {
    let h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    for (n, status) in [
        (1, TeamStatus::Paused),
        (2, TeamStatus::Stopped),
        (3, TeamStatus::Archived),
        (4, TeamStatus::Finished),
    ] {
        add_team(db, n, status);
    }
    db.execute("UPDATE teams SET settings='null',bundle='null'", [])
        .unwrap();
    let rows = h.store.live_teams(200, None).unwrap().rows;
    assert_eq!(
        rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        vec![team_id(1), TEAM.into()]
    );
    let all = h.store.teams(200, None).unwrap();
    assert_eq!(all.rows.len(), 5);
    let value = serde_json::to_value(&all.rows[0]).unwrap();
    let mut fields = value
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    fields.sort();
    assert_eq!(
        fields,
        vec!["home", "id", "lead", "name", "project", "revision", "status"]
    );
}

#[test]
fn waiting_pages_open_cards_across_teams_oldest_first_and_exclude_additions() {
    let h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    let other = add_team(db, 1, TeamStatus::Paused);
    let a = add_card(db, &other, "open");
    add_card(db, TEAM, "closed");
    let b = add_card(db, TEAM, "open");
    let c = add_card(db, &other, "open");
    let first = h.store.waiting(2, None).unwrap();
    assert_eq!(
        first.rows.iter().map(|r| r.card.seq).collect::<Vec<_>>(),
        vec![a, b]
    );
    assert_eq!(first.rows[0].team, other);
    assert_eq!(first.rows[1].team, TEAM);
    assert_eq!(first.rows[0].card.text, "Team proposal");
    assert_eq!(first.rows[0].card.actor["person"], "local");
    add_card(db, TEAM, "open");
    let second = h.store.waiting(2, first.next.as_ref()).unwrap();
    assert_eq!(
        second.rows.iter().map(|r| r.card.seq).collect::<Vec<_>>(),
        vec![c]
    );
    assert!(second.next.is_none());
    assert_eq!(h.store.waiting(200, None).unwrap().rows.len(), 4);
}

#[test]
fn cross_team_reads_enforce_limits_and_scope_their_cursors() {
    let h = Harness::new();
    add_card(h.store.db.as_ref().unwrap(), TEAM, "open");
    for limit in [0, 201] {
        refused(h.store.teams(limit, None), code::OVER_CAP);
        refused(h.store.live_teams(limit, None), code::OVER_CAP);
        refused(h.store.waiting(limit, None), code::OVER_CAP);
    }
    let all = h.store.teams(1, None).unwrap().next.unwrap();
    let live = h.store.live_teams(1, None).unwrap().next.unwrap();
    let cards = h.store.waiting(1, None).unwrap().next.unwrap();
    refused(h.store.live_teams(1, Some(&all)), code::INVALID_STATE);
    refused(h.store.waiting(1, Some(&all)), code::INVALID_STATE);
    refused(h.store.teams(1, Some(&live)), code::INVALID_STATE);
    refused(h.store.teams(1, Some(&cards)), code::INVALID_STATE);
    refused(h.store.cards(TEAM, 1, Some(&cards)), code::INVALID_STATE);
}

#[test]
fn cross_team_reads_are_lazy_on_empty_stores_and_refuse_preview_off() {
    for initialized in [false, true] {
        let mut h = Harness::empty();
        if initialized {
            h.store.ensure().unwrap();
        }
        assert!(h.store.teams(200, None).unwrap().rows.is_empty());
        assert!(h.store.live_teams(200, None).unwrap().rows.is_empty());
        assert!(h.store.waiting(200, None).unwrap().rows.is_empty());
        assert_eq!(
            h.temp.path().join("teams/teams-v1.sqlite3").exists(),
            initialized
        );
        std::fs::remove_file(h.temp.path().join("teams/preview")).unwrap();
        refused(h.store.teams(1, None), code::TEAMS_INACTIVE);
        refused(h.store.live_teams(1, None), code::TEAMS_INACTIVE);
        refused(h.store.waiting(1, None), code::TEAMS_INACTIVE);
    }
}

#[test]
fn archived_teams_and_closed_cards_do_not_delay_the_first_live_page() {
    let mut h = Harness::new();
    let tx = h.store.db.as_mut().unwrap().transaction().unwrap();
    for n in 1..=5000 {
        add_team(&tx, n, TeamStatus::Archived);
        add_card(&tx, TEAM, "closed");
    }
    let card = add_card(&tx, TEAM, "open");
    tx.commit().unwrap();
    assert_eq!(h.store.live_teams(1, None).unwrap().rows[0].id, TEAM);
    assert_eq!(h.store.waiting(1, None).unwrap().rows[0].card.seq, card);
    let db = h.store.db.as_ref().unwrap();
    for (sql, index) in [
        (reads::teams_sql(true), "teams_live"),
        (reads::WAITING_SQL.to_owned(), "cards_waiting"),
    ] {
        let mut s = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = s
            .query_map(params!["", u64::MAX / 2, 1], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(plan.iter().any(|r| r.contains(index)), "{plan:?}");
        assert!(!plan.iter().any(|r| r.contains("TEMP B-TREE")), "{plan:?}");
    }
}

#[test]
fn opening_an_older_version_two_store_creates_missing_cross_team_indexes() {
    let mut h = Harness::new();
    let db = h.store.db.as_ref().unwrap();
    db.execute_batch("DROP INDEX IF EXISTS teams_live; DROP INDEX IF EXISTS cards_waiting;")
        .unwrap();
    let before = h.store.team(TEAM).unwrap();
    h.store = Store::open(h.temp.path()).unwrap();
    assert_eq!(h.store.team(TEAM).unwrap(), before);
    let db = h.store.db.as_ref().unwrap();
    let names = db.prepare("SELECT name FROM sqlite_schema WHERE type='index' AND name IN ('teams_live','cards_waiting') ORDER BY name").unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    assert_eq!(names, vec!["cards_waiting", "teams_live"]);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
}
