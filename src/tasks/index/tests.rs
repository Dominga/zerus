use super::*;
use std::cell::Cell;
use std::time::Instant;
use zerus_replica::{NewTask, SessionKey, Target, TaskAttachment};

/// What `f` decoded and stamped.
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    DECODED.with(|n| n.set(0));
    STAMPED.with(|n| n.set(0));
    let out = f();
    (out, DECODED.with(Cell::get), STAMPED.with(Cell::get))
}

fn new_task(store: &ProjectStore, id: &str, stage: Stage, rank: &str, text: &str) -> TaskDoc {
    TaskDoc::create(NewTask {
        project: store.project_id(),
        id,
        text,
        stage,
        target: Target::Any,
        author: Actor::Person,
        rank,
        now: 1,
    })
    .unwrap()
}

fn agent() -> SessionKey {
    SessionKey {
        machine: "example-computer".into(),
        session: "codex/example/worker".into(),
        session_id: "run_id:example".into(),
    }
}

/// Dates a task's files an hour back, so they are settled: the bounded-work
/// tests count reads without waiting for `SETTLE_MS`.
fn age(store: &ProjectStore, uuid: &str) {
    let past = std::time::SystemTime::now() - Duration::from_secs(3600);
    for ext in ["loro", "updates"] {
        let path = store.dir().join("tasks").join(format!("{uuid}.{ext}"));
        if let Ok(file) = fs::File::options().write(true).open(path) {
            file.set_modified(past).unwrap();
        }
    }
}

/// Saves `task` through the store, then dates its files back.
fn aged_save(store: &ProjectStore, task: &mut TaskDoc) {
    store.save(task).unwrap();
    age(store, task.uuid());
}

/// Writes `task` as a whole snapshot, as an older hgs could, without the
/// change journal and without syncing; its file is dated back.
fn write_plain(store: &ProjectStore, task: &TaskDoc) {
    let path = store.dir().join("tasks").join(format!("{}.loro", task.uuid()));
    fs::write(path, task.snapshot().unwrap()).unwrap();
    age(store, task.uuid());
}

/// A project in every stage: equal ranks, a long multi-byte text, files,
/// agent starts, and one task file that does not load.
fn project(temp: &Path) -> ProjectStore {
    let store = ProjectStore::open(temp, "example").unwrap();
    let long = "é".repeat(TEXT_BYTES);
    for (n, (stage, rank)) in [
        (Stage::Todo, "V"),
        (Stage::Todo, "U"),
        (Stage::Todo, "U"),
        (Stage::Todo, "a"),
        (Stage::Todo, "W"),
        (Stage::Backlog, "V"),
        (Stage::Backlog, "U"),
    ]
    .into_iter()
    .enumerate()
    {
        let text = if n == 1 { long.clone() } else { format!("Task {n}") };
        let mut task = new_task(&store, &format!("T-aaaa{n:02}"), stage, rank, &text);
        if n == 3 {
            let file = TaskAttachment {
                name: "a.txt".into(),
                mime: "text/plain".into(),
                data_base64: "aGk=".into(),
                reference: "[File #1]".into(),
            };
            task.set_attachments(&[file], &Actor::Person, 2).unwrap();
        }
        store.save(&mut task).unwrap();
    }
    for (n, end) in [(10, Stage::Active), (11, Stage::Review), (12, Stage::Done), (13, Stage::Done)] {
        let mut task = new_task(&store, &format!("T-aaaa{n}"), Stage::Todo, "V", "Sent");
        task.record_send(&agent(), &format!("request-{n}"), &Actor::Agent(agent()), now())
            .unwrap();
        if end != Stage::Active {
            task.record_report("Result", &Actor::Agent(agent()), now()).unwrap();
        }
        if end == Stage::Done {
            task.set_stage(Stage::Done, &Actor::Person, now()).unwrap();
        }
        store.save(&mut task).unwrap();
    }
    let damaged = store.dir().join("tasks").join(format!("{}.loro", ids::new_uuid()));
    fs::write(damaged, b"not a task").unwrap();
    store
}

fn all_pages(store: &ProjectStore, stage: Stage, limit: usize) -> Vec<Value> {
    let mut index = Index::open(store).unwrap();
    let mut anchor = Anchor::Start;
    let mut rows = Vec::new();
    loop {
        let page = index.page(stage, limit, &anchor).unwrap();
        let documents = super::super::documents_page(store, stage, limit, &anchor, None).unwrap();
        assert_eq!(json!(page.rows), documents["tasks"], "{stage:?} {limit}");
        assert_eq!(json!(page.next), documents["page"]["next"]);
        assert_eq!(json!(page.previous), documents["page"]["previous"]);
        assert_eq!(json!(page.position), documents["page"]["position"]);
        assert_eq!(page.position, rows.len());
        rows.extend(page.rows);
        match page.next {
            Some(next) => {
                let (rank, uuid) = parse_cursor(store.project_id(), stage, &next).unwrap();
                anchor = Anchor::After(rank, uuid);
            }
            None => break,
        }
    }
    rows
}

#[test]
fn pages_hold_the_documents_rows_in_each_stage_s_order() {
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    let (tasks, damaged) = store.listing().unwrap();
    assert_eq!(damaged.len(), 1);
    for stage in [Stage::Review, Stage::Active, Stage::Todo, Stage::Backlog, Stage::Done] {
        let expected: Vec<Value> = tasks
            .iter()
            .filter(|t| t.stage().unwrap() == stage)
            .map(|t| capped(row(t.view().unwrap(), t.revision()).unwrap()))
            .collect();
        for limit in 1..=4 {
            assert_eq!(all_pages(&store, stage, limit), expected, "{stage:?} {limit}");
        }
    }
    let index = Index::open(&store).unwrap();
    let counts = index.counts().unwrap();
    assert_eq!(counts["todo"], 5);
    assert_eq!(counts["done"], 2);
    let (shown, total) = index.damaged().unwrap();
    assert_eq!((shown.len(), total), (1, 1));
    assert_eq!(shown[0]["uuid"], json!(damaged[0].uuid));
    assert_eq!(shown[0]["error"], json!(capped_error(&damaged[0].error)));
    // The long text is cut at a character boundary and marked.
    let long = tasks.iter().find(|t| t.id() == "T-aaaa01").unwrap();
    let listed = capped(row(long.view().unwrap(), long.revision()).unwrap());
    assert!(listed["text"].as_str().unwrap().len() <= TEXT_BYTES);
    assert_eq!(listed["text_truncated"], json!(true));
    // Lookups answer as the store does.
    assert_eq!(index.agent_starts_since(0).unwrap(), store.agent_starts_since(0).unwrap());
    assert_eq!(index.agent_starts_since(i64::MAX).unwrap(), 0);
    for task in &tasks {
        let stage = task.stage().unwrap();
        let place = tasks.iter().filter(|t| t.stage().unwrap() == stage).position(|t| t.uuid() == task.uuid()).unwrap() + 1;
        assert_eq!(index.place(task.uuid()).unwrap(), Some(place));
        assert_eq!(index.short_id(&task.id()).unwrap(), (vec![task.uuid().to_owned()], true));
    }
    assert_eq!(index.short_id("T-zzzzzz").unwrap(), (vec![], true));
}

#[test]
fn pages_go_back_and_a_cursor_holds_its_place_as_tasks_arrive() {
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    let project = store.project_id().to_owned();
    let mut index = Index::open(&store).unwrap();
    let first = index.page(Stage::Todo, 2, &Anchor::Start).unwrap();
    let next = first.next.clone().unwrap();
    let (rank, uuid) = parse_cursor(&project, Stage::Todo, &next).unwrap();
    let second = index.page(Stage::Todo, 2, &Anchor::After(rank.clone(), uuid.clone())).unwrap();
    // Back from the second page is the first page again.
    let (rank0, uuid0) = parse_cursor(&project, Stage::Todo, second.previous.as_ref().unwrap()).unwrap();
    let back = index.page(Stage::Todo, 2, &Anchor::Before(rank0, uuid0)).unwrap();
    assert_eq!(back.rows, first.rows);
    assert_eq!(back.previous, None);
    drop(index);
    // A task added at the top does not shift the page after a cursor.
    let mut top = new_task(&store, "T-bbbbbb", Stage::Todo, "1", "New");
    store.save(&mut top).unwrap();
    let mut index = Index::open(&store).unwrap();
    let again = index.page(Stage::Todo, 2, &Anchor::After(rank, uuid)).unwrap();
    assert_eq!(again.rows, second.rows);
    assert_eq!(again.position, second.position + 1);
    // Cursors name their project and stage.
    assert!(parse_cursor(&project, Stage::Backlog, &next).is_err());
    assert!(parse_cursor("another", Stage::Todo, &next).is_err());
    for garbage in ["", "t1", "t1.x.y.z", &next.replace("t1.", "t2."), &format!("{next}.extra")] {
        assert!(parse_cursor(&project, Stage::Todo, garbage).is_err(), "{garbage}");
    }
    // Past either end: the nearest whole page, from the index and the documents alike.
    let all = index.page(Stage::Todo, 200, &Anchor::Start).unwrap();
    let key = |row: &Value| (row["rank"].as_str().unwrap().to_owned(), row["uuid"].as_str().unwrap().to_owned());
    let (rank, uuid) = key(all.rows.last().unwrap());
    let past = Anchor::After(rank, uuid);
    let page = index.page(Stage::Todo, 2, &past).unwrap();
    assert_eq!(page.rows, all.rows[all.rows.len() - 2..]);
    assert!(page.next.is_none() && page.previous.is_some());
    assert_eq!(page.position, all.rows.len() - 2);
    let documents = super::super::documents_page(&store, Stage::Todo, 2, &past, None).unwrap();
    assert_eq!(documents["tasks"], json!(page.rows));
    assert_eq!(documents["page"]["previous"], json!(page.previous));
    let (rank, uuid) = key(&all.rows[0]);
    let before = Anchor::Before(rank, uuid);
    let page = index.page(Stage::Todo, 2, &before).unwrap();
    assert_eq!(page.rows, all.rows[..2]);
    assert!(page.next.is_some() && page.previous.is_none() && page.position == 0);
    let documents = super::super::documents_page(&store, Stage::Todo, 2, &before, None).unwrap();
    assert_eq!(documents["tasks"], json!(page.rows));
    assert_eq!(documents["page"]["next"], json!(page.next));
    // A one-row stage clamps to that row; an empty stage has an empty page.
    let one = index.page(Stage::Review, 2, &Anchor::After("z".into(), ids::new_uuid())).unwrap();
    assert!(one.rows.len() == 1 && one.next.is_none() && one.previous.is_none());
    drop(index);
    let empty = tempfile::tempdir().unwrap();
    let empty = ProjectStore::open(empty.path(), "empty").unwrap();
    let mut index = Index::open(&empty).unwrap();
    let none = index.page(Stage::Todo, 2, &Anchor::After("V".into(), ids::new_uuid())).unwrap();
    assert!(none.rows.is_empty() && none.next.is_none() && none.previous.is_none() && none.position == 0);
    assert_eq!(index.counts().unwrap().get("todo"), None);
}

#[test]
fn reads_decode_only_what_changed() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut tasks: Vec<TaskDoc> = (0..300)
        .map(|n| new_task(&store, &format!("T-{n:06}"), Stage::Todo, "V", "Plain"))
        .collect();
    for task in &tasks {
        write_plain(&store, task);
    }
    let (_, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 300, "the first read builds the index");
    // Nothing changed: no task is decoded or stamped.
    let (_, decoded, stamped) = counted(|| Index::open(&store).unwrap());
    assert_eq!((decoded, stamped), (0, 1), "only the task folder is stamped");
    // The index started the journal: a saved change is found through it,
    // stamping only the folder and that task's two files.
    aged_save(&store, &mut tasks[7]);
    let (_, decoded, stamped) = counted(|| Index::open(&store).unwrap());
    assert_eq!((decoded, stamped), (1, 3));
    tasks[7].comment("note", &Actor::Person, 5).unwrap();
    aged_save(&store, &mut tasks[7]);
    let (index, decoded, stamped) = counted(|| Index::open(&store).unwrap());
    assert_eq!((decoded, stamped), (1, 3), "the folder and the changed task's two files");
    let revision = index.revision().unwrap();
    drop(index);
    // A writer without the journal, in place: a page's rows are checked
    // before they are served, so the changed one is indexed again.
    tasks[0].set_title("Retitled", &Actor::Person, 6).unwrap();
    write_plain(&store, &tasks[0]);
    let mut index = Index::open(&store).unwrap();
    let (page, decoded, _) = counted(|| index.page(Stage::Todo, 300, &Anchor::Start).unwrap());
    assert_eq!(decoded, 1);
    let listed = page.rows.iter().find(|r| r["uuid"] == json!(tasks[0].uuid())).unwrap();
    assert_eq!(listed["title"], json!("Retitled"));
    assert_ne!(index.revision().unwrap(), revision);
    // ... and the periodic full check finds one outside any page.
    tasks[1].set_title("Elsewhere", &Actor::Person, 7).unwrap();
    write_plain(&store, &tasks[1]);
    index.db.execute("UPDATE meta SET value='0' WHERE key='checked'", []).unwrap();
    drop(index);
    let (_, decoded, stamped) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 1);
    assert!(stamped >= 600, "{stamped}");
    // A new task file from such a writer changes the folder: a full check.
    let extra = new_task(&store, "T-extra1", Stage::Todo, "Z", "Late");
    write_plain(&store, &extra);
    let (index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 1);
    assert_eq!(index.counts().unwrap()["todo"], 301);
}

/// Windows has no inode and coarse file times: a rewrite of the same length
/// can keep its stamp. A task the journal names is indexed again anyway.
#[test]
fn a_task_the_journal_names_is_indexed_again_even_with_an_unchanged_stamp() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(&store, "T-aaaaaa", Stage::Todo, "V", "Plain");
    aged_save(&store, &mut task);
    let mut other = new_task(&store, "T-bbbbbb", Stage::Todo, "W", "Other");
    aged_save(&store, &mut other);
    drop(Index::open(&store).unwrap());
    task.set_title("Retitled", &Actor::Person, 5).unwrap();
    aged_save(&store, &mut task);
    // As if the rewritten files kept their stamps: the index already holds
    // the new stamps beside the old row. No index connection is open here.
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute(
            "UPDATE tasks SET files=?1 WHERE uuid=?2",
            params![files(store.dir(), task.uuid()), task.uuid()],
        )
        .unwrap();
    }
    let (mut index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 1, "the journal names it");
    let page = index.page(Stage::Todo, 10, &Anchor::Start).unwrap();
    let row = page.rows.iter().find(|r| r["uuid"] == json!(task.uuid())).unwrap();
    assert_eq!(row["title"], json!("Retitled"));
}

/// review-n on #1350: when the journal can no longer say what changed, a
/// current writer's change whose files kept their stamps is still found:
/// every row read before then is suspect, a page never serves one unread,
/// and each read reads a bounded batch of them again.
#[test]
fn an_unknown_journal_makes_every_earlier_row_suspect() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut tasks: Vec<TaskDoc> = (0..300)
        .map(|n| new_task(&store, &format!("T-{n:06}"), Stage::Todo, "V", "Plain"))
        .collect();
    for task in &mut tasks {
        aged_save(&store, task);
    }
    drop(Index::open(&store).unwrap());
    let (index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 0);
    let before = index.revision().unwrap();
    drop(index);
    // Nothing changed, but the journal is unknown: rows are read again in
    // batches, and the revision stays.
    fs::write(store.dir().join("changes"), b"damaged").unwrap();
    let (index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, BATCH as usize);
    assert_eq!(index.revision().unwrap(), before, "the rows read the same");
    drop(index);
    let (index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 300 - BATCH as usize);
    assert_eq!(index.revision().unwrap(), before);
    drop(index);
    // Now a change whose files keep their stamps, then the journal is lost.
    tasks[250].set_title("Retitled", &Actor::Person, 9).unwrap();
    aged_save(&store, &mut tasks[250]);
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute(
            "UPDATE tasks SET files=?1 WHERE uuid=?2",
            params![files(store.dir(), tasks[250].uuid()), tasks[250].uuid()],
        )
        .unwrap();
    }
    fs::remove_file(store.dir().join("changes")).unwrap();
    let (mut index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert!(decoded <= BATCH as usize, "{decoded}");
    let page = index.page(Stage::Todo, 200, &Anchor::Start).unwrap();
    let all = index.page(Stage::Todo, 200, &Anchor::End).unwrap();
    let found = page.rows.iter().chain(&all.rows).find(|r| r["uuid"] == json!(tasks[250].uuid())).unwrap();
    assert_eq!(found["title"], json!("Retitled"));
    drop(index);
    let index = Index::open(&store).unwrap();
    let floor = index.floor(now()).unwrap();
    let suspect: i64 = index.db.query_row("SELECT count(*) FROM tasks WHERE decoded < ?1", [floor], |r| r.get(0)).unwrap();
    assert_eq!(suspect, 0, "every row was read again");
}

/// A row read longer ago than the maximum age is read again before a page
/// serves it, whatever its stamps; one that reads the same keeps the revision.
#[test]
fn rows_past_the_maximum_age_are_read_again() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let tasks: Vec<TaskDoc> = (0..20)
        .map(|n| new_task(&store, &format!("T-{n:06}"), Stage::Todo, "V", "Plain"))
        .collect();
    for task in &tasks {
        write_plain(&store, task);
    }
    let revision = Index::open(&store).unwrap().revision().unwrap();
    let aged = |sql: &str| {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute(sql, params![now() - MAX_AGE_MS - 1000]).unwrap();
    };
    aged("UPDATE tasks SET decoded=?1");
    let (index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 20);
    assert_eq!(index.revision().unwrap(), revision);
    drop(index);
    // An unnoticed change on an aged row: the page reads it first.
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute("UPDATE tasks SET row=json_set(row, '$.text', 'Stale') WHERE uuid=?1", [tasks[5].uuid()]).unwrap();
    }
    let mut index = Index::open(&store).unwrap();
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute("UPDATE tasks SET decoded=?1 WHERE uuid=?2", params![now() - MAX_AGE_MS - 1000, tasks[5].uuid()]).unwrap();
    }
    let (page, decoded, _) = counted(|| index.page(Stage::Todo, 50, &Anchor::Start).unwrap());
    assert_eq!(decoded, 1);
    let row = page.rows.iter().find(|r| r["uuid"] == json!(tasks[5].uuid())).unwrap();
    assert_eq!(row["text"], json!("Plain"));
}

/// A file modified within `SETTLE_MS` could change again in the same clock
/// tick: its row stays suspect and is read again.
#[test]
fn an_unsettled_row_is_read_again() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(&store, "T-aaaaaa", Stage::Todo, "V", "Plain");
    store.save(&mut task).unwrap();
    let (_, settled) = stamped(store.dir(), task.uuid(), now());
    assert!(!settled);
    assert!(stamped(store.dir(), task.uuid(), now() + SETTLE_MS + 1000).1);
    let index = Index::open(&store).unwrap();
    let decoded: i64 = index.db.query_row("SELECT decoded FROM tasks WHERE uuid=?1", [task.uuid()], |r| r.get(0)).unwrap();
    assert_eq!(decoded, 0, "kept suspect");
    drop(index);
    let (_, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 1, "read again");
}

/// review-n on #1350: `--if-revision` answers only after the page's rows
/// were checked against their files.
#[test]
fn an_unchanged_revision_is_only_answered_after_the_page_check() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut tasks: Vec<TaskDoc> = (0..5)
        .map(|n| new_task(&store, &format!("T-{n:06}"), Stage::Todo, "V", "Plain"))
        .collect();
    for task in &tasks {
        write_plain(&store, task);
    }
    let mut index = Index::open(&store).unwrap();
    let first = super::super::indexed_page(&mut index, Stage::Todo, 10, &Anchor::Start, None).unwrap();
    let revision = first["revision"].as_str().unwrap().to_owned();
    let unchanged = super::super::indexed_page(&mut index, Stage::Todo, 10, &Anchor::Start, Some(&revision)).unwrap();
    assert_eq!(unchanged["unchanged"], json!(true));
    drop(index);
    // An older writer changes a task in place, without the journal.
    tasks[0].set_title("Retitled", &Actor::Person, 5).unwrap();
    write_plain(&store, &tasks[0]);
    let mut index = Index::open(&store).unwrap();
    let polled = super::super::indexed_page(&mut index, Stage::Todo, 10, &Anchor::Start, Some(&revision)).unwrap();
    assert!(polled.get("unchanged").is_none(), "{polled}");
    let row = polled["tasks"].as_array().unwrap().iter().find(|r| r["uuid"] == json!(tasks[0].uuid())).unwrap();
    assert_eq!(row["title"], json!("Retitled"));
}

/// review-n on #1350: a cursor's rank must be ASCII hex before it is cut.
#[test]
fn a_cursor_with_a_non_hex_rank_is_refused_without_panicking() {
    let uuid = ids::new_uuid();
    for rank in ["aéa", "zz", "4", "é0"] {
        let cursor = format!("t1.{}.{rank}.{uuid}", scope("example", Stage::Todo));
        assert!(parse_cursor("example", Stage::Todo, &cursor).is_err(), "{rank}");
    }
    let good = cursor("example", Stage::Todo, "V", &uuid);
    assert_eq!(parse_cursor("example", Stage::Todo, &good).unwrap(), ("V".to_owned(), uuid));
}

#[test]
fn a_damaged_foreign_or_shared_index_is_built_again() {
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    let path = store.dir().join(FILE);
    let expected = |store: &ProjectStore| {
        let mut index = Index::open(store).expect("the index is usable again");
        index.page(Stage::Todo, 200, &Anchor::Start).unwrap().rows
    };
    let good = expected(&store);
    // Not a database.
    fs::write(&path, b"garbage that is not SQLite").unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = fs::remove_file(store.dir().join(format!("{FILE}{suffix}")));
    }
    assert_eq!(expected(&store), good);
    // Another schema version, and a missing table.
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA user_version=9").unwrap();
    }
    assert_eq!(expected(&store), good);
    {
        let db = Connection::open(&path).unwrap();
        db.execute_batch("DROP TABLE tasks").unwrap();
    }
    assert_eq!(expected(&store), good);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Readable by others: removed and built again owner-only.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(expected(&store), good);
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        // A link in its place is removed; what it names is never touched.
        let outside = temp.path().join("outside");
        fs::write(&outside, b"kept").unwrap();
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(store.dir().join(format!("{FILE}{suffix}")));
        }
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert_eq!(expected(&store), good);
        assert_eq!(fs::read(&outside).unwrap(), b"kept");
        assert!(!fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
    }
}

#[test]
fn queries_use_their_indexes() {
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    let index = Index::open(&store).unwrap();
    let plan = |sql: &str| -> String {
        let mut query = index.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        query
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect::<Vec<_>>()
            .join("; ")
    };
    for (sql, wanted) in [
        ("SELECT uuid, rank, files, row FROM tasks WHERE stage='todo' AND (rank, uuid) > ('V', 'u') ORDER BY rank, uuid LIMIT 3", "tasks_order"),
        ("SELECT uuid, rank, files, row FROM tasks WHERE stage='todo' AND (rank, uuid) < ('V', 'u') ORDER BY rank DESC, uuid DESC LIMIT 3", "tasks_order"),
        ("SELECT count(*) FROM tasks WHERE stage='todo' AND (rank, uuid) < ('V', 'u')", "tasks_order"),
        ("SELECT uuid FROM tasks WHERE short_id='T-aaaa01'", "tasks_short"),
        ("SELECT count(*) FROM sends WHERE at >= 5", "sends_at"),
    ] {
        let found = plan(sql);
        assert!(found.contains(wanted) && !found.contains("TEMP B-TREE"), "{sql}: {found}");
    }
}

/// zerus-iiee: a second index open in this process never closes a raw
/// descriptor of the live file, so the live connection keeps its locks.
#[cfg(unix)]
#[test]
fn opening_the_index_again_keeps_a_live_index_s_locks() {
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    let path = store.dir().join(FILE);
    let live = Index::open(&store).unwrap();
    live.db.execute_batch("BEGIN").unwrap();
    assert_eq!(live.counts().unwrap()["todo"], 5);
    assert!(crate::sqlite_file::foreign_lock_refused(&path), "the live index holds its lock");
    for _ in 0..2 {
        let again = Index::open(&store).expect("a second index opens beside a reader");
        assert_eq!(again.counts().unwrap()["todo"], 5);
        drop(again);
        assert!(crate::sqlite_file::foreign_lock_refused(&path), "opening again kept the lock");
    }
    drop(fs::File::open(&path).unwrap());
    assert!(
        !crate::sqlite_file::foreign_lock_refused(&path),
        "the control: a closed raw descriptor dropped the lock"
    );
    live.db.execute_batch("COMMIT").unwrap();
}

/// A fixture of `count` To do tasks written directly, then indexed once.
fn scaled(temp: &Path, count: usize) -> ProjectStore {
    let store = ProjectStore::open(temp, "example").unwrap();
    for (n, rank) in zerus_replica::rank::spread(count).iter().enumerate() {
        write_plain(&store, &new_task(&store, &format!("T-{n:06}"), Stage::Todo, rank, "Plain"));
    }
    store
}

#[test]
fn a_large_project_pages_and_polls_with_bounded_work() {
    let temp = tempfile::tempdir().unwrap();
    let store = scaled(temp.path(), 5_000);
    let (_, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 5_000);
    // A poll with nothing changed: one stamp, no task decoded.
    let (index, decoded, stamped) = counted(|| Index::open(&store).unwrap());
    assert_eq!((decoded, stamped), (0, 1));
    let revision = index.revision().unwrap();
    drop(index);
    // A page reads its rows and checks only their files.
    let mut index = Index::open(&store).unwrap();
    let (page, decoded, stamped) = counted(|| index.page(Stage::Todo, 50, &Anchor::Start).unwrap());
    assert_eq!((page.rows.len(), decoded, stamped), (50, 0, 100));
    assert_eq!(index.revision().unwrap(), revision);
    let deep = page.next.unwrap();
    let (rank, uuid) = parse_cursor(store.project_id(), Stage::Todo, &deep).unwrap();
    let (page, decoded, _) = counted(|| index.page(Stage::Todo, 50, &Anchor::After(rank, uuid)).unwrap());
    assert_eq!((page.position, decoded), (50, 0));
}

/// design-tima's scale smoke: run it in release, with
/// `cargo test --release --bin hgs -- --ignored --exact tasks::index::tests::twenty_thousand_tasks_scale_smoke --nocapture`.
#[test]
#[ignore = "scale smoke; run explicitly in release"]
fn twenty_thousand_tasks_scale_smoke() {
    let temp = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let store = scaled(temp.path(), 20_000);
    let fixture = started.elapsed();
    let started = Instant::now();
    let (_, decoded, _) = counted(|| Index::open(&store).unwrap());
    let build = started.elapsed();
    assert_eq!(decoded, 20_000);
    let revision = {
        let mut index = Index::open(&store).unwrap();
        super::super::indexed_page(&mut index, Stage::Todo, 50, &Anchor::Start, None).unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let mut polls = Vec::new();
    for _ in 0..20 {
        // `--if-revision`: open, check the page's rows, answer unchanged.
        let started = Instant::now();
        let (answer, decoded, stamped) = counted(|| {
            let mut index = Index::open(&store).unwrap();
            super::super::indexed_page(&mut index, Stage::Todo, 50, &Anchor::Start, Some(&revision)).unwrap()
        });
        polls.push(started.elapsed());
        assert_eq!(answer["unchanged"], json!(true));
        assert_eq!((decoded, stamped), (0, 101), "the poll stamps the folder and its page");
    }
    let started = Instant::now();
    let mut index = Index::open(&store).unwrap();
    let (page, decoded, stamped) = counted(|| index.page(Stage::Todo, 50, &Anchor::Start).unwrap());
    let first_page = started.elapsed();
    assert_eq!((page.rows.len(), decoded, stamped), (50, 0, 100));
    let counts = index.counts().unwrap();
    assert_eq!(counts["todo"], 20_000);
    drop(index);
    let started = Instant::now();
    let legacy = store.listing().unwrap().0.len();
    let full_decode = started.elapsed();
    assert_eq!(legacy, 20_000);
    polls.sort();
    println!(
        "scale smoke 20000 tasks: fixture {fixture:?}, index build {build:?}, poll median {:?} max {:?}, first page {first_page:?}, legacy full decode {full_decode:?}",
        polls[polls.len() / 2],
        polls[polls.len() - 1]
    );
    assert!(first_page < full_decode / 10, "{first_page:?} vs {full_decode:?}");
}
