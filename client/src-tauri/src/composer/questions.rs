//! Agent question cards and approvals of the open session: Qt's
//! `QuestionCard` state and `SessionsWindow::renderQuestion`
//! (`tray/src/QuestionCard.cpp`, `tray/src/SessionsWindow.cpp:5003-5068`).
//! The rules are the core's (`questions`, R7). This module holds the answer
//! drafts, loads and saves them through the app's one store under Qt's keys,
//! and projects the shown card into the composer view. A2-2 adds the delivery.
//!
//! The card's fields follow the composer's text epoch rules: the webview owns
//! what the user types and sends it with the epoch its fields are based on;
//! Rust applies only an edit of the current epoch, and every change of the
//! answers that comes from Rust (a rebuild, a load, a seeded default, a
//! refused edit) bumps the epoch, so the webview takes Rust's answers again.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;
use serde_json::Value;
use zerus_client_core::drafts::Loaded;
use zerus_client_core::questions::{
    self as rules, AnswerDraft, AnswerFacts, CardContext, CardView, Edit, Question, Selection,
};

use super::send::Context;
use super::{Composers, Hub};

/// At most this many answer drafts stay in memory; Qt keeps every one.
const HELD: usize = 64;
/// At most this many remembered choices among optional cards.
const CHOICES: usize = 64;
/// A failed save is tried again after this long (Qt's 2 s timer, 261-265).
const RETRY_SAVE: f64 = 2.0;

pub(crate) type Done = Box<dyn FnOnce(bool) + Send>;

/// The shown card, as the composer view carries it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct QuestionView {
    /// The card's draft key; every card command names it.
    pub card: String,
    /// Bumps when the card is built again (Qt's `changed` in `setQuestion`,
    /// `QuestionCard.cpp:387`: a new card, a changed form or a restored
    /// answer): the webview restarts the review delay and resets its fields.
    pub revision: u64,
    /// Bumps with every change of the answers that comes from Rust, each
    /// revision's included: the webview takes the fields' values again, and
    /// `question_edit` applies only an edit of the current epoch.
    pub epoch: u64,
    pub asked: String,
    pub asked_tip: String,
    pub terminal_label: String,
    pub view: CardView,
}

/// The question cards' state (`QuestionCard`'s drafts and key sets).
#[derive(Default)]
pub(crate) struct Answers {
    pub(crate) drafts: HashMap<String, AnswerDraft>,
    /// Least recently shown first.
    order: VecDeque<String>,
    /// Keys whose load has not answered yet (`m_loadingKeys`).
    pub(crate) loading: HashSet<String>,
    /// The newest save per key (`m_saveRevision`): an older save's answer is ignored.
    saves: HashMap<String, u64>,
    /// Keys whose newest save has not answered yet: never evicted, so an edit
    /// whose save then fails is still here to be saved again.
    unsaved: HashSet<String>,
    /// The ticket of each key's load under way: only that load's answer
    /// counts, so a read an older ensure started never replaces a newer one.
    loads: HashMap<String, u64>,
    next_load: u64,
    /// Keys whose last save failed (`m_failedSaves`).
    failed: HashSet<String>,
    /// The chosen optional card per (session, run, conversation) (`m_questionSelections`).
    choices: VecDeque<(String, String)>,
    /// The shown card's key and question, to tell a rebuild from a body refresh.
    shown: Option<(String, Question)>,
    pub(crate) revision: u64,
    pub(crate) epoch: u64,
    last_retry: f64,
    /// The listed cards' "Asked" texts by `created_at` (its bits), formatted
    /// before the hub lock is taken: local time may read the time zone files.
    asked: Vec<(u64, (String, String))>,
}

impl Answers {
    fn choice(&self, scope: &str) -> Option<&str> {
        self.choices
            .iter()
            .find(|(s, _)| s == scope)
            .map(|(_, id)| id.as_str())
    }

    fn remember(&mut self, scope: String, chosen: Option<String>) {
        self.choices.retain(|(s, _)| *s != scope);
        if let Some(id) = chosen {
            self.choices.push_back((scope, id));
            while self.choices.len() > CHOICES {
                self.choices.pop_front();
            }
        }
    }

    /// The selection changed: the card shown next is built again.
    pub(crate) fn deselect(&mut self) {
        self.shown = None;
    }

    /// The card is built again: the review restarts and the fields reset.
    fn rebuild(&mut self) {
        self.revision += 1;
        self.epoch += 1;
    }
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Where a choice among optional cards is kept (`renderQuestion`'s scope).
fn scope(key: &str, details: &Value) -> String {
    serde_json::to_string(&[
        key,
        text(details, "run_id"),
        text(details, "conversation_id"),
    ])
    .unwrap_or_default()
}

/// The answer rules' view of the open session (`renderQuestion` 5054-5065).
pub(crate) fn facts(ctx: &Context) -> AnswerFacts {
    AnswerFacts {
        online: ctx.online,
        state: ctx.row.state.clone(),
        updating: ctx.facts.terminating || ctx.facts.updating,
        inspect_error: ctx.facts.inspect_error,
    }
}

/// "Asked 28 Oct 14:03:05" (with the year when it is not this year) and the
/// full time with its zone (`setQuestion`, 376-379); formatted in Rust, never
/// in the webview, and never under the hub lock.
fn asked(created: f64) -> (String, String) {
    use chrono::{Datelike, Local, TimeZone};
    if !(created > 0.0 && created < 253_402_300_800.0) {
        return (String::new(), String::new());
    }
    let Some(time) = Local.timestamp_opt(created as i64, 0).single() else {
        return (String::new(), String::new());
    };
    let format = if time.year() == Local::now().year() {
        "%-d %b %H:%M:%S"
    } else {
        "%-d %b %Y %H:%M:%S"
    };
    (
        format!("Asked {}", time.format(format)),
        time.format("%-d %b %Y %H:%M:%S %Z").to_string(),
    )
}

impl Composers {
    /// The open session's cards as Qt chooses them, without side effects.
    fn selection(&self, hub: &Hub, ctx: &Context, navigation: i32) -> Option<(Selection, String)> {
        let details = ctx.details.as_deref()?;
        let questions = rules::pending(details);
        let answers = &hub.answers;
        // `hasSubmittedAnswer` (359-368): the held draft, or the card's own
        // record of a submitted answer.
        let submitted = |q: &Question| {
            if q.source != "codex_async" || !q.optional {
                return false;
            }
            let Some(key) = rules::store_key(&ctx.key, q) else {
                return false;
            };
            let mut draft = answers.drafts.get(&key).cloned().unwrap_or_default();
            draft.restore_submitted(q);
            draft.submitted
        };
        let scope = scope(&ctx.key, details);
        Some((
            rules::select(&questions, &submitted, answers.choice(&scope), navigation),
            scope,
        ))
    }

    /// What a change of the open session's cards sets off (`renderQuestion`,
    /// `setQuestion`): drafts load, Codex's submitted answers are restored,
    /// OpenCode's defaults are seeded, the choice is remembered, and a new
    /// card is built.
    pub(crate) fn sync_questions(&self) {
        self.sync_moving(0);
    }

    fn sync_moving(&self, navigation: i32) {
        let listed = {
            let hub = self.lock();
            self.context(&hub).and_then(|ctx| ctx.details)
        };
        let asked: Vec<_> = listed
            .as_deref()
            .map(rules::pending)
            .unwrap_or_default()
            .iter()
            .map(|q| (q.created_at.to_bits(), asked(q.created_at)))
            .collect();
        let mut hub = self.lock();
        // Only the details the times were formatted from: newer ones format
        // their own on the sync they set off.
        let current = self.context(&hub).and_then(|ctx| ctx.details);
        if let (Some(listed), Some(current)) = (&listed, &current) {
            if std::sync::Arc::ptr_eq(listed, current) {
                hub.answers.asked = asked;
            }
        }
        let Some(ctx) = self.context(&hub) else {
            hub.answers.shown = None;
            return;
        };
        let Some(details) = ctx.details.clone() else {
            // Qt clears its details on a selection change (SessionsWindow.cpp:451),
            // so the card shown next is a new one for setQuestion.
            hub.answers.shown = None;
            return;
        };
        let label = rules::label(&ctx.key);
        let listed: HashSet<String> = rules::pending(&details)
            .iter()
            .filter_map(|q| rules::store_key(&ctx.key, q))
            .collect();
        let mut restored = HashSet::new();
        for q in rules::pending(&details)
            .into_iter()
            .filter(|q| q.source == "codex_async" && q.optional)
        {
            let Some(key) = rules::store_key(&ctx.key, &q) else {
                continue;
            };
            self.ensure_answer(&mut hub, &key, &label);
            let Some(draft) = hub.answers.drafts.get_mut(&key) else {
                continue;
            };
            let changed = draft.restore_submitted(&q);
            if draft.submitted {
                // A verified record wins over a load still on its way (Qt 321).
                hub.answers.loading.remove(&key);
            }
            if changed {
                restored.insert(key.clone());
                self.save_answer(&mut hub, &key, false, None);
            }
        }
        let Some((selection, scope)) = self.selection(&hub, &ctx, navigation) else {
            return;
        };
        hub.answers.remember(scope, selection.chosen.clone());
        let Some(question) = selection.question else {
            hub.answers.shown = None;
            drop(hub);
            self.changed();
            return;
        };
        let Some(key) = rules::store_key(&ctx.key, &question) else {
            return;
        };
        self.ensure_answer(&mut hub, &key, &label);
        let mut seeded = false;
        if !hub.answers.loading.contains(&key) {
            if let Some(draft) = hub.answers.drafts.get_mut(&key) {
                let before = draft.answers.clone();
                draft.seed(&question);
                seeded = draft.answers != before;
            }
        }
        let rebuilt = restored.contains(&key)
            || match &hub.answers.shown {
                Some((shown, old)) => {
                    *shown != key
                        || (old.items != question.items && !rules::same_form(old, &question))
                }
                None => true,
            };
        if rebuilt {
            hub.answers.rebuild();
        } else if seeded {
            hub.answers.epoch += 1;
        }
        hub.answers.shown = Some((key, question));
        Self::trim_answers(&mut hub, &listed);
        drop(hub);
        self.changed();
    }

    /// Loads a card's draft that is not in memory (`ensureDraft`, 282-309).
    fn ensure_answer(&self, hub: &mut Hub, key: &str, label: &str) {
        hub.answers.order.retain(|k| k != key);
        hub.answers.order.push_back(key.to_owned());
        // The app quits: the store takes no more work (`exit`, lib.rs).
        if hub.closed || hub.answers.drafts.contains_key(key) {
            return;
        }
        hub.answers.drafts.insert(
            key.to_owned(),
            AnswerDraft {
                label: label.to_owned(),
                ..AnswerDraft::default()
            },
        );
        hub.answers.loading.insert(key.to_owned());
        hub.answers.next_load += 1;
        let ticket = hub.answers.next_load;
        hub.answers.loads.insert(key.to_owned(), ticket);
        let me = self.clone();
        let (owned, label) = (key.to_owned(), label.to_owned());
        self.0.store.load(key, move |loaded| {
            me.answer_loaded(&owned, ticket, &label, &loaded)
        });
    }

    /// The draft loaded: the shown card takes its answers. Qt rebuilds the
    /// form without restarting the review (289-301), so only the epoch moves.
    fn answer_loaded(&self, key: &str, ticket: u64, label: &str, loaded: &Loaded) {
        let mut hub = self.lock();
        // A load whose mark a restore cleared, or that a later ensure of the
        // same key replaced, read an older store: it counts for nothing.
        if !hub.answers.loading.contains(key) || hub.answers.loads.get(key) != Some(&ticket) {
            return;
        }
        hub.answers.loading.remove(key);
        hub.answers.loads.remove(key);
        hub.answers
            .drafts
            .insert(key.to_owned(), AnswerDraft::from_loaded(loaded, label));
        if hub
            .answers
            .shown
            .as_ref()
            .is_some_and(|(shown, _)| shown == key)
        {
            hub.answers.epoch += 1;
        }
        drop(hub);
        self.sync_questions();
    }

    /// Keeps at most `HELD` drafts, never one the open session lists now,
    /// the shown one, one loading, one sending, one whose save has not
    /// answered yet or one whose save failed (Task 3a's composer trim).
    fn trim_answers(hub: &mut Hub, listed: &HashSet<String>) {
        let shown = hub.answers.shown.as_ref().map(|(key, _)| key.clone());
        let answers = &mut hub.answers;
        let mut i = 0;
        while answers.drafts.len() > HELD && i < answers.order.len() {
            let key = answers.order[i].clone();
            let keep = listed.contains(&key)
                || shown.as_deref() == Some(key.as_str())
                || answers.loading.contains(&key)
                || answers.failed.contains(&key)
                || answers.unsaved.contains(&key)
                || answers.drafts.get(&key).is_some_and(|d| d.sending);
            if keep {
                i += 1;
                continue;
            }
            answers.order.remove(i);
            answers.drafts.remove(&key);
            answers.saves.remove(&key);
        }
    }

    /// Saves a card's draft (`saveDraft`, 709-739); `durable` for the save
    /// before a submission, which nothing replaces. `done` hears whether it
    /// was written, never under the hub's lock.
    pub(crate) fn save_answer(
        &self,
        hub: &mut Hub,
        key: &str,
        durable: bool,
        done: Option<Done>,
    ) -> bool {
        let refuse = |done: Option<Done>| {
            if let Some(done) = done {
                (self.0.deps.spawn)(Box::new(move || done(false)));
            }
            false
        };
        if hub.closed || hub.answers.loading.contains(key) {
            return refuse(done);
        }
        let Some(draft) = hub.answers.drafts.get(key) else {
            return refuse(done);
        };
        if !durable && draft.sending {
            return false;
        }
        let stored = draft.stored();
        let save = {
            let n = hub.answers.saves.entry(key.to_owned()).or_default();
            *n += 1;
            *n
        };
        hub.answers.unsaved.insert(key.to_owned());
        let me = self.clone();
        let owned = key.to_owned();
        let finished =
            move |result: Result<(), String>| me.answer_saved(&owned, save, result, done);
        if durable {
            self.0.store.save_durable(key, stored, finished);
        } else {
            self.0.store.save(key, stored, finished);
        }
        true
    }

    fn answer_saved(&self, key: &str, save: u64, result: Result<(), String>, done: Option<Done>) {
        let mut hub = self.lock();
        if hub.answers.saves.get(key) == Some(&save) {
            hub.answers.unsaved.remove(key);
        }
        if hub.answers.saves.get(key) != Some(&save) || !hub.answers.drafts.contains_key(key) {
            drop(hub);
            if let Some(done) = done {
                done(false);
            }
            return;
        }
        let ok = result.is_ok();
        if let Some(draft) = hub.answers.drafts.get_mut(key) {
            draft.storage_error = result.err().unwrap_or_default();
        }
        if ok {
            hub.answers.failed.remove(key);
        } else {
            hub.answers.failed.insert(key.to_owned());
        }
        drop(hub);
        self.changed();
        if let Some(done) = done {
            done(ok);
        }
    }

    /// Saves again, every 2 s, the drafts whose last save failed.
    pub(crate) fn retry_answer_saves(&self) {
        let now = self.now();
        let mut hub = self.lock();
        if hub.closed || hub.answers.failed.is_empty() || now - hub.answers.last_retry < RETRY_SAVE
        {
            return;
        }
        hub.answers.last_retry = now;
        let keys: Vec<String> = hub.answers.failed.iter().cloned().collect();
        for key in keys {
            self.save_answer(&mut hub, &key, false, None);
        }
    }

    /// The question part of the open composer's view: the shown card and
    /// whether the editor shows. A failed question read is the session
    /// status (`send::session_view`), as Qt shows it.
    pub(crate) fn question_part(&self, hub: &Hub) -> (Option<QuestionView>, bool) {
        let Some(ctx) = self.context(hub) else {
            return (None, true);
        };
        let Some(details) = ctx.details.as_deref() else {
            return (None, true);
        };
        let Some((selection, _)) = self.selection(hub, &ctx, 0) else {
            return (None, true);
        };
        let input_visible = selection.input_visible();
        let Some(question) = selection.question.as_ref() else {
            return (None, input_visible);
        };
        let Some(key) = rules::store_key(&ctx.key, question) else {
            return (None, input_visible);
        };
        let draft = hub.answers.drafts.get(&key).cloned().unwrap_or_default();
        let reason = rules::unavailable(&facts(&ctx), question, details);
        let card = CardContext {
            available: reason.is_none(),
            unavailable_reason: reason.unwrap_or_default().to_owned(),
            loading: hub.answers.loading.contains(&key) || !hub.answers.drafts.contains_key(&key),
            in_flight: false,
            pending: selection.pending,
            queue_index: selection.index,
            queue_count: selection.optional.len(),
        };
        let (asked, asked_tip) = hub
            .answers
            .asked
            .iter()
            .find(|(at, _)| *at == question.created_at.to_bits())
            .map(|(_, texts)| texts.clone())
            .unwrap_or_default();
        let view = QuestionView {
            card: key,
            revision: hub.answers.revision,
            epoch: hub.answers.epoch,
            asked,
            asked_tip,
            terminal_label: if ctx.facts.cmd == "dsh" {
                "Open native UI"
            } else {
                "Open Terminal"
            }
            .to_owned(),
            view: rules::card(question, &draft, &card),
        };
        (Some(view), input_visible)
    }

    /// The shown card when `card` names it; commands for another card do nothing.
    pub(crate) fn shown_question(&self, hub: &Hub, card: &str) -> Option<(Context, Question)> {
        let ctx = self.context(hub)?;
        let (selection, _) = self.selection(hub, &ctx, 0)?;
        let question = selection.question?;
        (rules::store_key(&ctx.key, &question).as_deref() == Some(card)).then_some((ctx, question))
    }

    /// A change on the card (`capture`, 676-689): saved as the user makes it.
    /// An edit typed against an older epoch is dropped (Rust's answers
    /// stand, and their view is on its way); one the card refuses hands
    /// Rust's answers back under a new epoch.
    pub fn question_edit(&self, card: &str, epoch: u64, edit: Edit) {
        let mut hub = self.lock();
        if hub.closed || epoch != hub.answers.epoch {
            return;
        }
        let Some((_, question)) = self.shown_question(&hub, card) else {
            return;
        };
        let applied = !hub.answers.loading.contains(card)
            && hub
                .answers
                .drafts
                .get_mut(card)
                .is_some_and(|draft| draft.apply(&question, edit));
        if applied {
            self.save_answer(&mut hub, card, false, None);
        } else {
            hub.answers.epoch += 1;
        }
        drop(hub);
        self.changed();
    }

    /// A tab, Back or Next (`currentChanged`, 249-255): the page is kept with
    /// the draft.
    pub fn question_page(&self, card: &str, page: usize) {
        let mut hub = self.lock();
        if hub.closed
            || hub.answers.loading.contains(card)
            || self.shown_question(&hub, card).is_none()
        {
            return;
        }
        let Some(draft) = hub.answers.drafts.get_mut(card) else {
            return;
        };
        if draft.locked() || draft.page == page {
            return;
        }
        draft.page = page;
        self.save_answer(&mut hub, card, false, None);
        drop(hub);
        self.changed();
    }

    /// The optional cards' arrows: -1 for newer, +1 for older.
    pub fn question_navigate(&self, direction: i32) {
        self.sync_moving(direction.signum());
    }

    /// Allow retry on the card (256-259), after the user checked Terminal.
    pub fn question_allow_retry(&self, card: &str) {
        let mut hub = self.lock();
        // Qt's Allow retry acts on the shown card (`m_key`, 256-259).
        if hub.closed || self.shown_question(&hub, card).is_none() {
            return;
        }
        let Some(draft) = hub.answers.drafts.get_mut(card) else {
            return;
        };
        draft.allow_retry();
        self.save_answer(&mut hub, card, false, None);
        drop(hub);
        self.changed();
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn question_edit(state: tauri::State<'_, Composers>, card: String, epoch: u64, edit: Edit) {
    state.question_edit(&card, epoch, edit);
}

#[tauri::command(rename_all = "snake_case")]
pub fn question_page(state: tauri::State<'_, Composers>, card: String, page: usize) {
    state.question_page(&card, page);
}

#[tauri::command(rename_all = "snake_case")]
pub fn question_navigate(state: tauri::State<'_, Composers>, direction: i32) {
    state.question_navigate(direction);
}

#[tauri::command(rename_all = "snake_case")]
pub fn question_allow_retry(state: tauri::State<'_, Composers>, card: String) {
    state.question_allow_retry(&card);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::composer::send::tests::{
        opened, other_session, ready, send_hub, send_hub_on, state,
    };
    use crate::composer::tests::{dir, drafts, hold, session, until};
    use crate::composer::{lock, ComposerView};
    use serde_json::json;
    use std::sync::Arc;
    use zerus_client_core::drafts::{read_draft, DraftStore, StoredDraft};

    pub(crate) const HASH: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// A Claude AskUserQuestion card, as `hgs inspect` lists it.
    pub(crate) fn card_json(id: &str) -> Value {
        json!({"question_id": id, "question_hash": HASH, "run_id": "run-1", "conversation_id": "conv-1",
               "source": "hook", "answer_transport": "claude_hook", "created_at": 1_793_000_000.0, "can_answer": true,
               "questions": [{"id": "q_0", "question": "Which checks?", "header": "Checks", "body": "", "multi_select": false,
                              "allow_other": true, "options": [{"id": "opt_0_0", "label": "Unit", "description": ""},
                                                                 {"id": "opt_0_1", "label": "UI", "description": ""}]}]})
    }

    pub(crate) fn asking(cards: Vec<Value>) -> Value {
        let mut details = ready();
        details["phase"] = json!("input");
        details["pending_questions"] = json!(cards);
        details
    }

    pub(crate) fn store_key(id: &str) -> String {
        format!("question\n[\"studio\\nclaude/sample-project/review\",\"{id}\",\"{HASH}\"]\nrun-1\nconv-1")
    }

    /// The composer view once a card shows and its draft has loaded.
    pub(crate) fn with_card(c: &Composers) -> ComposerView {
        until(c, |v| {
            v.question.as_ref().is_some_and(|q| {
                q.view.status != "Loading your saved answer…" && !q.view.forms.is_empty()
            })
        })
    }

    /// The shown card's key and epoch, for its commands.
    pub(crate) fn shown(c: &Composers) -> (String, u64) {
        let q = with_card(c).question.unwrap();
        (q.card, q.epoch)
    }

    fn checked(view: &ComposerView) -> Vec<bool> {
        let q = view.question.as_ref().unwrap();
        q.view.forms[0].options.iter().map(|o| o.checked).collect()
    }

    /// An answer draft with `option` chosen, saved before the hub opens it.
    fn saved_choice(store: &DraftStore, option: &str) {
        saved_choice_for(store, "tool:t1", option);
    }

    fn saved_choice_for(store: &DraftStore, id: &str, option: &str) {
        let mut stored = StoredDraft::default();
        stored.form_state.insert(
            "answers".into(),
            json!([{"id": "q_0", "text": "", "other": false, "options": [option]}]),
        );
        store.save_durable(&store_key(id), stored, |_| {});
        store.flush();
    }

    /// An optional Codex card, asked at `at`.
    fn optional_card(id: &str, at: f64) -> Value {
        let mut card = card_json(id);
        card["source"] = json!("codex_async");
        card["optional"] = json!(true);
        card["can_skip"] = json!(true);
        card["created_at"] = json!(at);
        card
    }

    /// The answer Codex recorded for `id`: UI chosen.
    fn delivered(id: &str) -> Value {
        json!({"status": "submitted", "question_id": id, "question_hash": HASH, "run_id": "run-1",
               "conversation_id": "conv-1", "answers": [{"question_id": "q_0", "selected_option_ids": ["opt_0_1"], "text": ""}]})
    }

    fn loaded_all(c: &Composers) {
        until(c, |_| c.lock().answers.loading.is_empty());
    }

    #[test]
    fn a_required_card_replaces_the_editor() {
        let root = dir("q-required");
        let (c, _) = send_hub(&root);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let view = with_card(&c);
        assert!(
            !view.input_visible,
            "a required card hides the editor and the action row"
        );
        let q = view.question.unwrap();
        assert_eq!(q.card, store_key("tool:t1"));
        assert_eq!(q.view.heading, "Agent needs your answer");
        assert_eq!(
            q.view.forms[0]
                .options
                .iter()
                .map(|o| o.label.as_str())
                .collect::<Vec<_>>(),
            ["Unit", "UI"]
        );
        assert_eq!(q.view.status, "Answer each question to continue.");
        assert_eq!(q.terminal_label, "Open Terminal");
        assert!(q.asked.starts_with("Asked "));
    }

    #[test]
    fn edits_are_saved_under_qts_key() {
        let root = dir("q-saved");
        let store = drafts(&root);
        let (c, _) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let (card, epoch) = shown(&c);
        c.question_edit(
            &card,
            epoch,
            Edit::Option {
                item: "q_0".into(),
                option: "opt_0_1".into(),
                checked: true,
            },
        );
        let view = until(&c, |v| checked(v) == [false, true]);
        assert_eq!(
            view.question.unwrap().view.status,
            "Your choices are ready to send."
        );
        store.flush();
        let saved = read_draft(&root.join("drafts"), &store_key("tool:t1"));
        assert_eq!(
            saved.draft.label,
            "studio / claude/sample-project/review (question answer)"
        );
        assert_eq!(
            saved.draft.form_state["answers"],
            json!([{"id": "q_0", "text": "", "other": false, "options": ["opt_0_1"]}])
        );
    }

    #[test]
    fn an_answer_draft_survives_reopening() {
        let root = dir("q-reopen");
        let store = drafts(&root);
        let (c, _) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let (card, epoch) = shown(&c);
        c.question_edit(
            &card,
            epoch,
            Edit::Other {
                item: "q_0".into(),
                checked: true,
            },
        );
        c.question_edit(
            &card,
            epoch,
            Edit::Text {
                item: "q_0".into(),
                text: "Lint only".into(),
            },
        );
        c.flush();
        store.flush();
        drop(c);
        let (again, _) = send_hub_on(&store);
        opened(&again, asking(vec![card_json("tool:t1")]));
        let q = until(&again, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.view.forms[0].other.as_ref().is_some_and(|o| o.checked))
        })
        .question
        .unwrap();
        assert_eq!(q.view.forms[0].text.as_ref().unwrap().value, "Lint only");
        assert!(q.view.forms[0].text.as_ref().unwrap().visible);
    }

    #[test]
    fn optional_cards_are_newest_first_and_keep_the_choice() {
        let root = dir("q-optional");
        let (c, _) = send_hub(&root);
        let optional = |id: &str, at: f64| {
            let mut card = card_json(id);
            card["source"] = json!("codex_async");
            card["optional"] = json!(true);
            card["can_skip"] = json!(true);
            card["created_at"] = json!(at);
            card
        };
        let mut details = ready();
        details["pending_questions"] =
            json!([optional("codex:old", 10.0), optional("codex:new", 20.0)]);
        opened(&c, details.clone());
        let view = with_card(&c);
        assert!(view.input_visible, "an optional card keeps the composer");
        let q = view.question.unwrap();
        assert_eq!(q.card, store_key("codex:new"));
        assert_eq!(q.view.heading, "Optional question #1/2");
        c.question_navigate(1);
        let q = until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.card == store_key("codex:old"))
        })
        .question
        .unwrap();
        assert_eq!(q.view.heading, "Optional question #2/2");
        assert!(q
            .view
            .nav
            .as_ref()
            .is_some_and(|nav| nav.previous && !nav.next));
        c.observe(&state(details));
        assert_eq!(
            c.current().unwrap().question.unwrap().card,
            store_key("codex:old"),
            "a new state keeps the choice"
        );
    }

    #[test]
    fn a_rebuild_bumps_the_revision_and_the_epoch() {
        let root = dir("q-rebuild");
        let (c, _) = send_hub(&root);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let first = with_card(&c).question.unwrap();
        // A new card (Qt's `changed`, QuestionCard.cpp:387) restarts the
        // review and hands Rust's answers over: both counters grow.
        c.observe(&state(asking(vec![card_json("tool:t2")])));
        let next = until(&c, |v| {
            v.question.as_ref().is_some_and(|q| {
                q.card == store_key("tool:t2") && q.view.status != "Loading your saved answer…"
            })
        })
        .question
        .unwrap();
        assert!(next.revision > first.revision && next.epoch > first.epoch);
        // A changed form of the same card is a rebuild too.
        let mut changed = card_json("tool:t2");
        changed["questions"][0]["options"][1]["label"] = json!("Browser");
        c.observe(&state(asking(vec![changed])));
        let rebuilt = until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.view.forms[0].options[1].label == "Browser")
        })
        .question
        .unwrap();
        assert!(rebuilt.revision > next.revision && rebuilt.epoch > next.epoch);
        // The same card again changes neither.
        let mut same = card_json("tool:t2");
        same["questions"][0]["options"][1]["label"] = json!("Browser");
        c.observe(&state(asking(vec![same])));
        let again = c.current().unwrap().question.unwrap();
        assert_eq!(
            (again.revision, again.epoch),
            (rebuilt.revision, rebuilt.epoch)
        );
    }

    #[test]
    fn a_stale_edit_after_a_draft_load_leaves_rusts_answers() {
        let root = dir("q-stale");
        let store = drafts(&root);
        saved_choice(&store, "opt_0_0");
        let release = hold(&store);
        let (c, _) = send_hub_on(&store);
        c.open(Some(session()));
        c.observe(&state(asking(vec![card_json("tool:t1")])));
        let loading = until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.view.status == "Loading your saved answer…")
        })
        .question
        .unwrap();
        // An edit while the draft loads is refused and handed back.
        c.question_edit(
            &loading.card,
            loading.epoch,
            Edit::Option {
                item: "q_0".into(),
                option: "opt_0_1".into(),
                checked: true,
            },
        );
        let refused = c.current().unwrap().question.unwrap();
        assert!(
            refused.epoch > loading.epoch,
            "a refusal hands the answers back"
        );
        assert_eq!(refused.revision, loading.revision);
        release.send(()).unwrap();
        let loaded = with_card(&c);
        assert_eq!(checked(&loaded), [true, false]);
        let loaded = loaded.question.unwrap();
        // Qt's load rebuilds the form without restarting the review (282-309).
        assert_eq!(loaded.revision, loading.revision);
        assert!(loaded.epoch > refused.epoch);
        // A late edit typed against the form before the load changes nothing.
        c.question_edit(
            &loaded.card,
            refused.epoch,
            Edit::Option {
                item: "q_0".into(),
                option: "opt_0_1".into(),
                checked: true,
            },
        );
        let after = c.current().unwrap();
        assert_eq!(checked(&after), [true, false], "Rust's answers stand");
        assert_eq!(after.question.unwrap().epoch, loaded.epoch);
        store.flush();
        let saved = read_draft(&root.join("drafts"), &store_key("tool:t1"));
        assert_eq!(
            saved.draft.form_state["answers"][0]["options"],
            json!(["opt_0_0"])
        );
    }

    #[test]
    fn a_refused_edit_hands_the_answers_back() {
        let root = dir("q-refused");
        let (c, _) = send_hub(&root);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let (card, epoch) = shown(&c);
        let option = |checked| Edit::Option {
            item: "q_0".into(),
            option: "opt_0_0".into(),
            checked,
        };
        c.question_edit(&card, epoch, option(true));
        let chosen = until(&c, |v| checked(v) == [true, false]).question.unwrap();
        assert_eq!(chosen.epoch, epoch, "an accepted edit keeps the epoch");
        // Unchecking a radio button does nothing in Qt: Rust hands it back.
        c.question_edit(&card, epoch, option(false));
        let back = c.current().unwrap();
        assert_eq!(checked(&back), [true, false]);
        let back = back.question.unwrap();
        assert!(back.epoch > epoch);
        assert_eq!(back.revision, chosen.revision, "no review restart");
    }

    #[test]
    fn nothing_is_edited_once_the_app_quits() {
        let root = dir("q-closed");
        let store = drafts(&root);
        let (c, _) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let (card, epoch) = shown(&c);
        c.flush();
        c.question_edit(
            &card,
            epoch,
            Edit::Option {
                item: "q_0".into(),
                option: "opt_0_1".into(),
                checked: true,
            },
        );
        assert_eq!(checked(&c.current().unwrap()), [false, false]);
        store.flush();
        let saved = read_draft(&root.join("drafts"), &store_key("tool:t1"));
        assert!(
            saved.draft.form_state.get("answers").is_none(),
            "nothing saved"
        );
    }

    #[test]
    fn an_unavailable_session_says_why() {
        let root = dir("q-unavailable");
        let (c, _) = send_hub(&root);
        let mut details = asking(vec![card_json("tool:t1")]);
        details["runtime_state"] = json!("stopped");
        opened(&c, details);
        let q = with_card(&c).question.unwrap();
        assert!(!q.view.available && !q.view.submit.enabled);
        assert_eq!(q.view.status, "The agent is no longer running.");
    }

    #[test]
    fn a_sending_answer_reloads_uncertain() {
        let root = dir("q-sending");
        let store = drafts(&root);
        let mut stored = StoredDraft {
            sending: true,
            ..StoredDraft::default()
        };
        stored.form_state.insert(
            "answers".into(),
            json!([{"id": "q_0", "text": "", "other": false, "options": ["opt_0_0"]}]),
        );
        store.save_durable(&store_key("tool:t1"), stored, |_| {});
        store.flush();
        let (c, _) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let q = until(&c, |v| v.question.as_ref().is_some_and(|q| q.view.retry))
            .question
            .unwrap();
        assert_eq!(
            q.view.status,
            "Delivery was interrupted. Check Activity or Terminal before sending again."
        );
        assert!(q.view.status_error && !q.view.submit.enabled);
        assert!(q.view.forms[0].options[0].checked);
        c.question_allow_retry(&q.card);
        let q = until(&c, |v| v.question.as_ref().is_some_and(|q| !q.view.retry))
            .question
            .unwrap();
        assert!(q.view.submit.enabled);
    }

    #[test]
    fn a_card_answered_elsewhere_goes_away() {
        let root = dir("q-elsewhere");
        let (c, _) = send_hub(&root);
        opened(&c, asking(vec![card_json("tool:t1")]));
        with_card(&c);
        c.observe(&state(ready()));
        let view = until(&c, |v| v.question.is_none());
        assert!(view.input_visible, "the composer returns");
    }

    #[test]
    fn a_failed_question_read_is_the_session_status() {
        let root = dir("q-error");
        let (c, fake) = send_hub(&root);
        let mut details = ready();
        details["pending_questions_error"] = json!("timed out");
        let view = opened(&c, details.clone());
        // Qt shows the hint only through the composer's session status
        // (`m_hint->hide()`, `setSessionStatus`, SessionsWindow.cpp:3953, 3984-3986).
        assert_eq!(
            view.session_status,
            "Could not load agent questions: timed out. Check Terminal."
        );
        assert!(view.session_status_error && !view.session_blocks_input);
        assert!(view.question.is_none() && view.input_visible);
        // It locks the editor while a block reason stands.
        lock(&fake.row).state = "stopped".into();
        c.observe(&state(details));
        let view = until(&c, |v| v.session_blocks_input);
        assert_eq!(
            view.session_status,
            "Could not load agent questions: timed out. Check Terminal."
        );
    }

    // Review round 1 of A2-1.

    #[test]
    fn a_failed_save_is_tried_again_every_two_seconds() {
        let root = dir("q-retry-save");
        let blocker = root.join("blocked");
        std::fs::write(&blocker, "a file where the store's folder belongs").unwrap();
        let store = Arc::new(DraftStore::open(blocker.join("drafts")));
        let (c, fake) = send_hub_on(&store);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let (card, epoch) = shown(&c);
        c.question_edit(
            &card,
            epoch,
            Edit::Option {
                item: "q_0".into(),
                option: "opt_0_0".into(),
                checked: true,
            },
        );
        let q = until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.view.status.starts_with("Draft not saved"))
        })
        .question
        .unwrap();
        assert!(q.view.status_error);
        // The 2 s timer fires while the store still fails.
        *lock(&fake.now) += 2.0;
        c.tick();
        store.flush();
        assert!(c.lock().answers.failed.contains(&card));
        std::fs::remove_file(&blocker).unwrap();
        // Nothing is tried within the next 2 s (Qt's timer, 261-265).
        *lock(&fake.now) += 1.0;
        c.tick();
        store.flush();
        assert!(c.lock().answers.failed.contains(&card), "not before 2 s");
        *lock(&fake.now) += 1.0;
        c.tick();
        until(&c, |v| {
            v.question.as_ref().is_some_and(|q| !q.view.status_error)
        });
        store.flush();
        assert_eq!(
            read_draft(&blocker.join("drafts"), &card).draft.form_state["answers"][0]["options"],
            json!(["opt_0_0"])
        );
    }

    #[test]
    fn past_64_drafts_the_oldest_go_but_no_listed_shown_loading_failed_or_unsaved_one() {
        let root = dir("q-evict");
        let (c, _) = send_hub(&root);
        let mut details = ready();
        details["pending_questions"] = json!((0..70)
            .map(|i| optional_card(&format!("codex:{i}"), f64::from(i)))
            .collect::<Vec<_>>());
        opened(&c, details);
        with_card(&c);
        loaded_all(&c);
        assert_eq!(
            c.lock().answers.drafts.len(),
            70,
            "every card the session lists keeps its draft"
        );
        {
            let mut hub = c.lock();
            hub.answers.failed.insert(store_key("codex:1"));
            hub.answers.loading.insert(store_key("codex:2"));
            hub.answers.unsaved.insert(store_key("codex:3"));
        }
        c.observe(&state(asking(vec![card_json("tool:t1")])));
        until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.card == store_key("tool:t1"))
        });
        let hub = c.lock();
        assert_eq!(hub.answers.drafts.len(), HELD);
        for kept in ["codex:1", "codex:2", "codex:3", "tool:t1"] {
            assert!(hub.answers.drafts.contains_key(&store_key(kept)), "{kept}");
        }
        assert!(
            !hub.answers.drafts.contains_key(&store_key("codex:0")),
            "the oldest goes first"
        );
    }

    #[test]
    fn at_most_64_choices_are_remembered() {
        let mut answers = Answers::default();
        for i in 0..65 {
            answers.remember(format!("scope-{i}"), Some(format!("codex:{i}")));
        }
        assert_eq!(answers.choices.len(), CHOICES);
        assert_eq!(answers.choice("scope-0"), None, "the oldest goes first");
        assert_eq!(answers.choice("scope-64"), Some("codex:64"));
        answers.remember("scope-64".into(), None);
        assert_eq!(answers.choice("scope-64"), None);
    }

    #[test]
    fn a_submitted_codex_answer_is_skipped_and_saved_as_submitted() {
        let root = dir("q-submitted");
        let store = drafts(&root);
        let (c, _) = send_hub_on(&store);
        let mut done = optional_card("codex:done", 20.0);
        done["answer_delivery"] = delivered("codex:done");
        let mut details = ready();
        details["pending_questions"] = json!([done, optional_card("codex:open", 10.0)]);
        opened(&c, details);
        let q = with_card(&c).question.unwrap();
        assert_eq!(
            q.card,
            store_key("codex:open"),
            "an answer Codex recorded is not offered again"
        );
        store.flush();
        let saved = read_draft(&root.join("drafts"), &store_key("codex:done"));
        assert_eq!(saved.draft.form_state["submitted"], json!(true));
        assert_eq!(
            saved.draft.form_state["answers"],
            json!([{"id": "q_0", "text": "", "other": false, "options": ["opt_0_1"]}])
        );
    }

    #[test]
    fn an_opencode_default_is_seeded_but_not_saved() {
        let root = dir("q-seed");
        let store = drafts(&root);
        let release = hold(&store);
        let (c, _) = send_hub_on(&store);
        let mut card = card_json("opencode:q1");
        card["source"] = json!("opencode_native");
        card["answer_transport"] = json!("opencode_native");
        card["questions"][0]["default_option_ids"] = json!(["opt_0_1"]);
        c.open(Some(session()));
        c.observe(&state(asking(vec![card])));
        let loading = until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.view.status == "Loading your saved answer…")
        })
        .question
        .unwrap();
        release.send(()).unwrap();
        let seeded = until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.view.forms[0].options[1].checked)
        })
        .question
        .unwrap();
        assert!(
            seeded.epoch >= loading.epoch + 2,
            "the load and the seed each hand Rust's answers over"
        );
        assert_eq!(seeded.revision, loading.revision);
        store.flush();
        let saved = read_draft(&root.join("drafts"), &store_key("opencode:q1"));
        assert!(
            saved.draft.form_state.get("answers").is_none(),
            "a seeded default is never saved"
        );
    }

    #[test]
    fn a_new_run_forgets_the_choice_among_optional_cards() {
        let root = dir("q-new-run");
        let (c, _) = send_hub(&root);
        let mut details = ready();
        details["pending_questions"] = json!([
            optional_card("codex:old", 10.0),
            optional_card("codex:new", 20.0)
        ]);
        opened(&c, details.clone());
        with_card(&c);
        c.question_navigate(1);
        until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.card == store_key("codex:old"))
        });
        let mut next = details;
        next["run_id"] = json!("run-2");
        for card in next["pending_questions"].as_array_mut().unwrap() {
            card["run_id"] = json!("run-2");
        }
        c.observe(&state(next));
        let q = until(&c, |v| {
            v.question
                .as_ref()
                .is_some_and(|q| q.card.ends_with("\nrun-2\nconv-1"))
        })
        .question
        .unwrap();
        assert!(q.card.contains("\"codex:new\""), "the newest card again");
    }

    #[test]
    fn the_page_is_kept_with_the_draft() {
        let root = dir("q-page");
        let store = drafts(&root);
        let (c, _) = send_hub_on(&store);
        let mut card = card_json("tool:t1");
        card["questions"].as_array_mut().unwrap().push(
            json!({"id": "q_1", "question": "Which platform?",
            "header": "Platform", "body": "", "multi_select": false, "allow_other": true,
            "options": [{"id": "opt_1_0", "label": "Linux", "description": ""}]}),
        );
        opened(&c, asking(vec![card]));
        let (key, _) = shown(&c);
        c.question_page(&key, 1);
        until(&c, |v| {
            v.question.as_ref().is_some_and(|q| q.view.page == 1)
        });
        store.flush();
        assert_eq!(
            read_draft(&root.join("drafts"), &key).draft.form_state["page"],
            json!(1)
        );
    }

    #[test]
    fn selecting_the_session_again_builds_its_card_again() {
        let root = dir("q-reselect");
        let (c, _) = send_hub(&root);
        let details = asking(vec![card_json("tool:t1")]);
        opened(&c, details.clone());
        let first = with_card(&c).question.unwrap();
        // Another session without details yet, then this one again.
        c.open(Some(other_session()));
        c.open(Some(session()));
        c.observe(&state(details));
        let again = until(&c, |v| v.question.is_some()).question.unwrap();
        assert!(
            again.revision > first.revision,
            "a new selection restarts the review (SessionsWindow.cpp:451)"
        );
    }

    #[test]
    fn no_store_work_once_the_app_quits() {
        let root = dir("q-quit-store");
        let (c, _) = send_hub(&root);
        opened(&c, asking(vec![card_json("tool:t1")]));
        let (card, _) = shown(&c);
        c.flush();
        c.observe(&state(asking(vec![card_json("tool:t2")])));
        c.lock().answers.failed.insert(card.clone());
        c.tick();
        let hub = c.lock();
        assert!(
            !hub.answers.drafts.contains_key(&store_key("tool:t2"))
                && hub.answers.loading.is_empty(),
            "no load starts"
        );
        assert!(!hub.answers.unsaved.contains(&card), "no save starts");
    }

    #[test]
    fn allow_retry_acts_on_the_shown_card_only() {
        let root = dir("q-retry-shown");
        let store = drafts(&root);
        let mut stored = StoredDraft {
            sending: true,
            ..StoredDraft::default()
        };
        stored.form_state.insert(
            "answers".into(),
            json!([{"id": "q_0", "text": "", "other": false, "options": ["opt_0_0"]}]),
        );
        store.save_durable(&store_key("codex:hidden"), stored, |_| {});
        store.flush();
        let (c, _) = send_hub_on(&store);
        let mut details = asking(vec![card_json("tool:t1")]);
        details["pending_questions"]
            .as_array_mut()
            .unwrap()
            .push(optional_card("codex:hidden", 10.0));
        opened(&c, details);
        with_card(&c);
        loaded_all(&c);
        let hidden = store_key("codex:hidden");
        assert!(c.lock().answers.drafts[&hidden].uncertain);
        c.question_allow_retry(&hidden);
        assert!(
            c.lock().answers.drafts[&hidden].uncertain,
            "only the shown card (Qt's m_key)"
        );
    }

    #[test]
    fn a_load_an_older_ensure_started_never_lands() {
        let root = dir("q-ticket");
        let store = drafts(&root);
        // The store holds an older answer: Unit chosen, nothing submitted.
        saved_choice_for(&store, "codex:k", "opt_0_0");
        let release = hold(&store);
        let (c, _) = send_hub_on(&store);
        let key = store_key("codex:k");
        c.open(Some(session()));
        // L1 starts; Codex's record clears its mark and saves the submitted
        // answer behind it.
        let mut recorded = optional_card("codex:k", 10.0);
        recorded["answer_delivery"] = delivered("codex:k");
        let mut details = ready();
        details["pending_questions"] = json!([recorded]);
        c.observe(&state(details));
        assert!(c.lock().answers.drafts[&key].submitted);
        // The draft leaves memory, as an eviction does once its save answered ...
        {
            let mut hub = c.lock();
            hub.answers.drafts.remove(&key);
            hub.answers.order.retain(|k| *k != key);
            hub.answers.saves.remove(&key);
            hub.answers.unsaved.remove(&key);
        }
        // ... and comes back without the record: L2 starts.
        let mut details = ready();
        details["pending_questions"] = json!([optional_card("codex:k", 10.0)]);
        c.observe(&state(details));
        assert!(c.lock().answers.loading.contains(&key));
        release.send(()).unwrap();
        store.flush();
        until(&c, |_| !c.lock().answers.loading.contains(&key));
        assert!(
            c.lock().answers.drafts[&key].submitted,
            "L2 read the submitted save; L1's older read never landed"
        );
    }
}
