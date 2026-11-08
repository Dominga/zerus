//! Task 117: phase timings of a read the desktops poll (`hgs inspect`), and a
//! bounded log of the slow ones. Activity said "Could not refresh activity:
//! timed out" with nothing naming the step that waited; the timings now ride
//! in the answer (`timings_ms`) and a call over `SLOW_MS` leaves one line in
//! `<state>/logs/slow-calls.log`, which never grows past `LOG_BYTES` (the
//! older half moves to `slow-calls.log.1`).
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A call this slow is logged.
pub(crate) const SLOW_MS: u64 = 1000;
/// The log's size before it rotates; with its one older file, twice this.
const LOG_BYTES: u64 = 256 * 1024;
/// At most this many phases are named per call.
const MAX_PHASES: usize = 32;

/// The phases of one call, in order, each with its milliseconds.
pub(crate) struct Phases {
    started: Instant,
    last: Instant,
    phases: Vec<(&'static str, u64)>,
}

impl Phases {
    pub(crate) fn start() -> Self {
        let now = Instant::now();
        Self { started: now, last: now, phases: Vec::new() }
    }

    /// Ends the phase `name` now; a phase seen again adds up.
    pub(crate) fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        let ms = now.duration_since(self.last).as_millis() as u64;
        self.last = now;
        if let Some(found) = self.phases.iter_mut().find(|(seen, _)| *seen == name) {
            found.1 += ms;
        } else if self.phases.len() < MAX_PHASES {
            self.phases.push((name, ms));
        }
    }

    /// A part of an earlier phase measured on its own (the team store read
    /// inside the session state): named, without moving the phase clock.
    pub(crate) fn note(&mut self, name: &'static str, ms: u64) {
        if self.phases.len() < MAX_PHASES {
            self.phases.push((name, ms));
        }
    }

    pub(crate) fn total_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    /// `{"total":ms,"<phase>":ms,...}` for the answer.
    pub(crate) fn value(&self) -> Value {
        let mut out = Map::new();
        out.insert("total".into(), json!(self.total_ms()));
        for (name, ms) in &self.phases {
            out.insert((*name).into(), json!(ms));
        }
        Value::Object(out)
    }

    /// Logs the call when it was slow: one JSON line with the command, its
    /// subject, the total and every phase. Never fails the call.
    pub(crate) fn finish(&self, command: &str, subject: &str) -> Value {
        let value = self.value();
        if self.total_ms() >= SLOW_MS {
            let line = json!({"at":crate::state::now(),"command":command,"subject":subject.chars().take(200).collect::<String>(),
                "pid":std::process::id(),"timings_ms":value});
            let _ = append(&log_path(), &line.to_string());
        }
        value
    }
}

pub(crate) fn log_path() -> PathBuf {
    super::storage::root().join("logs").join("slow-calls.log")
}

/// Appends one line, first moving a full log aside (one older file).
fn append(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() >= LOG_BYTES) {
        let older = path.with_extension("log.1");
        let _ = std::fs::remove_file(&older);
        std::fs::rename(path, &older)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(format!("{line}\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_add_up_and_the_answer_names_them() {
        let mut phases = Phases::start();
        std::thread::sleep(std::time::Duration::from_millis(15));
        phases.mark("lock");
        phases.mark("record");
        std::thread::sleep(std::time::Duration::from_millis(5));
        phases.mark("lock");
        let value = phases.value();
        assert!(value["lock"].as_u64().unwrap() >= 20, "{value}");
        assert!(value["total"].as_u64().unwrap() >= value["lock"].as_u64().unwrap());
        assert_eq!(value.as_object().unwrap().keys().filter(|key| *key != "total").count(), 2);
    }

    #[test]
    fn the_log_stays_bounded_with_one_older_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("slow-calls.log");
        let line = "x".repeat(10_000);
        for _ in 0..80 {
            append(&path, &line).unwrap();
        }
        let current = std::fs::metadata(&path).unwrap().len();
        let older = std::fs::metadata(path.with_extension("log.1")).unwrap().len();
        assert!(current <= LOG_BYTES + 10_001 && older <= LOG_BYTES + 10_001, "{current} {older}");
        assert!(!dir.path().join("logs").join("slow-calls.log.1.1").exists());
    }
}
