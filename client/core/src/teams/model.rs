//! What the Teams views follow, which read is due, and the view each reply
//! gives. Pure and clock-driven, so tests replay replies without `hgs`.
//!
//! A follow names the team list, what waits for the person and at most one
//! watched team: a team tab follows one team, and following another ends the
//! first. A followed read runs every [`POLL`] while the tab is visible; a
//! hidden tab reads nothing. The view is read again after the desktop's own
//! action ([`TeamsModel::refresh`]) and when the project's tasks topic changed
//! ([`TeamsModel::tasks_changed`]); a change that arrives while its read is in
//! flight is not lost: it marks the slot and the slot is read again at once
//! when the reply lands.
//!
//! A reply becomes an update only when the view it projects differs from the
//! last one sent for that key, so an unchanged team costs a read and nothing
//! more. Keys: `list`, `waiting` and `team:<id>`. One page is kept per key; a
//! page of a key is read on request with [`TeamsModel::page`] and replaces the
//! shown page, and the projected view names the next page's cursor.
use super::read::{TeamError, TeamRead};
use super::view::{self, TeamList, TeamView, WaitingView};
mod phases;
use crate::ComputerId;
use serde::Serialize;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often a followed read runs while the Team tab is visible.
pub const POLL: Duration = Duration::from_secs(10);
/// Page reads that may wait at once (one key each).
const QUEUED: usize = 8;

/// What the Teams views follow on this computer. A team tab follows one team
/// at a time: `team` is a single id, never a set.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TeamsFollow {
    /// The teams of this computer.
    pub list: bool,
    /// What waits for the person.
    pub waiting: bool,
    /// The team whose tab is open.
    pub team: Option<String>,
}

/// One change of a Teams view, keyed as the module says.
#[derive(Clone, Debug, PartialEq)]
pub enum TeamsUpdate {
    List(Arc<TeamList>),
    Team(Arc<TeamView>),
    Waiting(Arc<WaitingView>),
    /// The read failed; the view keeps what it showed.
    Failed {
        code: String,
        message: String,
    },
    /// The view is no longer followed; its data is gone.
    Ended,
}

impl TeamsUpdate {
    /// The batch's `kind`: the view's name, `failed` or `ended`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::List(_) => "list",
            Self::Team(_) => "team",
            Self::Waiting(_) => "waiting",
            Self::Failed { .. } => "failed",
            Self::Ended => "ended",
        }
    }

    /// The batch's `payload`: the view model, `{code, message}` or null.
    pub fn payload(&self) -> Value {
        fn value<T: Serialize>(view: &T) -> Value {
            serde_json::to_value(view).unwrap_or(Value::Null)
        }
        match self {
            Self::List(view) => value(&**view),
            Self::Team(view) => value(&**view),
            Self::Waiting(view) => value(&**view),
            Self::Failed { code, message } => serde_json::json!({"code": code, "message": message}),
            Self::Ended => Value::Null,
        }
    }

    /// Whether it carries a view (not a failure or an end).
    pub fn is_view(&self) -> bool {
        !matches!(self, Self::Failed { .. } | Self::Ended)
    }
}

/// The key of a view about one team.
pub fn team_key(kind: &str, team: &str) -> String {
    format!("{kind}:{team}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Slot {
    Phases,
    Draft,
    Status,
    Queue,
    List,
    Waiting,
}

impl Slot {
    fn rank(self) -> u8 {
        match self {
            Self::Status => 0,
            Self::Queue => 1,
            Self::List => 2,
            Self::Waiting => 3,
            Self::Phases => 4,
            Self::Draft => 5,
        }
    }

    fn of(read: &TeamRead) -> Self {
        match read {
            TeamRead::List { .. } => Self::List,
            TeamRead::Waiting { .. } => Self::Waiting,
            TeamRead::Status { .. } => Self::Status,
            TeamRead::Queue { .. } => Self::Queue,
            TeamRead::Phases { .. } => Self::Phases,
            TeamRead::Draft { .. } => Self::Draft,
        }
    }
}

/// The replies a watched team's view is built from: its status (the team and
/// its members) and one page of its queue.
#[derive(Clone, Debug, Default)]
struct Parts {
    phases: Option<view::phases::PhaseList>,
    draft: Option<view::phases::PhaseDraft>,
    phases_failure: Option<view::phases::PhaseFailure>,
    draft_failure: Option<view::phases::PhaseFailure>,
    status: Option<Value>,
    queue: Option<Value>,
    /// The cursor the queue reply was read from; none for the first page.
    queue_after: Option<String>,
}

/// The Teams read model of this computer.
#[derive(Debug, Default)]
pub struct TeamsModel {
    phases_enabled: bool,
    phase_selected: Option<u32>,
    phase_epoch: u64,
    phase_issued: BTreeMap<Slot, u64>,
    follow: TeamsFollow,
    due: BTreeMap<Slot, Instant>,
    /// The digest of the last update sent per key.
    shown: BTreeMap<String, u64>,
    in_flight: BTreeSet<Slot>,
    /// Slots a refresh or a tasks change marked while their read was in
    /// flight: they are due at once when the reply lands.
    dirty: BTreeSet<Slot>,
    /// Page reads asked for by the view, run before the due ones.
    queued: VecDeque<TeamRead>,
    /// The watched team's replies, dropped when it is no longer watched.
    parts: Option<(String, Parts)>,
    hidden: bool,
    /// The scope records of the keys followed now: a reply or an end issued
    /// under an older generation never touches the scope followed now. A
    /// retired scope's record goes, so the map stays bounded by the followed
    /// views; the counter itself only grows.
    generations: BTreeMap<String, u64>,
    /// The last generation handed out; one counter for the whole model.
    generation_counter: u64,
    /// The generation the slot's in-flight read was issued under.
    issued: BTreeMap<Slot, u64>,
    /// The selected page cursor per followed key; absent means page one.
    selected: BTreeMap<String, String>,
}

/// Feeds the serialized payload straight into the hasher, without a string.
struct HashWriter<'a>(&'a mut DefaultHasher);

impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn digest(kind: &str, payload: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    kind.hash(&mut hasher);
    let _ = serde_json::to_writer(HashWriter(&mut hasher), payload);
    hasher.finish()
}

impl TeamsModel {
    pub fn following(&self) -> &TeamsFollow {
        &self.follow
    }

    /// Follows `next` from now on. A scope that stops being followed is
    /// retired before anything of the new one can be published: its end goes
    /// out now, and a reply or a late end of its old generation is dropped. A
    /// scope that starts is due at once with a new generation.
    pub fn follow(&mut self, next: TeamsFollow, now: Instant) -> Vec<(String, TeamsUpdate)> {
        let mut out = Vec::new();
        let previous = std::mem::replace(&mut self.follow, next);
        if previous.list && !self.follow.list {
            self.retire("list", &mut out);
        }
        if previous.waiting && !self.follow.waiting {
            self.retire("waiting", &mut out);
        }
        if previous.team != self.follow.team {
            self.phases_enabled = false;
            self.phase_selected = None;
            self.phase_epoch = self.phase_epoch.saturating_add(1);
            if let Some(old) = &previous.team {
                self.selected.remove(&team_key("phases", old));
                self.retire(&team_key("team", old), &mut out);
            }
            self.parts = None;
            for slot in [Slot::Status, Slot::Queue, Slot::Phases, Slot::Draft] {
                self.due.remove(&slot);
                self.dirty.remove(&slot);
            }
        }
        if !previous.list && self.follow.list {
            self.start("list", now);
        }
        if !previous.waiting && self.follow.waiting {
            self.start("waiting", now);
        }
        if previous.team != self.follow.team {
            if let Some(team) = self.follow.team.clone() {
                self.start(&team_key("team", &team), now);
            }
        }
        let wanted = [
            (Slot::List, self.follow.list),
            (Slot::Waiting, self.follow.waiting),
            (Slot::Status, self.follow.team.is_some()),
            (Slot::Queue, self.follow.team.is_some()),
            (Slot::Phases, self.phases_enabled),
            (Slot::Draft, self.phases_enabled && self.phase_selected.is_some()),
        ];
        for (slot, on) in wanted {
            if !on {
                self.due.remove(&slot);
                self.dirty.remove(&slot);
            }
        }
        out
    }

    /// Ends a scope and forgets its page: the old end is published before the
    /// next view of the key, and its late replies are stale.
    fn retire(&mut self, key: &str, out: &mut Vec<(String, TeamsUpdate)>) {
        self.end(key, out);
        self.selected.remove(key);
        self.generations.remove(key);
        self.queued.retain(|read| key_of(read) != key);
    }

    /// Starts a scope with a new generation, due at once.
    fn start(&mut self, key: &str, now: Instant) {
        self.generation_counter = self.generation_counter.saturating_add(1);
        let generation = self.generation_counter;
        self.generations.insert(key.to_owned(), generation);
        match key {
            "list" => {
                self.due.insert(Slot::List, now);
            }
            "waiting" => {
                self.due.insert(Slot::Waiting, now);
            }
            _ => {
                self.due.insert(Slot::Status, now);
                self.due.insert(Slot::Queue, now);
            }
        }
    }

    fn generation_of(&self, key: &str) -> u64 {
        self.generations.get(key).copied().unwrap_or(0)
    }

    /// Whether the tab is hidden: a hidden tab reads nothing.
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// The window was hidden (`true`) or shown again. Hidden, nothing is read;
    /// shown, every followed read is due at once, so the views are current.
    pub fn set_hidden(&mut self, hidden: bool, now: Instant) {
        if self.hidden == hidden {
            return;
        }
        self.hidden = hidden;
        if !hidden {
            self.refresh(now);
        }
    }

    /// Reads every followed view again at once: after the desktop's own action.
    /// A slot whose read is in flight is marked and read again at once when
    /// that reply lands, so the action never loses its reread.
    pub fn refresh(&mut self, now: Instant) {
        for slot in [Slot::Status, Slot::Queue, Slot::List, Slot::Waiting, Slot::Phases, Slot::Draft] {
            if self.due.contains_key(&slot) {
                self.mark(slot, now);
            }
        }
    }

    /// An explicit discovery retry needs a fresh answer even if unchanged:
    /// the requesting webview may have lost or never received the last one.
    pub fn recheck(&mut self, now: Instant) {
        self.shown.clear();
        self.refresh(now);
    }

    /// The project's tasks topic changed: the watched team's status and queue
    /// are read again (a report moves a node's state).
    pub fn tasks_changed(&mut self, now: Instant) {
        for slot in [Slot::Status, Slot::Queue, Slot::Phases, Slot::Draft] {
            if self.due.contains_key(&slot) {
                self.mark(slot, now);
            }
        }
    }

    /// Due now, or marked when its read is in flight.
    fn mark(&mut self, slot: Slot, now: Instant) {
        if self.in_flight.contains(&slot) {
            self.dirty.insert(slot);
        } else if let Some(due) = self.due.get_mut(&slot) {
            *due = now;
        }
    }

    /// Asks for one page of a followed key by its cursor, or for the first
    /// page when there is none. The page is selected for the key, so every
    /// later read — the poll, a refresh, a tasks change — reads that page
    /// until another is selected.
    pub fn page(&mut self, key: &str, cursor: Option<&str>) -> bool {
        if self.followed_key(key).is_none() {
            return false;
        }
        let after = match cursor {
            Some(cursor) => {
                if !valid_cursor(cursor) {
                    return false;
                }
                self.selected.insert(key.to_owned(), cursor.to_owned());
                Some(cursor.to_owned())
            }
            None => {
                self.selected.remove(key);
                None
            }
        };
        let read = match key {
            "list" => TeamRead::List { after },
            "waiting" => TeamRead::Waiting { after },
            _ if key.starts_with("phases:") => TeamRead::Phases { team: key[7..].to_owned(), after },
            _ => {
                let Some(team) = key.strip_prefix("team:") else {
                    return false;
                };
                TeamRead::Queue {
                    team: team.to_owned(),
                    after,
                }
            }
        };
        if self.queued.len() >= QUEUED || self.queued.contains(&read) {
            return false;
        }
        self.queued.push_back(read);
        true
    }

    /// The key when the follow wants its view.
    fn followed_key<'k>(&self, key: &'k str) -> Option<&'k str> {
        match key {
            "list" if self.follow.list => Some("list"),
            "waiting" if self.follow.waiting => Some("waiting"),
            _ if self.phases_enabled && key.strip_prefix("phases:") == self.watched() => Some(key),
            _ => {
                let team = key.strip_prefix("team:")?;
                (self.follow.team.as_deref() == Some(team)).then_some(key)
            }
        }
    }

    /// The next read to run now, if any: an asked-for page first, then the
    /// watched team's status, its queue, the list and what waits for the
    /// person. Its slot is in flight until its reply is applied.
    pub fn next(&mut self, now: Instant) -> Option<TeamRead> {
        if self.hidden {
            return None;
        }
        let follow = self.follow.clone();
        let in_flight = self.in_flight.clone();
        let phases_enabled = self.phases_enabled;
        let phase_selected = self.phase_selected;
        let wanted = |read: &TeamRead| {
            !in_flight.contains(&Slot::of(read))
                && match read {
                    TeamRead::List { .. } => follow.list,
                    TeamRead::Waiting { .. } => follow.waiting,
                    TeamRead::Phases { team, .. } => phases_enabled && follow.team.as_deref() == Some(team.as_str()),
                    TeamRead::Draft { team, phase } => phases_enabled && phase_selected == Some(*phase) && follow.team.as_deref() == Some(team.as_str()),
                    TeamRead::Status { team } | TeamRead::Queue { team, .. } => {
                        follow.team.as_deref() == Some(team.as_str())
                    }
                }
        };
        if let Some(read) = self.queued.pop_front_if(|read| wanted(read)) {
            let slot = Slot::of(&read);
            self.in_flight.insert(slot);
            self.issue_phase(slot);
            self.issued.insert(slot, self.generation_of(&key_of(&read)));
            self.due.insert(slot, now + POLL);
            return Some(read);
        }
        let watched = self.follow.team.clone();
        let slot = self
            .due
            .iter()
            .filter(|(slot, due)| **due <= now && !self.in_flight.contains(slot))
            .min_by_key(|(slot, due)| (slot.rank(), **due))
            .map(|(slot, _)| *slot)?;
        let read = match slot {
            Slot::Phases => {
                let team = watched?;
                TeamRead::Phases { after: self.selected.get(&team_key("phases", &team)).cloned(), team }
            }
            Slot::Draft => TeamRead::Draft { team: watched?, phase: self.phase_selected? },
            Slot::List => TeamRead::List {
                after: self.selected.get("list").cloned(),
            },
            Slot::Waiting => TeamRead::Waiting {
                after: self.selected.get("waiting").cloned(),
            },
            Slot::Status => TeamRead::Status {
                team: watched.clone()?,
            },
            Slot::Queue => {
                let team = watched?;
                let after = self.selected.get(&team_key("team", &team)).cloned();
                TeamRead::Queue { team, after }
            }
        };
        self.in_flight.insert(slot);
        self.issue_phase(slot);
        self.issued.insert(slot, self.generation_of(&key_of(&read)));
        self.due.insert(slot, now + POLL);
        Some(read)
    }

    /// How long until a read is due, at most [`POLL`].
    pub fn wait(&self, now: Instant) -> Duration {
        if self.hidden {
            return POLL;
        }
        if !self.queued.is_empty() {
            return Duration::ZERO;
        }
        self.due
            .iter()
            .filter(|(slot, _)| !self.in_flight.contains(slot))
            .map(|(_, due)| due.saturating_duration_since(now))
            .min()
            .unwrap_or(POLL)
            .min(POLL)
    }

    fn end(&mut self, key: &str, out: &mut Vec<(String, TeamsUpdate)>) {
        if self.shown.remove(key).is_some() {
            out.push((key.to_owned(), TeamsUpdate::Ended));
        }
    }

    fn send(&mut self, key: String, update: TeamsUpdate, out: &mut Vec<(String, TeamsUpdate)>) {
        let payload = update.payload();
        let digest = digest(update.kind(), &payload);
        if self.shown.get(&key) != Some(&digest) {
            self.shown.insert(key.clone(), digest);
            out.push((key, update));
        }
    }

    fn watched(&self) -> Option<&str> {
        self.follow.team.as_deref()
    }

    /// The watched team's view from the replies it has.
    fn team_view(&self) -> Option<TeamView> {
        let (team, parts) = self.parts.as_ref()?;
        if Some(team.as_str()) != self.watched() {
            return None;
        }
        if parts.status.is_none() && parts.queue.is_none() {
            return None;
        }
        let empty = Value::Null;
        let status = parts.status.as_ref().unwrap_or(&empty);
        let queue = parts.queue.as_ref().unwrap_or(&empty);
        Some(TeamView {
            plan: self.phases_enabled.then(|| view::phases::PhasePane {
                epoch: self.phase_epoch,
                list: parts.phases.clone(), draft: parts.draft.clone(), selected: self.phase_selected,
                list_failure: parts.phases_failure.clone(), draft_failure: parts.draft_failure.clone(),
            }),
            after: parts.queue_after.clone(),
            ..view::team(status, queue)
        })
    }

    /// The slot's next due time: at once when a change marked it during the
    /// read, else one [`POLL`] from now.
    fn settled(&mut self, slot: Slot, now: Instant) {
        let due = if self.dirty.remove(&slot) {
            now
        } else {
            now + POLL
        };
        self.due.insert(slot, due);
    }

    /// Applies a read's reply: its slot is due again after [`POLL`] (or at
    /// once when a change marked it), and the views it changed are returned.
    /// A reply about a view no longer followed gives nothing.
    pub fn apply(
        &mut self,
        read: &TeamRead,
        reply: Result<Value, TeamError>,
        now: Instant,
    ) -> Vec<(String, TeamsUpdate)> {
        let mut out = Vec::new();
        let slot = Slot::of(read);
        self.in_flight.remove(&slot);
        let issued = self.issued.remove(&slot);
        let key = key_of(read);
        if issued.is_some_and(|issued| issued != self.generation_of(&key)) {
            // A reply of a scope that was retired and followed again: it never
            // touches the scope followed now.
            return out;
        }
        if matches!(read, TeamRead::Phases { .. } | TeamRead::Draft { .. }) {
            return self.apply_phase_read(read, reply, now);
        }
        if let (Some(after), Err(TeamError::Refused { code, message })) = (read.after(), &reply) {
            // Only the selected page of a still-followed key, in the generation
            // that issued the request, may reset. Any other refusal is
            // dropped: nothing is published and nothing is scheduled.
            let current = self.followed_key(&key).is_some()
                && self.selected.get(&key).map(String::as_str) == Some(after)
                && issued.is_some_and(|issued| issued == self.generation_of(&key));
            if !current {
                return out;
            }
            self.selected.remove(&key);
            self.queued.retain(|queued| key_of(queued) != key);
            self.due.insert(slot, now);
            let update = TeamsUpdate::Failed {
                code: "cursor_reset".into(),
                message: format!(
                    "The page could not be read ({code}: {message}); the first page will be shown."
                ),
            };
            self.send(key, update, &mut out);
            return out;
        }
        let after = read.after().map(str::to_owned);
        match read {
            TeamRead::Phases { .. } | TeamRead::Draft { .. } => {},
            TeamRead::List { .. } => self.apply_list(slot, after, reply, now, &mut out),
            TeamRead::Waiting { .. } => self.apply_waiting(slot, after, reply, now, &mut out),
            TeamRead::Status { team } | TeamRead::Queue { team, .. } => {
                self.apply_team(read, slot, team, reply, now, &mut out)
            }
        }
        out
    }

    /// A list page names the cursor it was read from, so the view keeps
    /// pages by their cursor and never takes a later page for the first.
    fn apply_list(
        &mut self,
        slot: Slot,
        after: Option<String>,
        reply: Result<Value, TeamError>,
        now: Instant,
        out: &mut Vec<(String, TeamsUpdate)>,
    ) {
        if !self.follow.list {
            return;
        }
        self.settled(slot, now);
        let update = match reply {
            Ok(reply) => TeamsUpdate::List(Arc::new(TeamList {
                after,
                ..view::list(&reply)
            })),
            Err(error) => failure(error),
        };
        self.send("list".into(), update, out);
    }

    /// A waiting page names the cursor it was read from: only the first page
    /// can say that nothing waits.
    fn apply_waiting(
        &mut self,
        slot: Slot,
        after: Option<String>,
        reply: Result<Value, TeamError>,
        now: Instant,
        out: &mut Vec<(String, TeamsUpdate)>,
    ) {
        if !self.follow.waiting {
            return;
        }
        self.settled(slot, now);
        let update = match reply {
            Ok(reply) => TeamsUpdate::Waiting(Arc::new(WaitingView {
                after,
                ..view::waiting(&reply)
            })),
            Err(error) => failure(error),
        };
        self.send("waiting".into(), update, out);
    }

    fn apply_team(
        &mut self,
        read: &TeamRead,
        slot: Slot,
        team: &str,
        reply: Result<Value, TeamError>,
        now: Instant,
        out: &mut Vec<(String, TeamsUpdate)>,
    ) {
        if Some(team) != self.watched() {
            // The slot belongs to the team followed now, which stays due; this
            // reply is about a team no tab shows.
            return;
        }
        self.settled(slot, now);
        if let Ok(reply) = &reply {
            if self
                .parts
                .as_ref()
                .is_none_or(|(watched, _)| watched != team)
            {
                self.parts = Some((team.to_owned(), Parts::default()));
            }
            if let Some((_, parts)) = self.parts.as_mut() {
                match read {
                    TeamRead::Status { .. } => parts.status = Some(reply.clone()),
                    _ => {
                        parts.queue = Some(reply.clone());
                        parts.queue_after = read.after().map(str::to_owned);
                    }
                }
            }
        }
        let update = match reply {
            Ok(_) => match self.team_view() {
                Some(view) => TeamsUpdate::Team(Arc::new(view)),
                None => return,
            },
            Err(error) => failure(error),
        };
        self.send(team_key("team", team), update, out);
    }
}

fn failure(error: TeamError) -> TeamsUpdate {
    TeamsUpdate::Failed {
        code: error.code().to_owned(),
        message: error.message().to_owned(),
    }
}

/// A change as the batch takes it: its key, its update and its payload.
#[derive(Clone, Debug, PartialEq)]
pub struct Sent {
    pub key: String,
    pub update: TeamsUpdate,
    pub payload: Arc<Value>,
}

impl Sent {
    pub fn new(key: String, update: TeamsUpdate) -> Self {
        let payload = Arc::new(update.payload());
        Self {
            key,
            update,
            payload,
        }
    }
}

impl From<(String, TeamsUpdate)> for Sent {
    fn from((key, update): (String, TeamsUpdate)) -> Self {
        Self::new(key, update)
    }
}

/// The computer a change belongs to, with the model's payload computed once.
pub fn change(computer: &ComputerId, sent: Sent) -> crate::Change {
    crate::Change::Teams {
        computer: computer.clone(),
        key: sent.key,
        update: sent.update,
        payload: sent.payload,
    }
}

#[cfg(test)]
mod tests;

/// The key of the view a read belongs to.
fn key_of(read: &TeamRead) -> String {
    match read {
        TeamRead::List { .. } => "list".to_owned(),
        TeamRead::Waiting { .. } => "waiting".to_owned(),
        TeamRead::Status { team } | TeamRead::Queue { team, .. } | TeamRead::Phases { team, .. } | TeamRead::Draft { team, .. } => team_key("team", team),
    }
}

/// A page cursor as the CLI gives it back (`t1.` and its page).
fn valid_cursor(cursor: &str) -> bool {
    !cursor.is_empty()
        && cursor.len() <= super::read::CURSOR_BYTES
        && !cursor.starts_with('-')
        && !cursor.chars().any(|c| c.is_control() || c.is_whitespace())
}
