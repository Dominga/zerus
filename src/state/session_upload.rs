//! U22: block DeepSeek's session-log upload. The setting is on by default and
//! lives with the other hgs state; the launch overlay turns the dsh plugin off
//! without touching the user's global configuration.
use super::*;
use std::fs;

pub(crate) fn path() -> PathBuf {
    root().join("session-upload-block.json")
}

/// Default on for every computer, including one without the file.
pub(crate) fn block() -> bool {
    fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value["block"].as_bool())
        .unwrap_or(true)
}

fn write(block: bool) -> Result<()> {
    let Some(dir) = path().parent().map(Path::to_path_buf) else { return Err("session upload setting has no parent".into()) };
    private_dir(&dir)?;
    let text = serde_json::to_string_pretty(&json!({"block":block})).map_err(|error| error.to_string())?;
    atomic(&path(), &text)
}

/// The dsh config row that turns the official session-log plugin off. The
/// overlay is a patch list; addressing the row by id replaces its config.
pub(crate) fn overlay_row() -> Value {
    json!({"insert":[{"id":"session-log-deepseek","name":"@deepseek-ai/dsh-session-log-deepseek","config":{"enabled":false}}]})
}

/// A session still uploads when any record is the plugin's acceptance event.
#[cfg(test)]
pub(crate) fn uploading(records: &[Value]) -> bool {
    records.iter().any(|record| {
        string(record, "type") == "session-log-deepseek/delivery-accepted"
            || string(record, "event") == "session-log-deepseek/delivery-accepted"
    })
}

/// `hgs session-upload-block get|set --json`.
pub(crate) fn dispatch(args: &[String]) -> Result<i32> {
    if args.len() != 2 || args[1] != "--json" {
        return Err("usage: hgs session-upload-block get|set --json".into());
    }
    match args[0].as_str() {
        "get" => println!("{}", json!({"block":block()})),
        "set" => {
            let input: Value = stdin_json()?;
            if !super::teams::store::canonical_uuid(string(&input, "request_id")) {
                return Err("A canonical request_id is required".into());
            }
            let value = input["block"].as_bool().ok_or("block is true or false")?;
            write(value)?;
            println!("{}", json!({"block":value}));
        }
        other => return Err(format!("Unknown session-upload-block operation {other}")),
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_defaults_on_and_round_trips() {
        let _guard = crate::state::ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::var("HGS_STATE_DIR").ok();
        std::env::set_var("HGS_STATE_DIR", dir.path());
        assert!(block());
        write(false).unwrap();
        assert!(!block());
        write(true).unwrap();
        assert!(block());
        match old {
            Some(value) => std::env::set_var("HGS_STATE_DIR", value),
            None => std::env::remove_var("HGS_STATE_DIR"),
        }
    }

    #[test]
    fn a_recorded_session_with_an_acceptance_event_still_uploads() {
        let clean = vec![json!({"type":"session-log-deepseek/delivery-accepted"})];
        assert!(uploading(&clean));
        assert!(!uploading(&[json!({"type":"user"}), json!({"type":"assistant"})]));
        assert!(overlay_row()["insert"][0]["config"]["enabled"] == false);
    }
}
