use super::*;
use crate::upstream_teams::view::GROUPS;
use serde_json::json;

const TEAM_A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const TEAM_B: &str = "bbbbbbbb-0000-4000-8000-000000000002";
const TEAM: &str = "00000000-0000-4000-8000-00000000beef";

#[test]
fn an_explicit_recheck_reads_and_publishes_an_unchanged_answer() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            ..TeamsFollow::default()
        },
        now,
    );
    let read = model.next(now).unwrap();
    let reply = list_reply();
    assert_eq!(model.apply(&read, Ok(reply.clone()), now).len(), 1);
    assert!(model.next(now).is_none());
    model.recheck(now);
    let read = model.next(now).expect("Read again is due immediately");
    assert_eq!(model.apply(&read, Ok(reply), now).len(), 1);
}

/// The pinned CLI replies of package 5, copied beside the module so the branch
/// runs before package 5 merges.
fn fixture(name: &str) -> Value {
    let text = match name {
        "status" => include_str!("../fixtures/cli-status.json"),
        "queue" => include_str!("../fixtures/cli-queue.json"),
        "cards" => include_str!("../fixtures/cli-cards.json"),
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).expect("a pinned CLI fixture parses")
}

/// `team status` about `team`.
fn status(team: &str) -> Value {
    let mut reply = fixture("status");
    reply["team"]["id"] = json!(team);
    reply
}

/// `team queue` about `team`, the pinned page of two nodes.
fn queue(team: &str) -> Value {
    let mut reply = fixture("queue");
    reply["team"] = json!(team);
    reply
}

/// `team list` and `team waiting`, whose fixtures arrive with package 5's fix
/// round; their shapes are the agreed ones.
fn list_reply() -> Value {
    json!({"ok": true, "actor": {"kind": "person"}, "rows": [], "next": null})
}

fn waiting_reply() -> Value {
    json!({"ok": true, "actor": {"kind": "person"}, "rows": [], "next": null})
}

fn keys(out: &[(String, TeamsUpdate)]) -> Vec<&str> {
    out.iter().map(|(key, _)| key.as_str()).collect()
}

fn kinds(out: &[(String, TeamsUpdate)]) -> Vec<&'static str> {
    out.iter().map(|(_, update)| update.kind()).collect()
}

fn follow_all(team: &str) -> TeamsFollow {
    TeamsFollow {
        list: true,
        waiting: true,
        team: Some(team.to_owned()),
    }
}

/// The last update per key, in the order the keys first changed.
fn latest(out: Vec<(String, TeamsUpdate)>) -> Vec<(String, TeamsUpdate)> {
    let mut order: Vec<String> = Vec::new();
    let mut last: BTreeMap<String, TeamsUpdate> = BTreeMap::new();
    for (key, update) in out {
        if !last.contains_key(&key) {
            order.push(key.clone());
        }
        last.insert(key, update);
    }
    order
        .into_iter()
        .map(|key| {
            let update = last.remove(&key).expect("listed");
            (key, update)
        })
        .collect()
}

/// Follows `team` and applies its status and queue replies.
fn watched(model: &mut TeamsModel, now: Instant, team: &str) -> Vec<(String, TeamsUpdate)> {
    let mut out = Vec::new();
    let read = TeamRead::Status {
        team: team.to_owned(),
    };
    assert_eq!(model.next(now), Some(read.clone()));
    out.extend(model.apply(&read, Ok(status(team)), now));
    assert_eq!(
        model.next(now),
        Some(TeamRead::Queue {
            team: team.to_owned(),
            after: None,
        })
    );
    out.extend(model.apply(
        &TeamRead::Queue {
            team: team.to_owned(),
            after: None,
        },
        Ok(queue(team)),
        now,
    ));
    latest(out)
}

#[test]
fn the_follow_set_never_holds_more_than_one_team() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    assert!(model.follow(follow_all(TEAM_A), now).is_empty());
    let first = watched(&mut model, now, TEAM_A);
    assert_eq!(keys(&first), [team_key("team", TEAM_A)]);
    assert_eq!(model.following().team.as_deref(), Some(TEAM_A));

    let ended = model.follow(follow_all(TEAM_B), now);
    assert_eq!(keys(&ended), [team_key("team", TEAM_A)]);
    assert!(matches!(ended[0].1, TeamsUpdate::Ended));
    assert_eq!(model.following().team.as_deref(), Some(TEAM_B));
    assert_eq!(
        model.next(now),
        Some(TeamRead::Status {
            team: TEAM_B.to_owned()
        }),
        "the new tab's team is read at once, never the old one"
    );
}

#[test]
fn each_followed_view_is_read_once_per_poll() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    assert_eq!(
        model.next(now),
        Some(TeamRead::Status {
            team: TEAM_A.to_owned()
        })
    );
    model.apply(
        &TeamRead::Status {
            team: TEAM_A.to_owned(),
        },
        Ok(status(TEAM_A)),
        now,
    );
    assert_eq!(
        model.next(now),
        Some(TeamRead::Queue {
            team: TEAM_A.to_owned(),
            after: None,
        })
    );
    model.apply(
        &TeamRead::Queue {
            team: TEAM_A.to_owned(),
            after: None,
        },
        Ok(queue(TEAM_A)),
        now,
    );
    assert_eq!(model.next(now), Some(TeamRead::List { after: None }));
    model.apply(&TeamRead::List { after: None }, Ok(list_reply()), now);
    assert_eq!(model.next(now), Some(TeamRead::Waiting { after: None }));
    model.apply(&TeamRead::Waiting { after: None }, Ok(waiting_reply()), now);
    assert_eq!(model.next(now), None, "one read per view per poll");
    assert_eq!(model.wait(now), POLL);
    assert_eq!(
        model.next(now + POLL),
        Some(TeamRead::Status {
            team: TEAM_A.to_owned()
        }),
        "every poll reads it again"
    );
}

#[test]
fn a_hidden_tab_reads_nothing() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    model.set_hidden(true, now);
    assert!(model.hidden());
    assert_eq!(model.next(now), None);
    assert_eq!(model.next(now + POLL * 10), None);
    assert_eq!(model.wait(now), POLL);
    model.set_hidden(false, now);
    assert_eq!(
        model.next(now),
        Some(TeamRead::Status {
            team: TEAM_A.to_owned()
        }),
        "shown again, the views are read at once"
    );
}

#[test]
fn the_desktop_action_reads_the_view_again() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    watched(&mut model, now, TEAM_A);
    assert_eq!(model.next(now), Some(TeamRead::List { after: None }));
    model.apply(&TeamRead::List { after: None }, Ok(list_reply()), now);
    assert_eq!(model.next(now), Some(TeamRead::Waiting { after: None }));
    model.apply(&TeamRead::Waiting { after: None }, Ok(waiting_reply()), now);
    assert_eq!(model.next(now), None, "all read, the poll is next");
    model.refresh(now);
    assert_eq!(
        model.next(now),
        Some(TeamRead::Status {
            team: TEAM_A.to_owned()
        }),
        "an action reads the team again"
    );
}

#[test]
fn a_tasks_topic_change_reads_the_team_again() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    watched(&mut model, now, TEAM_A);
    model.apply(&TeamRead::List { after: None }, Ok(list_reply()), now);
    model.apply(&TeamRead::Waiting { after: None }, Ok(waiting_reply()), now);
    assert_eq!(model.next(now), None);
    model.tasks_changed(now);
    assert_eq!(
        model.next(now),
        Some(TeamRead::Status {
            team: TEAM_A.to_owned()
        })
    );
    assert_eq!(
        model.next(now),
        Some(TeamRead::Queue {
            team: TEAM_A.to_owned(),
            after: None,
        })
    );
    assert_eq!(
        model.next(now),
        None,
        "a task change does not re-read the list"
    );
}

#[test]
fn a_change_during_a_read_is_not_lost() {
    // A tasks change while the status read is in flight: the reply predates
    // it, so the status is read again at once.
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    let read = model.next(now).expect("the status read");
    assert!(matches!(read, TeamRead::Status { .. }));
    model.tasks_changed(now + Duration::from_secs(1));
    model.apply(&read, Ok(status(TEAM_A)), now + Duration::from_secs(2));
    assert_eq!(
        model.next(now + Duration::from_secs(2)),
        Some(TeamRead::Status {
            team: TEAM_A.to_owned()
        }),
        "a task change during a read forces it again at once"
    );

    // The same for a refresh during the Waiting read.
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: false,
            waiting: true,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the waiting read");
    assert!(matches!(read, TeamRead::Waiting { .. }));
    model.refresh(now + Duration::from_secs(1));
    model.apply(&read, Ok(waiting_reply()), now + Duration::from_secs(2));
    assert_eq!(
        model.next(now + Duration::from_secs(2)),
        Some(TeamRead::Waiting { after: None }),
        "a refresh during a read forces it again at once"
    );
}

#[test]
fn an_unchanged_view_is_not_sent_twice() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    let first = watched(&mut model, now, TEAM_A);
    assert_eq!(first.len(), 1);
    assert_eq!(kinds(&first), ["team"]);
    let read = TeamRead::Status {
        team: TEAM_A.to_owned(),
    };
    let again = model.apply(&read, Ok(status(TEAM_A)), now);
    assert!(again.is_empty(), "the same reply shows nothing new");
    let read = TeamRead::Queue {
        team: TEAM_A.to_owned(),
        after: None,
    };
    let again = model.apply(&read, Ok(queue(TEAM_A)), now);
    assert!(again.is_empty());
}

#[test]
fn the_view_carries_the_cli_queues_contents() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM), now);
    let out = watched(&mut model, now, TEAM);
    let TeamsUpdate::Team(view) = &out[0].1 else {
        panic!("a team view");
    };
    assert_eq!(view.bar.name, "Fixes");
    assert_eq!(view.bar.revision, 16);
    assert_eq!(view.members.len(), 4);
    assert_eq!(view.members[2].state, "not_created");
    assert_eq!(view.groups.len(), GROUPS.len());
    assert_eq!(view.bar.counts.nodes.pending, 1);
    assert_eq!(view.bar.counts.nodes.reported, 1);
    assert_eq!(view.bar.counts.inbox, 1);
    let pending = view
        .groups
        .iter()
        .find(|group| group.state == "pending")
        .expect("the pending group");
    assert_eq!(pending.cards.len(), 1);
    assert_eq!(pending.cards[0].node, 1);
    assert_eq!(pending.cards[0].title, "Task 1");
    assert_eq!(pending.cards[0].rank, "a01");
    let reported = view
        .groups
        .iter()
        .find(|group| group.state == "reported")
        .expect("the reported group");
    assert_eq!(reported.cards[0].node, 2);
    assert_eq!(reported.cards[0].implementer, "m1");
    assert_eq!(view.next, None);
}

#[test]
fn a_failure_is_sent_and_the_view_is_not_ended() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    watched(&mut model, now, TEAM_A);
    let read = TeamRead::Status {
        team: TEAM_A.to_owned(),
    };
    let out = model.apply(
        &read,
        Err(TeamError::Refused {
            code: "teams_inactive".into(),
            message: "Teams are off.".into(),
        }),
        now,
    );
    assert_eq!(keys(&out), [team_key("team", TEAM_A)]);
    assert_eq!(out[0].1.kind(), "failed");
    assert_eq!(
        out[0].1.payload(),
        json!({"code": "teams_inactive", "message": "Teams are off."})
    );
    assert!(!out[0].1.is_view());
    let again = model.apply(
        &read,
        Err(TeamError::Refused {
            code: "teams_inactive".into(),
            message: "Teams are off.".into(),
        }),
        now,
    );
    assert!(again.is_empty());
}

#[test]
fn a_missing_hgs_is_unavailable() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    let read = model.next(now).expect("the status read");
    let out = model.apply(
        &read,
        Err(TeamError::Unavailable {
            message: "hgs could not run here".into(),
        }),
        now,
    );
    assert_eq!(out[0].1.kind(), "failed");
    assert_eq!(out[0].1.payload()["code"], "unavailable");
}

#[test]
fn an_unfollowed_view_ends() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    watched(&mut model, now, TEAM_A);
    let out = model.follow(TeamsFollow::default(), now);
    assert_eq!(keys(&out), [team_key("team", TEAM_A)]);
    assert!(matches!(out[0].1, TeamsUpdate::Ended));
    assert_eq!(model.next(now), None);
    assert!(model.following().team.is_none());
    assert!(model.follow(TeamsFollow::default(), now).is_empty());
}

#[test]
fn a_late_list_or_waiting_reply_cannot_resurrect_its_slot() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            waiting: true,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the list read");
    assert!(matches!(read, TeamRead::List { .. }));
    // The list is no longer followed while its read is in flight.
    model.follow(
        TeamsFollow {
            list: false,
            waiting: true,
            team: None,
        },
        now,
    );
    let late = model.apply(&read, Ok(list_reply()), now);
    assert!(late.is_empty(), "a late list reply shows nothing");
    assert_eq!(
        model.next(now),
        Some(TeamRead::Waiting { after: None }),
        "only the waiting read is still followed"
    );
    model.apply(&TeamRead::Waiting { after: None }, Ok(waiting_reply()), now);
    model.follow(TeamsFollow::default(), now);
    assert_eq!(
        model.next(now + POLL * 2),
        None,
        "an unfollowed view is never polled"
    );

    // The waiting slot has the same check.
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: false,
            waiting: true,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the waiting read");
    model.follow(TeamsFollow::default(), now);
    assert!(model.apply(&read, Ok(waiting_reply()), now).is_empty());
    assert_eq!(model.next(now + POLL * 2), None);
}

#[test]
fn a_page_read_replaces_the_shown_page() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM), now);
    watched(&mut model, now, TEAM);
    let mut first = queue(TEAM);
    first["next"] = json!("t1.first");
    let out = model.apply(
        &TeamRead::Queue {
            team: TEAM.to_owned(),
            after: None,
        },
        Ok(first),
        now,
    );
    let TeamsUpdate::Team(view) = &out[0].1 else {
        panic!("a team view");
    };
    assert_eq!(view.next.as_deref(), Some("t1.first"));
    assert!(model.page(
        "team:00000000-0000-4000-8000-00000000beef",
        Some("t1.first")
    ));
    let read = model.next(now).expect("the page read");
    assert_eq!(
        read,
        TeamRead::Queue {
            team: TEAM.to_owned(),
            after: Some("t1.first".into()),
        }
    );
    let out = model.apply(&read, Ok(queue(TEAM)), now);
    let TeamsUpdate::Team(view) = &out[0].1 else {
        panic!("a team view");
    };
    assert_eq!(view.next, None, "the page replaced the shown one");
    // Only a followed key can ask, and only with a cursor.
    assert!(!model.page("team:00000000-0000-4000-8000-00000000ffff", Some("t1.x")));
    assert!(!model.page("unknown", Some("t1.x")));
    assert!(!model.page("team:00000000-0000-4000-8000-00000000beef", Some("")));
    assert!(!model.page(
        "team:00000000-0000-4000-8000-00000000beef",
        Some("has space")
    ));
    let mut bare = TeamsModel::default();
    assert!(!bare.page("list", Some("t1.x")), "the list is not followed");
    assert!(!bare.page("waiting", Some("t1.x")));
    assert!(!bare.page("team:00000000-0000-4000-8000-00000000beef", Some("t1.x")));
}

#[test]
fn a_page_read_is_bounded_and_once() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM), now);
    for at in 0..QUEUED {
        let cursor = format!("t1.{at}");
        assert!(model.page("team:00000000-0000-4000-8000-00000000beef", Some(&cursor)));
    }
    assert!(!model.page(
        "team:00000000-0000-4000-8000-00000000beef",
        Some("t1.again")
    ));
    assert!(!model.page("team:00000000-0000-4000-8000-00000000beef", Some("t1.0")));
    assert_eq!(model.wait(now), Duration::ZERO);
}

#[test]
fn a_reply_about_a_team_no_longer_watched_gives_nothing() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    let read = TeamRead::Status {
        team: TEAM_A.to_owned(),
    };
    assert_eq!(model.next(now), Some(read.clone()));
    model.follow(follow_all(TEAM_B), now);
    let out = model.apply(&read, Ok(status(TEAM_A)), now);
    assert!(out.is_empty());
    assert_eq!(
        model.next(now),
        Some(TeamRead::Status {
            team: TEAM_B.to_owned()
        })
    );
}

#[test]
fn a_short_wait_never_exceeds_the_poll() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM_A), now);
    watched(&mut model, now, TEAM_A);
    model.apply(&TeamRead::List { after: None }, Ok(list_reply()), now);
    model.apply(&TeamRead::Waiting { after: None }, Ok(waiting_reply()), now);
    assert_eq!(model.wait(now), POLL);
    assert_eq!(model.wait(now + POLL / 2), POLL / 2);
}

#[test]
fn a_selected_page_is_polled_until_another_is_chosen() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the first page");
    model.apply(&read, Ok(list_reply()), now);
    assert!(model.page("list", Some("t1.second")));
    let read = model.next(now).expect("the selected page");
    assert_eq!(
        read,
        TeamRead::List {
            after: Some("t1.second".into())
        }
    );
    model.apply(&read, Ok(list_reply()), now);
    assert_eq!(
        model.next(now + POLL),
        Some(TeamRead::List {
            after: Some("t1.second".into())
        }),
        "the poll reads the selected page, not page one"
    );
    // The poll read above was answered; no cursor then selects the first page.
    model.apply(
        &TeamRead::List {
            after: Some("t1.second".into()),
        },
        Ok(list_reply()),
        now,
    );
    assert!(model.page("list", None));
    assert_eq!(model.next(now + POLL), Some(TeamRead::List { after: None }));
}

#[test]
fn a_refused_page_cursor_returns_to_the_first_page() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the first page");
    model.apply(&read, Ok(list_reply()), now);
    assert!(model.page("list", Some("t1.stale")));
    let read = model.next(now).expect("the selected page");
    let out = model.apply(
        &read,
        Err(TeamError::Refused {
            code: "invalid_state".into(),
            message: "The cursor does not belong to this list.".into(),
        }),
        now,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].1.kind(), "failed");
    assert_eq!(out[0].1.payload()["code"], "cursor_reset");
    assert!(
        out[0].1.payload()["message"]
            .as_str()
            .is_some_and(|message| message.contains("first page")),
        "the view says why the page was dropped"
    );
    assert_eq!(
        model.next(now),
        Some(TeamRead::List { after: None }),
        "the first page is read again at once"
    );
}

#[test]
fn reopening_a_key_during_its_read_retires_the_old_generation() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        },
        now,
    );
    // The first page is shown, then a refresh's read is in flight when the key
    // is unfollowed and followed again.
    let first = model.next(now).expect("the first read");
    model.apply(&first, Ok(list_reply()), now);
    model.refresh(now);
    let old = model.next(now).expect("the refresh read");
    let ended = model.follow(TeamsFollow::default(), now);
    assert!(matches!(ended[0].1, TeamsUpdate::Ended));
    model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        },
        now,
    );
    let late = model.apply(&old, Ok(list_reply()), now);
    assert!(late.is_empty(), "a reply of the retired scope is dropped");
    let fresh = model.next(now).expect("the reopened scope is read");
    assert_eq!(fresh, TeamRead::List { after: None });
    let out = model.apply(&fresh, Ok(list_reply()), now);
    assert_eq!(out.len(), 1, "the new scope publishes its view");
}

#[test]
fn a_refused_cursor_resets_only_the_followed_selected_page() {
    let refused = || TeamError::Refused {
        code: "invalid_state".into(),
        message: "The cursor does not belong to this list.".into(),
    };
    let now = Instant::now();

    // A key no longer followed: the refusal is dropped and nothing is read.
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the first page");
    model.apply(&read, Ok(list_reply()), now);
    assert!(model.page("list", Some("t1.old")));
    let read = model.next(now).expect("the old page");
    model.follow(TeamsFollow::default(), now);
    assert!(model.apply(&read, Err(refused()), now).is_empty());
    assert_eq!(model.next(now), None, "a retired key is not read again");

    // A newer selection: the old refusal neither resets nor is published.
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the first page");
    model.apply(&read, Ok(list_reply()), now);
    assert!(model.page("list", Some("t1.old")));
    let read = model.next(now).expect("the old page");
    assert!(model.page("list", Some("t1.new")));
    assert!(model.apply(&read, Err(refused()), now).is_empty());
    assert_eq!(
        model.next(now),
        Some(TeamRead::List {
            after: Some("t1.new".into())
        }),
        "the person's later choice stands"
    );

    // The current selection of a followed key: reset and mark the view.
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: None,
        },
        now,
    );
    let read = model.next(now).expect("the first page");
    model.apply(&read, Ok(list_reply()), now);
    assert!(model.page("list", Some("t1.bad")));
    let read = model.next(now).expect("the bad page");
    let out = model.apply(&read, Err(refused()), now);
    assert_eq!(out[0].1.payload()["code"], "cursor_reset");
    assert_eq!(model.next(now), Some(TeamRead::List { after: None }));
}

#[test]
fn generations_do_not_accumulate_for_retired_teams() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    for n in 0..100 {
        model.follow(
            TeamsFollow {
                list: false,
                waiting: false,
                team: Some(format!("00000000-0000-4000-8000-{n:012x}")),
            },
            now,
        );
    }
    model.follow(TeamsFollow::default(), now);
    assert!(
        model.generations.len() <= 3,
        "retired team generation entries retained: {}",
        model.generations.len()
    );
}

/// A page names the cursor it was read from (second review, findings 2, 3
/// and 13): the views keep list pages by their cursor and never take a later
/// waiting page for the first one. The first page names none, and its
/// serialized view is unchanged.
#[test]
fn a_page_names_the_cursor_it_was_read_from() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM), now);
    let first = model.apply(&TeamRead::List { after: None }, Ok(list_reply()), now);
    let TeamsUpdate::List(view) = &first[0].1 else {
        panic!("a list update")
    };
    assert_eq!(view.after, None);
    assert!(first[0].1.payload().get("after").is_none());
    assert!(model.page("list", Some("t1.next")));
    let later = model.apply(
        &TeamRead::List {
            after: Some("t1.next".into()),
        },
        Ok(list_reply()),
        now,
    );
    let TeamsUpdate::List(view) = &later[0].1 else {
        panic!("a list update for the later page")
    };
    assert_eq!(view.after.as_deref(), Some("t1.next"));
    assert_eq!(later[0].1.payload()["after"], "t1.next");

    let waiting = model.apply(&TeamRead::Waiting { after: None }, Ok(waiting_reply()), now);
    let TeamsUpdate::Waiting(view) = &waiting[0].1 else {
        panic!("a waiting update")
    };
    assert_eq!(view.after, None);
    assert!(model.page("waiting", Some("w1.next")));
    let later = model.apply(
        &TeamRead::Waiting {
            after: Some("w1.next".into()),
        },
        Ok(waiting_reply()),
        now,
    );
    let TeamsUpdate::Waiting(view) = &later[0].1 else {
        panic!("a later waiting update")
    };
    assert_eq!(view.after.as_deref(), Some("w1.next"));
}

#[test]
fn a_team_view_names_its_queue_page() {
    let now = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_all(TEAM), now);
    let out = watched(&mut model, now, TEAM);
    let (_, TeamsUpdate::Team(view)) = out
        .iter()
        .find(|(key, _)| key == &team_key("team", TEAM))
        .unwrap()
    else {
        panic!("a team view")
    };
    assert_eq!(view.after, None);
    assert!(model.page(&team_key("team", TEAM), Some("t1.q2")));
    let read = TeamRead::Queue {
        team: TEAM.to_owned(),
        after: Some("t1.q2".into()),
    };
    let out = model.apply(&read, Ok(queue(TEAM)), now);
    let TeamsUpdate::Team(view) = &out[0].1 else {
        panic!("a team view of the later page")
    };
    assert_eq!(view.after.as_deref(), Some("t1.q2"));
}
