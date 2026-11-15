//! Private-state helpers for the session scale audit. No production entry point.
use serde_json::json;
use std::{
    env,
    io::{self, BufRead},
    path::Path,
    time::Instant,
};
use zerus_client_core::{CatalogState, Change, Coalescer, ComputerId};
use zerus_replica::{Actor, NewTask, ProjectStore, Stage, Target, TaskDoc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("tasks") {
        let root = Path::new(args.get(2).ok_or("private replica root required")?);
        let fixture = root
            .parent()
            .and_then(Path::parent)
            .ok_or("private fixture root")?;
        if root.file_name().is_none_or(|name| name != "replica")
            || !fixture.join(".zerus-scale-fixture").is_file()
        {
            return Err("tasks requires a marked private fixture's state/replica path".into());
        }
        std::fs::create_dir(root)?; // Refuse an existing destination; never append a second fixture.
        for project in 0..100 {
            let id = format!("project-{project:03}");
            let store = ProjectStore::open(root, &id)?;
            for n in 0..100 {
                let mut task = TaskDoc::create(NewTask {
                    project: &id,
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
        println!("{}", json!({"projects":100,"tasks":10000}));
        return Ok(());
    }
    let start = Instant::now();
    let mut state = CatalogState::new();
    let mut batches = Coalescer::new();
    let computer = ComputerId::local();
    let mut bytes = 0;
    let mut frames = 0;
    let mut page_rows = 0;
    let mut page_projection = 0;
    for line in io::stdin().lock().lines() {
        let line = line?;
        bytes += line.len() + 1;
        if args.get(1).map(String::as_str) == Some("pages") {
            let value: serde_json::Value = serde_json::from_str(&line)?;
            if value["t"] != "hello" {
                let request = value["request_id"]
                    .as_str()
                    .ok_or("request identity")?
                    .to_owned();
                let epoch = value["epoch"].as_str().ok_or("service epoch")?.to_owned();
                let page = zerus_client_core::desktop::service::CatalogPage::checked(
                    value, &request, &epoch, "",
                )?;
                for item in page.items {
                    let row = state.project_row(&computer, &item.row);
                    batches.push(Change::Upsert {
                        computer: computer.clone(),
                        key: item.key,
                        row,
                    });
                    page_rows += 1;
                }
                page_projection += serde_json::to_vec(&batches.take())?.len();
                frames += 1;
                continue;
            }
        }
        let frame = zerus_watch_protocol::decode(&line)?;
        for change in state.apply(&computer, &frame) {
            batches.push(change);
        }
        frames += 1;
    }
    let rows = state.rows().len() + page_rows;
    let projection = serde_json::to_vec(&batches.take())?;
    println!(
        "{}",
        json!({"frames":frames,"rows":rows,"input_bytes":bytes,
        "projection_bytes":projection.len()+page_projection,"seconds":start.elapsed().as_secs_f64()})
    );
    Ok(())
}
