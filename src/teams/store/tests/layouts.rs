use super::*;

#[test]
fn published_core_and_extension_layouts_have_distinct_allocations() {
    // .165 reserves this namespace in timers_extension.version, never in
    // PRAGMA user_version. Its low bits keep the ordinary migration layout.
    use super::super::bases::{PINNED_EXTENSION, PINNED_LAYOUT_FLAG};
    let ordinary = super::super::timers::published_layouts();
    let mut allocations = vec![
        1,
        LEGACY_QUEUE_LAYOUT,
        LEGACY_PLANNING_LAYOUT,
        SCHEMA_VERSION,
    ];
    assert!(allocations.iter().all(|v| *v < PINNED_LAYOUT_FLAG));
    for layout in ordinary {
        assert!(layout < PINNED_LAYOUT_FLAG);
        allocations.extend([layout, PINNED_LAYOUT_FLAG | layout]);
    }
    let unique = allocations
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), allocations.len(), "{allocations:?}");
    assert_eq!(PINNED_EXTENSION, 2147483660);
}
