//! Person-only catalog operations. Fixed CLI verbs, bounded JSON, no renderer
//! path writes, automatic retries or session effects.
use serde_json::Value;
use tauri::AppHandle;
use zerus_client_core::actions::{Exit, Invocation};
use zerus_client_core::teams::catalog_edit::{self, Save, BYTES};

async fn call(args: Vec<String>, stdin: Option<Vec<u8>>) -> Result<Exit, String> {
    let invocation = Invocation {
        args,
        stdin,
        request_id: None,
        timeout: std::time::Duration::from_secs(15),
    };
    let hgs = zerus_client_core::default_hgs();
    tauri::async_runtime::spawn_blocking(move || {
        zerus_client_core::run_limited(&hgs, &invocation, &[], 1_048_576)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|_| "The catalog answer exceeds its bound.".into())
}
#[tauri::command]
pub async fn catalog_editor_load() -> Result<Value, String> {
    let exit = call(
        vec![
            "team".into(),
            "catalog".into(),
            "snapshot".into(),
            "--json".into(),
        ],
        None,
    )
    .await?;
    let Exit::Code {
        code: 0, stdout, ..
    } = exit
    else {
        return Err("The catalog could not be read.".into());
    };
    let mut reply: Value = serde_json::from_slice(&stdout).map_err(|e| e.to_string())?;
    if reply["ok"] != true || reply["schema"] != 1 || !reply["revision"].is_string() {
        return Err("The catalog answer is not an editor snapshot.".into());
    }
    catalog_edit::validate(reply["person"].clone(), reply["builtin"].clone())?;
    reply["schema_fields"] = catalog_edit::schema();
    Ok(reply)
}
#[tauri::command]
pub fn catalog_editor_validate(value: Value, base: Value) -> Result<Value, String> {
    catalog_edit::validate(value, base)
}
#[tauri::command]
pub fn catalog_editor_import(value: Value, current: Value, base: Value) -> Result<Value, String> {
    catalog_edit::import(value, current, base)
}
#[tauri::command]
pub async fn catalog_editor_save(edit: Save) -> Result<Value, String> {
    let inv = catalog_edit::prepare(&edit)?;
    let exit = call(inv.args, inv.stdin).await;
    Ok(match exit {
        Ok(exit) => catalog_edit::interpret(&edit.request_id, &exit),
        Err(_) => catalog_edit::interpret(&edit.request_id, &Exit::TimedOut),
    })
}
#[tauri::command]
pub async fn catalog_editor_receipt(request: String) -> Result<Value, String> {
    if !catalog_edit::valid_request(&request) {
        return Err("Not a catalog request UUID.".into());
    }
    let exit = call(
        vec![
            "team".into(),
            "catalog".into(),
            "receipt".into(),
            request.clone(),
            "--json".into(),
        ],
        None,
    )
    .await;
    Ok(match exit {
        Ok(exit) => catalog_edit::interpret_receipt(&request, &exit),
        Err(_) => catalog_edit::interpret_receipt(&request, &Exit::TimedOut),
    })
}
#[tauri::command]
pub async fn catalog_editor_import_file(app: AppHandle) -> Result<Option<Value>, String> {
    use tauri_plugin_dialog::DialogExt;
    let window = crate::main_window(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().add_filter("JSON", &["json"]);
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        let Some(file) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        // Selection is one explicit local file, not a config or command path.
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.len() > BYTES as u64 {
            return Err("The catalog import exceeds its bound or is not a regular file.".into());
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > BYTES {
            return Err("The catalog import exceeds its bound.".into());
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn catalog_editor_export(app: AppHandle, value: Value) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    if bytes.len() > BYTES {
        return Err("The catalog export exceeds its bound.".into());
    }
    let window = crate::main_window(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app
            .dialog()
            .file()
            .set_file_name("zerus-team-catalog.json")
            .add_filter("JSON", &["json"]);
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        let Some(file) = dialog.blocking_save_file() else {
            return Ok(false);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        zerus_client_core::fsx::write_private_atomic(&path, &bytes, true)
            .map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Exports the selected saved template through the existing person-only op.
#[tauri::command]
pub async fn catalog_editor_export_template(app: AppHandle, id: String) -> Result<bool, String> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid template ID.".into());
    }
    let exit = call(
        vec![
            "team".into(),
            "template".into(),
            "export".into(),
            id,
            "--json".into(),
        ],
        None,
    )
    .await?;
    let Exit::Code {
        code: 0, stdout, ..
    } = exit
    else {
        return Err("The template could not be exported.".into());
    };
    let reply: Value = serde_json::from_slice(&stdout).map_err(|e| e.to_string())?;
    if reply["ok"] != true || !reply["bundle"].is_object() {
        return Err("The template export has no bundle.".into());
    }
    catalog_editor_export(app, reply["bundle"].clone()).await
}
