use serde_json::json;
use std::sync::Arc;
use zerus_client_core::swarm::grouping::{ArchivedRow, Grouping};
use zerus_client_core::swarm::layout::{Layout, MAX_MEMBERS};
use zerus_client_core::swarm::{archive_member, members, INBOX};
use zerus_client_core::{Change, ComputerId, ViewRow};

fn present(grouping: &mut Grouping, alpha: &[&str], beta: &[&str], revision: u64) {
    let org = json!({"default_project": INBOX, "projects": [
        {"id": "alpha", "name": "Alpha", "sessions": alpha},
        {"id": "beta", "name": "Beta", "sessions": beta},
        {"id": INBOX, "name": "Inbox", "sessions": []}
    ]});
    grouping.present(&org, Arc::new(members(&org)), revision);
}

fn archived(machine: &str, id: &str, run: &str) -> ArchivedRow {
    ArchivedRow {
        computer: ComputerId::local(),
        key: format!("archive:{id}"),
        machine: machine.into(),
        archive_id: id.into(),
        run_id: run.into(),
    }
}

fn live(machine: &str, name: &str, run: &str) -> Change {
    Change::Upsert {
        computer: ComputerId::local(),
        key: format!("run:{run}"),
        row: ViewRow {
            machine: machine.into(),
            name: name.into(),
            state: "running".into(),
            run_id: Some(run.into()),
            ..ViewRow::default()
        },
    }
}

fn saved(grouping: &mut Grouping) -> Layout {
    Layout::from_json(&grouping.take_layout().expect("layout changed"))
}

#[test]
fn an_archived_row_takes_its_listed_archive_identity_and_local_rank() {
    let first = archive_member("workstation", "first");
    let second = archive_member("workstation", "second");
    assert_eq!(first, "workstation\narchive\nfirst");
    let mut layout = Layout::default();
    layout.remember_run("workstation", "run", Some("beta"));
    layout.place("alpha", &[second.clone(), first.clone()], None);
    let mut grouping = Grouping::new(layout);
    present(&mut grouping, &[&first, &second], &[], 1);
    grouping.take();
    let rows = [
        archived("workstation", "first", "run"),
        archived("workstation", "second", "run"),
    ];
    let page = grouping.place_archived(&rows);
    assert_eq!(page.len(), 2);
    assert!(page.iter().all(|row| row.group == "alpha"));
    assert!(
        page[1].rank < page[0].rank,
        "local order wins over page order"
    );
    assert_eq!(page[0].computer, rows[0].computer);
    assert_eq!(page[0].key, rows[0].key);
    assert_eq!(grouping.place_archived(&rows), page, "repeat keeps ranks");
    assert!(
        grouping.take().0.is_empty(),
        "archive pages add no live rows"
    );
    assert!(grouping.members_of("alpha").is_empty());
}

#[test]
fn an_unlisted_archive_takes_its_runs_project_then_the_default() {
    let mut layout = Layout::default();
    layout.remember_run("workstation", "run", Some("beta"));
    let mut grouping = Grouping::new(layout);
    present(&mut grouping, &[], &[], 1);
    let rows = [
        archived("workstation", "a", "run"),
        archived("studio", "b", "run"),
        archived("workstation", "c", ""),
    ];
    let groups = grouping.place_archived(&rows);
    assert_eq!(
        groups.iter().map(|g| g.group.as_str()).collect::<Vec<_>>(),
        ["beta", INBOX, INBOX]
    );
    let org = json!({"default_project": "alpha", "projects": [{"id": "alpha", "sessions": []}]});
    grouping.present(&org, Arc::new(members(&org)), 2);
    assert!(
        grouping
            .place_archived(&rows)
            .iter()
            .all(|g| g.group == "alpha"),
        "a deleted remembered project falls to the current default"
    );
    assert_eq!(
        saved(&mut grouping).run_project("workstation", "run"),
        Some("beta"),
        "archive reads never overwrite live run memory"
    );
}

#[test]
fn run_memory_follows_live_rows_on_observe_and_present_and_forgets_implicit_ones() {
    let mut grouping = Grouping::default();
    let member = "workstation\ncodex/sample-project/review";
    grouping.observe(&live("workstation", "codex/sample-project/review", "run"));
    assert!(
        grouping.take_layout().is_none(),
        "wait for the organization before recording membership"
    );
    present(&mut grouping, &[member], &[], 1);
    assert_eq!(
        saved(&mut grouping).run_project("workstation", "run"),
        Some("alpha")
    );
    present(&mut grouping, &[], &[member], 2);
    assert_eq!(
        saved(&mut grouping).run_project("workstation", "run"),
        Some("beta")
    );
    grouping.observe(&live("workstation", "codex/sample-project/review", "run"));
    assert!(
        grouping.take_layout().is_none(),
        "an unchanged row does not refresh the written age"
    );
    present(&mut grouping, &[], &[], 3);
    assert_eq!(saved(&mut grouping).run_project("workstation", "run"), None);
    present(&mut grouping, &[member], &[], 4);
    grouping.observe(&live("workstation", "codex/sample-project/review", "next"));
    assert_eq!(
        saved(&mut grouping).run_project("workstation", "next"),
        Some("alpha"),
        "a new run arriving after present is remembered too"
    );
    grouping.observe(&live("workstation", "codex/sample-project/review", "run"));
    assert!(
        grouping.take_layout().is_none(),
        "an older unchanged run is not rewritten as newest either"
    );
    grouping.replace_runs(zerus_client_core::swarm::runs::Runs::default());
    assert!(
        grouping.take_layout().is_none(),
        "loading rename tokens preserves actual slot runs and their project ages"
    );
    grouping.observe(&Change::Remove {
        computer: ComputerId::local(),
        key: "run:next".into(),
        reason: Some("archived".into()),
    });
    assert_eq!(
        grouping.place_archived(&[archived("workstation", "a", "next")])[0].group,
        "alpha",
        "leaving the live list keeps the last assignment"
    );
    present(&mut grouping, &[], &[], 5);
    let layout = saved(&mut grouping);
    assert_eq!(
        layout.run_project("workstation", "run"),
        None,
        "token startup reload kept the live slot run for later membership changes"
    );
    assert_eq!(
        layout.run_project("workstation", "next"),
        Some("alpha"),
        "the completed run retains its historical assignment"
    );
}

#[test]
fn loading_a_seed_reobserves_current_live_membership() {
    let mut grouping = Grouping::default();
    grouping.observe(&live("workstation", "codex/sample-project/review", "run"));
    present(&mut grouping, &[], &[], 1);
    let mut seeded = Layout::default();
    seeded.remember_run("workstation", "run", Some("alpha"));
    seeded.remember_run("workstation", "old-run", Some("beta"));
    grouping.replace_layout(seeded);
    let layout = saved(&mut grouping);
    assert_eq!(
        layout.run_project("workstation", "run"),
        None,
        "a slow seed cannot restore a live implicit assignment"
    );
    assert_eq!(
        layout.run_project("workstation", "old-run"),
        Some("beta"),
        "historical runs survive"
    );
}

#[test]
fn a_worker_seed_merges_with_live_layout_changes_and_applies_once() {
    let mut grouping = Grouping::default();
    present(
        &mut grouping,
        &["workstation\ncodex/sample-project/review"],
        &[],
        1,
    );
    grouping.observe(&live(
        "workstation",
        "codex/sample-project/review",
        "native",
    ));
    grouping.observe(&live(
        "workstation",
        "codex/sample-project/implicit",
        "implicit",
    ));
    grouping.set_collapsed("alpha", true);
    grouping.seed_runs(json!({"workstation\nnative":"beta", "workstation\nimplicit":"beta", "workstation\nhistorical":"beta"}).as_object().unwrap());
    let layout = saved(&mut grouping);
    assert!(layout.runs_seeded());
    assert!(layout.collapsed("alpha"));
    assert_eq!(layout.run_project("workstation", "native"), Some("alpha"));
    assert_eq!(layout.run_project("workstation", "implicit"), None);
    assert_eq!(
        layout.run_project("workstation", "historical"),
        Some("beta")
    );
    grouping.seed_runs(json!({"workstation\nlate":"alpha"}).as_object().unwrap());
    assert!(
        grouping.take_layout().is_none(),
        "the seed is not reapplied"
    );
}

#[test]
fn archived_or_runless_catalog_rows_do_not_change_live_run_memory() {
    let mut layout = Layout::default();
    layout.remember_run("workstation", "run", Some("beta"));
    let mut grouping = Grouping::new(layout);
    present(&mut grouping, &[], &[], 1);
    let mut change = live("workstation", "codex/sample-project/review", "run");
    if let Change::Upsert { row, .. } = &mut change {
        row.state = "archived".into();
        row.archive_id = Some("archive".into());
    }
    grouping.observe(&change);
    let mut no_run = live("workstation", "codex/sample-project/review", "runless");
    if let Change::Upsert { row, .. } = &mut no_run {
        row.run_id = None;
    }
    grouping.observe(&no_run);
    let saved = saved(&mut grouping);
    assert_eq!(saved.run_project("workstation", "run"), Some("beta"));
    assert_eq!(saved.run_project("workstation", "runless"), None);
}

#[test]
fn archive_placement_is_bounded_by_one_page_and_prunes_old_cached_ranks() {
    let mut layout = Layout::default();
    for n in 0..MAX_MEMBERS + 1024 {
        layout.rank(INBOX, &format!("workstation\narchive\nold-{n}"));
    }
    let mut grouping = Grouping::new(layout);
    // No presented organization: archive reads still use the Inbox. The page
    // itself must perform pruning, even while no live catalog changes arrive.
    let rows: Vec<_> = (0..60)
        .map(|n| archived("workstation", &format!("new-{n}"), ""))
        .collect();
    let page = grouping.place_archived(&rows);
    assert_eq!(page.len(), zerus_client_core::reads::ARCHIVE_PAGE_ROWS);
    assert_eq!(
        saved(&mut grouping).len(),
        page.len(),
        "only the current page remains protected"
    );
    assert_eq!(
        grouping.place_archived(&rows),
        page,
        "pruning preserves the page's ranks"
    );
    assert!(
        grouping.take().0.is_empty(),
        "no retained archive row cache"
    );
}

#[test]
fn pruning_archive_ranks_keeps_live_and_explicit_members_as_well_as_the_page() {
    let mut grouping = Grouping::default();
    let listed = archive_member("workstation", "listed");
    present(&mut grouping, &[&listed], &[], 1);
    grouping.observe(&live("workstation", "codex/sample-project/review", "run"));
    let mut layout = Layout::default();
    for n in 0..MAX_MEMBERS + 1024 {
        layout.rank(INBOX, &format!("workstation\narchive\nold-{n}"));
    }
    layout.rank("alpha", &listed);
    grouping.replace_layout(layout);
    let page = grouping.place_archived(&[archived("workstation", "page", "")]);
    let layout = saved(&mut grouping);
    assert_eq!(
        layout.len(),
        3,
        "one live, one explicit archive, one page identity"
    );
    assert_eq!(
        grouping.place_archived(&[archived("workstation", "listed", "")])[0].group,
        "alpha"
    );
    assert_eq!(
        grouping.place_archived(&[archived("workstation", "page", "")]),
        page
    );
    assert_eq!(grouping.members_of(INBOX).len(), 1);
}

#[test]
fn loaded_archive_pages_follow_project_and_order_edits_without_repeating_unchanged_rows() {
    use zerus_client_core::swarm::edit::Intent;
    let computer = ComputerId::local();
    let a = archive_member("workstation", "a");
    let b = archive_member("workstation", "b");
    let mut grouping = Grouping::default();
    present(&mut grouping, &[&a, &b], &[], 1);
    grouping.take();
    let reply = grouping
        .retain_archive_page(
            &computer,
            17,
            &[
                archived("workstation", "a", ""),
                archived("workstation", "b", ""),
            ],
        )
        .unwrap();
    assert!(
        grouping.take().0.is_empty(),
        "the reply itself is not a delta"
    );
    present(&mut grouping, &[&b], &[&a], 2);
    let delta = grouping.take().0;
    assert_eq!(delta.len(), 1);
    assert_eq!(
        (&*delta[0].key, &*delta[0].group, delta[0].archive_request),
        ("archive:a", "beta", Some(17))
    );
    let a_rank = delta[0].rank;
    assert_eq!(
        reply[0].group, "alpha",
        "the earlier reply snapshot remains immutable"
    );
    grouping.edited(&Intent::Move {
        members: vec![b.clone()],
        project: "beta".into(),
        before: Some(a.clone()),
    });
    present(&mut grouping, &[], &[&a, &b], 3);
    assert!(grouping
        .take()
        .0
        .iter()
        .any(|place| place.key == "archive:b" && place.group == "beta"));
    grouping.edited(&Intent::Sort {
        project: "beta".into(),
        members: vec![a.clone(), b.clone()],
    });
    present(&mut grouping, &[], &[&a, &b], 4);
    let delta = grouping.take().0;
    assert_eq!(
        delta.len(),
        1,
        "the first member already had its sorted rank"
    );
    assert_eq!(delta[0].key, "archive:b");
    assert_eq!(delta[0].archive_request, Some(17));
    assert!(delta[0].rank > a_rank);
    present(&mut grouping, &[], &[&a, &b], 5);
    assert!(grouping.take().0.is_empty());
    assert!(
        grouping.members_of("beta").is_empty(),
        "identities never become live sessions"
    );
}

#[test]
fn loaded_archive_pages_follow_default_and_delayed_run_memory_changes() {
    let computer = ComputerId::local();
    let mut grouping = Grouping::default();
    present(&mut grouping, &[], &[], 1);
    grouping.take();
    let reply = grouping
        .retain_archive_page(&computer, 1, &[archived("workstation", "a", "run")])
        .unwrap();
    assert_eq!(reply[0].group, INBOX);
    let org =
        json!({"default_project":"beta", "projects":[{"id":"alpha"},{"id":"beta"},{"id":INBOX}]});
    grouping.present(&org, Arc::new(members(&org)), 2);
    assert_eq!(grouping.take().0[0].group, "beta");
    grouping.seed_runs(json!({"workstation\nrun":"alpha"}).as_object().unwrap());
    assert_eq!(grouping.take().0[0].group, "alpha");
    // A newly observed implicit live assignment forgets the remembered project.
    grouping.observe(&live("workstation", "codex/sample-project/review", "run"));
    let delta = grouping.take().0;
    assert_eq!(
        delta.iter().find(|p| p.key == "archive:a").unwrap().group,
        "beta"
    );
    assert!(delta
        .iter()
        .find(|p| p.key == "run:run")
        .unwrap()
        .archive_request
        .is_none());
}

#[test]
fn archive_reply_replacement_and_reset_reject_stale_requests_and_old_dirty_deltas() {
    let computer = ComputerId::local();
    let mut grouping = Grouping::default();
    present(&mut grouping, &[], &[], 1);
    grouping.take();
    grouping
        .retain_archive_page(&computer, 8, &[archived("workstation", "old", "")])
        .unwrap();
    let old = archive_member("workstation", "old");
    present(&mut grouping, &[&old], &[], 2);
    grouping
        .retain_archive_page(&computer, 10, &[archived("workstation", "new", "")])
        .unwrap();
    assert!(grouping
        .retain_archive_page(&computer, 9, &[archived("workstation", "stale", "")])
        .is_none());
    assert!(
        grouping.take().0.is_empty(),
        "a replaced page's unsent delta is discarded"
    );
    let new = archive_member("workstation", "new");
    present(&mut grouping, &[], &[&new], 3);
    let delta = grouping.take().0;
    assert_eq!(delta.len(), 1);
    assert_eq!(
        (&*delta[0].key, delta[0].archive_request),
        ("archive:new", Some(10))
    );
    grouping.reset_archive_pages(9);
    present(&mut grouping, &[&new], &[], 4);
    assert_eq!(
        grouping.take().0[0].archive_request,
        Some(10),
        "an older reset preserves a newer reply"
    );
    grouping.reset_archive_pages(10);
    present(&mut grouping, &[], &[&new], 5);
    assert!(grouping.take().0.is_empty());
    assert!(grouping.retain_archive_page(&computer, 10, &[]).is_none());
    assert!(grouping.retain_archive_page(&computer, 11, &[]).is_some());
}

#[test]
fn archive_identity_memory_is_one_fifty_row_page_per_computer_and_ends_with_its_source() {
    let mut grouping = Grouping::default();
    present(&mut grouping, &[], &[], 1);
    grouping.take();
    let rows: Vec<_> = (0..60)
        .map(|n| archived("workstation", &format!("a{n}"), ""))
        .collect();
    assert_eq!(
        grouping
            .retain_archive_page(&ComputerId::local(), 1, &rows)
            .unwrap()
            .len(),
        50
    );
    let peer = ComputerId::from("studio");
    let mut peer_row = archived("studio", "peer", "");
    peer_row.computer = peer.clone();
    grouping.retain_archive_page(&peer, 2, &[peer_row]).unwrap();
    let org = json!({"default_project":"alpha","projects":[{"id":"alpha"},{"id":INBOX}]});
    grouping.present(&org, Arc::new(members(&org)), 2);
    grouping.observe(&Change::Computer {
        computer: ComputerId::local(),
        status: zerus_client_core::ComputerStatus::Ended,
    });
    let delta = grouping.take().0;
    assert_eq!(
        delta.len(),
        1,
        "an ended computer loses even its pending deltas"
    );
    assert_eq!(delta[0].computer, peer);
    let org = json!({"default_project":INBOX,"projects":[{"id":"alpha"},{"id":INBOX}]});
    grouping.present(&org, Arc::new(members(&org)), 3);
    assert_eq!(grouping.take().0.len(), 1);
    // Replacing a page with an empty reply forgets its old identities too.
    grouping.retain_archive_page(&peer, 3, &[]).unwrap();
    grouping.present(&json!({"projects":[{"id":"alpha"}]}), Arc::default(), 4);
    assert!(grouping.take().0.is_empty());
}

#[test]
fn archive_request_is_additive_and_live_group_json_stays_unchanged() {
    let old = json!({"computer":"", "key":"run:r", "group":"alpha", "rank":5});
    let place: zerus_client_core::GroupOut = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(place.archive_request, None);
    assert_eq!(serde_json::to_value(place).unwrap(), old);
}

#[test]
fn evicting_another_computers_oldest_run_replaces_its_loaded_archive_placement() {
    let mut layout = Layout::default();
    layout.remember_run("workstation", "old", Some("alpha"));
    for n in 1..zerus_client_core::swarm::layout::MAX_RUNS {
        layout.remember_run("workstation", &format!("run-{n}"), Some("alpha"));
    }
    let mut grouping = Grouping::new(layout);
    let member = "studio\ncodex/sample-project/new";
    present(&mut grouping, &[], &[member], 1);
    grouping.take();
    assert_eq!(
        grouping
            .retain_archive_page(
                &ComputerId::local(),
                1,
                &[archived("workstation", "a", "old")]
            )
            .unwrap()[0]
            .group,
        "alpha"
    );
    let mut change = live("studio", "codex/sample-project/new", "new");
    if let Change::Upsert { computer, .. } = &mut change {
        *computer = ComputerId::from("studio");
    }
    grouping.observe(&change);
    let delta = grouping.take().0;
    assert_eq!(
        delta.iter().find(|p| p.key == "archive:a").unwrap().group,
        INBOX
    );
}

#[test]
fn pruning_on_a_quiet_catalog_protects_every_retained_computers_page() {
    let mut grouping = Grouping::default();
    present(&mut grouping, &[], &[], 1);
    grouping.take();
    grouping
        .retain_archive_page(&ComputerId::local(), 1, &[archived("workstation", "a", "")])
        .unwrap();
    let peer = ComputerId::from("studio");
    let mut row = archived("studio", "b", "");
    row.computer = peer.clone();
    grouping.retain_archive_page(&peer, 2, &[row]).unwrap();
    let mut layout = saved(&mut grouping);
    for n in 0..MAX_MEMBERS + 1024 {
        layout.rank(INBOX, &format!("workstation\narchive\nold-{n}"));
    }
    grouping.replace_layout(layout);
    grouping.place_archived(&[archived("workstation", "temporary", "")]);
    assert_eq!(saved(&mut grouping).len(), 3);
    let org = json!({"default_project":"alpha", "projects":[{"id":"alpha"},{"id":INBOX}]});
    grouping.present(&org, Arc::new(members(&org)), 2);
    let delta = grouping.take().0;
    assert_eq!(delta.len(), 2);
    assert!(delta
        .iter()
        .any(|p| p.computer == peer && p.key == "archive:b"));
}
