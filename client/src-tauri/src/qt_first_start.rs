//! Task 102: Qt's choices that live outside the settings store, read once
//! from one read-only snapshot of Qt's store when this client may read it
//! (`settings::seed_source`: the switch identity, never a replay or the dev
//! build): the WSL presets (task 73's `wsl-path-mappings.json`) and the
//! private-store notices already raised (task 75). Each file is written only
//! when it does not exist yet; Qt's store is never written.
use std::path::Path;
use zerus_client_core::drafts;
use zerus_client_core::private_notices;
use zerus_client_core::settings::QtStore;
use zerus_client_core::wsl_mapping::Presets;

/// The WSL presets file (task 73).
pub const PRESETS_FILE: &str = "wsl-path-mappings.json";

/// What one first start imported: presets and notices written, or None.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub presets: Option<usize>,
    pub notices: Option<usize>,
    /// Task 103: Qt's task-board form drafts taken into the draft store.
    pub forms: Option<usize>,
}

/// Reads Qt's store once when a destination is missing and writes what it
/// holds for it.
pub fn import(dir: &Path, seed: Option<QtStore>) -> Imported {
    let presets = dir.join(PRESETS_FILE);
    let notices = dir.join(private_notices::FILE);
    let forms = dir.join(drafts::DIR).join(drafts::QT_FORMS_MARKER);
    let Some(store) = seed else {
        return Imported::default();
    };
    if presets.exists() && notices.exists() && forms.exists() {
        return Imported::default();
    }
    let snapshot = match store.snapshot() {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return Imported::default(),
        Err(error) => {
            eprintln!("zerus-client: Qt store not read for presets, notices and forms: {error}");
            return Imported::default();
        }
    };
    Imported {
        presets: Presets::new(presets).seed_from_qt(&snapshot.values),
        notices: private_notices::seed_from_qt(&notices, &snapshot.values),
        forms: drafts::import_qt_forms(&dir.join(drafts::DIR), &snapshot.values),
    }
}

/// The notices already raised on this device (at most 512).
#[tauri::command]
pub fn private_store_notices_known(app: tauri::AppHandle) -> Vec<String> {
    private_notices::load(&crate::data_dir(&app).join(private_notices::FILE))
}

/// Keeps the notices raised so far for the next start.
#[tauri::command]
pub async fn private_store_notices_save(
    app: tauri::AppHandle,
    known: Vec<String>,
) -> Result<(), String> {
    let path = crate::data_dir(&app).join(private_notices::FILE);
    tauri::async_runtime::spawn_blocking(move || private_notices::save(&path, &known))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 102 fixture: a Qt INI as the fork's Qt writes it, with the five
    /// keys the move carries. Its bytes are checked unchanged afterwards.
    pub(crate) const FIXTURE_INI: &str = "[General]\r\n\
privateStores/notified=\"\\nC:\\\\Users\\\\example\\\\AppData\\\\Local\\\\Temp\\\\.tmp0IVyIz\\\\read-marks.json\\nit is not private, and it is outside Zerus's folders, so Zerus does not change it\", \"studio\\n/home/example/.local/state/hgs/x.json\\nowned by another user\", just text\r\n\
\r\n\
[mobile]\r\n\
localRelay=true\r\n\
localRelayPort=9443\r\n\
runConnector=false\r\n\
\r\n\
[pathMappings]\r\n\
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef=\"@ByteArray({\\\"mapping_id\\\":\\\"m-1\\\",\\\"workspace_id\\\":\\\"w-1\\\",\\\"windows_root\\\":\\\"D:\\\\\\\\Work\\\",\\\"mount_root\\\":\\\"/mnt/d/Work\\\"})\"\r\n\
not-a-hash=\"@ByteArray({\\\"mapping_id\\\":\\\"x\\\"})\"\r\n\
\r\n\
[speech]\r\n\
microphone=\"{0.0.1.00000000}.{00000000-0000-0000-0000-000000000000}\"\r\n\
\r\n\
[workspace]\r\n\
launchWorktree=none\r\n";

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "zerus-qt-first-start-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_first_start_writes_qt_s_presets_and_notices_once_and_never_the_ini() {
        let dir = scratch("import");
        let ini = dir.join("hgs-tray.ini");
        std::fs::write(&ini, FIXTURE_INI).unwrap();
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        // No Qt store (a replay, the dev identity): nothing is read or written.
        assert_eq!(import(&data, None), Imported::default());
        assert!(!data.join(PRESETS_FILE).exists() && !data.join(private_notices::FILE).exists());
        let imported = import(&data, Some(QtStore::Ini(ini.clone())));
        assert_eq!(
            imported,
            Imported {
                presets: Some(1),
                notices: Some(2),
                forms: Some(0)
            }
        );
        let presets = Presets::new(data.join(PRESETS_FILE));
        let preset =
            presets.load("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
        assert_eq!(preset.mapping_id, "m-1");
        assert_eq!(preset.fields.windows_root, "D:\\Work");
        assert_eq!(preset.fields.mount_root, "/mnt/d/Work");
        assert_eq!(preset.fields.workspace_id, "w-1");
        assert_eq!(
            private_notices::load(&data.join(private_notices::FILE)),
            [
                "\nC:\\Users\\example\\AppData\\Local\\Temp\\.tmp0IVyIz\\read-marks.json\nit is not private, and it is outside Zerus's folders, so Zerus does not change it",
                "studio\n/home/example/.local/state/hgs/x.json\nowned by another user",
            ]
        );
        // The client's own files win: a second start imports nothing.
        private_notices::save(
            &data.join(private_notices::FILE),
            &["\n/own\nreason".into()],
        )
        .unwrap();
        assert_eq!(
            import(&data, Some(QtStore::Ini(ini.clone()))),
            Imported::default()
        );
        assert_eq!(
            private_notices::load(&data.join(private_notices::FILE)),
            ["\n/own\nreason"]
        );
        // Qt's store is read, never written.
        assert_eq!(std::fs::read_to_string(&ini).unwrap(), FIXTURE_INI);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn at_most_64_presets_and_512_notices_are_taken() {
        let dir = scratch("bounds");
        let mut ini = String::from("[General]\r\nprivateStores/notified=");
        let notices: Vec<String> = (0..600)
            .map(|n| format!("\"\\n/p/{n:04}\\nreason\""))
            .collect();
        ini.push_str(&notices.join(", "));
        ini.push_str("\r\n\r\n[pathMappings]\r\n");
        for n in 0..80 {
            ini.push_str(&format!(
                "{n:064x}=\"@ByteArray({{\\\"mapping_id\\\":\\\"m-{n}\\\"}})\"\r\n"
            ));
        }
        let path = dir.join("hgs-tray.ini");
        std::fs::write(&path, &ini).unwrap();
        let data = dir.join("data");
        let imported = import(&data, Some(QtStore::Ini(path.clone())));
        assert_eq!(
            imported,
            Imported {
                presets: Some(64),
                notices: Some(512),
                forms: Some(0)
            }
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), ini);
        let _ = std::fs::remove_dir_all(&dir);
    }

    const FORMS_INI: &str = "[teams]\r\n\
hideFinished=true\r\n\
heat\\team-one=time\r\n\
\r\n\
[tasks/titleDraft/project-fixture]\r\n\
11111111-1111-4111-8111-111111111111=\"Draft title\"\r\n\
\r\n\
[tasks/newCard/project-fixture]\r\n\
title=\"New card title\"\r\n\
\r\n\
[tasks/noteDraft/project-fixture]\r\n\
22222222-2222-4222-8222-222222222222=\"Legacy note\"\r\n";

    fn form(store: &drafts::DraftStore, key: &str) -> String {
        let (tx, rx) = std::sync::mpsc::channel();
        store.load(key, move |value| {
            let _ = tx.send(value);
        });
        rx.recv().unwrap().draft.draft.text
    }

    /// Task 103: Qt's task-board forms become drafts in the store the task
    /// board reads, once; its INI is only read.
    #[test]
    fn the_first_start_imports_qt_s_task_forms_once_and_never_the_ini() {
        let dir = scratch("forms");
        let ini = dir.join("hgs-tray.ini");
        std::fs::write(&ini, FORMS_INI).unwrap();
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let imported = import(&data, Some(QtStore::Ini(ini.clone())));
        assert_eq!(imported.forms, Some(3));
        let store = drafts::DraftStore::open(data.join(drafts::DIR));
        assert_eq!(
            form(
                &store,
                "tasks/titleDraft/project-fixture/11111111-1111-4111-8111-111111111111"
            ),
            "Draft title"
        );
        assert_eq!(
            form(&store, "tasks/newCard/project-fixture/title"),
            "New card title"
        );
        assert_eq!(
            form(
                &store,
                "task-comment\nproject-fixture\n22222222-2222-4222-8222-222222222222"
            ),
            "Legacy note"
        );
        // The client's own files win: a second start imports nothing.
        assert_eq!(
            import(&data, Some(QtStore::Ini(ini.clone()))),
            Imported::default()
        );
        assert_eq!(std::fs::read_to_string(&ini).unwrap(), FORMS_INI);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_existing_draft_wins_over_qt_s_form_and_a_replay_reads_nothing() {
        let dir = scratch("forms-own");
        let ini = dir.join("hgs-tray.ini");
        std::fs::write(&ini, FORMS_INI).unwrap();
        let data = dir.join("data");
        let store = drafts::DraftStore::open(data.join(drafts::DIR));
        let key = "tasks/titleDraft/project-fixture/11111111-1111-4111-8111-111111111111";
        let (tx, rx) = std::sync::mpsc::channel();
        let mut own = drafts::StoredDraft::default();
        own.draft.text = "My own title".into();
        store.save_durable(key, own, move |saved| {
            let _ = tx.send(saved);
        });
        rx.recv().unwrap().unwrap();
        assert_eq!(
            import(&data, Some(QtStore::Ini(ini.clone()))).forms,
            Some(2)
        );
        assert_eq!(form(&store, key), "My own title");
        // No Qt store (a replay, the dev identity): nothing is read.
        let replay = dir.join("replay");
        assert_eq!(import(&replay, None), Imported::default());
        assert!(!replay.join(drafts::DIR).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
