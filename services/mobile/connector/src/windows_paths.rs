//! WhiteWh fork: Windows folders in the mobile protocol.
//!
//! The phone and the relay protocol name folders as absolute POSIX paths: the
//! apps accept a folder only when it starts with `/`. A Windows gateway's
//! native CLI answers `D:\Work\project`. At the boundary to `hgs.exe` a folder
//! travels as `/D:/Work/project` towards the phone and as `D:\Work\project`
//! towards hgs. Only whole values change: a string is a folder when it is one
//! completely, never a path inside other text.

use serde_json::Value;

/// `D:\Work\x` (or `D:/Work/x`, `D:\`) as the phone's `/D:/Work/x`.
pub fn to_phone(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    let rest = &text[2..];
    if !(rest.is_empty() || rest.starts_with('\\') || rest.starts_with('/')) || rest.contains('\0') {
        return None;
    }
    let drive = bytes[0].to_ascii_uppercase() as char;
    let tail = rest.replace('\\', "/");
    let tail = tail.trim_end_matches('/');
    Some(format!("/{drive}:{tail}"))
}

/// The phone's `/D:/Work/x` (or `/D:`) as hgs's `D:\Work\x` (`D:\`).
pub fn to_native(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'/' || !bytes[1].is_ascii_alphabetic() || bytes[2] != b':' {
        return None;
    }
    let rest = &text[3..];
    if !(rest.is_empty() || rest.starts_with('/')) || rest.contains('\\') || rest.contains('\0') {
        return None;
    }
    let drive = bytes[1].to_ascii_uppercase() as char;
    let tail = rest.trim_end_matches('/').replace('/', "\\");
    Some(format!("{drive}:\\{}", tail.trim_start_matches('\\')))
}

/// Payload fields that name a folder.
const FOLDER_KEYS: &[&str] = &[
    "directory", "path", "cwd", "parent", "destination", "location", "folder", "add_folder", "root",
];

/// Arguments and folder fields of a request, as hgs expects them.
pub fn native_argv(argv: &mut [String]) {
    for argument in argv.iter_mut() {
        if let Some(native) = to_native(argument) {
            *argument = native;
        }
    }
}

pub fn native_payload(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                match item {
                    Value::String(text) if FOLDER_KEYS.contains(&key.as_str()) => {
                        if let Some(native) = to_native(text) {
                            *text = native;
                        }
                    }
                    Value::Array(items) if key == "argv" => {
                        for item in items.iter_mut() {
                            if let Value::String(text) = item {
                                if let Some(native) = to_native(text) {
                                    *text = native;
                                }
                            }
                        }
                    }
                    Value::Object(_) | Value::Array(_) => native_payload(item),
                    _ => {}
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(native_payload),
        _ => {}
    }
}

/// Every whole Windows path of a native answer, as the phone names it.
pub fn phone_output(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(phone) = to_phone(text) {
                *text = phone;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(phone_output),
        Value::Object(map) => map.values_mut().for_each(phone_output),
        _ => {}
    }
}

/// Folder ids of [`add_local_folders`]: the desktop keeps a Windows
/// computer's folders out of the shared catalog, so they have no catalog id.
pub const LOCAL_FOLDER_PREFIX: &str = "windows-local-";

/// A Windows gateway starts phone sessions only in project working folders.
pub const PROJECT_FOLDERS_ONLY: &str =
    "On this computer the phone starts sessions only in a project's working folder";

/// The shared catalog (`hgs swarm get`) with this computer's own project
/// folders (`hgs project ls --json`, already in the phone's form) added to the
/// projects of the same name. The desktop keeps local Windows folders out of
/// the shared catalog; the phone needs them to start a session in a project.
pub fn add_local_folders(swarm: &mut Value, local: &Value) {
    let node = swarm["node_id"].as_str().unwrap_or_default().to_owned();
    if node.is_empty() {
        return;
    }
    let machine_name = swarm["machines"]
        .as_array()
        .and_then(|machines| machines.iter().find(|m| m["id"].as_str() == Some(node.as_str())))
        .and_then(|m| m["name"].as_str())
        .unwrap_or_default()
        .to_owned();
    let Some(projects) = swarm["organization"]["projects"].as_array_mut() else {
        return;
    };
    for entry in local.as_array().into_iter().flatten() {
        let (Some(name), Some(path)) = (entry["name"].as_str(), entry["dir"].as_str()) else {
            continue;
        };
        if entry["exists"] == Value::Bool(false) || !path.starts_with('/') {
            continue;
        }
        let Some(project) = projects.iter_mut().find(|p| p["name"].as_str() == Some(name)) else {
            continue;
        };
        if !project["folders"].is_array() {
            project["folders"] = Value::Array(Vec::new());
        }
        let folders = project["folders"].as_array_mut().expect("array");
        if folders.iter().any(|f| f["machine_id"].as_str() == Some(node.as_str()) && f["path"].as_str() == Some(path)) {
            continue;
        }
        let digest = ring::digest::digest(&ring::digest::SHA256, format!("{node}
{path}").as_bytes());
        let id: String = digest.as_ref()[..12].iter().map(|b| format!("{b:02x}")).collect();
        let label = path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(path);
        folders.push(serde_json::json!({
            "id": format!("{LOCAL_FOLDER_PREFIX}{id}"), "name": label, "path": path,
            "machine_id": node, "machine_name": machine_name,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn folders_round_trip_between_the_phone_and_hgs() {
        assert_eq!(to_phone(r"D:\Work\GIT\roguedod_s1").as_deref(), Some("/D:/Work/GIT/roguedod_s1"));
        assert_eq!(to_phone(r"c:\").as_deref(), Some("/C:"));
        assert_eq!(to_phone("C:").as_deref(), Some("/C:"));
        assert_eq!(to_native("/D:/Work/GIT/roguedod_s1").as_deref(), Some(r"D:\Work\GIT\roguedod_s1"));
        assert_eq!(to_native("/d:").as_deref(), Some(r"D:\"));
        assert_eq!(to_native("/D:/").as_deref(), Some(r"D:\"));
        for path in [r"D:\Work\x y", r"C:\Users\whitl\.codex"] {
            assert_eq!(to_native(&to_phone(path).unwrap()).as_deref(), Some(path));
        }
    }

    #[test]
    fn only_whole_folders_change() {
        for text in ["codex/Temp/ImageWorks", "/home/user", "D:", "Dx\\y", "see D:\\Work", "/D", "/D:x", "~", "", "/"] {
            assert!(to_native(text).is_none(), "{text}");
        }
        assert!(to_phone("D:x").is_none());
        assert!(to_phone("see D:\\Work").is_none());
        assert!(to_phone("codex/Temp/ImageWorks").is_none());
    }

    #[test]
    fn requests_and_answers_translate_their_folders() {
        let mut argv = vec!["claude".to_owned(), "/D:/Work/app".to_owned(), "--new".to_owned()];
        native_argv(&mut argv);
        assert_eq!(argv[1], r"D:\Work\app");
        let mut payload = json!({"directory": "/D:/Work/app", "text": "/D:/Work/app", "argv": ["dirs", "/C:/Users"]});
        native_payload(&mut payload);
        assert_eq!(payload["directory"], r"D:\Work\app");
        assert_eq!(payload["text"], "/D:/Work/app");
        assert_eq!(payload["argv"][1], r"C:\Users");
        let mut answer = json!({"path": r"D:\Work", "directories": [{"name": "app", "path": r"D:\Work\app"}], "name": "codex/x"});
        phone_output(&mut answer);
        assert_eq!(answer, json!({"path": "/D:/Work", "directories": [{"name": "app", "path": "/D:/Work/app"}], "name": "codex/x"}));
    }

    #[test]
    fn local_project_folders_join_the_shared_catalog() {
        let mut swarm = json!({"node_id": "n1", "machines": [{"id": "n1", "name": "msi"}],
            "organization": {"projects": [{"id": "p1", "name": "RogueDod", "folders": []}, {"id": "p2", "name": "Other", "folders": []}]}});
        let local = json!([{"name": "RogueDod", "dir": "/D:/Work/RogueDOD", "exists": true}, {"name": "Gone", "dir": "/D:/x", "exists": true}]);
        add_local_folders(&mut swarm, &local);
        add_local_folders(&mut swarm, &local);
        let folders = swarm["organization"]["projects"][0]["folders"].as_array().unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0]["path"], "/D:/Work/RogueDOD");
        assert_eq!(folders[0]["machine_id"], "n1");
        assert_eq!(folders[0]["name"], "RogueDOD");
        assert!(folders[0]["id"].as_str().unwrap().starts_with(LOCAL_FOLDER_PREFIX));
        assert!(swarm["organization"]["projects"][1]["folders"].as_array().unwrap().is_empty());
    }
}
