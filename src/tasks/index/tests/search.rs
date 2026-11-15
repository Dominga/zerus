use super::*;

fn search(query: &str) -> Scope {
    Scope::searched(None, query, "orchard")
}

fn ids(page: &Page) -> Vec<&str> {
    page.rows
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect()
}

#[test]
fn search_matches_short_and_qualified_refs_title_and_full_text() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(
        &store,
        "T-aaaa01",
        Stage::Todo,
        "V",
        &format!("{}Needle in body", "x".repeat(TEXT_BYTES + 100)),
    );
    task.set_title("Проверка интерфейса", &Actor::Person, 2)
        .unwrap();
    aged_save(&store, &mut task);
    let mut other = new_task(&store, "T-aaaa02", Stage::Todo, "W", "Different task");
    aged_save(&store, &mut other);
    let mut index = Index::open(&store).unwrap();
    for query in [
        "T-aaaa01",
        "orchard/T-aaaa01",
        "ORCHARD/T-AAAA01",
        "ПРОВЕРКА",
        "needle in body",
    ] {
        let filter = search(query);
        let page = index
            .page_in(Stage::Todo, 2, &Anchor::Start, Some(&filter))
            .unwrap();
        assert_eq!(ids(&page), ["T-aaaa01"], "{query}");
        assert!(page.rows[0]["text"].as_str().unwrap().len() <= TEXT_BYTES);
        let fallback = super::super::super::documents_page_in(
            &store,
            Stage::Todo,
            2,
            &Anchor::Start,
            None,
            Some(&filter),
        )
        .unwrap();
        assert_eq!(fallback["tasks"], json!(page.rows), "fallback {query}");
    }
    assert!(index
        .page_in(
            Stage::Todo,
            2,
            &Anchor::Start,
            Some(&search("other/T-aaaa01"))
        )
        .unwrap()
        .rows
        .is_empty());
}

#[test]
fn search_pages_only_matches_beyond_a_window_and_counts_each_stage() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    for n in 0..56 {
        let text = if n < 51 { "Unrelated" } else { "Needle" };
        let mut task = new_task(
            &store,
            &format!("T-aaa{n:03}"),
            Stage::Todo,
            &format!("V{n:03}V"),
            text,
        );
        aged_save(&store, &mut task);
    }
    let mut backlog = new_task(&store, "T-bbb001", Stage::Backlog, "V", "Needle backlog");
    aged_save(&store, &mut backlog);
    let mut index = Index::open(&store).unwrap();
    let filter = search("Needle");
    let mut anchor = Anchor::Start;
    let mut found = Vec::new();
    loop {
        let (page, decoded, stamped) = counted(|| {
            index
                .page_in(Stage::Todo, 2, &anchor, Some(&filter))
                .unwrap()
        });
        assert!(page.rows.len() <= 2);
        assert_eq!(decoded, 0);
        assert_eq!(
            stamped,
            page.rows.len() * 5,
            "only the returned tasks' five files are stamped"
        );
        assert_eq!(page.position, found.len());
        let fallback = super::super::super::documents_page_in(
            &store,
            Stage::Todo,
            2,
            &anchor,
            None,
            Some(&filter),
        )
        .unwrap();
        assert_eq!(fallback["tasks"], json!(page.rows));
        assert_eq!(fallback["page"]["next"], json!(page.next));
        assert_eq!(fallback["page"]["previous"], json!(page.previous));
        assert_eq!(fallback["page"]["position"], page.position);
        found.extend(ids(&page).into_iter().map(str::to_owned));
        match page.next {
            Some(next) => {
                let project = filter.cursor_project(store.project_id());
                assert!(parse_cursor(store.project_id(), Stage::Todo, &next).is_err());
                assert!(parse_cursor(
                    &search("Other").cursor_project(store.project_id()),
                    Stage::Todo,
                    &next
                )
                .is_err());
                let (rank, uuid) = parse_cursor(&project, Stage::Todo, &next).unwrap();
                anchor = Anchor::After(rank, uuid);
            }
            None => break,
        }
    }
    assert_eq!(
        found,
        (51..56).map(|n| format!("T-aaa{n:03}")).collect::<Vec<_>>()
    );
    let counts = index.counts_in(Some(&filter)).unwrap();
    assert_eq!(counts["todo"], 5);
    assert_eq!(counts["backlog"], 1);
    let empty = search("Missing");
    let page = index
        .page_in(Stage::Todo, 2, &Anchor::Start, Some(&empty))
        .unwrap();
    assert!(page.rows.is_empty());
    assert!(page.next.is_none());
    assert!(index
        .counts_in(Some(&empty))
        .unwrap()
        .values()
        .all(|n| *n == 0));
}

#[test]
fn search_is_literal_and_intersects_session_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    for (n, target, text) in [
        (1, Target::Session(agent()), "100%_literal's text"),
        (2, Target::Any, "100%_literal's text"),
        (
            3,
            Target::Session(SessionKey {
                session_id: "run_id:other".into(),
                ..agent()
            }),
            "100%_literal's text",
        ),
        (4, Target::Session(agent()), "100 percent unrelated"),
    ] {
        let mut task = new_task(
            &store,
            &format!("T-aaa{n:03}"),
            Stage::Todo,
            &format!("V{n}"),
            text,
        );
        task.set_target(&target, &Actor::Person, 2).unwrap();
        aged_save(&store, &mut task);
    }
    let owner = Scope::new(&agent().machine, &agent().session_id);
    let filter = Scope::searched(Some(owner), "100%_literal's", "orchard");
    let mut index = Index::open(&store).unwrap();
    let page = index
        .page_in(Stage::Todo, 2, &Anchor::Start, Some(&filter))
        .unwrap();
    assert_eq!(ids(&page), ["T-aaa001", "T-aaa002"]);
    assert_eq!(index.counts_in(Some(&filter)).unwrap()["todo"], 2);
    let fallback = super::super::super::documents_page_in(
        &store,
        Stage::Todo,
        2,
        &Anchor::Start,
        None,
        Some(&filter),
    )
    .unwrap();
    assert_eq!(fallback["tasks"], json!(page.rows));
}

#[test]
fn search_updates_when_text_beyond_the_preview_changes() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(
        &store,
        "T-aaaa01",
        Stage::Todo,
        "V",
        &format!("{}Old needle", "x".repeat(TEXT_BYTES + 100)),
    );
    aged_save(&store, &mut task);
    let mut index = Index::open(&store).unwrap();
    assert_eq!(
        ids(&index
            .page_in(Stage::Todo, 2, &Anchor::Start, Some(&search("old needle")))
            .unwrap()),
        ["T-aaaa01"]
    );
    drop(index);
    task.edit_text(
        &format!("{}New needle", "x".repeat(TEXT_BYTES + 100)),
        &Actor::Person,
        3,
    )
    .unwrap();
    aged_save(&store, &mut task);
    let mut index = Index::open(&store).unwrap();
    assert!(index
        .page_in(Stage::Todo, 2, &Anchor::Start, Some(&search("old needle")))
        .unwrap()
        .rows
        .is_empty());
    assert_eq!(
        ids(&index
            .page_in(Stage::Todo, 2, &Anchor::Start, Some(&search("new needle")))
            .unwrap()),
        ["T-aaaa01"]
    );
}
