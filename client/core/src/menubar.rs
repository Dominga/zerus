//! The menu-bar item's model, as the Qt client shows it: two counters with a
//! stale dot or "?" (`tray/src/IconFactory.cpp:57-65`, `:178-186`), the
//! tooltip (`tray/src/FleetState.cpp:437-470`) and the menu
//! (`tray/src/TrayMenu.cpp`). [`Desk`] mirrors the core's changes; [`Gate`]
//! decides what reaches the status item (`tray/src/QuietTrayIcon.h`). Pure:
//! the platform glue draws and shows it.
use crate::{Change, ComputerId, ComputerStatus, ViewRow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One computer as the menu bar knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Computer {
    pub status: ComputerStatus,
    /// A catalog listing arrived at least once (Qt's `everPolled`).
    pub listed: bool,
    /// The host name its machine card reports.
    pub host: Option<String>,
    /// False for a control-only computer (`local_sessions` in its card).
    pub local_sessions: bool,
    pub rows: BTreeMap<String, ViewRow>,
}

impl Default for Computer {
    fn default() -> Self {
        Self {
            status: ComputerStatus::Probing,
            listed: false,
            host: None,
            local_sessions: true,
            rows: BTreeMap::new(),
        }
    }
}

impl Computer {
    /// Its rows are current. Qt counts a peer's attention only within
    /// `kPeerStaleMs` of its last answer (`FleetState.cpp:421-435`); a stream
    /// is current while it is live or polled.
    pub fn fresh(&self) -> bool {
        self.listed && matches!(self.status, ComputerStatus::Live | ComputerStatus::Polled)
    }

    /// Sessions that are not archived (`visibleSessionCount`, `FleetState.cpp:17-21`).
    pub fn visible(&self) -> usize {
        self.rows
            .values()
            .filter(|row| row.state != "archived")
            .count()
    }
}

/// `SessionInfo::needsAttention` (`tray/src/HgsClient.h:107-108`) without
/// the device-local Review later mark, which this client does not keep yet.
pub fn needs_attention(row: &ViewRow) -> bool {
    (row.notify.needs_action && !row.notify.acknowledged) || (row.state != "archived" && row.unread)
}

/// Qt's badge text: "0".."99", then "99+" (`IconFactory::countText`).
pub fn count_text(count: usize) -> String {
    if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    }
}

/// What the tile shows (`IconFactory::forCounts`, `IconFactory::forError`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub total: String,
    pub attention: String,
    /// The attention row is amber.
    pub highlighted: bool,
    /// The dot in the top right corner: a peer is stale.
    pub dot: bool,
}

impl Face {
    pub fn counts(total: usize, attention: usize, stale: bool) -> Self {
        Self {
            total: count_text(total),
            attention: count_text(attention),
            highlighted: attention > 0,
            dot: stale,
        }
    }

    /// No trustworthy snapshot: unknown counters.
    pub fn error() -> Self {
        Self {
            total: "?".to_owned(),
            attention: "?".to_owned(),
            highlighted: false,
            dot: true,
        }
    }
}

/// What a running session's entry does: Qt's "Open in" submenu with the
/// macOS set of `AppConfig::openModes` (Terminal.app or the clipboard).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenMode {
    #[default]
    Terminal,
    Clipboard,
}

impl OpenMode {
    pub const ALL: [OpenMode; 2] = [OpenMode::Terminal, OpenMode::Clipboard];

    /// Qt's labels (`TrayMenu.cpp:78-81`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal.app",
            Self::Clipboard => "Clipboard",
        }
    }

    /// An `open-mode` value of `~/.config/hgs/tray.conf`; the Linux and
    /// Windows values are not macOS modes.
    pub fn from_config(value: &str) -> Option<Self> {
        match value.trim() {
            "terminal" => Some(Self::Terminal),
            "clipboard" => Some(Self::Clipboard),
            _ => None,
        }
    }
}

/// The value of `key` in a `tray.conf` text: `key=value` lines, `#` comments,
/// the last line winning (`AppConfig.cpp:349-380`).
pub fn tray_conf_value(text: &str, key: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let line = line.trim();
        if line.starts_with('#') {
            return None;
        }
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then(|| value.trim().to_owned())
    })
}

/// One menu entry, in Qt's order (`TrayMenu::rebuild`, `TrayMenu.cpp:156-246`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// "Open Zerus".
    Open,
    Separator,
    /// A line that cannot be chosen: a computer's header or its state.
    Label(String),
    /// A session; `text` is what the entry shows, with `&` doubled.
    Session {
        computer: ComputerId,
        key: String,
        name: String,
        text: String,
        running: bool,
    },
    /// The "Open in" submenu with the chosen mode checked.
    OpenIn(OpenMode),
    Quit,
}

/// Doubles `&`: menus read a single one as a mnemonic marker (`TrayMenu.cpp:18-21`).
pub fn escape(text: &str) -> String {
    text.replace('&', "&&")
}

/// The text Qt shows for a session (`TrayMenu.cpp:23-31`).
pub fn session_text(row: &ViewRow) -> String {
    let label = escape(&row.name);
    match row.state.as_str() {
        "paused" => format!("{label}  (paused)"),
        "stopped" => format!("{label}  (stopped)"),
        _ if row.notify.attached => format!("\u{25cf} {label}"),
        _ => label,
    }
}

/// The fleet as the menu bar sees it, kept from the core's changes.
#[derive(Clone, Debug, Default)]
pub struct Desk {
    computers: BTreeMap<ComputerId, Computer>,
}

impl Desk {
    /// Applies one change; returns the computer whose state it may change.
    pub fn observe(&mut self, change: &Change) -> Option<ComputerId> {
        match change {
            Change::Upsert { computer, key, row } => {
                self.entry(computer).rows.insert(key.clone(), row.clone());
            }
            Change::Remove { computer, key, .. } => {
                self.entry(computer).rows.remove(key);
            }
            Change::Replace { computer, rows } => {
                let entry = self.entry(computer);
                entry.rows = rows.iter().cloned().collect();
                entry.listed = true;
            }
            Change::Computer {
                computer,
                status: ComputerStatus::Ended,
            } => {
                self.computers.remove(computer)?;
            }
            Change::Computer { computer, status } => {
                self.entry(computer).status = *status;
            }
            Change::Machine { computer, machine } => {
                let entry = self.entry(computer);
                entry.host.clone_from(&machine.host);
                entry.local_sessions = machine.local_sessions.unwrap_or(true);
            }
            _ => return None,
        }
        Some(change.computer().clone())
    }

    pub fn computers(&self) -> &BTreeMap<ComputerId, Computer> {
        &self.computers
    }

    fn entry(&mut self, computer: &ComputerId) -> &mut Computer {
        self.computers.entry(computer.clone()).or_default()
    }

    fn local(&self) -> Option<&Computer> {
        self.computers.get(&ComputerId::local())
    }

    fn peers(&self) -> impl Iterator<Item = (&str, &Computer)> {
        self.computers
            .iter()
            .filter_map(|(id, computer)| Some((id.alias()?, computer)))
    }

    /// Why the counters are unknown: this computer's stream failed. Qt shows
    /// "?" after a failed `hgs` call (`TrayAgent.cpp:507-517`).
    pub fn error(&self) -> Option<&'static str> {
        matches!(self.local()?.status, ComputerStatus::Reconnecting { .. })
            .then_some("reconnecting to hgs on this computer")
    }

    pub fn total(&self) -> usize {
        self.computers
            .values()
            .filter(|c| c.listed)
            .map(Computer::visible)
            .sum()
    }

    pub fn attention(&self) -> usize {
        self.computers
            .values()
            .filter(|c| c.fresh())
            .flat_map(|c| c.rows.values())
            .filter(|row| !row.muted && needs_attention(row))
            .count()
    }

    /// A peer has not answered yet or is not current (`FleetState::isStale`);
    /// this computer is never stale.
    pub fn stale(&self) -> bool {
        self.peers().any(|(_, computer)| !computer.fresh())
    }

    pub fn face(&self) -> Face {
        if self.error().is_some() {
            return Face::error();
        }
        Face::counts(self.total(), self.attention(), self.stale())
    }

    /// The Dock badge: the exact attention count, none at zero or while the
    /// counters are unknown (`TrayAgent.cpp:540-541`, `MacApplicationBadge.mm`).
    pub fn badge(&self) -> Option<String> {
        let count = if self.error().is_some() {
            0
        } else {
            self.attention()
        };
        (count > 0).then(|| count.to_string())
    }

    /// Qt's tooltip; a stream has no poll age to show.
    pub fn tooltip(&self) -> String {
        if let Some(error) = self.error() {
            return format!("Zerus: {error}");
        }
        let (attention, total) = (self.attention(), self.total());
        let mut lines = vec![format!(
            "{}{}",
            if attention == 1 {
                "1 needs attention".to_owned()
            } else {
                format!("{attention} need attention")
            },
            if total == 1 {
                ", 1 session total".to_owned()
            } else {
                format!(", {total} sessions total")
            },
        )];
        let local = self.local();
        if local.is_none_or(|computer| computer.local_sessions) {
            let here = local
                .filter(|computer| computer.listed)
                .map_or(0, Computer::visible);
            lines.push(format!("{here} here"));
        }
        for (alias, computer) in self.peers() {
            lines.push(if !computer.listed {
                format!("{alias}: not polled yet")
            } else if !computer.fresh() {
                format!("{alias}: unreachable")
            } else {
                format!("{} on {alias}", computer.visible())
            });
        }
        lines.join("\n")
    }

    /// The menu Qt builds, without Refresh and the update entries.
    pub fn menu(&self, mode: OpenMode) -> Vec<Entry> {
        let mut menu = vec![Entry::Open, Entry::Separator];
        let local = self.local();
        let local_section = local.is_none_or(|computer| computer.local_sessions);
        if local_section {
            let header = local
                .and_then(|computer| computer.host.clone())
                .filter(|host| !host.is_empty())
                .unwrap_or_else(|| "this box".to_owned());
            menu.push(Entry::Label(escape(&header)));
            match local.filter(|computer| computer.listed) {
                None => menu.push(Entry::Label("polling\u{2026}".to_owned())),
                Some(computer) => sessions(&mut menu, &ComputerId::local(), computer),
            }
        }
        let peers: Vec<(&str, &Computer)> = self.peers().collect();
        if !local_section && peers.is_empty() {
            menu.push(Entry::Label("No remote machines".to_owned()));
        }
        for (index, (alias, computer)) in peers.into_iter().enumerate() {
            if local_section || index > 0 {
                menu.push(Entry::Separator);
            }
            let alias_text = escape(alias);
            menu.push(Entry::Label(alias_text.clone()));
            if !computer.listed {
                menu.push(Entry::Label(format!("{alias_text}: polling\u{2026}")));
            } else if !computer.fresh() {
                menu.push(Entry::Label(format!("{alias_text}: unreachable")));
            } else {
                sessions(&mut menu, &ComputerId::from(alias), computer);
            }
        }
        menu.extend([Entry::Separator, Entry::OpenIn(mode), Entry::Quit]);
        menu
    }
}

/// A computer's sessions by name, or "no sessions" (`TrayMenu.cpp:178-195`).
fn sessions(menu: &mut Vec<Entry>, computer: &ComputerId, state: &Computer) {
    let mut rows: Vec<(&String, &ViewRow)> = state
        .rows
        .iter()
        .filter(|(_, row)| row.state != "archived")
        .collect();
    if rows.is_empty() {
        menu.push(Entry::Label("no sessions".to_owned()));
        return;
    }
    rows.sort_by(|a, b| a.1.name.cmp(&b.1.name).then_with(|| a.0.cmp(b.0)));
    for (key, row) in rows {
        menu.push(Entry::Session {
            computer: computer.clone(),
            key: key.clone(),
            name: row.name.clone(),
            text: session_text(row),
            running: row.state == "running",
        });
    }
}

/// One change to the status item.
#[derive(Clone, Debug, PartialEq)]
pub enum Apply {
    Face(Face),
    Tooltip(String),
    Menu(Vec<Entry>),
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Look {
    face: Option<Face>,
    tooltip: Option<String>,
    menu: Option<Vec<Entry>>,
}

/// What reaches the status item: only changes, and nothing while its menu is
/// open, since an update then can deadlock AppKit on macOS 26
/// (`QuietTrayIcon.h`). The latest offer applies when the menu closes.
#[derive(Debug, Default)]
pub struct Gate {
    open: bool,
    shown: Look,
    wanted: Look,
}

impl Gate {
    pub fn offer(&mut self, face: Face, tooltip: String, menu: Vec<Entry>) -> Vec<Apply> {
        self.wanted = Look {
            face: Some(face),
            tooltip: Some(tooltip),
            menu: Some(menu),
        };
        if self.open {
            Vec::new()
        } else {
            self.flush()
        }
    }

    pub fn menu_opened(&mut self) {
        self.open = true;
    }

    pub fn menu_closed(&mut self) -> Vec<Apply> {
        self.open = false;
        self.flush()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn flush(&mut self) -> Vec<Apply> {
        let mut apply = Vec::new();
        if self.wanted.face != self.shown.face {
            apply.extend(self.wanted.face.clone().map(Apply::Face));
        }
        if self.wanted.tooltip != self.shown.tooltip {
            apply.extend(self.wanted.tooltip.clone().map(Apply::Tooltip));
        }
        if self.wanted.menu != self.shown.menu {
            apply.extend(self.wanted.menu.clone().map(Apply::Menu));
        }
        self.shown = self.wanted.clone();
        apply
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Change, ComputerId, ComputerStatus, MachineMeta, ViewRow};

    fn row(name: &str, state: &str) -> ViewRow {
        ViewRow {
            name: name.to_owned(),
            state: state.to_owned(),
            ..ViewRow::default()
        }
    }

    fn unread(name: &str) -> ViewRow {
        ViewRow {
            unread: true,
            ..row(name, "running")
        }
    }

    fn asking(name: &str) -> ViewRow {
        let mut row = row(name, "running");
        row.notify.needs_action = true;
        row
    }

    fn listed(desk: &mut Desk, computer: &str, rows: Vec<(&str, ViewRow)>) {
        let id = ComputerId::from(computer);
        let rows = rows
            .into_iter()
            .map(|(key, row)| (key.to_owned(), row))
            .collect();
        desk.observe(&Change::Replace {
            computer: id.clone(),
            rows,
        });
        desk.observe(&Change::Computer {
            computer: id,
            status: ComputerStatus::Live,
        });
    }

    fn status(desk: &mut Desk, computer: &str, status: ComputerStatus) {
        desk.observe(&Change::Computer {
            computer: ComputerId::from(computer),
            status,
        });
    }

    fn host(desk: &mut Desk, computer: &str, name: &str, local_sessions: bool) {
        let machine = MachineMeta {
            host: Some(name.to_owned()),
            local_sessions: Some(local_sessions),
            ..MachineMeta::default()
        };
        desk.observe(&Change::Machine {
            computer: ComputerId::from(computer),
            machine,
        });
    }

    #[test]
    fn counts_follow_qt_total_and_attention() {
        let mut desk = Desk::default();
        let mut acknowledged = asking("claude/p/acknowledged");
        acknowledged.notify.acknowledged = true;
        let mut muted = asking("claude/p/muted");
        muted.muted = true;
        listed(
            &mut desk,
            "",
            vec![
                ("a", asking("claude/p/approval")),
                ("b", unread("codex/p/reply")),
                ("c", acknowledged),
                ("d", muted),
                ("e", row("claude/p/old", "archived")),
            ],
        );
        assert_eq!(desk.total(), 4, "archived sessions are not counted");
        assert_eq!(
            desk.attention(),
            2,
            "acknowledged and muted sessions do not count"
        );
        assert_eq!(
            desk.face(),
            Face {
                total: "4".into(),
                attention: "2".into(),
                highlighted: true,
                dot: false
            }
        );
    }

    #[test]
    fn counts_cap_at_ninety_nine() {
        assert_eq!(count_text(0), "0");
        assert_eq!(count_text(99), "99");
        assert_eq!(count_text(100), "99+");
        assert!(!Face::counts(3, 0, false).highlighted);
    }

    #[test]
    fn the_dock_badge_is_the_exact_attention_count() {
        let mut desk = Desk::default();
        assert_eq!(desk.badge(), None);
        listed(
            &mut desk,
            "",
            vec![("a", asking("claude/p/a")), ("b", unread("claude/p/b"))],
        );
        assert_eq!(desk.badge().as_deref(), Some("2"));
        status(&mut desk, "", ComputerStatus::Reconnecting { attempt: 1 });
        assert_eq!(
            desk.badge(),
            None,
            "no badge while the counters are unknown"
        );
    }

    #[test]
    fn a_peer_that_is_not_current_marks_the_tile_stale_and_counts_no_attention() {
        let mut desk = Desk::default();
        listed(&mut desk, "", vec![("a", row("claude/p/here", "running"))]);
        listed(&mut desk, "mac", vec![("b", asking("claude/p/there"))]);
        assert_eq!(desk.attention(), 1);
        assert!(!desk.stale());
        status(
            &mut desk,
            "mac",
            ComputerStatus::Reconnecting { attempt: 1 },
        );
        assert_eq!(desk.total(), 2, "the last known rows still count");
        assert_eq!(desk.attention(), 0);
        assert!(desk.stale());
        status(&mut desk, "studio", ComputerStatus::Probing);
        assert!(desk.face().dot, "a peer that has not answered yet is stale");
    }

    #[test]
    fn a_reconnecting_local_stream_shows_the_question_mark() {
        let mut desk = Desk::default();
        listed(&mut desk, "", vec![("a", row("claude/p/x", "running"))]);
        status(&mut desk, "", ComputerStatus::Reconnecting { attempt: 2 });
        assert_eq!(desk.face(), Face::error());
        let face = desk.face();
        assert_eq!((face.total.as_str(), face.attention.as_str()), ("?", "?"));
        assert!(face.dot && !face.highlighted);
        assert_eq!(
            desk.tooltip(),
            "Zerus: reconnecting to hgs on this computer"
        );
    }

    #[test]
    fn the_tooltip_has_qt_lines_without_poll_ages() {
        let mut desk = Desk::default();
        host(&mut desk, "", "arch", true);
        listed(
            &mut desk,
            "",
            vec![
                ("a", unread("claude/p/a")),
                ("b", row("claude/p/b", "paused")),
            ],
        );
        listed(&mut desk, "mac", vec![("c", row("codex/p/c", "running"))]);
        status(&mut desk, "studio", ComputerStatus::Probing);
        listed(&mut desk, "win", vec![]);
        status(
            &mut desk,
            "win",
            ComputerStatus::Reconnecting { attempt: 1 },
        );
        assert_eq!(
            desk.tooltip(),
            "1 needs attention, 3 sessions total\n2 here\n1 on mac\nstudio: not polled yet\nwin: unreachable"
        );
        let mut empty = Desk::default();
        host(&mut empty, "", "arch", true);
        assert_eq!(
            empty.tooltip(),
            "0 need attention, 0 sessions total\n0 here"
        );
    }

    #[test]
    fn the_menu_lists_computers_and_sessions_in_qt_order() {
        let mut desk = Desk::default();
        host(&mut desk, "", "arch", true);
        let mut attached = row("claude/R&D/main", "running");
        attached.notify.attached = true;
        listed(
            &mut desk,
            "",
            vec![
                ("k2", row("codex/p/zeta", "stopped")),
                ("k1", attached),
                ("k3", row("claude/p/old", "archived")),
            ],
        );
        listed(&mut desk, "mac", vec![]);
        status(&mut desk, "studio", ComputerStatus::Probing);
        listed(&mut desk, "win", vec![("k4", row("claude/p/w", "running"))]);
        status(
            &mut desk,
            "win",
            ComputerStatus::Reconnecting { attempt: 1 },
        );
        let local = ComputerId::local();
        assert_eq!(
            desk.menu(OpenMode::Clipboard),
            vec![
                Entry::Open,
                Entry::Separator,
                Entry::Label("arch".into()),
                Entry::Session {
                    computer: local.clone(),
                    key: "k1".into(),
                    name: "claude/R&D/main".into(),
                    text: "\u{25cf} claude/R&&D/main".into(),
                    running: true
                },
                Entry::Session {
                    computer: local,
                    key: "k2".into(),
                    name: "codex/p/zeta".into(),
                    text: "codex/p/zeta  (stopped)".into(),
                    running: false
                },
                Entry::Separator,
                Entry::Label("mac".into()),
                Entry::Label("no sessions".into()),
                Entry::Separator,
                Entry::Label("studio".into()),
                Entry::Label("studio: polling\u{2026}".into()),
                Entry::Separator,
                Entry::Label("win".into()),
                Entry::Label("win: unreachable".into()),
                Entry::Separator,
                Entry::OpenIn(OpenMode::Clipboard),
                Entry::Quit,
            ]
        );
    }

    #[test]
    fn a_local_computer_that_has_not_answered_is_this_box_polling() {
        let desk = Desk::default();
        assert_eq!(
            &desk.menu(OpenMode::Terminal)[..4],
            &[
                Entry::Open,
                Entry::Separator,
                Entry::Label("this box".into()),
                Entry::Label("polling\u{2026}".into()),
            ]
        );
    }

    #[test]
    fn a_control_only_computer_without_peers_says_so() {
        let mut desk = Desk::default();
        host(&mut desk, "", "win", false);
        listed(&mut desk, "", vec![]);
        assert_eq!(
            desk.menu(OpenMode::Terminal),
            vec![
                Entry::Open,
                Entry::Separator,
                Entry::Label("No remote machines".into()),
                Entry::Separator,
                Entry::OpenIn(OpenMode::Terminal),
                Entry::Quit,
            ]
        );
        assert_eq!(desk.tooltip(), "0 need attention, 0 sessions total");
    }

    #[test]
    fn an_ended_computer_leaves_the_desk() {
        let mut desk = Desk::default();
        listed(&mut desk, "mac", vec![("a", row("claude/p/a", "running"))]);
        assert_eq!(
            desk.observe(&Change::Computer {
                computer: ComputerId::from("mac"),
                status: ComputerStatus::Ended
            }),
            Some(ComputerId::from("mac"))
        );
        assert!(desk.computers().is_empty());
    }

    #[test]
    fn session_text_follows_qt_suffixes() {
        assert_eq!(session_text(&row("a/b", "paused")), "a/b  (paused)");
        assert_eq!(session_text(&row("a/b", "stopped")), "a/b  (stopped)");
        assert_eq!(session_text(&row("a/b&c", "running")), "a/b&&c");
        let mut attached = row("a/b", "paused");
        attached.notify.attached = true;
        assert_eq!(
            session_text(&attached),
            "a/b  (paused)",
            "a saved session shows no attachment"
        );
    }

    #[test]
    fn gate_applies_only_changes() {
        let mut gate = Gate::default();
        let face = Face::counts(1, 0, false);
        let applied = gate.offer(face.clone(), "t".into(), vec![Entry::Quit]);
        assert_eq!(
            applied,
            vec![
                Apply::Face(face.clone()),
                Apply::Tooltip("t".into()),
                Apply::Menu(vec![Entry::Quit]),
            ]
        );
        assert!(gate
            .offer(face.clone(), "t".into(), vec![Entry::Quit])
            .is_empty());
        assert_eq!(
            gate.offer(face, "u".into(), vec![Entry::Quit]),
            vec![Apply::Tooltip("u".into())]
        );
    }

    #[test]
    fn gate_defers_everything_while_the_menu_is_open() {
        let mut gate = Gate::default();
        gate.offer(Face::counts(1, 0, false), "a".into(), vec![Entry::Open]);
        gate.menu_opened();
        assert!(gate.is_open());
        assert!(gate
            .offer(Face::counts(2, 1, false), "b".into(), vec![Entry::Quit])
            .is_empty());
        assert!(gate
            .offer(Face::counts(3, 1, true), "c".into(), vec![Entry::Quit])
            .is_empty());
        // Only the latest offer reaches the status item once the menu closes.
        assert_eq!(
            gate.menu_closed(),
            vec![
                Apply::Face(Face::counts(3, 1, true)),
                Apply::Tooltip("c".into()),
                Apply::Menu(vec![Entry::Quit]),
            ]
        );
        assert!(gate.menu_closed().is_empty());
    }

    #[test]
    fn open_mode_reads_tray_conf() {
        let conf = "# comment\nhgs=/opt/example/hgs\nopen-mode = clipboard\n\nopen-mode=terminal\n";
        assert_eq!(
            tray_conf_value(conf, "open-mode").as_deref(),
            Some("terminal")
        );
        assert_eq!(tray_conf_value(conf, "icon-color"), None);
        assert_eq!(
            OpenMode::from_config("clipboard"),
            Some(OpenMode::Clipboard)
        );
        assert_eq!(
            OpenMode::from_config("tab"),
            None,
            "Linux modes are not macOS modes"
        );
        assert_eq!(OpenMode::Terminal.label(), "Terminal.app");
    }
}
