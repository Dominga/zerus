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
    for ext in ["loro", "updates", "managed"] {
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
    // stamping only the folder and that task's three files (snapshot,
    // updates and mark).
    aged_save(&store, &mut tasks[7]);
    let (_, decoded, stamped) = counted(|| Index::open(&store).unwrap());
    assert_eq!((decoded, stamped), (1, 4));
    tasks[7].comment("note", &Actor::Person, 5).unwrap();
    aged_save(&store, &mut tasks[7]);
    let (index, decoded, stamped) = counted(|| Index::open(&store).unwrap());
    assert_eq!((decoded, stamped), (1, 4), "the folder and the changed task's three files");
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
    let suspect: i64 = index.db.query_row("SELECT count(*) FROM tasks WHERE checked < ?1 OR trusted = 0", [floor], |r| r.get(0)).unwrap();
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
    aged("UPDATE tasks SET checked=?1");
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
        db.execute("UPDATE tasks SET checked=?1 WHERE uuid=?2", params![now() - MAX_AGE_MS - 1000, tasks[5].uuid()]).unwrap();
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
    let trusted: bool = index.db.query_row("SELECT trusted FROM tasks WHERE uuid=?1", [task.uuid()], |r| r.get(0)).unwrap();
    assert!(!trusted, "kept suspect");
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

/// review-n on #1350 R2: a clock set back must not make earlier reads look
/// recent. Reads dated after now are suspect, and so is a suspect mark.
#[test]
fn a_clock_set_back_leaves_no_row_trusted_by_its_future_date() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let tasks: Vec<TaskDoc> = (0..20)
        .map(|n| new_task(&store, &format!("T-{n:06}"), Stage::Todo, "V", "Plain"))
        .collect();
    for task in &tasks {
        write_plain(&store, task);
    }
    drop(Index::open(&store).unwrap());
    // As if every read and mark happened an hour ahead of the clock now, and
    // one row changed unnoticed meanwhile.
    let ahead = now() + 3_600_000;
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute("UPDATE tasks SET checked=?1", [ahead]).unwrap();
        db.execute("INSERT OR REPLACE INTO meta VALUES('suspect', ?1)", [ahead.to_string()]).unwrap();
        db.execute("UPDATE meta SET value=?1 WHERE key='checked'", [ahead.to_string()]).unwrap();
        db.execute("UPDATE tasks SET row=json_set(row, '$.text', 'Stale') WHERE uuid=?1", [tasks[3].uuid()]).unwrap();
    }
    let (mut index, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 20, "every future-dated read is read again");
    let page = index.page(Stage::Todo, 50, &Anchor::Start).unwrap();
    let row = page.rows.iter().find(|r| r["uuid"] == json!(tasks[3].uuid())).unwrap();
    assert_eq!(row["text"], json!("Plain"));
    let mark: i64 = index.meta("suspect").unwrap().unwrap().parse().unwrap();
    assert!(mark <= now(), "the suspect mark is not left in the future");
    // And it settles: the next read decodes nothing.
    drop(index);
    let (_, decoded, _) = counted(|| Index::open(&store).unwrap());
    assert_eq!(decoded, 0);
}

/// review-n on #1350 R2: the size limit is in bytes, whatever the page size.
#[test]
fn the_index_size_limit_is_in_bytes_whatever_its_page_size() {
    assert_eq!(budget(4096, 10), Some(65_536));
    assert_eq!(budget(8192, 10), Some(32_768));
    assert_eq!(budget(8192, 40_000), None, "over the budget");
    assert_eq!(budget(0, 1), None);
    assert_eq!(budget(-4096, 1), None);
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    drop(Index::open(&store).unwrap());
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA page_size=8192; VACUUM;").unwrap();
        let size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap();
        assert_eq!(size, 8192);
    }
    let index = Index::open(&store).unwrap();
    let pages: i64 = index.db.query_row("PRAGMA max_page_count", [], |r| r.get(0)).unwrap();
    assert_eq!(pages * 8192, BUDGET_BYTES);
}

/// review-n's starvation finding on #1350 R2: the background queue is
/// ordered by when each row was last read, apart from whether it is trusted.
/// Rows that stay unsettled (here, files dated in the future) go to the back
/// each time they are read, so an expired row whose stage changed unnoticed
/// is reached, and its stage count and membership become right.
#[test]
fn permanently_unsettled_rows_cannot_starve_an_expired_row() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut tasks: Vec<TaskDoc> = (0..300)
        .map(|n| new_task(&store, &format!("T-{n:06}"), Stage::Backlog, "V", "Plain"))
        .collect();
    for task in &tasks {
        write_plain(&store, task);
    }
    drop(Index::open(&store).unwrap());
    // 260 tasks whose files are dated a day ahead: never settled.
    let ahead = std::time::SystemTime::now() + Duration::from_secs(86_400);
    for task in &tasks[..260] {
        let path = store.dir().join("tasks").join(format!("{}.loro", task.uuid()));
        fs::File::options().write(true).open(path).unwrap().set_modified(ahead).unwrap();
    }
    // The last one moves to To do; its new files keep the stamps the index holds.
    let moved = 299;
    tasks[moved].set_stage(Stage::Todo, &Actor::Person, 5).unwrap();
    write_plain(&store, &tasks[moved]);
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        for task in &tasks[..260] {
            db.execute("UPDATE tasks SET files=?1, checked=0 WHERE uuid=?2", params![files(store.dir(), task.uuid()), task.uuid()]).unwrap();
        }
        db.execute(
            "UPDATE tasks SET files=?1, checked=?2 WHERE uuid=?3",
            params![files(store.dir(), tasks[moved].uuid()), now() - MAX_AGE_MS - 5_000, tasks[moved].uuid()],
        )
        .unwrap();
    }
    let mut seen = false;
    for read in 0..3 {
        let mut index = Index::open(&store).unwrap();
        let page = index.page(Stage::Todo, 10, &Anchor::Start).unwrap();
        let count = index.counts().unwrap().get("todo").copied().unwrap_or(0);
        if page.rows.len() == 1 && count == 1 {
            assert_eq!(page.rows[0]["uuid"], json!(tasks[moved].uuid()));
            seen = true;
            assert!(read <= 1, "reached within ceil(261 / 256) reads, not {read}");
            break;
        }
    }
    assert!(seen, "the moved task never reached To do");
}

/// review-n on #1350 R3: a cached row that is not the object the index
/// wrote for that task is a damaged index, never served or labelled.
#[test]
fn a_cached_row_of_another_shape_is_damage() {
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    // Settled files: the page serves its cached rows without reading them again.
    for uuid in store.uuids().unwrap() {
        age(&store, &uuid);
    }
    let good = Index::open(&store).unwrap().page(Stage::Todo, 200, &Anchor::Start).unwrap().rows;
    let first = good[0]["uuid"].as_str().unwrap().to_owned();
    let other = good[1]["uuid"].as_str().unwrap().to_owned();
    for raw in [
        "true".to_owned(),
        "\"text\"".to_owned(),
        "[1, 2]".to_owned(),
        "{}".to_owned(),
        "not json".to_owned(),
        json!({"uuid": other, "stage": "todo", "id": "T-aaaa00"}).to_string(),
        json!({"uuid": first, "stage": "backlog", "id": "T-aaaa00"}).to_string(),
    ]
    .into_iter()
    .chain(
        // The real row with one identity field changed: review-n's R4 probe.
        [
            ("id", None),
            ("id", Some(json!(7))),
            ("id", Some(good[1]["id"].clone())),
            ("rank", Some(json!("zzz"))),
            ("uuid", Some(json!(other))),
        ]
        .into_iter()
        .map(|(field, value)| {
            let mut row = Index::open(&store).unwrap().page(Stage::Todo, 200, &Anchor::Start).unwrap().rows[0].clone();
            match value {
                Some(value) => row[field] = value,
                None => drop(row.as_object_mut().unwrap().remove(field)),
            }
            row.to_string()
        }),
    ) {
        let mut index = Index::open(&store).unwrap();
        index.db.execute("UPDATE tasks SET row=?1 WHERE uuid=?2", params![raw, first]).unwrap();
        assert!(matches!(index.page(Stage::Todo, 200, &Anchor::Start), Err(Failure::Damaged)), "{raw}");
        let polled = super::super::indexed_page(&mut index, Stage::Todo, 200, &Anchor::Start, None);
        assert!(matches!(polled, Err(Failure::Damaged)), "{raw}");
        index.discard(&Failure::Damaged);
        // Built again from the documents.
        assert_eq!(Index::open(&store).unwrap().page(Stage::Todo, 200, &Anchor::Start).unwrap().rows, good, "{raw}");
    }
}

/// The same for a cached history: a task the journal names is read again
/// even when its files kept their stamps.
#[test]
fn a_changed_task_s_history_is_read_again_even_with_an_unchanged_stamp() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(&store, "T-aaaaaa", Stage::Todo, "V", "Plain");
    aged_save(&store, &mut task);
    let history = |store: &ProjectStore, uuid: &str| super::super::history::page(store, uuid, 80, None, false).unwrap();
    history(&store, task.uuid());
    task.comment("later", &Actor::Person, 5).unwrap();
    aged_save(&store, &mut task);
    {
        let db = Connection::open(store.dir().join(FILE)).unwrap();
        db.execute(
            "UPDATE histories SET files=?1 WHERE uuid=?2",
            params![files(store.dir(), task.uuid()), task.uuid()],
        )
        .unwrap();
    }
    let page = history(&store, task.uuid());
    assert!(page["log"].as_array().unwrap().iter().any(|e| e["text"] == json!("later")), "{page}");
}

/// review-n on #1360: a cached history is held to the same trust rules as a
/// page's rows. With other suspect rows ahead in the background queue, an
/// expired or future-dated requested task is read again before its history
/// is served, even when an older writer's change kept its stamps.
#[test]
fn a_requested_history_is_revalidated_like_a_page_row() {
    for future in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let store = ProjectStore::open(temp.path(), "example").unwrap();
        let mut tasks: Vec<TaskDoc> = (0..300)
            .map(|n| new_task(&store, &format!("T-{n:06}"), Stage::Todo, "V", "Plain"))
            .collect();
        for task in &tasks {
            write_plain(&store, task);
        }
        let target = 299;
        let history = |store: &ProjectStore, uuid: &str| super::super::history::page(store, uuid, 80, None, false).unwrap();
        history(&store, tasks[target].uuid());
        // An older writer comments without the journal; its files keep the
        // stamps the index holds, for the row and for the cached history.
        tasks[target].comment("newer", &Actor::Person, 9).unwrap();
        write_plain(&store, &tasks[target]);
        let new_files = files(store.dir(), tasks[target].uuid());
        {
            let db = Connection::open(store.dir().join(FILE)).unwrap();
            db.execute("UPDATE tasks SET files=?1 WHERE uuid=?2", params![new_files, tasks[target].uuid()]).unwrap();
            db.execute("UPDATE histories SET files=?1 WHERE uuid=?2", params![new_files, tasks[target].uuid()]).unwrap();
            // 256 other rows wait ahead of it in the suspect queue.
            let long_ago = now() - 5 * MAX_AGE_MS;
            for task in &tasks[..256] {
                db.execute("UPDATE tasks SET checked=?1 WHERE uuid=?2", params![long_ago, task.uuid()]).unwrap();
            }
            let requested = if future { now() + 3_600_000 } else { now() - MAX_AGE_MS - 60_000 };
            db.execute("UPDATE tasks SET checked=?1 WHERE uuid=?2", params![requested, tasks[target].uuid()]).unwrap();
        }
        let page = history(&store, tasks[target].uuid());
        let texts: Vec<&str> = page["log"].as_array().unwrap().iter().filter_map(|e| e["text"].as_str()).collect();
        assert!(texts.contains(&"newer"), "future={future}: {texts:?}");
    }
}

/// The tasks window (zerus-cg5h.2 slice C): the first page of every stage,
/// the same from the index and from the documents read once.
#[cfg(unix)]
#[test]
fn a_window_is_the_first_page_of_every_stage_from_the_index_or_the_documents() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let store = project(temp.path());
    let indexed = super::super::window_pages(&store, 2).unwrap();
    assert_eq!(indexed.len(), 5);
    for (stage, page) in super::super::LIST_STAGES.iter().zip(&indexed) {
        let expected = super::super::documents_page(&store, *stage, 2, &Anchor::Start, None).unwrap();
        assert_eq!(page["tasks"], expected["tasks"], "{stage:?}");
        assert_eq!(page["page"]["total"], expected["page"]["total"], "{stage:?}");
        assert_eq!(page["page"]["next"], expected["page"]["next"], "{stage:?}");
    }
    // The folder is not private: the index cannot be used, the documents can.
    fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o755)).unwrap();
    let read = super::super::window_pages(&store, 2).unwrap();
    fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    for (page, from_documents) in indexed.iter().zip(&read) {
        assert_eq!(page["tasks"], from_documents["tasks"]);
        assert_eq!(page["page"], from_documents["page"]);
        assert_eq!(page["counts"], from_documents["counts"]);
    }
    assert!(read[0]["revision"].as_str().unwrap().starts_with("documents-"));
}

#[test]
fn a_window_keeps_a_task_a_later_page_moves_to_an_earlier_stage() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(&store, "T-aaaaaa", Stage::Todo, "V", "Plain");
    aged_save(&store, &mut task);
    let initial = super::super::window_pages(&store, 1).unwrap();
    assert_eq!(initial[2]["tasks"].as_array().unwrap().len(), 1);
    // An older hgs moves the cached To do task to Review before the next
    // window, without a journal note: only the To do page's check finds it.
    task.record_send(&agent(), "window-request", &Actor::Agent(agent()), now()).unwrap();
    task.record_report("Done; please review", &Actor::Agent(agent()), now() + 1).unwrap();
    write_plain(&store, &task);
    let pages = super::super::window_pages(&store, 1).unwrap();
    let expected = super::super::documents_page(&store, Stage::Review, 1, &Anchor::Start, None).unwrap();
    assert_eq!(pages[0]["tasks"], expected["tasks"]);
    assert_eq!(pages[0]["page"]["total"], json!(1));
    assert!(pages.iter().all(|p| p["revision"] == pages[0]["revision"] && p["counts"] == pages[0]["counts"]));
    let shown: usize = pages.iter().map(|p| p["tasks"].as_array().unwrap().len()).sum();
    assert_eq!(shown, 1, "the task is in exactly one stage of the window");
}

#[test]
fn a_window_is_read_again_until_its_pages_share_one_revision() {
    let pages = |revisions: &[&str]| -> Vec<Value> { revisions.iter().map(|r| json!({"revision": r})).collect() };
    let mut passes = vec![pages(&["a", "a", "b", "b", "b"]), pages(&["b"; 5])].into_iter();
    let mut reads = 0;
    let window = super::super::coherent_window(|| {
        reads += 1;
        passes.next().ok_or(())
    });
    assert_eq!((window, reads), (Ok(Some(pages(&["b"; 5]))), 2));
    // A window that keeps changing is not returned: the documents answer.
    let mut reads = 0;
    let window = super::super::coherent_window(|| {
        reads += 1;
        Ok::<_, ()>(pages(&[&reads.to_string(), "next", "next", "next", "next"]))
    });
    assert_eq!((window, reads), (Ok(None), super::super::WINDOW_ATTEMPTS));
    // A failed pass is not repeated.
    let mut reads = 0;
    let window = super::super::coherent_window(|| {
        reads += 1;
        Err::<Vec<Value>, _>("failed")
    });
    assert_eq!((window, reads), (Err("failed"), 1));
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
        ("SELECT id, payload FROM entries WHERE uuid='u' AND (at, ordinal) < (5, 1) ORDER BY at DESC, ordinal DESC LIMIT 3", "entries_place"),
        ("SELECT at, ordinal FROM entries WHERE uuid='u' AND id='1@2'", "entries_id"),
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
    assert_eq!((page.rows.len(), decoded, stamped), (50, 0, 150));
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
        assert_eq!((decoded, stamped), (0, 151), "the poll stamps the folder and its page");
    }
    let started = Instant::now();
    let mut index = Index::open(&store).unwrap();
    let (page, decoded, stamped) = counted(|| index.page(Stage::Todo, 50, &Anchor::Start).unwrap());
    let first_page = started.elapsed();
    assert_eq!((page.rows.len(), decoded, stamped), (50, 0, 150));
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

/// review-n on #1415 (R5): a save that fails after its mark advanced (the
/// append is refused) leaves the copy kept, and a warm index shows it as
/// damaged, never as its old healthy cached row.
#[cfg(unix)]
#[test]
fn a_mark_only_change_reaches_the_warm_listing() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(&store, "T-aaaaaa", Stage::Todo, "V", "Plain");
    assert!(task.floor().unwrap());
    aged_save(&store, &mut task);
    let uuid = task.uuid().to_owned();
    drop(Index::open(&store).unwrap());
    let updates = store.dir().join("tasks").join(format!("{uuid}.updates"));
    fs::set_permissions(&updates, fs::Permissions::from_mode(0o400)).unwrap();
    task.set_title("Never appended", &Actor::Person, 5).unwrap();
    let failed = store.save(&mut task).is_err();
    fs::set_permissions(&updates, fs::Permissions::from_mode(0o600)).unwrap();
    // A writer that ignores permissions cannot show this case.
    if !failed {
        return;
    }
    assert!(store.load(&uuid).is_err());
    assert_eq!(store.tasks().unwrap().1.len(), 1);
    let mut index = Index::open(&store).unwrap();
    let page = index.page(Stage::Todo, 10, &Anchor::Start).unwrap();
    assert!(page.rows.iter().all(|r| r["uuid"] != json!(uuid)), "{:?}", page.rows);
    let (shown, total) = index.damaged().unwrap();
    assert_eq!(total, 1);
    assert_eq!(shown[0]["uuid"], json!(uuid));
}

/// A mark changed without a journal note (a writer that does not journal)
/// is still found: the task's stamps include its mark.
#[test]
fn a_mark_changed_outside_the_journal_is_found_by_its_stamp() {
    let temp = tempfile::tempdir().unwrap();
    let store = ProjectStore::open(temp.path(), "example").unwrap();
    let mut task = new_task(&store, "T-aaaaaa", Stage::Todo, "V", "Plain");
    assert!(task.floor().unwrap());
    aged_save(&store, &mut task);
    let uuid = task.uuid().to_owned();
    drop(Index::open(&store).unwrap());
    let tasks = store.dir().join("tasks");
    let path = tasks.join(format!("{uuid}.managed"));
    let files = |ext: &str| fs::read(tasks.join(format!("{uuid}.{ext}"))).unwrap();
    let (snapshot, updates) = (files("loro"), files("updates"));
    let before = stamp(&tasks.join(format!("{uuid}.loro"))) + &stamp(&tasks.join(format!("{uuid}.updates")));
    let mut mark: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for counter in mark["saved"].as_object_mut().unwrap().values_mut() {
        *counter = json!(counter.as_i64().unwrap() + 1);
    }
    // Replaced as this store replaces it, with no journal note; only the
    // mark is dated back, so the snapshot and updates keep their stamps.
    let temporary = tasks.join("mark-outside-the-journal");
    fs::write(&temporary, serde_json::to_vec(&mark).unwrap()).unwrap();
    fs::rename(&temporary, &path).unwrap();
    let past = std::time::SystemTime::now() - Duration::from_secs(3600);
    fs::File::options().write(true).open(&path).unwrap().set_modified(past).unwrap();
    assert_eq!((files("loro"), files("updates")), (snapshot, updates));
    assert_eq!(
        stamp(&tasks.join(format!("{uuid}.loro"))) + &stamp(&tasks.join(format!("{uuid}.updates"))),
        before
    );
    assert!(store.load(&uuid).is_err());
    let mut index = Index::open(&store).unwrap();
    let page = index.page(Stage::Todo, 10, &Anchor::Start).unwrap();
    assert!(page.rows.iter().all(|r| r["uuid"] != json!(uuid)), "{:?}", page.rows);
    assert_eq!(index.damaged().unwrap().1, 1);
}
