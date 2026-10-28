//! Read-only evidence for an explicitly selected Windows/WSL file alias.
//! No local original is opened or modified here. Native access remains scoped
//! to the saved conversation; a desktop must independently verify local bytes.
use crate::{
    config::Config,
    path_ref::{PathFlavor, PathInput, PathRef, WslMapping},
    platform, wsl,
};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    process::Stdio,
    time::{Duration, Instant},
};

const INPUT_LIMIT: u64 = 64 * 1024;
const FILE_LIMIT: usize = 10 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 14 * 1024 * 1024 + 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    schema_version: u32,
    request_id: String,
    machine_id: String,
    connection: wsl::WslConnection,
    mapping: WslMapping,
    session: String,
    run_id: String,
    conversation_id: String,
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    selection: Selection,
    #[serde(default)]
    expected_preview: Option<String>,
}
fn text(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}
fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Selection {
    fn source(&self, path: &str) -> Result<PathRef> {
        PathRef::describe(
            PathInput {
                machine_id: self.machine_id.clone(),
                environment_id: self.mapping.wsl_environment_id.clone(),
                path_flavor: PathFlavor::Posix,
                native_path: path.to_owned(),
                workspace_id: Some(self.mapping.workspace_id.clone()),
            },
            Some(&self.mapping),
        )
    }
    fn validate(&self) -> Result<PathRef> {
        let id =
            uuid::Uuid::parse_str(&self.request_id).context("Invalid file proof request ID")?;
        ensure!(
            self.schema_version == 1 && !id.is_nil() && id.to_string() == self.request_id,
            "Invalid file proof schema or request ID"
        );
        self.connection
            .validate()
            .map_err(|e| anyhow::anyhow!(e.message))?;
        ensure!(
            text(&self.machine_id, 256)
                && text(&self.session, 512)
                && text(&self.run_id, 128)
                && text(&self.conversation_id, 256)
                && text(&self.path, 8192),
            "Invalid selected conversation or path"
        );
        for field in [&self.archive_id, &self.agent_id] {
            ensure!(
                field.as_ref().is_none_or(|v| text(v, 256)),
                "Invalid archive or child identity"
            );
        }
        ensure!(
            self.mapping.machine_id == self.machine_id
                && self.mapping.distribution == self.connection.distribution
                && self.mapping.user == self.connection.user
                && self.mapping.windows_environment_id == "windows-native",
            "Mapping belongs to another selected machine or WSL environment"
        );
        let source = self.source(&self.path)?;
        // Validate a lossless Windows target before reading any native content.
        self.mapping.map(&source)?;
        Ok(source)
    }
    fn native_request(&self) -> Value {
        let mut value = json!({"request_id":self.request_id,"expected_run_id":self.run_id,
            "expected_conversation_id":self.conversation_id,"path":self.path});
        for (key, field) in [
            ("archive_id", &self.archive_id),
            ("agent_id", &self.agent_id),
        ] {
            if let Some(field) = field {
                value[key] = json!(field);
            }
        }
        value
    }
}
fn checked_connection(config: &Config, alias: &str, selection: &Selection) -> Result<()> {
    let connection = config
        .machines
        .get(alias)
        .context("The selected WSL connection no longer exists")?;
    ensure!(
        config.host() == selection.machine_id
            && connection.enabled
            && connection.transport == wsl::Transport::WslLocal
            && connection.wsl.as_ref() == Some(&selection.connection),
        "The selected machine, WSL distribution, user or receiver changed; preview again"
    );
    Ok(())
}
fn evidence(alias: &str, selection: &Selection, source: &PathRef, native: &Value) -> Result<Value> {
    ensure!(
        native["request_id"] == selection.request_id
            && native["name"] == selection.session
            && native["run_id"] == selection.run_id
            && native["conversation_id"] == selection.conversation_id
            && native["requested_path"] == selection.path,
        "Native file reply belongs to another request or conversation"
    );
    for (key, expected) in [
        ("archive_id", &selection.archive_id),
        ("agent_id", &selection.agent_id),
    ] {
        ensure!(
            native[key].as_str() == expected.as_deref(),
            "Native file reply belongs to another archive or child"
        );
    }
    let path = native["path"]
        .as_str()
        .filter(|p| text(p, 8192))
        .context("Missing native resolved path")?;
    let resolved = selection.source(path)?;
    ensure!(resolved.canonical_path==source.canonical_path,"The native file resolves to another path; use its explicit resolved path or download a copy");
    let target = selection.mapping.map(&resolved)?;
    let length = native["bytes"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= FILE_LIMIT as u64)
        .context("Invalid native file size")?;
    let digest = native["sha256"]
        .as_str()
        .filter(|s| sha(s))
        .context("Invalid native file digest")?;
    let encoded = native["data_base64"]
        .as_str()
        .context("Missing native file snapshot")?;
    ensure!(
        encoded.len() <= FILE_LIMIT.div_ceil(3) * 4,
        "Native file snapshot exceeds bounds"
    );
    let data = STANDARD
        .decode(encoded)
        .context("Malformed native file snapshot")?;
    ensure!(
        data.len() as u64 == length && format!("{:x}", Sha256::digest(&data)) == digest,
        "Native file snapshot does not match its digest"
    );
    // Do not expose or persist snapshot bytes in the proof. Digest/size describe
    // evidence only, never a capability to bypass either environment's ACLs.
    let mut result = json!({"schema_version":1,"selection":selection,"alias":alias,
        "native_path":path,"target":target,"bytes":length,"sha256":digest,
        "native_changed":false,"local_verified":false});
    result["preview_identity"] = json!(format!(
        "{:x}",
        Sha256::digest(result.to_string().as_bytes())
    ));
    Ok(result)
}
fn read_native(selection: &Selection) -> Result<Value> {
    // The CLI capability is probed before a command on an older receiver can
    // accidentally become an unknown agent launch. Never try SSH/default WSL.
    let help = wsl::capture(&selection.connection, &["--help".into()], 128 * 1024, 5)
        .map_err(|e| anyhow::anyhow!(e.message))?;
    ensure!(
        help.status.success()
            && String::from_utf8_lossy(&help.stdout).contains("file-read <session> --json"),
        "This WSL receiver does not offer scoped file reads; update the matching candidate"
    );
    let args = vec![
        "file-read".into(),
        selection.session.clone(),
        "--json".into(),
    ];
    let mut command =
        wsl::command(&selection.connection, &args).map_err(|e| anyhow::anyhow!(e.message))?;
    let mut input = tempfile::tempfile()?;
    input.write_all(&serde_json::to_vec(&selection.native_request())?)?;
    input.seek(SeekFrom::Start(0))?;
    let (status, out, _err) = platform::run_bounded_stdin(
        &mut command,
        Stdio::from(input),
        [OUTPUT_LIMIT, 8192],
        Instant::now() + Duration::from_secs(20),
        &|| false,
    )
    .map_err(|e| anyhow::anyhow!("The scoped WSL file read failed: {e:?}"))?;
    ensure!(status.success(),"The selected conversation could not authorize or read this file; reopen it from the original session");
    serde_json::from_slice(&out).context("Invalid scoped WSL file reply")
}
pub fn run(config: &Config, alias: &str, dry: bool) -> Result<()> {
    ensure!(
        !dry,
        "File proof performs explicit read-only checks; it does not support dry-run"
    );
    let mut input = Vec::new();
    std::io::stdin()
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut input)?;
    ensure!(
        input.len() as u64 <= INPUT_LIMIT,
        "File proof request exceeds 64 KiB"
    );
    let request: Request = serde_json::from_slice(crate::state::without_bom(&input)).context("Invalid file proof request")?;
    let source = request.selection.validate()?;
    ensure!(
        request.expected_preview.as_ref().is_none_or(|s| sha(s)),
        "Invalid file preview identity"
    );
    checked_connection(config, alias, &request.selection)?;
    let native = read_native(&request.selection)?;
    let result = evidence(alias, &request.selection, &source, &native)?;
    let current = Config::load().map_err(|e| anyhow::anyhow!(e.message))?;
    checked_connection(&current, alias, &request.selection)?;
    if let Some(expected) = request.expected_preview {
        ensure!(
            result["preview_identity"] == expected,
            "The file or selection changed since preview; nothing was opened"
        );
    }
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> Selection {
        serde_json::from_value(json!({"schema_version":1,
        "request_id":"11111111-2222-4333-8444-555555555555","machine_id":"machine-a",
        "connection":{"distribution":"Ubuntu-24.04","user":"alice","hgs_path":"/opt/zerus/hgs"},
        "mapping":{"mapping_id":"m","workspace_id":"orbit","machine_id":"machine-a","windows_environment_id":"windows-native",
          "wsl_environment_id":"wsl:Ubuntu-24.04:alice","distribution":"Ubuntu-24.04","user":"alice","mount_root":"/mnt","windows_root":"C:/Projects/Orbit"},
        "session":"opencode/orbit/work","run_id":"run-a","conversation_id":"ses_a","path":"/mnt/c/Projects/Orbit/file.txt"})).unwrap()
    }
    fn native(s: &Selection) -> Value {
        json!({"request_id":s.request_id,"name":s.session,"run_id":s.run_id,
        "conversation_id":s.conversation_id,"requested_path":s.path,"path":s.path,
        "bytes":5,"sha256":format!("{:x}",Sha256::digest(b"hello")),"data_base64":STANDARD.encode(b"hello")})
    }
    #[test]
    fn proof_has_only_verified_metadata_and_exact_target() {
        let s = selection();
        let result = evidence("wsl-box", &s, &s.validate().unwrap(), &native(&s)).unwrap();
        assert_eq!(
            result["target"]["native_path"],
            r"C:\Projects\Orbit\file.txt"
        );
        assert!(result.get("data_base64").is_none());
        assert_eq!(result["local_verified"], false);
        assert_eq!(result["native_changed"], false);
        assert!(sha(result["preview_identity"].as_str().unwrap()));
    }
    #[test]
    fn mismatched_scope_content_and_symlink_targets_fail_closed() {
        let s = selection();
        let source = s.validate().unwrap();
        for (key, value) in [
            ("request_id", json!("wrong")),
            ("name", json!("other")),
            ("run_id", json!("other")),
            ("conversation_id", json!("ses_other")),
            ("requested_path", json!("other")),
            ("agent_id", json!("child")),
            ("archive_id", json!("archive")),
            ("path", json!("/mnt/c/Projects/Outside/file.txt")),
            ("path", json!("/mnt/c/Projects/Orbit/redirect.txt")),
            ("bytes", json!(6)),
            ("bytes", json!(0)),
            ("sha256", json!("0".repeat(64))),
            ("data_base64", json!("aGVsbG8x")),
        ] {
            let mut answer = native(&s);
            answer[key] = value;
            assert!(evidence("wsl-box", &s, &source, &answer).is_err(), "{key}");
        }
    }
    #[test]
    fn mapping_namespace_is_explicit_and_not_inferred_from_spelling() {
        let mut s = selection();
        s.mapping.user = "bob".into();
        assert!(s.validate().is_err());
        let mut s = selection();
        s.path = "/home/alice/file.txt".into();
        assert!(s.validate().is_err());
        let mut s = selection();
        s.path = "/mnt/c/Projects/Orbit/../outside.txt".into();
        assert!(s.validate().is_err());
        let mut s = selection();
        s.request_id = "00000000-0000-0000-0000-000000000000".into();
        assert!(s.validate().is_err());
    }
}
