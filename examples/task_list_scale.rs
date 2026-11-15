//! Seed only a marked synthetic task-list benchmark, never existing state.
use std::path::Path;
use zerus_replica::{Actor, NewTask, ProjectStore, SessionKey, Stage, Target, TaskDoc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().collect();
    let root = args.get(1).ok_or("private fixture root required")?;
    let root = Path::new(&root);
    if !root.join(".zerus-task-list-fixture").is_file() {
        return Err("a marked private task-list fixture is required".into());
    }
    let replica = root.join("state/replica");
    if args.get(2).is_some_and(|mode| mode == "bind-project") {
        let node = args
            .get(3)
            .and_then(|v| v.to_str())
            .ok_or("private node required")?;
        uuid::Uuid::parse_str(node)?;
        let store = ProjectStore::open(&replica, "project-001")?;
        let target = Target::Session(SessionKey {
            machine: node.into(),
            session: "codex/project-001/session-00001".into(),
            session_id: format!("run_id:{}", uuid::Uuid::from_u128(2)),
        });
        for mut task in store.listing()?.0 {
            task.set_target(&target, &Actor::Person, 1794650401000)?;
            store.save(&mut task)?;
        }
        println!("Bound project-001's 100 synthetic targets");
        return Ok(());
    }
    std::fs::create_dir(&replica)?;
    for project in 0..100 {
        let project = format!("project-{project:03}");
        let store = ProjectStore::open(&replica, &project)?;
        for n in 0..100 {
            let mut task = TaskDoc::create(NewTask {
                project: &project,
                id: &format!("T-{n:06x}"),
                text: "Synthetic benchmark task",
                stage: Stage::Todo,
                target: Target::Any,
                author: Actor::Person,
                rank: &format!("{n:06}V"),
                now: 1794650400000,
            })?;
            store.save(&mut task)?;
        }
    }
    println!("100 projects, 10000 synthetic tasks");
    Ok(())
}
