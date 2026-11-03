use serde_json::json;
use std::collections::BTreeSet;
use zerus_client_core::{swarm, worktree_overview::*};
fn source() -> Source {
    plan(&json!({"projects":[{"id":"a","name":"Alpha","folders":[{"id":"f","machine":"workstation","path":"/srv/example"}]}]}),"workstation",false).sources.remove(0)
}
fn row(index: usize, category: u8) -> Row {
    let source = source();
    let raw = json!({"path":format!("/srv/example/w-{index:05}"),"branch":format!("branch-{index}"),"kind":"linked","available":true,"verdict":"ready","fingerprint":"proof","disk_bytes":index,"disk_complete":true,"last_activity":index,"sessions":[],"integration":{"kind":"ancestor","base":"main"}});
    let mut row = project("scope", 0, &source, &raw, Some(&raw), true, 1000).unwrap();
    row.category = category;
    row
}
#[test]
fn source_plan_reuses_original_folder_decoder_and_groups_links_without_writing() {
    let original = json!({"projects":[{"id":"a","name":"Alpha","folders":[{"machine":"workstation","path":"/srv/example","name":"","unknown":"kept"}]},{"id":"b","name":"Beta","folders":[{"id":"other","machine":"workstation","path":"/srv/example"}]},{"id":"hidden","name":"Hidden","accessible":false,"folders":[{"machine":"peer","path":"/srv/hidden"}]}]});
    let expected = original.clone();
    let p = plan(&original, "workstation", false);
    assert_eq!(original, expected);
    assert_eq!(p.sources.len(), 1);
    assert_eq!(p.sources[0].computer, "");
    assert_eq!(p.sources[0].references.len(), 2);
    assert_eq!(
        p.sources[0].references[0].folder,
        swarm::folders(&original["projects"][0])[0]
    );
    assert_eq!(plan(&original, "workstation", true).sources.len(), 2);
}
#[test]
fn source_count_and_source_item_pressure_are_explicitly_partial() {
    let mut projects = Vec::new();
    for n in 0..3 {
        projects.push(json!({"id":format!("p{n}"),"name":"Example","folders":(0..200).map(|i|json!({"machine":"peer","path":format!("/srv/{n}/{i}")})).collect::<Vec<_>>()}));
    }
    let p = plan(&json!({"projects":projects}), "workstation", false);
    assert_eq!(p.sources.len(), SOURCES);
    assert!(p.partial);
    let p = plan(
        &json!({"projects":[{"id":"a","name":"A","folders":[{"machine":"peer","path":format!("/{}","x".repeat(8192))}]}]}),
        "workstation",
        false,
    );
    assert!(p.sources.is_empty() && p.partial);
}
#[test]
fn filters_sort_and_group_before_paging_with_full_group_totals() {
    let mut model = Model::default();
    for i in 0..600 {
        assert!(model.put(row(i, if i < 300 { 0 } else { 1 })));
    }
    let first = model.page(&Filter::default(), 0, 1001).unwrap();
    assert_eq!(first.total, 600);
    assert_eq!(first.estimated_bytes, Some(44_850));
    assert_eq!(first.groups.len(), 1);
    assert_eq!(first.groups[0].total, 300);
    assert_eq!(first.groups[0].rows.len(), PAGE);
    assert!(first.groups[0].rows[0].path.ends_with("00299"));
    let second = model.page(&Filter::default(), 1, 1001).unwrap();
    assert_eq!(
        second.groups.iter().map(|g| g.rows.len()).sum::<usize>(),
        PAGE
    );
    assert_eq!((second.groups[0].total, second.groups[1].total), (300, 300));
    assert_eq!(second.groups[0].category, 0);
    assert_eq!(second.groups[1].category, 1);
    let last = model.page(&Filter::default(), usize::MAX, 1001).unwrap();
    assert_eq!(last.index, 2);
    assert_eq!(last.groups[0].rows.len(), 88);
    let filter = Filter {
        search: "branch-59".into(),
        sort: Sort::Name,
        ..Filter::default()
    };
    let page = model.page(&filter, 0, 1001).unwrap();
    assert_eq!(page.total, 11);
    assert!(page
        .groups
        .iter()
        .flat_map(|g| &g.rows)
        .all(|r| r.branch.starts_with("branch-59")));
    assert!(model
        .page(
            &Filter {
                search: "x".repeat(4097),
                ..Filter::default()
            },
            0,
            1001
        )
        .is_err());
}
#[test]
fn exact_selection_survives_pages_and_refresh_until_an_identity_leaves() {
    let mut model = Model::default();
    let a = row(0, 0);
    let b = row(599, 1);
    let c = row(100, 4);
    for r in [a.clone(), b.clone(), c.clone()] {
        model.put(r);
    }
    assert!(model.select(&a.id, true));
    assert!(model.select(&b.id, true));
    assert!(!model.select(&c.id, true));
    assert_eq!(model.page(&Filter::default(), 1, 1001).unwrap().selected, 2);
    assert!(!model.replace_selection(&["not-a-row".into()]));
    assert_eq!(model.selection().len(), 2);
    model.retain(&BTreeSet::from([b.id.clone(), c.id]));
    assert_eq!(model.selection(), [b.id]);
    assert!(model.row(&a.id).is_none());
}
#[test]
fn stale_or_future_display_times_cannot_be_fresh_candidates() {
    let mut model = Model::default();
    model.put(row(1, 0));
    assert_eq!(
        model.page(&Filter::default(), 0, 1001).unwrap().candidates,
        1
    );
    for now in [999, 1300] {
        let page = model.page(&Filter::default(), 0, now).unwrap();
        assert_eq!(page.candidates, 0);
        assert_eq!(page.groups[0].category, 3);
        assert!(!page.groups[0].rows[0].fresh);
    }
}
#[test]
fn row_count_and_byte_pressure_never_evict_or_partially_replace_a_row() {
    let mut model = Model::default();
    for i in 0..ROWS {
        assert!(model.put(row(i, 0)));
    }
    let original = model.row(&row(0, 0).id).unwrap().path.clone();
    assert!(!model.put(row(ROWS, 0)));
    assert!(model.partial);
    assert_eq!(model.row(&row(0, 0).id).unwrap().path, original);
    let mut replacement = row(0, 0);
    replacement.remains = "x".repeat(ITEM_BYTES_FOR_TEST);
    assert!(!model.put(replacement));
    assert_eq!(model.row(&row(0, 0).id).unwrap().path, original);
    assert!(model.retained_bytes() <= ROW_BYTES);
    let mut bytes = Model::default();
    let mut count = 0;
    loop {
        let mut r = row(count, 0);
        r.remains = "x".repeat(60_000);
        if !bytes.put(r) {
            break;
        }
        count += 1;
    }
    assert!(count > 200 && count < 400);
    assert!(bytes.retained_bytes() <= ROW_BYTES);
    assert_eq!(
        bytes.page(&Filter::default(), 0, 1001).unwrap().total,
        count
    );
}
const ITEM_BYTES_FOR_TEST: usize = 64 * 1024;
#[test]
fn projection_refuses_large_private_items_and_uses_existing_review_meanings() {
    let source = source();
    let raw = json!({"path":"/srv/example/w","kind":"linked","available":true,"verdict":"review","ignored_count":1,"ignored_fingerprint":"fp","disk_bytes":42,"disk_complete":false,"integration":{"kind":"remaining","base":"main"}});
    let row = project("scope", 0, &source, &raw, Some(&raw), true, 1000).unwrap();
    assert_eq!(row.category, 1);
    assert_eq!(
        row.remains,
        zerus_client_core::worktree_review::summary_line(&raw)
    );
    assert_eq!(row.size, zerus_client_core::worktree_review::size(&raw));
    let mut large = raw;
    large["private_extension"] = json!("x".repeat(ITEM_BYTES_FOR_TEST));
    assert!(project("scope", 0, &source, &large, None, false, 1000).is_none());
}

#[test]
fn candidate_and_cleanup_limits_refuse_the_whole_selection_without_a_prefix() {
    let mut model = Model::default();
    for i in 0..65 {
        model.put(row(i, 0));
    }
    let first = row(0, 0);
    model.select(&first.id, true);
    let original = model.selection();
    assert!(model.select_candidates(&Filter::default(), 1001).is_err());
    assert_eq!(model.selection(), original);
    let filter = Filter {
        search: "branch-0".into(),
        ..Filter::default()
    };
    model.select_candidates(&filter, 1001).unwrap();
    assert_eq!(model.selection(), [first.id]);
    for i in 0..65 {
        model.select(&row(i, 0).id, true);
    }
    assert!(model.cleanup_selection().is_err());
    assert_eq!(model.selection().len(), 65);
    model.select(&row(64, 0).id, false);
    assert_eq!(model.cleanup_selection().unwrap().len(), 64);
}

#[test]
fn estimate_refuses_overflow_and_never_includes_filtered_or_stale_candidates() {
    let mut model = Model::default();
    let mut a = row(0, 0);
    a.bytes = Some(u64::MAX);
    let mut b = row(1, 0);
    b.bytes = Some(1);
    model.put(a);
    model.put(b);
    assert_eq!(
        model
            .page(&Filter::default(), 0, 1001)
            .unwrap()
            .estimated_bytes,
        None
    );
    let filter = Filter {
        search: "branch-1".into(),
        ..Filter::default()
    };
    assert_eq!(
        model.page(&filter, 0, 1001).unwrap().estimated_bytes,
        Some(1)
    );
    assert_eq!(
        model
            .page(&Filter::default(), 0, 1300)
            .unwrap()
            .estimated_bytes,
        Some(0)
    );
}
fn leftover_row(path: &str, repository: bool) -> serde_json::Value {
    let mut raw = json!({"path":path,"kind":"leftover","verdict":"partially_removed",
        "common_dir":"/srv/example/.git","branch":"feature/left","branch_retained":true,
        "size":4096,"size_complete":false,"sessions":["codex/example/probe"],"process_count":0,
        "reasons":[{"code":"sessions","message":"Zerus sessions use this folder."}],
        "deletable":false,"fingerprint":"ab".repeat(32),
        "removal":{"path":path,"status":"partial","reserved_at":50.0,"detail":"Permission denied"}});
    if !repository {
        raw["repository_available"] = json!(false);
    }
    raw
}
#[test]
fn leftovers_form_their_own_first_group_and_never_join_a_cleanup_selection() {
    let source = source();
    let mut model = Model::default();
    assert!(model.put(row(1, 0)));
    let left = project_leftover(
        "scope",
        0,
        &source,
        &leftover_row("/srv/example/left", true),
        true,
        1000,
    )
    .unwrap();
    assert_eq!((left.category, left.kind.as_str()), (LEFTOVERS, "leftover"));
    assert_eq!(left.state, "Partially removed");
    assert_eq!(left.remains, "Zerus sessions use this folder.");
    assert_eq!(left.size, "≥ 4.00 KiB");
    assert_eq!(left.projects, ["a"]);
    // A malformed or mismatched row is not shown.
    assert!(project_leftover(
        "scope",
        0,
        &source,
        &leftover_row("/srv/example/left", false),
        true,
        1000
    )
    .is_none());
    let mut linked = leftover_row("/srv/example/left", true);
    linked["kind"] = json!("linked");
    assert!(project_leftover("scope", 0, &source, &linked, true, 1000).is_none());
    let id = left.id.clone();
    assert!(model.put(left));
    // A vanished repository's row is the same folder's row: same scope, same ID.
    let orphan = project_leftover(
        "scope",
        0,
        &source,
        &leftover_row("/srv/example/left", false),
        false,
        1000,
    )
    .unwrap();
    assert_eq!(orphan.id, id);
    assert_eq!(orphan.state, "Repository missing");
    assert!(
        orphan.projects.is_empty() && orphan.remains == "Keep the folder or clean it up yourself."
    );
    // A leftover category needs leftover data, and leftover data its category.
    let mut forged = row(2, LEFTOVERS);
    assert!(!model.put(forged.clone()));
    forged.category = 1;
    forged.leftover = model.row(&id).unwrap().leftover.clone();
    assert!(!model.put(forged));
    let page = model.page(&Filter::default(), 0, 1001).unwrap();
    assert_eq!(
        page.groups.iter().map(|g| g.category).collect::<Vec<_>>(),
        [LEFTOVERS, 0]
    );
    assert_eq!(page.groups[0].name, "Partially removed");
    assert_eq!(page.candidates, 1);
    assert!(!model.select(&id, true));
    assert!(!model.replace_selection(std::slice::from_ref(&id)));
    model.select_candidates(&Filter::default(), 1001).unwrap();
    assert!(!model.is_selected(&id));
    let only = Filter {
        category: Some(LEFTOVERS),
        ..Filter::default()
    };
    assert_eq!(model.page(&only, 0, 1001).unwrap().total, 1);
    assert!(model
        .page(
            &Filter {
                category: Some(6),
                ..Filter::default()
            },
            0,
            1001
        )
        .is_err());
}
#[test]
fn a_leftover_outcome_is_recorded_once_until_a_fresh_review() {
    use zerus_client_core::worktree_leftover::Settled;
    let source = source();
    let mut model = Model::default();
    let left = project_leftover(
        "scope",
        0,
        &source,
        &leftover_row("/srv/example/left", true),
        true,
        1000,
    )
    .unwrap();
    let id = left.id.clone();
    assert!(model.put(left.clone()));
    assert!(!model.settle(&row(3, 0).id, Settled::Deleted));
    assert!(model.settle(
        &id,
        Settled::Unknown {
            detail: "Timed out.".into()
        }
    ));
    let shown = model.row(&id).unwrap();
    assert_eq!(shown.state, "Result unknown");
    assert_eq!(
        shown.remains,
        "Timed out. An unknown result will not be retried."
    );
    // A second outcome never overwrites the first in the same review.
    assert!(!model.settle(&id, Settled::Deleted));
    assert_eq!(model.row(&id).unwrap().state, "Result unknown");
    // The fresh review's row replaces it and may be acted on again.
    assert!(model.put(left));
    assert!(model
        .row(&id)
        .unwrap()
        .leftover
        .as_ref()
        .unwrap()
        .outcome
        .is_none());
    assert!(model.settle(&id, Settled::Forgotten));
    assert_eq!(model.row(&id).unwrap().state, "Folder kept");
}
