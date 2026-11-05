//! The Teams read model against real `hgs team ... --json` replies of a running
//! team (sanitized: long texts and local paths are synthetic; every key and the
//! shape are as `hgs` 1.56 printed them), in `tests/fixtures/teams`.
use super::model::{team_key, TeamsFollow, TeamsModel, TeamsUpdate, FEED_KEPT, POLL};
use super::read::{decode, invocation, TeamRead, FEED_PAGE, LIST_LIMIT, WAITING_PAGE};
use super::view;
use crate::actions::Exit;
use crate::batch::Coalescer;
use crate::reads::ReadError;
use crate::{Change, ComputerId};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TEAM: &str = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/teams/team-{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}")),
    )
    .unwrap()
}

fn exit(value: &Value) -> Exit {
    Exit::Code {
        code: 0,
        stdout: value.to_string().into_bytes(),
        stderr: String::new(),
    }
}

fn status() -> TeamRead {
    TeamRead::Status { team: TEAM.into() }
}

#[test]
fn reads_ask_as_qt_does() {
    let peer = ComputerId::from("studio");
    let cases = [
        (
            TeamRead::List,
            json!({"include_cancelled": true, "limit": LIST_LIMIT}),
            "list",
        ),
        (
            status(),
            json!({"team": TEAM, "include_cancelled": true}),
            "status",
        ),
        (
            TeamRead::Events {
                team: TEAM.into(),
                before: None,
            },
            json!({"team": TEAM, "limit": FEED_PAGE}),
            "events",
        ),
        (
            TeamRead::Events {
                team: TEAM.into(),
                before: Some(13318),
            },
            json!({"team": TEAM, "limit": FEED_PAGE, "before": 13318}),
            "events",
        ),
        (
            TeamRead::Inbox { team: TEAM.into() },
            json!({"team": TEAM}),
            "inbox",
        ),
        (
            TeamRead::Waiting { before: None },
            json!({"limit": WAITING_PAGE}),
            "waiting",
        ),
        (
            TeamRead::Waiting {
                before: Some(json!({"since": 5.0, "id": "a"})),
            },
            json!({"limit": WAITING_PAGE, "before": {"since": 5.0, "id": "a"}}),
            "waiting",
        ),
        (
            TeamRead::Spans {
                team: TEAM.into(),
                after: 4,
            },
            json!({"team": TEAM, "after": 4, "limit": 50}),
            "spans",
        ),
        (
            TeamRead::Profile { team: TEAM.into() },
            json!({"team": TEAM}),
            "profile",
        ),
    ];
    for (read, request, op) in cases {
        let local = invocation(&ComputerId::local(), &read).unwrap();
        assert_eq!(local.args, ["team", op, "--json"], "{read:?}");
        assert_eq!(local.timeout, Duration::from_secs(15));
        let sent: Value = serde_json::from_slice(local.stdin.as_deref().unwrap()).unwrap();
        assert_eq!(sent, request, "{read:?}");
        let remote = invocation(&peer, &read).unwrap();
        assert_eq!(remote.args, ["@studio", "team", op, "--json"]);
        assert_eq!(remote.timeout, Duration::from_secs(45));
    }
    // A request names a real team and a real computer, or runs nothing.
    for team in ["", "--all", "two words", &"x".repeat(65)] {
        assert!(matches!(
            invocation(
                &ComputerId::local(),
                &TeamRead::Status { team: team.into() }
            ),
            Err(ReadError::BadRequest { .. })
        ));
    }
    assert!(matches!(
        invocation(&ComputerId::from("-oProxyCommand=x"), &TeamRead::List),
        Err(ReadError::BadRequest { .. })
    ));
}

#[test]
fn replies_are_checked_for_the_shape_each_view_relies_on() {
    for (read, name) in [
        (TeamRead::List, "list"),
        (status(), "status"),
        (
            TeamRead::Events {
                team: TEAM.into(),
                before: None,
            },
            "events",
        ),
        (TeamRead::Inbox { team: TEAM.into() }, "inbox"),
        (TeamRead::Waiting { before: None }, "waiting"),
        (
            TeamRead::Spans {
                team: TEAM.into(),
                after: 0,
            },
            "spans",
        ),
        (TeamRead::Profile { team: TEAM.into() }, "profile"),
        (TeamRead::Catalog, "catalog"),
    ] {
        let value = fixture(name);
        assert_eq!(decode(&read, exit(&value)).as_ref(), Ok(&value), "{name}");
    }
    let invalid = |read: &TeamRead, reply: &str| {
        matches!(
            decode(
                read,
                Exit::Code {
                    code: 0,
                    stdout: reply.into(),
                    stderr: String::new()
                }
            ),
            Err(ReadError::Invalid { .. })
        )
    };
    assert!(invalid(&TeamRead::List, "[]"));
    assert!(invalid(&TeamRead::List, r#"{"items": {}}"#));
    assert!(invalid(&TeamRead::Catalog, r#"{"roles": []}"#));
    assert!(invalid(&status(), r#"{"id": "t", "members": []}"#));
    assert!(invalid(&status(), "not json"));
    assert_eq!(
        decode(
            &status(),
            Exit::Code {
                code: 1,
                stdout: Vec::new(),
                stderr: "hgs: Unknown team\n".into()
            }
        ),
        Err(ReadError::Failed {
            code: Some(1),
            message: "hgs: Unknown team".into()
        })
    );
}

#[test]
fn the_catalog_reply_is_projected_and_its_read_names_the_catalog() {
    let value = fixture("catalog");
    let catalog = view::catalog(&value);
    assert_eq!((catalog.version, catalog.builtin, catalog.draft_base), (3, false, 2));
    assert_eq!(catalog.roles.len(), 2);
    assert_eq!((catalog.roles[1].id.as_str(), catalog.roles[1].model.as_str()), ("tester", "kimi-k2"));
    assert_eq!(catalog.templates[0].max_members, 6);
    assert_eq!(catalog.templates[0].roster[0].role, "tester");
    assert_eq!(catalog.routing[0].r#use, "feature");
    assert_eq!(catalog.pipelines[0].stages[0].kind, "test");
    assert_eq!(catalog.proposals[0].state, "kept");
    assert_eq!(catalog.proposals[0].value["model"], "Sonnet");
    let request = invocation(&ComputerId::local(), &TeamRead::Catalog).unwrap();
    assert_eq!(request.args, ["team", "catalog", "get", "--json"]);
    assert_eq!(request.stdin.as_deref(), Some(&b"{}"[..]));
}

#[test]
fn the_list_projects_each_team_with_its_members_and_counts() {
    let list = view::list(&fixture("list"));
    assert_eq!(list.teams.len(), 3);
    assert!(!list.truncated);
    let team = &list.teams[0];
    assert_eq!(
        (team.id.as_str(), team.name.as_str(), team.status.as_str()),
        (TEAM, "Zerus Next for the fork", "running")
    );
    assert_eq!(
        (
            team.members.len(),
            team.lead.as_str(),
            team.tasks,
            team.accepted,
            team.open_proposals
        ),
        (7, "lead", 4, 8, 1)
    );
    let lead = team.members.iter().find(|member| member.lead).unwrap();
    assert_eq!(
        lead.session.as_ref().unwrap().name,
        "claude/Zerus/work-6926cceb"
    );
    assert!(!team.draft());
    assert_eq!(list.teams[1].status, "paused");
    // A list over the cap keeps the newest LIST_KEPT and says so.
    let items: Vec<Value> = (0..view::LIST_KEPT + 7)
        .map(|n| json!({"id": format!("t{n}"), "name": "T"}))
        .collect();
    let long = view::list(&json!({"items": items}));
    assert_eq!((long.teams.len(), long.truncated), (view::LIST_KEPT, true));
}

#[test]
fn a_status_projects_the_team_tab_and_its_plan_graph() {
    let value = fixture("status");
    let team = view::team(&value);
    assert_eq!(
        (team.id.as_str(), team.status.as_str(), team.lead.as_str()),
        (TEAM, "running", "lead")
    );
    assert_eq!(team.members.len(), 7);
    let lead = &team.members[0];
    assert!(lead.lead && lead.role == "Lead");
    let core = team
        .members
        .iter()
        .find(|member| member.id == "core")
        .unwrap();
    assert_eq!(
        (core.role.as_str(), core.agent.as_str(), core.state.as_str()),
        ("Tauri core (Rust)", "claude", "active")
    );
    assert_eq!(
        core.session.as_ref().unwrap().name,
        "claude/Zerus-team-33fbafeb-core/zerus-next-for-the-fork-core"
    );
    assert_eq!(
        (core.live, core.queue.as_slice(), core.stuck),
        (Some(true), &[6][..], false)
    );
    assert_eq!(team.tasks.len(), 27);
    let task = team.tasks.iter().find(|task| task.id == 6).unwrap();
    assert_eq!(
        (task.status.as_str(), task.assignee.as_str(), task.phase),
        ("in_progress", "core", 1)
    );
    assert_eq!(
        task.deps,
        vec![view::DepView {
            task: 4,
            hard: true
        }]
    );
    assert_eq!(
        task.output,
        ["client/core/src/teams", "client/src-tauri/src"]
    );
    assert_eq!(team.phases.len(), 1);
    assert_eq!(
        (
            team.phases[0].done,
            team.phases[0].total,
            team.phases[0].state.as_str()
        ),
        (8, 27, "running")
    );
    // Only open proposals become cards; each carries the hash an answer names.
    assert_eq!(team.proposals.len(), 1);
    assert_eq!(
        (
            team.proposals[0].id.as_str(),
            team.proposals[0].kind.as_str()
        ),
        ("p4", "add-to-phase")
    );
    assert!(!team.proposals[0].hash.is_empty() && team.proposals[0].change.is_object());
    assert_eq!(team.ready, [2, 19, 23, 26]);
    assert!(team.cycle.is_empty() && team.draft.is_none() && !team.truncated);
    assert_eq!(team.settings["max_members"], 6);
    // The graph: every task in its hgs layer, every dependency an edge.
    let graph = view::graph(&value, &team);
    assert_eq!((graph.nodes.len(), graph.edges.len()), (27, 39));
    let node = graph.nodes.iter().find(|node| node.id == 6).unwrap();
    assert_eq!(
        (node.layer, node.role.as_str(), node.ready),
        (1, "Tauri core (Rust)", false)
    );
    assert!(graph.edges.contains(&view::GraphEdge {
        from: 4,
        to: 6,
        hard: true
    }));
    assert_eq!(
        graph.layers,
        graph.nodes.iter().map(|node| node.layer).max().unwrap() + 1
    );
    assert!(graph
        .nodes
        .iter()
        .filter(|node| node.ready)
        .map(|node| node.id)
        .eq([2, 19, 23, 26]));
    // A running team is no New team form.
    assert!(view::new_team(&value, &team).is_none());
}

#[test]
fn a_draft_projects_the_new_team_form() {
    let value = fixture("status-draft");
    let team = view::team(&value);
    let form = view::new_team(&value, &team).expect("a draft is a form");
    assert_eq!((form.team.as_str(), form.draft.revision), (TEAM, 9));
    assert_eq!(form.draft.unseen, ["members", "goal"]);
    assert_eq!(form.draft.owner_edited, ["settings"]);
    assert_eq!(form.members.len(), 7);
    assert!(form.members[0].lead);
    assert_eq!(form.addendum, "Synthetic lead_protocol text.");
    assert_eq!(form.validation.errors[0].kind, "members");
    assert!(form.validation.errors[0]
        .text
        .contains("over the team limit of 6"));
    assert_eq!(form.validation.warnings.len(), 1);
    assert_eq!(form.tasks.len(), 27);
}

#[test]
fn inbox_waiting_spans_and_profile_project_their_replies() {
    let inbox = view::inbox(TEAM, &fixture("inbox"));
    assert_eq!(inbox.items.len(), 4);
    assert_eq!(inbox.items[0].kind, "build_broker");
    let waiting = view::waiting(&fixture("waiting"));
    assert_eq!(
        (waiting.items.len(), waiting.total, waiting.more),
        (5, 5, false)
    );
    assert_eq!(waiting.items[0].kind, "owner_word_asked");
    assert!(waiting
        .items
        .iter()
        .all(|item| !item.id.is_empty() && !item.kind.is_empty()));
    let spans = view::spans(TEAM, &fixture("spans"));
    assert_eq!(
        (spans.tasks.len(), spans.next, spans.truncated),
        (27, None, false)
    );
    let profile = view::profile(TEAM, &fixture("profile"));
    assert!(profile.profile["losses"].is_object() && profile.profile["summary"].is_object());
}

fn follow_team() -> TeamsFollow {
    TeamsFollow {
        list: true,
        waiting: true,
        team: Some(TEAM.into()),
        catalog: false,
    }
}

/// Runs every due read against the fixtures and applies it.
fn poll_once(
    model: &mut TeamsModel,
    now: Instant,
    reply: impl Fn(&TeamRead) -> Value,
) -> Vec<(String, TeamsUpdate)> {
    let mut out = Vec::new();
    while let Some(read) = model.next(now) {
        out.extend(model.apply(&read, Ok(reply(&read)), now));
    }
    out
}

fn replies(read: &TeamRead) -> Value {
    match read {
        TeamRead::List => fixture("list"),
        TeamRead::Status { .. } => fixture("status"),
        TeamRead::Events { before: None, .. } => fixture("events"),
        TeamRead::Events {
            before: Some(_), ..
        } => fixture("events-older"),
        TeamRead::Inbox { .. } => fixture("inbox"),
        TeamRead::Waiting { .. } => fixture("waiting"),
        TeamRead::Spans { .. } => fixture("spans"),
        TeamRead::Profile { .. } => fixture("profile"),
        TeamRead::Catalog => fixture("catalog"),
        TeamRead::CatalogRoute { .. } => serde_json::json!({"rule": null, "template": null}),
    }
}

#[test]
fn followed_reads_run_every_three_seconds_and_unchanged_replies_send_nothing() {
    let start = Instant::now();
    let mut model = TeamsModel::default();
    assert!(
        model.next(start).is_none(),
        "nothing followed, nothing read"
    );
    assert_eq!(model.wait(start), POLL);
    assert!(model.follow(follow_team(), start).is_empty());
    let first = poll_once(&mut model, start, replies);
    let keys: Vec<&str> = first.iter().map(|(key, _)| key.as_str()).collect();
    for key in [
        "list",
        "waiting",
        &team_key("team", TEAM),
        &team_key("graph", TEAM),
        &team_key("inbox", TEAM),
        &team_key("feed", TEAM),
    ] {
        assert!(keys.contains(&key), "{key} missing from {keys:?}");
    }
    assert!(
        !keys.contains(&team_key("new", TEAM).as_str()),
        "a running team has no form"
    );
    // Each read ran once; nothing is due before three seconds pass.
    assert!(model.next(start + Duration::from_millis(2900)).is_none());
    assert!(model.wait(start + Duration::from_secs(1)) <= Duration::from_secs(2));
    // The same replies again change no view.
    assert!(poll_once(&mut model, start + POLL, replies).is_empty());
    // A changed team sends its views again; an unchanged list does not.
    let later = poll_once(&mut model, start + POLL * 2, |read| match read {
        TeamRead::Status { .. } => {
            let mut value = fixture("status");
            value["tasks"][0]["status"] = json!("accepted");
            value
        }
        other => replies(other),
    });
    let keys: Vec<&str> = later.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, [team_key("team", TEAM), team_key("graph", TEAM)]);
    // A failed read says so once; the view keeps what it showed.
    let mut failing = TeamsModel::default();
    failing.follow(follow_team(), start);
    let read = failing.next(start).unwrap();
    let error = || {
        Err(ReadError::Failed {
            code: Some(1),
            message: "hgs: Unknown team".into(),
        })
    };
    let out = failing.apply(&read, error(), start);
    assert!(
        matches!(&out[..], [(_, TeamsUpdate::Failed { message, .. })] if message == "hgs: Unknown team")
    );
    // Every followed read fails once and says so...
    while let Some(read) = failing.next(start) {
        assert_eq!(failing.apply(&read, error(), start).len(), 1, "{read:?}");
    }
    // ...and the same failures again send nothing.
    let mut again = 0;
    while let Some(read) = failing.next(start + POLL) {
        again += 1;
        assert!(
            failing.apply(&read, error(), start + POLL).is_empty(),
            "the same failure is not sent twice: {read:?}"
        );
    }
    assert_eq!(again, 5);
}

/// Task 8: the reads a moved revision makes due, in order.
fn due_reads(model: &mut TeamsModel, now: Instant) -> Vec<String> {
    let mut names = Vec::new();
    while let Some(read) = model.next(now) {
        names.push(match &read {
            TeamRead::List => "list".to_owned(),
            TeamRead::Waiting { .. } => "waiting".to_owned(),
            TeamRead::Status { .. } => "status".to_owned(),
            TeamRead::Inbox { .. } => "inbox".to_owned(),
            TeamRead::Events { .. } => "feed".to_owned(),
            other => format!("{other:?}"),
        });
        model.apply(&read, Ok(replies(&read)), now);
    }
    names.sort();
    names
}

#[test]
fn with_the_streams_team_heads_a_team_is_read_when_its_revision_moved() {
    use std::sync::Arc;
    let heads = |pairs: &[(&str, i64)]| {
        Some(Arc::new(
            pairs
                .iter()
                .map(|(id, rev)| ((*id).to_owned(), *rev))
                .collect::<super::Heads>(),
        ))
    };
    let start = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_team(), start);
    assert_eq!(
        due_reads(&mut model, start),
        ["feed", "inbox", "list", "status", "waiting"]
    );
    // The stream's heads arrive: everything is read once more, then nothing
    // is due after three seconds; a safety read comes after a minute.
    model.heads(heads(&[(TEAM, 5), ("other", 1)]), start);
    assert!(model.streamed());
    assert_eq!(due_reads(&mut model, start).len(), 5);
    assert!(
        due_reads(&mut model, start + POLL * 3).is_empty(),
        "no polling while the stream gives heads"
    );
    // Another team changed: only the list and Waiting are read.
    model.heads(heads(&[(TEAM, 5), ("other", 2)]), start + POLL * 4);
    assert_eq!(due_reads(&mut model, start + POLL * 4), ["list", "waiting"]);
    // The watched team changed: its reads and the list.
    model.heads(heads(&[(TEAM, 6), ("other", 2)]), start + POLL * 5);
    assert_eq!(
        due_reads(&mut model, start + POLL * 5),
        ["feed", "inbox", "list", "status", "waiting"]
    );
    // A change while a read runs reads again at its reply, not a minute later.
    let at = start + POLL * 6;
    model.heads(heads(&[(TEAM, 7), ("other", 2)]), at);
    let running = model.next(at).unwrap();
    assert_eq!(running, TeamRead::List);
    model.heads(heads(&[(TEAM, 8), ("other", 2)]), at);
    model.apply(&running, Ok(replies(&running)), at);
    assert_eq!(
        due_reads(&mut model, at),
        ["feed", "inbox", "list", "status", "waiting"],
        "the list again, at once"
    );
    assert_eq!(model.next(at + POLL), None);
    // The safety read after a minute.
    assert_eq!(
        due_reads(&mut model, at + super::STREAM_POLL + POLL).len(),
        5
    );
    // The stream lost its heads (a gap, an older peer): polling again.
    model.heads(None, at + super::STREAM_POLL * 2);
    assert!(!model.streamed());
    assert_eq!(due_reads(&mut model, at + super::STREAM_POLL * 2).len(), 5);
    assert_eq!(
        due_reads(&mut model, at + super::STREAM_POLL * 2 + POLL).len(),
        5,
        "every three seconds"
    );
}

#[test]
fn a_new_follow_ends_the_views_it_no_longer_shows_and_drops_late_replies() {
    let start = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(follow_team(), start);
    poll_once(&mut model, start, replies);
    let in_flight = model.next(start + POLL).unwrap();
    // The tab moves to another team: the old team's views end, once each.
    let ended = model.follow(
        TeamsFollow {
            list: true,
            waiting: false,
            team: Some("other".into()),
            catalog: false,
        },
        start + POLL,
    );
    let mut keys: Vec<String> = ended
        .iter()
        .map(|(key, update)| {
            assert_eq!(update, &TeamsUpdate::Ended);
            key.clone()
        })
        .collect();
    keys.sort();
    let mut expected = vec!["waiting".to_owned()];
    expected.extend(["feed", "graph", "inbox", "team"].map(|kind| team_key(kind, TEAM)));
    expected.sort();
    assert_eq!(keys, expected);
    // A reply about the team no longer watched gives nothing.
    if in_flight.team().is_some() {
        assert!(model
            .apply(&in_flight, Ok(replies(&in_flight)), start + POLL)
            .is_empty());
    }
    // The new team's reads are due at once.
    let next = model.next(start + POLL).unwrap();
    assert!(
        next.team().is_none() || next.team() == Some("other"),
        "{next:?}"
    );
}

/// A long journal: Load older adds pages until FEED_KEPT and then stops; the
/// feed never holds more.
#[test]
fn a_long_journal_stays_within_the_feed_cap() {
    let start = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            team: Some(TEAM.into()),
            ..TeamsFollow::default()
        },
        start,
    );
    let page = |newest: i64| {
        let items: Vec<Value> = (0..FEED_PAGE as i64)
            .map(
                |n| json!({"seq": newest - n, "at": 1.0, "kind": "started", "detail": {"task": n}}),
            )
            .collect();
        json!({"items": items, "next_cursor": newest - FEED_PAGE as i64 + 1})
    };
    let mut newest = 1_000_000;
    poll_once(&mut model, start, |read| match read {
        TeamRead::Events { .. } => page(newest),
        other => replies(other),
    });
    assert_eq!(model.feed_len(), FEED_PAGE as usize);
    let mut cursor = newest - FEED_PAGE as i64 + 1;
    let mut asked = 0;
    while model.request_older() {
        asked += 1;
        let read = model.next(start).unwrap();
        assert_eq!(
            read,
            TeamRead::Events {
                team: TEAM.into(),
                before: Some(cursor)
            }
        );
        let reply = page(cursor - 1);
        cursor -= FEED_PAGE as i64;
        let out = model.apply(&read, Ok(reply), start);
        let TeamsUpdate::Feed(feed) = &out.last().unwrap().1 else {
            panic!("{out:?}")
        };
        assert!(feed.events.len() <= FEED_KEPT);
        assert!(
            feed.events.windows(2).all(|pair| pair[0].seq > pair[1].seq),
            "newest first"
        );
        assert!(asked < 100, "Load older must stop");
    }
    assert_eq!(model.feed_len(), FEED_KEPT);
    // New events push the oldest out; the feed stays at its cap and says so.
    newest += 20;
    let out = poll_once(&mut model, start + POLL, |read| match read {
        TeamRead::Events { .. } => page(newest),
        other => replies(other),
    });
    let feed = out
        .iter()
        .find_map(|(_, update)| match update {
            TeamsUpdate::Feed(feed) => Some(feed.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (feed.events.len(), feed.capped, feed.more),
        (FEED_KEPT, true, false)
    );
    assert_eq!(feed.events[0].seq, newest);
    assert!(!model.request_older());
    // More than a page since the last read: the feed starts over from the
    // newest page instead of holding a hole.
    newest += 10_000;
    poll_once(&mut model, start + POLL * 2, |read| match read {
        TeamRead::Events { .. } => page(newest),
        other => replies(other),
    });
    assert_eq!(model.feed_len(), FEED_PAGE as usize);
    assert!(model.request_older());
}

#[test]
fn the_real_feed_pages_join_and_load_older_names_the_cursor() {
    let start = Instant::now();
    let mut model = TeamsModel::default();
    model.follow(
        TeamsFollow {
            team: Some(TEAM.into()),
            ..TeamsFollow::default()
        },
        start,
    );
    poll_once(&mut model, start, replies);
    assert_eq!(model.feed_len(), 50);
    assert!(model.request_older());
    assert!(!model.request_older(), "one older page at a time");
    let read = model.next(start).unwrap();
    assert_eq!(
        read,
        TeamRead::Events {
            team: TEAM.into(),
            before: Some(13318)
        }
    );
    let out = model.apply(&read, Ok(fixture("events-older")), start);
    let TeamsUpdate::Feed(feed) = &out[0].1 else {
        panic!("{out:?}")
    };
    assert_eq!(
        (
            feed.events.len(),
            feed.events[0].seq,
            feed.events[99].seq,
            feed.more
        ),
        (100, 13367, 13268, true)
    );
    assert_eq!(feed.events[0].kind, "started");
    assert!(feed.events[0].detail.is_object());
}

#[test]
fn spans_and_profile_are_read_on_request_for_the_watched_team_only() {
    let start = Instant::now();
    let mut model = TeamsModel::default();
    assert!(
        !model.request_spans(0) && !model.request_profile(),
        "no team watched"
    );
    model.follow(
        TeamsFollow {
            team: Some(TEAM.into()),
            ..TeamsFollow::default()
        },
        start,
    );
    poll_once(&mut model, start, replies);
    assert!(model.request_spans(0) && model.request_profile());
    let out = poll_once(&mut model, start, replies);
    let keys: Vec<&str> = out.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(keys, [team_key("spans", TEAM), team_key("profile", TEAM)]);
}

/// The poller runs the reads off the calling thread and the coalescer folds
/// its changes to one entry per key, each carrying a projected view.
#[test]
fn the_poller_feeds_one_batch_entry_per_view() {
    let (tx, rx) = mpsc::channel();
    let poller = super::TeamsPoller::spawn_with(
        ComputerId::local(),
        Box::new(|read: &TeamRead| Ok(replies(read))),
        Box::new(move |change| {
            let _ = tx.send(change);
        }),
    )
    .unwrap();
    poller.follow(follow_team());
    let mut coalescer = Coalescer::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut keys = std::collections::BTreeSet::new();
    while keys.len() < 6 && Instant::now() < deadline {
        if let Ok(change) = rx.recv_timeout(Duration::from_millis(200)) {
            if let Change::Teams { key, .. } = &change {
                keys.insert(key.clone());
            }
            coalescer.push(change);
        }
    }
    assert_eq!(keys.len(), 6, "{keys:?}");
    let batch = coalescer.take();
    assert_eq!(batch.teams.len(), 6);
    let team = batch
        .teams
        .iter()
        .find(|out| out.key == team_key("team", TEAM))
        .unwrap();
    assert_eq!(team.kind, "team");
    // The payload is the view model, not the hgs reply.
    assert_eq!(team.payload["members"][1]["id"], "core");
    assert!(team.payload.get("health").is_some() && team.payload.get("phase_summary").is_none());
    let graph = batch.teams.iter().find(|out| out.kind == "graph").unwrap();
    assert_eq!(graph.payload["nodes"].as_array().unwrap().len(), 27);
    // Leaving the team ends its views through the same path.
    poller.follow(TeamsFollow::default());
    let mut ended = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while ended < 6 && Instant::now() < deadline {
        if let Ok(Change::Teams {
            update: TeamsUpdate::Ended,
            ..
        }) = rx.recv_timeout(Duration::from_millis(200))
        {
            ended += 1;
        }
    }
    assert_eq!(ended, 6);
    drop(poller);
}

/// A failure never replaces a view the batch has not sent yet.
#[test]
fn a_failure_keeps_a_pending_view_in_the_batch() {
    let mut coalescer = Coalescer::new();
    let team = view::team(&fixture("status"));
    let key = team_key("team", TEAM);
    let change = |update| Change::Teams {
        computer: ComputerId::local(),
        key: key.clone(),
        update,
    };
    coalescer.push(change(TeamsUpdate::Team(std::sync::Arc::new(team))));
    coalescer.push(change(TeamsUpdate::Failed {
        code: "timed_out".into(),
        message: "late".into(),
    }));
    let batch = coalescer.take();
    assert_eq!(
        (batch.teams.len(), batch.teams[0].kind.as_str()),
        (1, "team")
    );
    coalescer.push(change(TeamsUpdate::Failed {
        code: "timed_out".into(),
        message: "late".into(),
    }));
    let batch = coalescer.take();
    assert_eq!(
        batch.teams[0].payload,
        json!({"code": "timed_out", "message": "late"})
    );
    assert!(coalescer.take().is_empty());
}
