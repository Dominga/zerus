//! Exact-thread native queue for a TUI that owns multiple independent tasks.
//! It never types into whichever task happens to be visible in the terminal.
use super::*;
use std::io::{BufRead, BufReader, Read};
use std::time::Instant;

pub(super) struct Queue {
    command: std::process::Command,
    executable: PathBuf,
    held: fs::File,
    file: crate::platform::FileIdentity,
    pid: u32,
    process: crate::platform::ProcessIdentity,
    conversation: String,
    until: Instant,
}

fn reply(stdout: &[u8], conversation: &str) -> Option<String> {
    let line = std::str::from_utf8(stdout).ok()?.strip_suffix('\n')?;
    let (id, target) = line
        .strip_prefix("Queued message ")?
        .split_once(" for thread ")?;
    if target != format!("{conversation}.")
        || !uuid::Uuid::parse_str(id).is_ok_and(|value| !value.is_nil() && value.to_string() == id)
    {
        return None;
    }
    Some(id.into())
}

impl Queue {
    pub(super) fn prepare(record: &Value, text: &str) -> Result<Self> {
        let until = Instant::now() + Duration::from_secs(10);
        let conversation = string(record, "conversation_id");
        if !uuid::Uuid::parse_str(conversation)
            .is_ok_and(|id| !id.is_nil() && id.to_string() == conversation)
            || !process_alive(record)
        {
            return Err("Codex task identity is unavailable; no message was queued".into());
        }
        let pid = record["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .ok_or("Codex process identity is unavailable")?;
        let process = crate::platform::process_identity(pid).ok_or("Codex process exited")?;
        let image = crate::platform::describe(pid).ok_or("Codex executable is unavailable")?;
        let executable = PathBuf::from(image.executable);
        if !executable.is_absolute()
            || !matches!(
                crate::platform::program_name(&executable).as_deref(),
                Some("codex" | "codex.exe")
            )
        {
            return Err(
                "This Codex task needs its native queue; no Terminal input was sent".into(),
            );
        }
        let held = crate::platform::no_follow(OpenOptions::new().read(true))
            .open(&executable)
            .map_err(|e| e.to_string())?;
        let file = crate::platform::FileIdentity::with_metadata(&held)
            .map_err(|e| e.to_string())?
            .0;
        let account = PathBuf::from(string(record, "agent_home"));
        let account = crate::platform::canonical(account).map_err(|e| e.to_string())?;
        let transcript = PathBuf::from(string(record, "transcript"));
        if !transcript.is_absolute() || !transcript.starts_with(account.join("sessions")) {
            return Err("Codex native task history is unavailable".into());
        }
        let history = crate::platform::no_follow(OpenOptions::new().read(true))
            .open(&transcript)
            .map_err(|e| e.to_string())?;
        if !history.metadata().map_err(|e| e.to_string())?.is_file()
            || !crate::platform::holds_open(pid, &transcript)
                .map_err(|_| "Codex native task file ownership is unavailable".to_owned())?
        {
            return Err("The original Codex process does not hold this task history".into());
        }
        let mut first = Vec::new();
        BufReader::new(history.take((64 << 10) + 1))
            .read_until(b'\n', &mut first)
            .map_err(|e| e.to_string())?;
        if first.len() > 64 << 10 || !first.ends_with(b"\n") {
            return Err("Codex history header exceeds its bound".into());
        }
        let header: Value = serde_json::from_slice(&first).map_err(|e| e.to_string())?;
        if header["type"] != "session_meta" || header["payload"]["id"] != conversation {
            return Err("Codex native task history identity changed".into());
        }
        let version = string(&header["payload"], "cli_version");
        if version.is_empty() || version.len() > 32 {
            return Err("Codex native task version is unavailable".into());
        }
        let mut probe = std::process::Command::new(&executable);
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HGS_"))
        {
            probe.env_remove(key);
        }
        probe.env("CODEX_HOME", &account).arg("--version");
        let (status, stdout, _) =
            crate::platform::run_bounded(&mut probe, [1024, 4096], until, &|| false)
                .map_err(|_| "Codex queue version is unavailable".to_owned())?;
        if !status.success()
            || std::str::from_utf8(&stdout).ok().map(str::trim)
                != Some(format!("codex-cli {version}").as_str())
        {
            return Err("The native queue version differs from the running Codex task".into());
        }
        let mut help = std::process::Command::new(&executable);
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HGS_"))
        {
            help.env_remove(key);
        }
        help.env("CODEX_HOME", &account).args(["queue", "--help"]);
        let (status, output, _) =
            crate::platform::run_bounded(&mut help, [16 << 10, 4096], until, &|| false)
                .map_err(|_| "Codex queue capability is unavailable".to_owned())?;
        let capability =
            std::str::from_utf8(&output).map_err(|_| "Codex queue help is unavailable")?;
        if !status.success()
            || !["--thread", "--message", "--resume"]
                .iter()
                .all(|flag| capability.split_whitespace().any(|word| word == *flag))
        {
            return Err("This native Codex does not support exact-thread queued input".into());
        }
        if text.len() > 96 << 10 || (cfg!(windows) && text.encode_utf16().count() > 24_000) {
            return Err(
                "This message exceeds the native Codex queue argument bound; nothing was queued"
                    .into(),
            );
        }
        let mut command = std::process::Command::new(&executable);
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("HGS_"))
        {
            command.env_remove(key);
        }
        command
            .env("CODEX_HOME", account)
            .current_dir(string(record, "cwd"));
        // No --resume: this operation cannot launch a daemon or a new model task.
        command.args(["queue", "--thread", conversation, "--message", text]);
        let queue = Self {
            command,
            executable,
            held,
            file,
            pid,
            process,
            conversation: conversation.into(),
            until,
        };
        queue.verify()?;
        Ok(queue)
    }

    fn verify(&self) -> Result<()> {
        if crate::platform::process_identity(self.pid).as_ref() != Some(&self.process)
            || crate::platform::file_identity(&self.executable) != Some(self.file)
            || crate::platform::FileIdentity::with_metadata(&self.held)
                .ok()
                .map(|v| v.0)
                != Some(self.file)
        {
            return Err("Codex original process or native queue executable changed".into());
        }
        Ok(())
    }

    /// Once invoked, every failure is delivery-unknown. The caller reserved the
    /// original request first, so no caller can replay this native queue write.
    pub(super) fn offer(&mut self) -> Result<String> {
        self.verify()?;
        let (status, stdout, _) =
            crate::platform::run_bounded(&mut self.command, [4096, 8192], self.until, &|| false)
                .map_err(|_| "Codex native queue outcome is unknown".to_owned())?;
        if !status.success() {
            return Err("Codex native queue did not confirm submission; do not replay it".into());
        }
        self.verify()?;
        reply(&stdout, &self.conversation)
            .ok_or_else(|| "Codex native queue returned an unknown receipt".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_one_exact_native_queue_identity_is_accepted() {
        let target = uuid::Uuid::new_v4().to_string();
        let queued = uuid::Uuid::new_v4().to_string();
        let body = format!("Queued message {queued} for thread {target}.\n");
        assert_eq!(reply(body.as_bytes(), &target), Some(queued));
        assert!(reply(body.as_bytes(), &uuid::Uuid::new_v4().to_string()).is_none());
        assert!(reply(format!("{body}extra\n").as_bytes(), &target).is_none());
        assert!(reply(b"Queued successfully\n", &target).is_none());
    }
}
