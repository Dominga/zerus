//! Task 73: WSL path mapping and WSL file open (Qt WslPathMappingDialog,
//! WslFileOpenDialog, WslPathMapping, MappedLocalFile and HgsClient's
//! path-ref and file-proof requests).
//!
//! A mapping is an owner-selected lexical alias between a Windows workspace
//! root and a WSL drive mount root, for one explicitly configured local WSL
//! connection. Previewing it never touches a filesystem: `hgs path-ref
//! describe` names the Windows source, `hgs path-ref map` the WSL target, and
//! each reply must carry the exact machine, workspace, mapping and
//! environment that were sent. Using it reads the connection again first.
//!
//! Opening a mapped Windows original is a separate explicit action. `hgs
//! machine file-proof <alias>` reads the file through the selected
//! conversation in WSL and returns its digest and a preview identity token;
//! this computer then verifies the Windows file without following reparse
//! points and only then opens it. Opening asks for a fresh proof bound to the
//! preview's token and refuses anything that changed. Nothing here writes the
//! original or marks it as a download.
use crate::actions::{failure_detail, Exit, Invocation};
use crate::text::Text;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub const WINDOWS_ENVIRONMENT: &str = "windows-native";
/// The help marker of a CLI that proves WSL files.
pub const PROOF_MARKER: &str = "wsl-file-proof-v1";
pub const REQUEST_LIMIT: usize = 64 * 1024;
pub const MACHINES_LIMIT: usize = 1024 * 1024;
pub const PATH_REF_LIMIT: usize = 64 * 1024;
pub const PROOF_LIMIT: usize = 128 * 1024;
pub const FILE_LIMIT: u64 = 10 * 1024 * 1024;
const PRESETS: usize = 64;
const PRESET_LIMIT: usize = 16 * 1024;

/// The explicit mapping fields the owner types.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fields {
    #[serde(default)]
    pub windows_root: String,
    #[serde(default)]
    pub mount_root: String,
    #[serde(default)]
    pub workspace_id: String,
}

impl Fields {
    pub fn complete(&self) -> bool {
        !self.windows_root.is_empty()
            && !self.mount_root.is_empty()
            && !self.workspace_id.is_empty()
    }
}

/// One local WSL connection as `hgs machine ls` reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Connection {
    pub alias: String,
    /// This computer's identity (`local`), the paths' machine.
    pub machine_id: String,
    /// The connection's `wsl` object, sent back exactly as read.
    pub profile: Value,
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

impl Connection {
    pub fn distribution(&self) -> &str {
        string(&self.profile, "distribution")
    }
    pub fn user(&self) -> &str {
        string(&self.profile, "user")
    }
    /// `wsl:<distribution>:<user>`.
    pub fn environment(&self) -> String {
        format!("wsl:{}:{}", self.distribution(), self.user())
    }
    /// The device-local preset key (Qt's WslPathMapping::key).
    pub fn key(&self) -> String {
        let identity =
            json!({"host": self.alias, "machine": self.machine_id, "profile": self.profile});
        format!("{:x}", Sha256::digest(identity.to_string().as_bytes()))
    }
    /// The mapping object hgs takes.
    pub fn mapping(&self, mapping_id: &str, fields: &Fields) -> Value {
        json!({"mapping_id": mapping_id, "workspace_id": fields.workspace_id, "machine_id": self.machine_id,
            "windows_environment_id": WINDOWS_ENVIRONMENT, "wsl_environment_id": self.environment(),
            "distribution": self.profile["distribution"], "user": self.profile["user"],
            "mount_root": fields.mount_root, "windows_root": fields.windows_root})
    }
}

/// `hgs machine ls`, read fresh for each dialog and again before use.
pub fn machines_call() -> Invocation {
    Invocation {
        args: vec!["machine".into(), "ls".into()],
        stdin: None,
        timeout: Duration::from_secs(8),
        request_id: None,
    }
}

fn row<'a>(list: &'a Value, alias: &str) -> Option<&'a Value> {
    list["machines"]
        .as_array()?
        .iter()
        .find(|row| row["alias"] == alias)?
        .get("connection")
}

/// The alias's local WSL connection. `strict` (file open) also requires it
/// enabled with an explicit hgs path, as Qt's WslFileOpenDialog does.
pub fn connection(list: &Value, alias: &str, strict: bool) -> Result<Connection, Text> {
    let connection = row(list, alias).cloned().unwrap_or(Value::Null);
    let profile = connection.get("wsl").cloned().unwrap_or(Value::Null);
    let machine_id = string(list, "local").to_owned();
    let found = Connection {
        alias: alias.to_owned(),
        machine_id,
        profile,
    };
    let wsl = connection["transport"] == "wsl-local"
        && found.profile.as_object().is_some_and(|p| !p.is_empty());
    if strict {
        if !wsl
            || found.machine_id.is_empty()
            || connection["enabled"] == false
            || found.distribution().is_empty()
            || found.user().is_empty()
            || string(&found.profile, "hgs_path").is_empty()
        {
            return Err(crate::text!(
                "Choose an enabled, explicitly configured local WSL connection. SSH and default-distribution fallbacks are not allowed."
            ));
        }
    } else if !wsl {
        return Err(crate::text!(
            "This action requires an explicitly configured local WSL connection."
        ));
    }
    Ok(found)
}

/// Whether a fresh list still names exactly this connection.
pub fn unchanged(list: &Value, found: &Connection) -> bool {
    let connection = row(list, &found.alias).cloned().unwrap_or(Value::Null);
    list["local"] == found.machine_id.as_str()
        && connection["transport"] == "wsl-local"
        && connection.get("wsl") == Some(&found.profile)
}

/// `hgs path-ref describe|map`, the request on stdin.
pub fn path_ref_call(op: &str, input: &Value) -> Result<Invocation, Text> {
    let bytes = input.to_string().into_bytes();
    if !matches!(op, "describe" | "map") || bytes.len() > REQUEST_LIMIT {
        return Err(crate::text!("Invalid or oversized path identity request"));
    }
    Ok(Invocation {
        args: vec!["path-ref".into(), op.into()],
        stdin: Some(bytes),
        timeout: Duration::from_secs(10),
        request_id: None,
    })
}

/// The Windows source the owner names, as `describe` takes it.
pub fn source_input(found: &Connection, source: &str, workspace_id: &str) -> Value {
    json!({"machine_id": found.machine_id, "environment_id": WINDOWS_ENVIRONMENT, "path_flavor": "windows",
        "native_path": source, "workspace_id": workspace_id})
}

fn common(result: &Value, found: &Connection, mapping: &Value) -> bool {
    result["schema_version"] == 1
        && result["machine_id"] == found.machine_id.as_str()
        && result["workspace_id"] == mapping["workspace_id"]
        && result["mapping_id"] == mapping["mapping_id"]
}

/// The described source must be the exact Windows path sent.
pub fn check_described(
    result: &Value,
    found: &Connection,
    mapping: &Value,
    input: &Value,
) -> Result<(), Text> {
    if !common(result, found, mapping)
        || result["environment_id"] != WINDOWS_ENVIRONMENT
        || result["path_flavor"] != "windows"
        || result["native_path"] != input["native_path"]
    {
        return Err(crate::text!(
            "The source identity response does not match this mapping."
        ));
    }
    Ok(())
}

/// The mapped target: this environment, the source's canonical identity and
/// an absolute POSIX path. Gives its native path.
pub fn check_mapped(
    result: &Value,
    found: &Connection,
    mapping: &Value,
    described: &Value,
) -> Result<String, Text> {
    let path = string(result, "native_path");
    if !common(result, found, mapping)
        || result["environment_id"] != found.environment().as_str()
        || result["path_flavor"] != "posix"
        || result["canonical_path"] != described["canonical_path"]
        || !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('\0')
    {
        return Err(crate::text!(
            "The mapped response does not match the selected environment."
        ));
    }
    Ok(path.to_owned())
}

/// The conversation and file a proof is scoped to (Qt's scope object).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub session: String,
    pub run_id: String,
    pub conversation_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}

impl Scope {
    /// Qt offers the action only for a known run and conversation and an absolute WSL path.
    pub fn check(&self) -> Result<(), Text> {
        let plain = |s: &str| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control);
        let optional = |s: &Option<String>| s.as_deref().is_none_or(plain);
        if !plain(&self.session)
            || !plain(&self.run_id)
            || !plain(&self.conversation_id)
            || !plain(&self.path)
            || !self.path.starts_with('/')
            || !optional(&self.archive_id)
            || !optional(&self.agent_id)
        {
            return Err(crate::text!(
                "This file has no conversation and WSL path to prove it with."
            ));
        }
        Ok(())
    }
}

/// The proof's selection (Qt's WslFileOpenDialog::selection).
pub fn selection(scope: &Scope, request_id: &str, found: &Connection, mapping: &Value) -> Value {
    let mut value = serde_json::to_value(scope).unwrap_or(Value::Null);
    value["schema_version"] = json!(1);
    value["request_id"] = json!(request_id);
    value["machine_id"] = json!(found.machine_id);
    value["connection"] = found.profile.clone();
    value["mapping"] = mapping.clone();
    value
}

/// `hgs --help`, for the proof marker.
pub fn help_call() -> Invocation {
    Invocation {
        args: vec!["--help".into()],
        stdin: None,
        timeout: Duration::from_secs(5),
        request_id: None,
    }
}

/// `hgs machine file-proof <alias> --json`.
pub fn proof_call(alias: &str, request: &Value) -> Result<Invocation, Text> {
    let bytes = request.to_string().into_bytes();
    if alias.is_empty() || !crate::fleet::valid_alias(alias) || bytes.len() > REQUEST_LIMIT {
        return Err(crate::text!("Invalid or oversized file mapping request."));
    }
    Ok(Invocation {
        args: vec![
            "machine".into(),
            "file-proof".into(),
            alias.into(),
            "--json".into(),
        ],
        stdin: Some(bytes),
        timeout: Duration::from_secs(30),
        request_id: None,
    })
}

fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A proof that matched what was sent.
#[derive(Clone, Debug, PartialEq)]
pub struct Proof {
    pub token: String,
    pub sha256: String,
    pub bytes: u64,
    /// The WSL path that was read.
    pub wsl_path: String,
    /// The Windows original to verify and open.
    pub windows_path: String,
    pub raw: Value,
}

/// Qt's `receive` exactness check: the proof echoes the sent selection, names
/// the alias, changed nothing, and targets a safe Windows drive path in this
/// mapping.
pub fn check_proof(
    proof: &Value,
    sent: &Value,
    alias: &str,
    found: &Connection,
) -> Result<Proof, Text> {
    let target = &proof["target"];
    let mapping = &sent["mapping"];
    let token = string(proof, "preview_identity");
    let digest = string(proof, "sha256");
    let windows = string(target, "native_path");
    let bytes = proof["bytes"].as_u64().unwrap_or(0);
    let exact = proof["schema_version"] == 1
        && &proof["selection"] == sent
        && proof["alias"] == alias
        && proof["native_changed"] == false
        && proof["local_verified"] == false
        && target["schema_version"] == 1
        && target["machine_id"] == found.machine_id.as_str()
        && target["environment_id"] == WINDOWS_ENVIRONMENT
        && target["workspace_id"] == mapping["workspace_id"]
        && target["mapping_id"] == mapping["mapping_id"]
        && target["path_flavor"] == "windows"
        && sha(token)
        && sha(digest)
        && (1..=FILE_LIMIT).contains(&bytes)
        && safe_drive_path(windows);
    if !exact {
        return Err(crate::text!(
            "The file proof no longer matches this preview. Nothing was opened."
        ));
    }
    Ok(Proof {
        token: token.to_owned(),
        sha256: digest.to_owned(),
        bytes,
        wsl_path: string(proof, "native_path").to_owned(),
        windows_path: windows.to_owned(),
        raw: proof.clone(),
    })
}

/// Qt's MappedLocalFile::safeDrivePath: a drive path without UNC or device
/// namespaces, alternate streams, empty, dot or reserved components,
/// trailing dots or spaces, or control characters.
pub fn safe_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    if path.chars().count() > 8192
        || bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'/' | b'\\')
    {
        return false;
    }
    let rest = path[3..].replace('\\', "/");
    rest.split('/').all(|part| {
        let upper = part.to_uppercase();
        let stem = upper.split('.').next().unwrap_or("");
        let reserved = matches!(stem, "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.chars().count() == 4
                && stem
                    .chars()
                    .nth(3)
                    .is_some_and(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³')));
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part.ends_with('.')
            && !part.ends_with(' ')
            && !reserved
            && !part
                .chars()
                .any(|c| c.is_control() || "<>:\"|?*\\".contains(c))
    })
}

/// Checked local handles, kept open until the open dispatch has returned.
#[derive(Debug, Default)]
pub struct Guard(#[allow(dead_code)] Vec<std::fs::File>);

/// Verifies the Windows original: a local fixed drive, every component a
/// plain directory or file under its exact spelling, the size and SHA-256 of
/// the WSL snapshot. Blocking bounded I/O: run it off the UI thread.
pub fn verify_local(path: &str, sha256: &str, bytes: u64) -> Result<Guard, Text> {
    if !safe_drive_path(path) || !sha(sha256) || !(1..=FILE_LIMIT).contains(&bytes) {
        return Err(crate::text!(
            "The mapped local path or file evidence is invalid."
        ));
    }
    verify_platform(path, sha256, bytes)
}

#[cfg(not(windows))]
fn verify_platform(_path: &str, _sha256: &str, _bytes: u64) -> Result<Guard, Text> {
    Err(crate::text!(
        "Opening a mapped Windows original requires a Windows desktop."
    ))
}

#[cfg(windows)]
fn verify_platform(path: &str, sha256: &str, bytes: u64) -> Result<Guard, Text> {
    use std::io::Read;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::time::Instant;
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetFileInformationByHandle, GetFileType, GetFinalPathNameByHandleW,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_NAME_NORMALIZED,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK, VOLUME_NAME_DOS,
    };
    const DRIVE_FIXED: u32 = 3;
    let clean = path.replace('\\', "/");
    let root: Vec<u16> = std::ffi::OsStr::new(&clean[..3].replace('/', "\\"))
        .encode_wide()
        .chain([0])
        .collect();
    // SAFETY: a NUL-terminated root such as "C:\".
    if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_FIXED {
        return Err(crate::text!(
            "Only a local fixed-drive workspace can be opened through this mapping."
        ));
    }
    let mut prefixes = vec![clean[..3].to_owned()];
    for (at, ch) in clean.char_indices().skip(3) {
        if ch == '/' {
            prefixes.push(clean[..at].to_owned());
        }
    }
    prefixes.push(clean.clone());
    let started = Instant::now();
    let budget = || started.elapsed() > Duration::from_secs(5);
    let mut handles = Vec::new();
    for (index, prefix) in prefixes.iter().enumerate() {
        let leaf = index + 1 == prefixes.len();
        let native = format!(r"\\?\{}", prefix.replace('/', "\\"));
        let mut options = std::fs::OpenOptions::new();
        if leaf {
            options
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        } else {
            options
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS);
        }
        let file = options.open(&native).map_err(|_| {
            crate::text!("The local file is unavailable, busy or unreadable. Nothing was opened.")
        })?;
        let handle = file.as_raw_handle() as _;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: an open handle and a zeroed out-structure.
        let read = unsafe { GetFileInformationByHandle(handle, &mut info) } != 0;
        // SAFETY: an open handle.
        let disk = unsafe { GetFileType(handle) } == FILE_TYPE_DISK;
        let directory = info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0;
        if !read
            || !disk
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || directory == leaf
        {
            return Err(crate::text!(
                "The mapped path contains a reparse point or a non-regular file. Use the existing download workflow instead."
            ));
        }
        let mut actual = vec![0u16; 32768];
        // SAFETY: an open handle and a buffer of the stated length.
        let size = unsafe {
            GetFinalPathNameByHandleW(
                handle,
                actual.as_mut_ptr(),
                actual.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if size == 0 || size >= actual.len() {
            return Err(crate::text!(
                "The final local file identity could not be verified."
            ));
        }
        let final_path = String::from_utf16_lossy(&actual[..size]).replace('\\', "/");
        let Some(final_path) = final_path.strip_prefix("//?/") else {
            return Err(crate::text!(
                "The local path resolved outside the selected drive namespace."
            ));
        };
        if final_path.to_lowercase() != prefix.to_lowercase() {
            return Err(crate::text!("The local path resolves to another spelling or target. Select its explicit native path."));
        }
        if budget() {
            return Err(crate::text!(
                "Local file verification exceeded its read budget."
            ));
        }
        handles.push(file);
    }
    let file = handles.last_mut().expect("the leaf");
    if file.metadata().map(|m| m.len()).ok() != Some(bytes) {
        return Err(crate::text!(
            "The Windows file size differs from the scoped WSL snapshot."
        ));
    }
    let mut hash = Sha256::new();
    let mut read = 0u64;
    let mut buffer = vec![0u8; 65536];
    while read < bytes {
        let want = buffer.len().min((bytes - read) as usize);
        let count = file.read(&mut buffer[..want]).unwrap_or(0);
        if count == 0 {
            return Err(crate::text!(
                "The Windows file could not be read completely."
            ));
        }
        hash.update(&buffer[..count]);
        read += count as u64;
        if budget() {
            return Err(crate::text!(
                "Local file verification exceeded its read budget."
            ));
        }
    }
    if format!("{:x}", hash.finalize()) != sha256 {
        return Err(crate::text!(
            "The Windows and WSL files have different contents. Nothing was opened."
        ));
    }
    Ok(Guard(handles))
}

/// A saved preset: the fields and the mapping id they were used with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preset {
    pub fields: Fields,
    pub mapping_id: String,
}

/// Device-local presets (Qt's `pathMappings/<key>`), one small file of at
/// most 64 mappings. They never change a project or catalog identity.
pub struct Presets {
    path: PathBuf,
}

impl Presets {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    fn all(&self) -> BTreeMap<String, Value> {
        std::fs::read(&self.path)
            .ok()
            .filter(|bytes| bytes.len() <= PRESETS * PRESET_LIMIT)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    pub fn load(&self, key: &str) -> Preset {
        let all = self.all();
        let Some(saved) = all.get(key) else {
            return Preset::default();
        };
        Preset {
            fields: Fields {
                windows_root: string(saved, "windows_root").to_owned(),
                mount_root: string(saved, "mount_root").to_owned(),
                workspace_id: string(saved, "workspace_id").to_owned(),
            },
            mapping_id: string(saved, "mapping_id").to_owned(),
        }
    }
    pub fn store(&self, key: &str, mapping: &Value) -> Result<(), Text> {
        if mapping.to_string().len() > PRESET_LIMIT {
            return Err(crate::text!("The mapping is too large to remember."));
        }
        let mut all = self.all();
        all.insert(key.to_owned(), mapping.clone());
        while all.len() > PRESETS {
            let Some(first) = all.keys().find(|k| *k != key).cloned() else {
                break;
            };
            all.remove(&first);
        }
        let bytes = serde_json::to_vec(&all).unwrap_or_default();
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        crate::fsx::write_private_atomic(&self.path, &bytes, true)
            .map_err(|_| crate::text!("The mapping could not be remembered on this device."))
    }
}

/// Runs one call; tests pass a fake.
pub type Run<'a> = dyn FnMut(&Invocation, usize) -> Exit + 'a;

fn object(exit: Exit, invalid: Text) -> Result<Value, Text> {
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => serde_json::from_slice::<Value>(&stdout)
            .ok()
            .filter(Value::is_object)
            .ok_or(invalid),
        other => Err(Text::verbatim(failure_detail(other))),
    }
}

fn fresh_list(run: &mut Run) -> Result<Value, Text> {
    object(
        run(&machines_call(), MACHINES_LIMIT),
        crate::text!("Invalid machine settings response"),
    )
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// What a dialog shows of its connection.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Target {
    pub alias: String,
    pub machine_id: String,
    pub distribution: String,
    pub user: String,
    pub environment: String,
    /// The remembered mapping fields, if any.
    pub saved: Fields,
}

fn target(found: &Connection, saved: &Fields) -> Target {
    Target {
        alias: found.alias.clone(),
        machine_id: found.machine_id.clone(),
        distribution: found.distribution().to_owned(),
        user: found.user().to_owned(),
        environment: found.environment(),
        saved: saved.clone(),
    }
}

/// A previewed mapping.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Mapped {
    pub windows: String,
    pub wsl: String,
    pub canonical: String,
}

/// Qt's WslPathMappingDialog: one dialog's state.
pub struct MappingDialog {
    found: Connection,
    key: String,
    mapping_id: String,
    fields: Fields,
    previewed: Option<(String, Fields, Value, String)>,
}

impl MappingDialog {
    /// Reads the connection; refuses anything but a configured local WSL one.
    pub fn open(run: &mut Run, presets: &Presets, alias: &str) -> Result<(Self, Target), Text> {
        let found = connection(&fresh_list(run)?, alias, false)?;
        let key = found.key();
        let saved = presets.load(&key);
        let shown = target(&found, &saved.fields);
        let mapping_id = if saved.mapping_id.is_empty() {
            new_id()
        } else {
            saved.mapping_id
        };
        Ok((
            Self {
                found,
                key,
                mapping_id,
                fields: saved.fields,
                previewed: None,
            },
            shown,
        ))
    }

    /// `describe` then `map`, each reply checked. A changed mapping field
    /// gives the mapping a new id; a changed source keeps it.
    pub fn preview(
        &mut self,
        run: &mut Run,
        source: &str,
        fields: &Fields,
    ) -> Result<Mapped, Text> {
        self.previewed = None;
        if fields != &self.fields {
            self.fields = fields.clone();
            self.mapping_id = new_id();
        }
        if source.is_empty()
            || !fields.complete()
            || self.found.distribution().is_empty()
            || self.found.user().is_empty()
        {
            return Err(crate::text!(
                "Enter every mapping field, then preview the exact target path."
            ));
        }
        let mapping = self.found.mapping(&self.mapping_id, fields);
        let input = source_input(&self.found, source, &fields.workspace_id);
        let invalid = || crate::text!("Invalid path identity response");
        let described = object(
            run(
                &path_ref_call("describe", &json!({"path": input, "mapping": mapping}))?,
                PATH_REF_LIMIT,
            ),
            invalid(),
        )?;
        check_described(&described, &self.found, &mapping, &input)?;
        let mapped = object(
            run(
                &path_ref_call("map", &json!({"path": described, "mapping": mapping}))?,
                PATH_REF_LIMIT,
            ),
            invalid(),
        )?;
        let wsl = check_mapped(&mapped, &self.found, &mapping, &described)?;
        let canonical = string(&mapped, "canonical_path").to_owned();
        self.previewed = Some((source.to_owned(), fields.clone(), mapping, wsl.clone()));
        Ok(Mapped {
            windows: source.to_owned(),
            wsl,
            canonical,
        })
    }

    /// Rechecks the connection, remembers the mapping when asked, and gives
    /// the WSL path for the folder browser, which still reads it natively.
    pub fn accept(
        &mut self,
        run: &mut Run,
        presets: &Presets,
        source: &str,
        fields: &Fields,
        remember: bool,
    ) -> Result<String, Text> {
        let Some((sent, sent_fields, mapping, wsl)) = self.previewed.clone() else {
            return Err(crate::text!(
                "The selection changed. Preview its exact mapping before using it."
            ));
        };
        if sent != source || &sent_fields != fields {
            self.previewed = None;
            return Err(crate::text!(
                "The selection changed. Preview its exact mapping before using it."
            ));
        }
        if !unchanged(&fresh_list(run)?, &self.found) {
            self.previewed = None;
            return Err(crate::text!(
                "The WSL connection changed. Close this preview and select the machine again."
            ));
        }
        if remember {
            presets.store(&self.key, &mapping)?;
        }
        Ok(wsl)
    }
}

/// A verified preview of a mapped Windows original.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FilePreview {
    pub wsl: String,
    pub windows: String,
    pub sha256: String,
}

/// Qt's WslFileOpenDialog: one dialog's state.
pub struct FileDialog {
    found: Connection,
    key: String,
    scope: Scope,
    request_id: String,
    mapping_id: String,
    fields: Fields,
    preview: Option<(Value, Proof)>,
}

fn proof_exit(run: &mut Run, alias: &str, request: &Value) -> Result<Value, Text> {
    let call = proof_call(alias, request)?;
    match run(&help_call(), PROOF_LIMIT) {
        Exit::Code {
            code: 0, stdout, ..
        } if String::from_utf8_lossy(&stdout).contains(PROOF_MARKER) => {}
        _ => {
            return Err(crate::text!(
                "Update the matching local CLI to use scoped WSL file mapping."
            ))
        }
    }
    match run(&call, PROOF_LIMIT) {
        Exit::Code {
            code: 0, stdout, ..
        } => serde_json::from_slice::<Value>(&stdout)
            .ok()
            .filter(Value::is_object)
            .ok_or(crate::text!("Invalid scoped WSL file proof.")),
        Exit::Code { stderr, .. } | Exit::Abnormal { stderr } if !stderr.trim().is_empty() => Err(
            Text::verbatim(stderr.trim().chars().take(2000).collect::<String>()),
        ),
        _ => Err(crate::text!(
            "The scoped WSL file proof was unavailable or refused."
        )),
    }
}

impl FileDialog {
    pub fn open(
        run: &mut Run,
        presets: &Presets,
        alias: &str,
        scope: Scope,
    ) -> Result<(Self, Target), Text> {
        scope.check()?;
        let found = connection(&fresh_list(run)?, alias, true)?;
        let key = found.key();
        let saved = presets.load(&key);
        let shown = target(&found, &saved.fields);
        let mapping_id = if saved.mapping_id.is_empty() {
            new_id()
        } else {
            saved.mapping_id
        };
        Ok((
            Self {
                found,
                key,
                scope,
                request_id: new_id(),
                mapping_id,
                fields: saved.fields,
                preview: None,
            },
            shown,
        ))
    }

    fn sent(&mut self, fields: &Fields) -> Value {
        if fields != &self.fields {
            // Any change is a new request and a new mapping (Qt's invalidate).
            self.fields = fields.clone();
            self.request_id = new_id();
            self.mapping_id = new_id();
            self.preview = None;
        }
        let mapping = self.found.mapping(&self.mapping_id, fields);
        selection(&self.scope, &self.request_id, &self.found, &mapping)
    }

    /// Reads the file through its conversation in WSL, checks the proof and
    /// verifies the Windows original. Opens nothing.
    pub fn preview(
        &mut self,
        run: &mut Run,
        verify: &dyn Fn(&str, &str, u64) -> Result<Guard, Text>,
        fields: &Fields,
    ) -> Result<FilePreview, Text> {
        self.preview = None;
        if !fields.complete() {
            return Err(crate::text!(
                "Enter every mapping field, then preview and verify the file."
            ));
        }
        let sent = self.sent(fields);
        let raw = proof_exit(run, &self.found.alias, &json!({"selection": sent}))?;
        let proof = check_proof(&raw, &sent, &self.found.alias, &self.found)?;
        verify(&proof.windows_path, &proof.sha256, proof.bytes)?;
        let shown = FilePreview {
            wsl: proof.wsl_path.clone(),
            windows: proof.windows_path.clone(),
            sha256: proof.sha256.clone(),
        };
        self.preview = Some((sent, proof));
        Ok(shown)
    }

    /// Asks again for the proof bound to the preview's token, verifies the
    /// original again and opens it while the checked handles stay open.
    /// Gives the opened path and the guard to hold through the open.
    pub fn recheck(
        &mut self,
        run: &mut Run,
        verify: &dyn Fn(&str, &str, u64) -> Result<Guard, Text>,
        fields: &Fields,
    ) -> Result<(String, Guard), Text> {
        let Some((sent, preview)) = self.preview.clone() else {
            return Err(crate::text!(
                "Preview and verify the file before opening it."
            ));
        };
        if self.sent(fields) != sent {
            return Err(crate::text!(
                "Preview and verify the file before opening it."
            ));
        }
        let changed =
            || crate::text!("The file proof no longer matches this preview. Nothing was opened.");
        let result = (|| {
            let raw = proof_exit(
                run,
                &self.found.alias,
                &json!({"selection": sent, "expected_preview": preview.token}),
            )?;
            let proof = check_proof(&raw, &sent, &self.found.alias, &self.found)?;
            if proof.token != preview.token || proof.raw != preview.raw {
                return Err(changed());
            }
            let guard = verify(&proof.windows_path, &proof.sha256, proof.bytes)?;
            Ok((proof.windows_path, guard))
        })();
        if result.is_err() {
            self.preview = None;
        }
        result
    }

    /// After a successful open: remembers the mapping when asked.
    pub fn remember(&self, presets: &Presets) -> Result<(), Text> {
        presets.store(
            &self.key,
            &self.found.mapping(&self.mapping_id, &self.fields),
        )
    }
}

/// Runs a call with the given output cap.
pub fn runner(hgs: PathBuf) -> impl FnMut(&Invocation, usize) -> Exit {
    move |call: &Invocation, cap: usize| {
        crate::run::run_limited(&hgs, call, &[], cap).unwrap_or(Exit::Abnormal {
            stderr: "The reply was too large.".into(),
        })
    }
}

#[cfg(test)]
mod tests;
