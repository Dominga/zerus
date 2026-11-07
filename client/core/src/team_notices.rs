//! The always-on Team Waiting notices (task 258; Qt's `TeamAttentionWatch`
//! and `TeamWaitingNotices`): one read of `hgs team waiting` names what waits
//! for the owner across every team, and each item is announced once by its
//! id, whichever part of the client sees it first. The shown set is the
//! client's own `teams/attentionNotified` map, an item id to its notice token,
//! so a restart does not raise the same item again. A card the server held
//! while the owner was away (`notify:false`) is not announced until it comes
//! back, and cards that come back together (`batch`) are one notice.
//!
//! The caller reads the items and runs `hgs`, keeps the map in settings and
//! posts the changes; this module is only the decision.

use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The most waiting cards one read keeps (`hgs team waiting` pages at 100).
pub const ITEMS: usize = 200;

/// One waiting card as the notice decision needs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub team: String,
    pub team_name: String,
    pub kind: String,
    pub title: String,
    /// False when the server held this card while the owner was away.
    pub notify: bool,
    /// Cards of one arrival share this batch; empty for a lone card.
    pub batch: String,
}

/// One page of `hgs team waiting --json`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WaitingRead {
    pub items: Vec<Item>,
    /// The page holds every waiting item, so absent ids no longer wait.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub token: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub raised: Vec<Notice>,
    pub cleared: Vec<String>,
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Reads one page. A missing `notify` means the server did not hold the card;
/// `more: true` means the page is not the whole list.
pub fn decode(reply: &Value) -> WaitingRead {
    let items = reply
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .take(ITEMS)
                .filter_map(|item| {
                    let id = text(item, "id");
                    (!id.is_empty()).then(|| Item {
                        id,
                        team: text(item, "team"),
                        team_name: text(item, "team_name"),
                        kind: text(item, "kind"),
                        title: text(item, "title"),
                        notify: item.get("notify").and_then(Value::as_bool) != Some(false),
                        batch: text(item, "batch"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    WaitingRead {
        items,
        complete: reply.get("more").and_then(Value::as_bool) != Some(true),
    }
}

/// The heading of a card's notice (`TeamWaitingNotices.h:67-71`).
pub fn title(item: &Item) -> String {
    if item.kind == "disk_low" {
        format!("Low disk space on {}", item.team_name)
    } else {
        format!(
            "Waiting for you in {}",
            if item.team_name.is_empty() {
                &item.team
            } else {
                &item.team_name
            }
        )
    }
}

/// The token of a lone card; a batch's cards share one.
pub fn token(item: &Item) -> String {
    format!("team-waiting:{}", item.id)
}

pub fn batch_token(batch: &str) -> String {
    format!("team-waiting:batch:{batch}")
}

/// The id→token map as settings keep it; a malformed or older value is empty.
pub fn decode_posted(text: &str) -> BTreeMap<String, String> {
    serde_json::from_str::<BTreeMap<String, String>>(text).unwrap_or_default()
}

pub fn encode_posted(posted: &BTreeMap<String, String>) -> String {
    serde_json::to_string(posted).unwrap_or_else(|_| "{}".to_owned())
}

/// One read: a notice for each card seen for the first time that is neither
/// held nor a paused team; `complete` drops cards that no longer wait and
/// withdraws their notices. `posted` is updated exactly as `observe` decides.
pub fn observe(items: &[Item], complete: bool, posted: &mut BTreeMap<String, String>) -> Changes {
    let present: BTreeSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
    let fresh: Vec<&Item> = items
        .iter()
        .filter(|item| {
            !posted.contains_key(&item.id) && item.kind != "team_paused" && item.notify
        })
        .collect();
    let mut groups: BTreeMap<&str, Vec<&Item>> = BTreeMap::new();
    for item in &fresh {
        if !item.batch.is_empty() {
            groups
                .entry(item.batch.as_str())
                .or_default()
                .push(item);
        }
    }
    let mut raised = Vec::new();
    let mut done = BTreeSet::new();
    for item in fresh {
        if done.contains(item.id.as_str()) {
            continue;
        }
        let group = groups.get(item.batch.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let together: Vec<&Item> = if group.len() > 1 {
            group.to_vec()
        } else {
            vec![item]
        };
        let shared = together.len() > 1;
        let token = if shared {
            batch_token(&item.batch)
        } else {
            token(item)
        };
        for found in &together {
            posted.insert(found.id.clone(), token.clone());
            done.insert(found.id.as_str());
        }
        raised.push(Notice {
            token,
            title: title(item),
            body: together
                .iter()
                .map(|found| found.title.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        });
    }
    let mut cleared = Vec::new();
    if complete {
        let gone: Vec<(String, String)> = posted
            .iter()
            .filter(|(id, _)| !present.contains(id.as_str()))
            .map(|(id, token)| (id.clone(), token.clone()))
            .collect();
        for (id, token) in gone {
            posted.remove(&id);
            if !posted.values().any(|kept| kept == &token) {
                cleared.push(token);
            }
        }
    }
    Changes { raised, cleared }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> Item {
        Item {
            id: id.into(),
            team: "t".into(),
            team_name: "Orbit".into(),
            kind: "owner_word_asked".into(),
            title: id.into(),
            notify: true,
            batch: String::new(),
        }
    }

    fn reply(items: Value, more: bool) -> Value {
        serde_json::json!({ "items": items, "total": 2, "more": more })
    }

    #[test]
    fn the_page_keeps_top_level_presence_and_batch_and_says_when_it_is_complete() {
        let read = decode(&reply(
            serde_json::json!([
                {"id":"t/a/1","team":"t","team_name":"Orbit","kind":"owner_word_asked","title":"One","notify":false},
                {"id":"t/a/2","team":"t","team_name":"Orbit","kind":"owner_word_asked","title":"Two","notify":true,"batch":"b1"}
            ]),
            false,
        ));
        assert!(read.complete);
        assert_eq!(read.items.len(), 2);
        assert!(!read.items[0].notify);
        assert_eq!((read.items[1].notify, read.items[1].batch.as_str()), (true, "b1"));
        assert!(decode(&reply(serde_json::json!([]), false)).items.is_empty());
        assert!(decode(&serde_json::json!({})).complete);
    }

    #[test]
    fn a_card_is_announced_once_and_never_again_for_its_id() {
        let items = [item("t/a"), item("t/b")];
        let mut posted = BTreeMap::new();
        let first = observe(&items, false, &mut posted);
        assert_eq!(first.raised.len(), 2);
        assert_eq!(first.raised[0].title, "Waiting for you in Orbit");
        assert_eq!(first.raised[0].body, "t/a");
        assert_eq!(first.raised[0].token, "team-waiting:t/a");
        assert!(first.cleared.is_empty());
        assert_eq!(posted.get("t/a").map(String::as_str), Some("team-waiting:t/a"));
        // The same page, and a restart's map, raise nothing.
        assert!(observe(&items, false, &mut posted).raised.is_empty());
        let mut restarted = decode_posted(&encode_posted(&posted));
        assert!(observe(&items, false, &mut restarted).raised.is_empty());
    }

    #[test]
    fn a_held_card_waits_and_its_return_is_one_batch_notice() {
        let mut held = item("t/a");
        held.notify = false;
        let mut posted = BTreeMap::new();
        assert!(observe(&[held], true, &mut posted).raised.is_empty());
        assert!(posted.is_empty(), "a held card is not marked as shown");
        let mut first = item("t/a");
        first.notify = true;
        first.batch = "b1".into();
        let mut second = item("t/b");
        second.batch = "b1".into();
        let back = observe(&[first, second], true, &mut posted);
        assert_eq!(back.raised.len(), 1);
        assert_eq!(back.raised[0].token, "team-waiting:batch:b1");
        assert_eq!(back.raised[0].body, "t/a\nt/b");
        assert_eq!(posted.len(), 2);
        // Not again, and the batch's notice stays while one card remains.
        assert!(observe(&[item("t/a")], true, &mut posted).raised.is_empty());
        assert_eq!(posted.get("t/a").map(String::as_str), Some("team-waiting:batch:b1"));
    }

    #[test]
    fn complete_drops_gone_cards_and_paused_teams_never_notify() {
        let mut posted = BTreeMap::new();
        observe(&[item("t/a"), item("t/b")], false, &mut posted);
        let gone = observe(&[item("t/b")], true, &mut posted);
        assert_eq!(gone.cleared, vec!["team-waiting:t/a".to_owned()]);
        assert!(!posted.contains_key("t/a"));
        let mut paused = item("t/paused");
        paused.kind = "team_paused".into();
        let changes = observe(&[paused], false, &mut posted);
        assert!(changes.raised.is_empty());
        assert!(!posted.contains_key("t/paused"));
    }

    #[test]
    fn a_card_that_returns_after_being_dropped_is_announced_again_as_a_new_wait() {
        let mut posted = BTreeMap::new();
        observe(&[item("t/a")], true, &mut posted);
        assert_eq!(observe(&[], true, &mut posted).cleared, vec!["team-waiting:t/a".to_owned()]);
        assert_eq!(observe(&[item("t/a")], true, &mut posted).raised.len(), 1);
    }

    #[test]
    fn the_heading_names_a_disk_shortage_and_the_team() {
        let mut low = item("disk");
        low.kind = "disk_low".into();
        low.team_name = "Studio".into();
        assert_eq!(title(&low), "Low disk space on Studio");
        let mut unnamed = item("t/a");
        unnamed.team_name.clear();
        assert_eq!(title(&unnamed), "Waiting for you in t");
    }
}
