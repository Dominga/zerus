//! The File reference click uses the maintained terminal, not an ssh URL handler.
use std::path::Path;

use serde_json::Value;
use zerus_client_core::{terminal_launch, MachineMeta, ViewRow};

#[derive(Clone)]
pub(super) struct Identity {
    name: String,
    run: String,
    conversation: String,
    archive: Option<String>,
    machine: Option<String>,
}

impl Identity {
    pub(super) fn capture(
        computer: &str,
        row: Option<&ViewRow>,
        machine: Option<&MachineMeta>,
    ) -> Option<Self> {
        if computer.is_empty() || !zerus_client_core::fleet::valid_alias(computer) {
            return None;
        }
        let row = row?;
        let run = row.run_id.as_ref().filter(|id| !id.is_empty())?;
        let conversation = row.conversation_id.as_ref().filter(|id| !id.is_empty())?;
        Some(Self {
            name: row.name.clone(),
            run: run.clone(),
            conversation: conversation.clone(),
            archive: row.archive_id.clone(),
            machine: machine.and_then(|m| m.machine_id.clone()),
        })
    }
}

pub(super) fn current(
    identity: Option<&Identity>,
    row: Option<&ViewRow>,
    machine: Option<&MachineMeta>,
) -> bool {
    let (Some(identity), Some(row)) = (identity, row) else {
        return false;
    };
    row.name == identity.name
        && row.run_id.as_ref() == Some(&identity.run)
        && row.conversation_id.as_ref() == Some(&identity.conversation)
        && row.archive_id == identity.archive
        && machine.and_then(|m| m.machine_id.as_ref()) == identity.machine.as_ref()
}

pub(super) fn configured(alias: &str, machines: &Value) -> bool {
    !alias.is_empty()
        && zerus_client_core::fleet::valid_alias(alias)
        && machines["local"].as_str() != Some(alias)
        && machines["machines"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["alias"].as_str() == Some(alias)))
}

pub(super) fn open(
    terminal: &mut terminal_launch::Launcher,
    hgs: &Path,
    alias: &str,
) -> Result<Option<String>, String> {
    terminal.open_program(
        &hgs.to_string_lossy(),
        &terminal_launch::ssh_arguments(alias),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use terminal_launch::{Desktop, Launcher, NoKonsole, Platform, Settings, Start};

    #[test]
    fn click_rejects_replaced_session_computer_and_forgotten_alias() {
        let row = ViewRow {
            name: "sh/example/probe".into(),
            run_id: Some("run-1".into()),
            conversation_id: Some("conversation-1".into()),
            ..Default::default()
        };
        let machine = MachineMeta {
            machine_id: Some("machine-1".into()),
            ..Default::default()
        };
        let identity = Identity::capture("example-peer", Some(&row), Some(&machine)).unwrap();
        assert!(current(Some(&identity), Some(&row), Some(&machine)));
        for changed in [
            ViewRow {
                name: "sh/example/other".into(),
                ..row.clone()
            },
            ViewRow {
                run_id: Some("run-2".into()),
                ..row.clone()
            },
            ViewRow {
                conversation_id: Some("conversation-2".into()),
                ..row.clone()
            },
            ViewRow {
                archive_id: Some("archive-1".into()),
                ..row.clone()
            },
        ] {
            assert!(!current(Some(&identity), Some(&changed), Some(&machine)));
        }
        assert!(!current(Some(&identity), None, Some(&machine)));
        assert!(!current(Some(&identity), Some(&row), None));
        assert!(!current(
            Some(&identity),
            Some(&row),
            Some(&MachineMeta {
                machine_id: Some("machine-2".into()),
                ..machine.clone()
            })
        ));
        assert!(Identity::capture("", Some(&row), Some(&machine)).is_none());
        assert!(Identity::capture("example-peer", None, Some(&machine)).is_none());
        let saved =
            json!({"local":"example-host","machines":[{"alias":"example-peer","enabled":false}]});
        // Offline and unmonitored aliases still open a terminal for login repair.
        assert!(configured("example-peer", &saved));
        assert!(!configured("example-host", &saved));
        assert!(!configured("forgotten", &saved));
        assert!(!configured("-oProxyCommand=example", &saved));
    }

    struct Recorded(Arc<Mutex<Vec<Start>>>);
    impl Desktop for Recorded {
        fn find(&self, _: &str) -> Option<String> {
            None
        }
        fn env(&self, _: &str) -> Option<String> {
            None
        }
        fn exists(&self, _: &str) -> bool {
            false
        }
        fn home(&self) -> String {
            "/synthetic/home".into()
        }
        fn console_host(&self) -> String {
            "unused".into()
        }
        fn start(&mut self, start: &Start) -> Result<(), String> {
            self.0.lock().unwrap().push(start.clone());
            Ok(())
        }
    }

    #[test]
    fn configured_launcher_runs_local_hgs_machine_ssh_exact_alias() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut terminal = Launcher::with(
            Settings {
                template: "example-terminal --command {cmd}".into(),
                ..Default::default()
            },
            Platform::Linux,
            Box::new(Recorded(calls.clone())),
            Box::new(NoKonsole),
        );
        assert_eq!(
            open(
                &mut terminal,
                Path::new("/synthetic/bin/hgs"),
                "example-peer"
            ),
            Ok(None)
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "example-terminal");
        assert_eq!(
            calls[0].arguments,
            ["--command", "/synthetic/bin/hgs machine ssh example-peer"]
        );
    }
}
