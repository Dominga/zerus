//! Native terminal-shaped inputs. Session, inspection and input rows use the
//! product writers; only the backend's screen and native metadata are stand-ins.
use super::*;
use std::sync::Mutex;

pub(crate) struct Terminal {
    pub record: Value,
    pub screen: String,
    pub x: usize,
    pub y: usize,
    staged: Mutex<Vec<u8>>,
    pub submissions: Mutex<Vec<String>>,
}
impl Terminal {
    pub fn new(mut record: Value, shape: &str) -> &'static Self {
        let agent = string(&record, "agent");
        let (screen, x, y) = match (agent, shape) {
            ("codex", "ready") => ("Working\n\n› \x1b[2mAsk Codex to do anything\x1b[0m\n\n  GPT-6-Astra xhigh · /project · Session title    Pursuing goal (1h)\n  ? for shortcuts".to_owned(), 2, 2),
            ("claude", "ready") => ("Ready\n\x1b[39m❯\u{a0}\n──────────────\n  auto mode on".to_owned(), 2, 1),
            ("kimi", "ready") => (" ╭────────────╮\n │ >          │\n ╰────────────╯".to_owned(), 5, 1),
            (_, "trust") => (include_str!("../../tests/fixtures/codex-folder-trust.txt").replace("/work/example", string(&record, "cwd")), 2, 1),
            _ => ("Starting native agent…".to_owned(), 0, 0),
        };
        let pid = std::process::id();
        record["pid"] = json!(pid);
        record["process_start"] = json!(process_start(pid));
        listing_fixture::save(&mut record).unwrap();
        Box::leak(Box::new(Self {
            record,
            screen,
            x,
            y,
            staged: Mutex::new(Vec::new()),
            submissions: Mutex::new(Vec::new()),
        }))
    }
    pub fn with<T>(&'static self, body: impl FnOnce() -> T) -> T {
        backend::with_fake(self, body)
    }
    pub fn inspection(&self) -> Value {
        journal::inspection(string(&self.record, "name"), 0, None, false).unwrap()
    }
}
impl backend::TerminalBackend for Terminal {
    fn server_running(&self) -> bool {
        true
    }
    fn listing(&self) -> Result<backend::Listing> {
        Ok(backend::Listing {
            clients: vec![],
            sessions: vec![backend::SessionRow {
                name: terminal_name(&self.record).into(),
                id: "$1".into(),
                attached: 0,
                created: 1,
                launch_id: string(&self.record, "launch_id").into(),
            }],
        })
    }
    fn panes(&self) -> Result<Vec<backend::PaneRow>> {
        Ok(vec![backend::PaneRow {
            session: terminal_name(&self.record).into(),
            session_id: "$1".into(),
            pane: "%1".into(),
            dead: "0".into(),
            run_marker: string(&self.record, "run_id").into(),
        }])
    }
    fn pane_paths(&self) -> Result<Vec<backend::PanePath>> {
        Ok(vec![])
    }
    fn capture(&self, _: &str, _: backend::Capture) -> Result<Vec<u8>> {
        Ok(self.screen.as_bytes().to_vec())
    }
    fn input_state(&self, claim: &backend::InputClaim) -> Result<backend::InputState> {
        assert_eq!(
            (claim.name, claim.run_id, claim.pane),
            (
                terminal_name(&self.record),
                string(&self.record, "run_id"),
                "%1"
            )
        );
        Ok(backend::InputState {
            cursor_x: self.x.to_string(),
            cursor_y: self.y.to_string(),
        })
    }
    fn pane_info(
        &self,
        _: backend::Target,
        fields: &[backend::PaneField],
    ) -> Result<backend::PaneInfo> {
        use backend::PaneField::*;
        Ok(backend::PaneInfo::new(
            fields
                .iter()
                .map(|f| match f {
                    Width => "80".into(),
                    Height => "24".into(),
                    CursorX => self.x.to_string(),
                    CursorY => self.y.to_string(),
                    InMode | Dead => "0".into(),
                    BracketedPaste => "1".into(),
                    SessionId => "$1".into(),
                    PaneId => "%1".into(),
                    RunMarker => string(&self.record, "run_id").into(),
                    SessionName => terminal_name(&self.record).into(),
                    _ => String::new(),
                })
                .collect::<Vec<_>>()
                .join("\t"),
        ))
    }
    fn stage(&self, _: &str, text: &[u8], _: backend::StageErrors) -> Result<bool> {
        *self.staged.lock().unwrap() = text.to_vec();
        Ok(true)
    }
    fn paste(&self, _: &str, _: &str, _: backend::Paste) -> Result<()> {
        Ok(())
    }
    fn send(&self, _: &str, keys: &[backend::Key]) -> Result<()> {
        for key in keys {
            if matches!(key, backend::Key::Named("Enter")) {
                self.submissions
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(self.staged.lock().unwrap().clone()).unwrap());
            }
        }
        Ok(())
    }
}
