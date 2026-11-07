//! Task 176 (owner, 2026-11-07): the first start of the Tauri build showed an
//! endless skeleton. `hgs desktop status` refused because the session state
//! folder had inherited a sandbox group's read access (Codex grants it on
//! `~/.local`), and only the service start repairs that. The startup check now
//! recognizes that refusal, runs the service start's own repair once
//! (`hgs desktop prepare-state`) and checks again; a refusal that stays is
//! shown with its folder, access entry and repair, never as loading rows.
use std::sync::{Mutex, PoisonError};

/// The start of hgs's refusal of a desktop state folder that is not private.
pub(super) const MARKER: &str = "desktop folders must be private directories";
/// The code a connection ends with while the refusal stays.
pub(super) const CODE: &str = "state_folder_not_private";
const LIMIT: usize = 1000;

/// The refusal in hgs's error output: from the marker to the end of its
/// line, bounded. None when the output is about something else.
pub(super) fn parse(stderr: &str) -> Option<String> {
    let at = stderr.find(MARKER)?;
    let line = stderr[at..].lines().next().unwrap_or_default().trim_end();
    Some(line.chars().take(LIMIT).collect())
}

/// The refusal the last failed hgs call printed, and the one shown.
#[derive(Default)]
pub(super) struct Seen {
    last: Mutex<Option<String>>,
    shown: Mutex<Option<String>>,
}

impl Seen {
    /// Called with the error output of every failed hgs call.
    pub fn observe(&self, stderr: &str) {
        if let Some(refusal) = parse(stderr) {
            *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Some(refusal);
        }
    }
    pub fn take(&self) -> Option<String> {
        self.last.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
    pub fn show(&self, refusal: Option<String>) {
        *self.shown.lock().unwrap_or_else(PoisonError::into_inner) = refusal;
    }
    pub fn shown(&self) -> Option<String> {
        self.shown.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Whether this hgs offers the state preparation the service start runs.
pub(super) fn can_prepare(help: &[u8]) -> bool {
    std::str::from_utf8(help).ok().is_some_and(|text| {
        text.lines()
            .any(|line| line.trim() == zerus_client_core::desktop::ownership::PREPARE_STATE_HELP_MARKER)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFUSAL: &str = r"desktop folders must be private directories: C:\Users\me\.local\state\hgs\sessions: it grants MSI\CodexSandboxUsers (S-1-5-21-1-2-3-1009) read and execute (RX), mask 0x001200A9, inherited from its parent folder. Repair: starting the background service (or hgs desktop prepare-state --json) makes a folder you own private in place; a folder another account owns must be fixed by its owner";

    #[test]
    fn the_refusal_is_read_from_hgs_error_output_with_its_folder_and_entry() {
        let stderr = format!("hgs: {REFUSAL}\nsecond line\n");
        let found = parse(&stderr).unwrap();
        assert_eq!(found, REFUSAL);
        assert!(found.contains(r"\sessions") && found.contains("CodexSandboxUsers") && found.contains("prepare-state"));
        assert_eq!(parse("hgs: the desktop service is not running\n"), None);
        // Bounded, whatever hgs prints.
        let long = format!("{MARKER}: {}", "x".repeat(5000));
        assert_eq!(parse(&long).unwrap().chars().count(), LIMIT);
    }

    #[test]
    fn the_last_refusal_is_taken_once_and_the_shown_one_stays() {
        let seen = Seen::default();
        seen.observe("hgs: something else");
        assert_eq!(seen.take(), None);
        seen.observe(REFUSAL);
        assert_eq!(seen.take().as_deref(), Some(REFUSAL));
        assert_eq!(seen.take(), None);
        seen.show(Some(REFUSAL.into()));
        assert_eq!(seen.shown().as_deref(), Some(REFUSAL));
        seen.show(None);
        assert_eq!(seen.shown(), None);
    }

    #[test]
    fn preparation_needs_the_help_marker() {
        let help = format!("usage\n  {}\n", zerus_client_core::desktop::ownership::PREPARE_STATE_HELP_MARKER);
        assert!(can_prepare(help.as_bytes()));
        assert!(!can_prepare(b"usage\n"));
    }
}
