//! Settings > Automatic approvals (AutomaticApprovalSettings.cpp): its
//! journal only. This client never approves a tool request, so its switch
//! stays off; the journal is read through `hgs approval-log list|detail` on
//! the blocking pool, and a passed probe is remembered as HgsClient
//! remembers it. A replay never drives the CLI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::State;
use zerus_client_core::approvals::{self, Operation, Page};
use zerus_client_core::default_hgs;

/// Whether the local `hgs` answered the probe, and whether this is a replay.
pub struct Approvals {
    supported: Arc<AtomicBool>,
    blocked: bool,
}

pub fn start() -> Approvals {
    Approvals {
        supported: Arc::new(AtomicBool::new(false)),
        blocked: crate::actions::blocked(),
    }
}

/// Runs one journal operation off the main thread.
async fn read(
    approvals: &Approvals,
    operation: Operation,
    data: serde_json::Value,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    if approvals.blocked {
        return Err("The automatic approval journal is not read in a replay.".into());
    }
    let supported = Arc::clone(&approvals.supported);
    tauri::async_runtime::spawn_blocking(move || {
        let mut known = supported.load(Ordering::Acquire);
        let reply = approvals::call(&default_hgs(), operation, &data, &mut known);
        if known {
            supported.store(true, Ordering::Release);
        }
        reply
    })
    .await
    .map_err(|error| error.to_string())?
}

/// One page of the journal: exact computer and session filters, and a
/// newer (`after`) or older (`before`) cursor, 0 for none.
#[tauri::command]
pub async fn approvals_list(
    approvals: State<'_, Approvals>,
    host: String,
    session: String,
    before: i64,
    after: i64,
) -> Result<Page, String> {
    let reply = read(
        &approvals,
        Operation::List,
        approvals::list_request(&host, &session, before, after),
    )
    .await?;
    approvals::page(&reply)
}

/// One entry's question and outcome, as Qt words them.
#[tauri::command]
pub async fn approvals_detail(approvals: State<'_, Approvals>, id: i64) -> Result<String, String> {
    let reply = read(
        &approvals,
        Operation::Detail,
        serde_json::json!({ "id": id }),
    )
    .await?;
    Ok(approvals::detail(&reply))
}
