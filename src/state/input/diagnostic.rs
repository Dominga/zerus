//! A bounded last-refusal sample per agent, only for deliberate send attempts.
//! Other rows are structural metadata: never retain continuation drafts, queued
//! messages, tool output, histories, attachments, or the attempted send body.
use super::*;

/// Read only the verified same-user agent's environment. Keep no environment
/// values, and never let an unreadable or replaced process change the refusal.
pub(super) fn no_color(record: &Value) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let Some(pid) = record["pid"].as_u64().and_then(|pid| u32::try_from(pid).ok()) else {
            return false;
        };
        let Some(before) = crate::platform::process_identity(pid) else {
            return false;
        };
        if !process_alive(record) {
            return false;
        }
        let Ok(bytes) = crate::state::task_reconcile::environment(pid) else {
            return false;
        };
        let present = bytes.split(|b| *b == 0).any(|entry| entry.starts_with(b"NO_COLOR="));
        present
            && process_alive(record)
            && crate::platform::process_identity(pid).is_some_and(|after| after.birth == before.birth)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = record;
        false
    }
}

fn row_shape(row: &str) -> Value {
    let cells = styled_cells(row, &mut TerminalStyle::default());
    let plain: String = cells.iter().map(|(ch, _)| ch).collect();
    json!({
        "columns": plain.chars().count(),
        "leading_spaces": plain.chars().take_while(|ch| ch.is_whitespace()).count(),
        "blank": plain.trim().is_empty(),
        "separator": plain.chars().count() >= 12 && plain.chars().all(|ch| "─━╌".contains(ch)),
        "dim": cells.iter().any(|(_, style)| style.dim),
        "inverse": cells.iter().any(|(_, style)| style.inverse),
        "background": cells.iter().any(|(_, style)| style.background),
        "shortcuts": plain.trim_start().starts_with("? for shortcuts"),
        "codex_status": codex_status_row(&plain, None),
        "codex_warning": codex_warning_footer(row),
    })
}

pub(super) fn save(record: &Value, screen: &str, x: usize, y: usize, stable: bool) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if effort::picker_owned_send() {
        // Keeper observations cannot escape into the ordinary atomic writer.
        // Refusal text is retained; this operation deliberately loses the
        // best-effort screen sample rather than issuing an unowned write.
        return;
    }
    let agent = string(record, "agent");
    if !matches!(agent, "codex" | "claude") {
        return;
    }
    let rows: Vec<_> = screen.lines().collect();
    // The cursor row is the only row whose text is retained. Bound raw styled
    // text as well as row count; metadata never copies strings from other rows.
    let cursor_row: String = if stable {
        rows.get(y).unwrap_or(&"").chars().take(1024).collect()
    } else {
        // A moved cursor cannot identify which captured row was the composer.
        // Keep only structural evidence instead of retaining an unrelated row.
        String::new()
    };
    let shapes: Vec<_> = rows
        .iter()
        .enumerate()
        .skip(y.saturating_sub(2))
        .take(8)
        .map(|(index, row)| json!({"row": index, "shape": row_shape(row)}))
        .collect();
    let value = json!({"version": 1, "at": now(), "agent": agent,
        "run_id": string(record, "run_id"), "conversation_id": string(record, "conversation_id"),
        "phase": string(record, "phase"), "cursor_x": x, "cursor_y": y,
        "cursor_stable": stable, "cursor_row": cursor_row, "rows": shapes});
    // Best effort diagnostics must never change a refusal into a delivery.
    // Fixed filenames replace the previous sample under the existing state lock.
    if let Ok(root) = absolute_root() {
        let contents = value.to_string();
        if contents.len() <= 16 * 1024 {
            let _ = atomic(
                &root.join("input_diagnostics").join(format!("{agent}.json")),
                &contents,
            );
        }
    }
}
