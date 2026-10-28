//! The Tasks board's rules, ported from the Qt client: a task as the listing
//! reports it, the stages and their filter, which tasks a session shows, the
//! actions per stage, what can be dragged where, and why Start task cannot
//! send. The board's data comes from the `tasks` topic; writes go through
//! [`crate::tasks_write`].
//!
//! Reference: `tray/src/TaskBoard.h`, `tray/src/TaskBoard.cpp`,
//! `tray/src/TaskDrag.h`, `tray/src/TasksView.cpp` (`canReorderTask`,
//! `takesOnTile`, `reorderTask`, `sendBlocker`).
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Done shows the last seven days; older tasks are asked for.
pub const RECENT_DONE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// One task as `hgs task list` reports it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskItem {
    pub r#ref: String,
    pub id: String,
    pub uuid: String,
    pub title: String,
    pub text: String,
    pub stage: String,
    pub summary: String,
    pub cancel_reason: String,
    /// The pinned session: its computer ("" for this one) and name; an empty
    /// name is Any session. `target_known` is false for a computer this one
    /// has not verified.
    pub target_host: String,
    pub target_session: String,
    pub target_id: String,
    pub target_kind: String,
    pub target_machine: String,
    pub target_known: bool,
    pub worker: Value,
    /// The session it was last sent to.
    pub session_host: String,
    pub session_name: String,
    pub session_id: String,
    pub session_known: bool,
    /// Who filed it: empty for the person, otherwise the agent's session.
    pub author: String,
    pub author_host: String,
    pub author_id: String,
    pub created_at: i64,
    pub stage_at: i64,
    pub cancelled_at: i64,
    pub writable: bool,
    /// Its place in the project's To do, from 1.
    pub queue: usize,
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or("").to_owned()
}

/// A session reference: host, name, and whether its computer is verified.
fn session(value: &Value) -> (String, String, bool) {
    let known = value.get("host").is_none_or(|host| !host.is_null())
        && value["resolved"].as_bool().unwrap_or(true);
    (text(value, "host"), text(value, "session"), known)
}

impl TaskItem {
    pub fn from_value(task: &Value) -> TaskItem {
        let mut item = TaskItem {
            r#ref: text(task, "ref"),
            id: text(task, "id"),
            uuid: text(task, "uuid"),
            title: text(task, "title"),
            text: text(task, "text"),
            stage: text(task, "stage"),
            summary: text(task, "summary"),
            cancel_reason: text(task, "cancel_reason"),
            created_at: task["created_at"].as_f64().unwrap_or(0.0) as i64,
            stage_at: task["stage_at"].as_f64().unwrap_or(0.0) as i64,
            cancelled_at: task["cancelled_at"].as_f64().unwrap_or(0.0) as i64,
            writable: task["writable"].as_bool().unwrap_or(true),
            worker: task["worker"].clone(),
            target_known: true,
            session_known: true,
            ..TaskItem::default()
        };
        let mut target = &task["target"];
        item.target_kind = text(target, "kind");
        if target.get("parent").is_some() {
            target = &target["parent"];
        }
        if item.target_kind != "any" {
            (item.target_host, item.target_session, item.target_known) = session(target);
        }
        if task["session"].is_object() {
            (item.session_host, item.session_name, item.session_known) = session(&task["session"]);
        }
        item.target_id = text(target, "session_id");
        item.target_machine = text(target, "machine");
        item.session_id = text(&task["session"], "session_id");
        let author = &task["author"];
        if author["kind"] == "agent" {
            item.author = text(author, "session");
            item.author_host = text(author, "host");
            item.author_id = text(author, "session_id");
        }
        item
    }

    /// The title, or the first line of the text.
    pub fn display_title(&self) -> String {
        if self.title.is_empty() {
            self.text.split('\n').next().unwrap_or("").to_owned()
        } else {
            self.title.clone()
        }
    }
}

/// A project's tasks in hgs order (each stage by rank).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub project_id: String,
    pub project_name: String,
    pub tasks: Vec<TaskItem>,
    /// Task files that could not be read.
    pub damaged: usize,
}

/// The open session the board is shown for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub host: String,
    pub session: String,
    pub id: String,
    /// "Only this session".
    pub only: bool,
    /// Milliseconds since the epoch.
    pub now: i64,
}

fn listed(stage: &str) -> bool {
    matches!(stage, "todo" | "backlog")
}

/// The tiles' order.
pub const STAGES: [&str; 5] = ["review", "active", "todo", "backlog", "done"];
/// The filters: the Active union first, then each stage.
pub const FILTERS: [&str; 6] = ["current", "review", "active", "todo", "backlog", "done"];

pub fn matches_filter(stage: &str, filter: &str) -> bool {
    if filter == "current" {
        matches!(stage, "active" | "review")
    } else {
        stage == filter
    }
}

impl Board {
    pub fn from_listing(listing: &Value) -> Board {
        let mut queue = 0;
        let tasks = listing["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|value| {
                let mut task = TaskItem::from_value(value);
                if task.stage == "todo" {
                    queue += 1;
                    task.queue = queue;
                }
                task
            })
            .collect();
        Board {
            project_id: text(&listing["project"], "id"),
            project_name: text(&listing["project"], "name"),
            tasks,
            damaged: listing["damaged"].as_array().map_or(0, Vec::len),
        }
    }

    /// A stage's rows for the view. Done holds the last seven days unless
    /// `older` asks for the rest.
    pub fn rows(&self, stage: &str, view: &View, older: bool) -> Vec<&TaskItem> {
        let done = stage == "done";
        self.tasks
            .iter()
            .filter(|task| matches_filter(&task.stage, stage))
            .filter(|task| !view.only || kept_for(task, &view.host, &view.session, &view.id))
            .filter(|task| !done || ((view.now - task.stage_at > RECENT_DONE_MS) == older))
            .collect()
    }

    pub fn count(&self, stage: &str, view: &View) -> usize {
        self.rows(stage, view, false).len()
    }

    /// Whether a dragged task can be reordered within `stage` of this board.
    pub fn can_reorder(&self, drag: &Drag, stage: &str) -> bool {
        !self.project_id.is_empty()
            && drag.project_id == self.project_id
            && drag.stage == stage
            && self
                .tasks
                .iter()
                .find(|task| task.uuid == drag.uuid)
                .is_some_and(|task| draggable(task) && task.stage == stage)
    }

    /// Whether a drop on a tile moves it to `stage`: only between To do and
    /// Backlog.
    pub fn can_stage(&self, drag: &Drag, stage: &str) -> bool {
        self.can_reorder(drag, &drag.stage)
            && listed(stage)
            && listed(&drag.stage)
            && drag.stage != stage
    }

    /// Whether a drop next to `anchor` reorders: the same stage, a writable
    /// task, not itself.
    pub fn can_move_next_to(&self, drag: &Drag, anchor: &str) -> bool {
        self.can_reorder(drag, &drag.stage)
            && anchor != drag.uuid
            && self
                .tasks
                .iter()
                .any(|task| task.uuid == anchor && task.stage == drag.stage && task.writable)
    }
}

/// The open session's tasks: pinned to it in To do and Backlog, sent to it
/// later.
pub fn belongs_to(task: &TaskItem, host: &str, name: &str, id: &str) -> bool {
    let pinned = listed(&task.stage);
    let (known, task_host, task_name, task_id) = if pinned {
        (
            task.target_known,
            &task.target_host,
            &task.target_session,
            &task.target_id,
        )
    } else {
        (
            task.session_known,
            &task.session_host,
            &task.session_name,
            &task.session_id,
        )
    };
    known
        && task_host == host
        && if task_id.is_empty() {
            !task_name.is_empty() && task_name == name
        } else {
            !id.is_empty() && task_id == id
        }
}

/// "Only this session" keeps those and the Any session tasks of To do and
/// Backlog.
pub fn kept_for(task: &TaskItem, host: &str, name: &str, id: &str) -> bool {
    belongs_to(task, host, name, id) || (listed(&task.stage) && task.target_session.is_empty())
}

pub fn stage_label(stage: &str) -> &'static str {
    match stage {
        "current" => "Active",
        "review" => "In review",
        "active" => "In progress",
        "todo" => "To do",
        "backlog" => "Backlog",
        _ => "Done",
    }
}

/// A stage's icon: the icon of the action that moves a task there.
pub fn stage_icon(stage: &str) -> &'static str {
    match stage {
        "current" | "todo" => "task-list",
        "review" => "task-inbox",
        "active" => "play",
        "backlog" => "task-clock",
        _ => "task-check",
    }
}

/// A session as the session list names it, then its computer.
pub fn session_label(host: &str, name: &str) -> String {
    let parts: Vec<&str> = name.splitn(3, '/').collect();
    let label = match parts.as_slice() {
        [_, _, tag] if !tag.is_empty() => (*tag).to_owned(),
        [_, project, ..] if !project.is_empty() => (*project).to_owned(),
        _ => name.to_owned(),
    };
    if host.is_empty() {
        label
    } else {
        format!("{label} @{host}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    Edit,
    Rename,
    ToBacklog,
    Top,
    SendNow,
    ToTodo,
    Cancel,
    BackToTodo,
    Accept,
    Pause,
}

/// The same actions in the same order on every surface; the main one last.
pub fn actions(stage: &str) -> Vec<TaskAction> {
    use TaskAction::*;
    match stage {
        "todo" => vec![Edit, Rename, ToBacklog, Top, SendNow],
        "backlog" => vec![Edit, Rename, ToTodo, SendNow],
        "active" => vec![Rename, Cancel],
        "review" => vec![Rename, BackToTodo, Accept],
        "done" => vec![Rename, BackToTodo],
        _ => vec![],
    }
}

pub fn primary_action(stage: &str) -> TaskAction {
    match stage {
        "todo" | "backlog" => TaskAction::SendNow,
        "review" => TaskAction::Accept,
        "active" => TaskAction::Pause,
        _ => TaskAction::BackToTodo,
    }
}

pub fn action_label(action: TaskAction) -> &'static str {
    match action {
        TaskAction::Edit => "Task details",
        TaskAction::Rename => "Rename…",
        TaskAction::ToBacklog => "Move to Backlog",
        TaskAction::Top => "Move to the top of To do",
        TaskAction::SendNow => "Start task",
        TaskAction::ToTodo => "Move to To do",
        TaskAction::Cancel => "Cancel task…",
        TaskAction::BackToTodo => "Back to To do",
        TaskAction::Accept => "Accept",
        TaskAction::Pause => "Pause task",
    }
}

pub fn action_icon(action: TaskAction) -> &'static str {
    match action {
        TaskAction::Edit => "task-list",
        TaskAction::Rename => "task-edit",
        TaskAction::ToBacklog => "task-clock",
        TaskAction::Top => "task-top",
        TaskAction::SendNow => "play",
        TaskAction::ToTodo | TaskAction::BackToTodo => "task-list-add",
        TaskAction::Cancel => "close",
        TaskAction::Accept => "task-check",
        TaskAction::Pause => "pause",
    }
}

/// A task being dragged on the board.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drag {
    pub project_id: String,
    pub uuid: String,
    pub id: String,
    pub stage: String,
}

/// Every writable stage supports board ordering; future-schema documents stay
/// read-only.
pub fn draggable(task: &TaskItem) -> bool {
    task.writable && STAGES.contains(&task.stage.as_str())
}

impl Drag {
    pub fn of(project_id: &str, task: &TaskItem) -> Drag {
        Drag {
            project_id: project_id.into(),
            uuid: task.uuid.clone(),
            id: task.id.clone(),
            stage: task.stage.clone(),
        }
    }
}

/// What Start task needs to know about the session it would go to.
pub enum Target<'a> {
    /// The session exists; its own reason it cannot take a task, if any.
    Found(Option<&'a str>),
    Missing,
}

/// Why Start task cannot send now (`sendBlocker`), `None` when it can.
/// `open` is the open session (host, name); `unsaved_description` when this
/// task's description is being edited; `executor_blocker` the planned
/// executor's launch reason; `find` looks a session up.
pub fn send_blocker<'a>(
    task: &TaskItem,
    open: (&str, &str),
    unsaved_description: bool,
    executor_blocker: Option<&str>,
    find: impl Fn(&str, &str) -> Target<'a>,
) -> Option<String> {
    if unsaved_description && !task.uuid.is_empty() {
        return Some("Save the description before starting this task".into());
    }
    if task.target_kind == "new_session" {
        if task.target_id.is_empty() || task.target_machine.is_empty() {
            return Some("Choose a parent with a confirmed computer and session identity".into());
        }
        return executor_blocker
            .filter(|reason| !reason.is_empty())
            .map(str::to_owned);
    }
    let any = task.target_session.is_empty();
    let (host, name) = if any {
        open
    } else {
        (task.target_host.as_str(), task.target_session.as_str())
    };
    if name.is_empty() {
        return Some("Open the session to send this task to".into());
    }
    if !any && !task.target_known {
        return Some(if task.target_id.is_empty() {
            format!(
                "{} is on a computer this one has not verified",
                session_label(host, name)
            )
        } else {
            format!(
                "{} no longer has a confirmed session identity — choose another session",
                session_label(host, name)
            )
        });
    }
    match find(host, name) {
        Target::Missing => Some(format!(
            "{} no longer exists — choose another session",
            session_label(host, name)
        )),
        Target::Found(reason) => reason
            .filter(|reason| !reason.is_empty())
            .map(str::to_owned),
    }
}

/// The note about unreadable task files.
pub fn damaged_text(damaged: usize, project: &str) -> String {
    if damaged == 1 {
        format!("1 task file of {project} could not be read.")
    } else {
        format!("{damaged} task files of {project} could not be read.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DAY: i64 = 24 * 60 * 60 * 1000;

    fn listing() -> Value {
        json!({"project": {"id": "alpha", "name": "Alpha"}, "damaged": ["x.json"], "tasks": [
            {"uuid": "r1", "stage": "review", "title": "Review me", "stage_at": 0},
            {"uuid": "t1", "stage": "todo", "text": "First line\nmore", "target": {"kind": "session", "host": "studio", "session": "claude/alpha/a", "session_id": "sid-a"}},
            {"uuid": "t2", "stage": "todo", "target": {"kind": "any"}},
            {"uuid": "b1", "stage": "backlog", "target": {"kind": "session", "host": null, "session": "claude/alpha/b"}},
            {"uuid": "d1", "stage": "done", "stage_at": 10 * DAY, "session": {"host": "studio", "session": "claude/alpha/a", "session_id": "sid-a"}},
            {"uuid": "d2", "stage": "done", "stage_at": DAY},
            {"uuid": "f1", "stage": "future", "writable": false},
            {"uuid": "n1", "stage": "todo", "target": {"kind": "new_session", "parent": {"host": "", "session": "claude/alpha/p", "session_id": "sid-p", "machine": "m1"}},
             "author": {"kind": "agent", "session": "claude/alpha/a", "host": "studio"}}
        ]})
    }

    #[test]
    fn a_listing_becomes_a_board_with_qt_rules() {
        let board = Board::from_listing(&listing());
        assert_eq!((board.project_id.as_str(), board.damaged), ("alpha", 1));
        let queue: Vec<_> = board
            .tasks
            .iter()
            .filter(|t| t.stage == "todo")
            .map(|t| t.queue)
            .collect();
        assert_eq!(queue, [1, 2, 3]);
        let t1 = &board.tasks[1];
        assert_eq!(
            (
                t1.target_host.as_str(),
                t1.target_session.as_str(),
                t1.target_id.as_str()
            ),
            ("studio", "claude/alpha/a", "sid-a")
        );
        assert_eq!(t1.display_title(), "First line");
        assert!(!board.tasks[3].target_known, "a null host is unverified");
        let planned = &board.tasks[7];
        assert_eq!(
            (
                planned.target_kind.as_str(),
                planned.target_session.as_str(),
                planned.target_machine.as_str()
            ),
            ("new_session", "claude/alpha/p", "m1")
        );
        assert_eq!(planned.author, "claude/alpha/a");
        let view = View {
            host: "studio".into(),
            session: "claude/alpha/a".into(),
            id: "sid-a".into(),
            only: true,
            now: 11 * DAY,
        };
        let todo: Vec<_> = board
            .rows("todo", &view, false)
            .iter()
            .map(|t| t.uuid.as_str())
            .collect();
        assert_eq!(todo, ["t1", "t2"], "its own and Any session");
        let all = View {
            only: false,
            ..view.clone()
        };
        assert_eq!(board.rows("current", &all, false).len(), 1);
        let recent: Vec<_> = board
            .rows("done", &all, false)
            .iter()
            .map(|t| t.uuid.as_str())
            .collect();
        assert_eq!(recent, ["d1"]);
        assert_eq!(board.rows("done", &all, true)[0].uuid, "d2");
        assert_eq!(board.count("done", &view,), 1, "sent to this session");
        assert_eq!(
            damaged_text(1, "Alpha"),
            "1 task file of Alpha could not be read."
        );
    }

    #[test]
    fn drags_follow_qt() {
        let board = Board::from_listing(&listing());
        let drag = Drag::of("alpha", &board.tasks[1]);
        assert!(board.can_reorder(&drag, "todo"));
        assert!(board.can_stage(&drag, "backlog"));
        assert!(!board.can_stage(&drag, "todo"), "the same stage");
        assert!(!board.can_stage(&drag, "done"), "only To do and Backlog");
        assert!(board.can_move_next_to(&drag, "t2"));
        assert!(!board.can_move_next_to(&drag, "t1"), "not itself");
        assert!(!board.can_move_next_to(&drag, "b1"), "another stage");
        assert!(
            !board.can_reorder(&Drag::of("beta", &board.tasks[1]), "todo"),
            "another project"
        );
        assert!(
            !draggable(&board.tasks[6]),
            "future documents stay read-only"
        );
    }

    #[test]
    fn labels_actions_and_send_blockers() {
        assert_eq!(stage_label("current"), "Active");
        assert_eq!(stage_label("active"), "In progress");
        assert_eq!(stage_icon("backlog"), "task-clock");
        assert_eq!(actions("todo").last(), Some(&TaskAction::SendNow));
        assert_eq!(primary_action("active"), TaskAction::Pause);
        assert_eq!(action_label(TaskAction::Top), "Move to the top of To do");
        assert_eq!(
            session_label("studio", "claude/alpha/review"),
            "review @studio"
        );
        assert_eq!(session_label("", "claude/alpha"), "alpha");
        let board = Board::from_listing(&listing());
        let open = ("studio", "claude/alpha/a");
        let found = |_: &str, _: &str| Target::Found(None);
        assert_eq!(
            send_blocker(&board.tasks[1], open, false, None, found),
            None
        );
        assert_eq!(
            send_blocker(&board.tasks[1], open, true, None, found).as_deref(),
            Some("Save the description before starting this task")
        );
        assert_eq!(
            send_blocker(&board.tasks[2], ("", ""), false, None, found).as_deref(),
            Some("Open the session to send this task to")
        );
        assert_eq!(
            send_blocker(&board.tasks[3], open, false, None, found).as_deref(),
            Some("b is on a computer this one has not verified")
        );
        assert_eq!(
            send_blocker(&board.tasks[1], open, false, None, |_, _| Target::Missing).as_deref(),
            Some("a @studio no longer exists — choose another session")
        );
        assert_eq!(
            send_blocker(&board.tasks[1], open, false, None, |_, _| Target::Found(
                Some("The session is paused.")
            ))
            .as_deref(),
            Some("The session is paused.")
        );
        assert_eq!(
            send_blocker(&board.tasks[7], open, false, Some("Launching…"), found).as_deref(),
            Some("Launching…")
        );
    }
}
