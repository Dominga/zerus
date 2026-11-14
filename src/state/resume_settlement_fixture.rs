//! Isolated test inputs; records and archives are produced by the real writers.
use super::*;
use std::{fs, process::Command, thread, time::Duration};

pub(crate) struct Fixture {
    pub raw: Value,
    pub id: String,
    pub name: String,
    pane_process: Option<(u32, String)>,
    server_process: Option<(u32, String)>,
    label: Option<Value>,
}
impl Fixture {
    pub fn new() -> Self {
        assert!(std::env::var_os("HGS_SETTLEMENT_FIXTURE").is_some());
        fs::create_dir_all(root()).unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .arg(root())
            .status()
            .unwrap()
            .success());
        let transcript = root().join("synthetic-history.jsonl");
        // Synthetic provider-history context, never claimed as a native turn.
        fs::write(&transcript, "{}\n").unwrap();
        let name = "claude/settlement/member".to_owned();
        let run = uuid::Uuid::new_v4().to_string();
        let conversation = uuid::Uuid::new_v4().to_string();
        let mut raw = json!({"version":1,"name":name,"agent":"claude","run_id":run,
            "session_id":uuid::Uuid::new_v4().to_string(),"launch_id":uuid::Uuid::new_v4().to_string(),
            "conversation_id":conversation,"conversation_state":"ended","account_id":"account-a",
            "cwd":root(),"launch_dir":root(),"base":["/bin/true"],"transcript":transcript,
            "worktree_owner":{"id":"owner-1"},"session_end":{"run_id":run,"conversation_id":conversation,
            "reason":"prompt_input_exit","at":now()}});
        write(&mut raw).unwrap();
        let mut spawned = supervisor::spawn(&["/bin/true".into()], &name, &run).unwrap();
        let pid = spawned.child.id();
        let status = supervisor::wait(&mut spawned.child).unwrap();
        supervisor::finish(&name, &run, pid, status).unwrap();
        raw = archive::records()
            .unwrap()
            .into_iter()
            .find(|r| r["run_id"] == run)
            .unwrap();
        let id = string(&raw, "archive_id").to_owned();
        assert_eq!(raw["completion_reason"], "clean_exit");
        Self {
            raw,
            id,
            name,
            pane_process: None,
            server_process: None,
            label: None,
        }
    }
    pub fn hold_pane(&mut self) {
        // An explicit plain fixture process; never self-exec a libtest image.
        assert!(Command::new("tmux")
            .args(["new-session", "-d", "-s", &self.name, "-c"])
            .arg(root())
            .args(["/bin/sleep", "60"])
            .status()
            .unwrap()
            .success());
        let pane = backend::current()
            .panes()
            .unwrap()
            .into_iter()
            .find(|p| p.session == self.name)
            .unwrap()
            .pane;
        let info = backend::current()
            .pane_info(
                backend::Target::Pane(&pane),
                &[backend::PaneField::PanePid, backend::PaneField::ServerPid],
            )
            .unwrap();
        let fields = info.fields();
        let pid: u32 = fields[0].parse().unwrap();
        let server: u32 = fields[1].parse().unwrap();
        self.pane_process = Some((pid, process_start(pid)));
        self.server_process = Some((server, process_start(server)));
    }
    pub fn hold_label(&mut self) {
        let mut record = self.raw.clone();
        for key in [
            "archive_id",
            "archived_at",
            "state",
            "completion_source",
            "completion_reason",
            "exit_code",
            "termination_signal",
            "exited_at",
            "session_end",
        ] {
            record.as_object_mut().unwrap().remove(key);
        }
        record["session_id"] = json!(uuid::Uuid::new_v4().to_string());
        record["run_id"] = json!(uuid::Uuid::new_v4().to_string());
        record["conversation_state"] = json!("active");
        record["pid"] = json!(std::process::id());
        record["process_start"] = json!(process_start(std::process::id()));
        let _guard = lock(None).unwrap();
        write(&mut record).unwrap();
        assert!(lifecycle::restore_name_busy(&self.name).unwrap());
        self.label = Some(record);
    }
    pub fn release(&mut self) {
        if let Some(record) = self.label.take() {
            let _guard = lock(None).unwrap();
            storage::remove_record(&record).unwrap();
        }
        if let Some((pid, birth)) = self.pane_process.take() {
            signal(pid, &birth);
            for _ in 0..300 {
                if !backend::current()
                    .panes()
                    .unwrap()
                    .iter()
                    .any(|p| p.session == self.name)
                {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        let _guard = lock(None).unwrap();
        assert!(!lifecycle::restore_name_busy(&self.name).unwrap());
    }
    pub fn view(&self) -> Value {
        reference_details(&self.name, &self.id, "").unwrap()
    }
    pub fn restore_admitted(&self) -> Result<i32> {
        lifecycle::resume_scoped(
            &[self.name.clone(), "--archive".into(), self.id.clone()],
            true,
            None,
        )
    }
}
fn signal(pid: u32, birth: &str) {
    if !birth.is_empty() && process_start(pid) == birth {
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some((pid, birth)) = &self.pane_process {
            signal(*pid, birth);
        }
        if let Some((pid, birth)) = &self.server_process {
            signal(*pid, birth);
        }
    }
}
