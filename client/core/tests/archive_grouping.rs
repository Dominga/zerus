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
