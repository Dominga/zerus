//! The sole allocation registry for private Teams store marks.
//! Reserve here before adding a migration. Reservations alone enable no layout.

macro_rules! allocations {
    ($($(#[$meta:meta])* $name:ident = $value:expr;)*) => {
        $($(#[$meta])* pub(crate) const $name: u32 = $value;)*
        #[cfg(test)]
        const ALLOCATIONS: &[(&str, u32)] = &[$((stringify!($name), $name)),*];
    };
}

allocations! {
    UNINITIALIZED = 0;
    RETIRED_PREVIEW = 1;
    CORE_LEGACY = 2;
    CORE_PLANNING = 3;
    /// Published by zerus-63rru.31 / PR #2451; ordinary opens migrate to this layout.
    CORE_NATIVE_PERSON = 4;
    /// Published by zerus-63rru.161 slice 3 only with durable approval inputs.
    CORE_PUBLICATION = 5;
    /// TODO(zerus-63rru.161 rework): implement, register and qualify this migration.
    CORE_REWORK = 6;
    /// TODO(zerus-63rru.31 S2): implement, register and qualify the native execution floor.
    CORE_NATIVE_FLOOR = 7;
    TIMER_BASE = 8;
    TIMER_ANSWERS = 9;
    TIMER_CARD_DECISIONS = 10;
    TIMER_HOLD_OWNERS = 11;
    TIMER_KEY_REFUSAL_ORDER = 12;
    PINNED_LAYOUT_FLAG = 1 << 31;
    PINNED_EXTENSION = PINNED_LAYOUT_FLAG | TIMER_CURRENT;
}

// TODO(reserved PRs above): promote only with the real older-store migration
// and its writer tests. Update these aliases and the controls below in that PR.
pub(crate) const CORE_CURRENT: u32 = CORE_PUBLICATION;
pub(crate) const TIMER_CURRENT: u32 = TIMER_KEY_REFUSAL_ORDER;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teams::ops::tests::{base_advance::GitFixture, create, person, Harness, TEAM_B};
    use crate::teams::ops::Operation;
    use crate::teams::store::{timers, Store};
    use std::collections::BTreeSet;

    const TIMER_LAYOUTS: &[u32] = &[
        TIMER_BASE,
        TIMER_ANSWERS,
        TIMER_CARD_DECISIONS,
        TIMER_HOLD_OWNERS,
        TIMER_KEY_REFUSAL_ORDER,
    ];
    const RESERVED_CORE: &[u32] = &[CORE_REWORK, CORE_NATIVE_FLOOR];

    #[test]
    fn teams_store_version_registry_wiring() {
        for (name, source) in [
            ("SCHEMA_VERSION", include_str!("mod.rs")),
            ("BASE", include_str!("timers/schema.rs")),
            ("EXTENSION", include_str!("timers/schema.rs")),
            ("PINNED_LAYOUT_FLAG", include_str!("bases.rs")),
            ("PINNED_EXTENSION", include_str!("bases.rs")),
        ] {
            let prefix = format!("const {name}: u32 = ");
            let line = source.lines().find(|line| line.contains(&prefix)).unwrap();
            let expression = line.split_once(" = ").unwrap().1;
            assert!(
                expression.contains("versions::"),
                "{name} must use the Teams version registry: {line}"
            );
        }
        assert!(
            include_str!("schema.sql").contains("__CORE_LEGACY_VERSION__"),
            "the initial SQL version must also come from the registry"
        );
    }

    #[test]
    fn teams_store_version_registry_allocations_are_unique() {
        let mut seen = BTreeSet::new();
        for (name, value) in ALLOCATIONS {
            assert!(
                seen.insert(*value),
                "duplicate Teams version {value}: {name}"
            );
        }
        for layout in TIMER_LAYOUTS.iter().filter(|v| **v != TIMER_CURRENT) {
            assert!(
                seen.insert(PINNED_LAYOUT_FLAG | layout),
                "pinned layout {layout}"
            );
        }
    }

    #[test]
    fn teams_store_version_registry_preserves_published_and_reserved_numbers() {
        assert_eq!(
            ALLOCATIONS
                .iter()
                .map(|(_, value)| *value)
                .collect::<Vec<_>>(),
            [
                0,
                1,
                2,
                3,
                4,
                5,
                6,
                7,
                8,
                9,
                10,
                11,
                12,
                0x8000_0000,
                0x8000_000c
            ]
        );
        assert_eq!(CORE_CURRENT, CORE_PUBLICATION);
        assert_eq!(super::super::LEGACY_QUEUE_LAYOUT, CORE_LEGACY);
        assert_eq!(super::super::LEGACY_PLANNING_LAYOUT, CORE_PLANNING);
        assert_eq!(TIMER_CURRENT, TIMER_KEY_REFUSAL_ORDER);
        assert_eq!(super::super::SCHEMA_VERSION, CORE_CURRENT);
        assert_eq!(timers::EXTENSION, TIMER_CURRENT);
        assert_eq!(super::super::bases::PINNED_EXTENSION, PINNED_EXTENSION);
    }

    fn core(h: &Harness) -> u32 {
        h.store
            .db
            .as_ref()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    fn planning(team: &str) -> Operation {
        let mut value = serde_json::to_value(create()).unwrap();
        value["op"] = "start_planning".into();
        value["goal"] = "Qualify the Teams version registry".into();
        value["integration_branch"] = format!("zerus/team/registry-{team}").into();
        serde_json::from_value(value).unwrap()
    }

    // Product-written queue/planning data. Removing only the newer native
    // metadata models legacy 2/3; retaining it models the old native layout 3.
    #[test]
    fn teams_store_version_registry_core_steps_reach_every_shipped_older_layout() {
        for (from, keep_native) in [
            (UNINITIALIZED, false),
            (CORE_LEGACY, false),
            (CORE_PLANNING, false),
            (CORE_PLANNING, true),
            (CORE_NATIVE_PERSON, true),
        ] {
            let mut h = if from == UNINITIALIZED {
                Harness::empty()
            } else {
                Harness::new()
            };
            let mut nonce = None;
            if from != UNINITIALIZED {
                assert_eq!(core(&h), CORE_NATIVE_PERSON);
                if from == CORE_PLANNING {
                    h.run_for(TEAM_B, &person(), planning(TEAM_B)).unwrap();
                }
                let db = h.store.db.as_ref().unwrap();
                if keep_native {
                    nonce = Some(
                        db.query_row(
                            "SELECT nonce FROM person_gesture_store WHERE id=1",
                            [],
                            |r| r.get::<_, String>(0),
                        )
                        .unwrap(),
                    );
                } else {
                    db.execute_batch("DROP TRIGGER person_gesture_store_immutable; DROP TRIGGER person_gesture_store_kept; DROP TRIGGER person_gestures_immutable; DROP TABLE person_gestures; DROP TABLE person_gesture_store;").unwrap();
                }
                db.pragma_update(None, "user_version", from).unwrap();
                assert_eq!(core(&h), from);
                h.store = Store::open(h.temp.path()).unwrap();
                assert_eq!(core(&h), CORE_NATIVE_PERSON, "upgrade from {from}");
            }
            let team = "00000000-0000-4000-8000-00000000cafe";
            h.run_for(team, &person(), planning(team)).unwrap();
            assert_eq!(core(&h), CORE_NATIVE_PERSON, "from {from}");
            h.store = Store::open(h.temp.path()).unwrap();
            assert_eq!(core(&h), CORE_NATIVE_PERSON, "reopen from {from}");
            if let Some(nonce) = nonce {
                let actual = h
                    .store
                    .db
                    .as_ref()
                    .unwrap()
                    .query_row(
                        "SELECT nonce FROM person_gesture_store WHERE id=1",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap();
                assert_eq!(actual, nonce, "native identity survives from {from}");
            }
        }
    }

    #[test]
    fn teams_store_version_registry_initial_sql_and_native_upgrade_use_allocated_marks() {
        use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
        use std::sync::{Arc, Mutex};
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        let writes = Arc::new(Mutex::new(Vec::new()));
        let observed = writes.clone();
        // Observe the production initializer, including its temporary legacy
        // mark inside the transaction; insert no application rows by hand.
        db.authorizer(Some(move |context: AuthContext<'_>| {
            if let AuthAction::Pragma {
                pragma_name: "user_version",
                pragma_value: Some(value),
            } = context.action
            {
                observed.lock().unwrap().push(value.parse::<u32>().unwrap());
            }
            Authorization::Allow
        }));
        super::super::initialize(&mut db).unwrap();
        assert_eq!(*writes.lock().unwrap(), [CORE_LEGACY, CORE_NATIVE_PERSON]);
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            CORE_NATIVE_PERSON
        );
    }

    // Application rows come from Create. Only the schema mark is deliberately
    // changed to represent an unsupported/future writer, never application rows.
    #[test]
    fn teams_store_version_registry_reserved_core_layouts_remain_unimplemented() {
        for version in std::iter::once(RETIRED_PREVIEW).chain(RESERVED_CORE.iter().copied()) {
            let h = Harness::new();
            h.store
                .db
                .as_ref()
                .unwrap()
                .pragma_update(None, "user_version", version)
                .unwrap();
            let error = Store::open(h.temp.path()).unwrap_err();
            assert_eq!(
                error.code,
                zerus_team_plan::problem::code::INVALID_STATE,
                "{version}"
            );
            assert_eq!(core(&h), version, "refusal changes no mark");
        }
    }

    #[test]
    fn teams_store_version_registry_every_timer_step_is_reachable_in_order() {
        assert_eq!(
            TIMER_LAYOUTS,
            (TIMER_BASE..=TIMER_CURRENT).collect::<Vec<_>>()
        );
        assert_eq!(
            timers::migration_steps(),
            TIMER_LAYOUTS[..TIMER_LAYOUTS.len() - 1]
        );
        let f = GitFixture::new();
        for pinned in [false, true] {
            for from in std::iter::once(None).chain(TIMER_LAYOUTS.iter().copied().map(Some)) {
                if pinned && from.is_none() {
                    continue;
                }
                let mut h = f.harness();
                if pinned {
                    let request = f.request(&h, &f.next);
                    f.apply(&mut h, &person(), &request).unwrap();
                }
                let flag = if pinned { PINNED_LAYOUT_FLAG } else { 0 };
                let db = h.store.db.as_ref().unwrap();
                // Product-written rows, with only the metadata mark lowered to
                // select an older migration entry point. No handwritten rows.
                if let Some(from) = from {
                    db.execute("UPDATE timers_extension SET version=?1", [from | flag])
                        .unwrap();
                } else {
                    db.execute("DELETE FROM timers_extension", []).unwrap();
                }
                timers::observe_migrations();
                h.store = Store::open(h.temp.path()).unwrap();
                let expected = TIMER_LAYOUTS
                    .iter()
                    .copied()
                    .filter(|v| *v >= from.unwrap_or(TIMER_BASE) && *v < TIMER_CURRENT)
                    .collect::<Vec<_>>();
                assert_eq!(
                    timers::take_migration_calls(),
                    expected,
                    "from {from:?}, pinned {pinned}"
                );
                let marker = || {
                    h.store
                        .db
                        .as_ref()
                        .unwrap()
                        .query_row("SELECT version FROM timers_extension WHERE id=1", [], |r| {
                            r.get::<_, u32>(0)
                        })
                        .unwrap()
                };
                assert_eq!(marker(), TIMER_CURRENT | flag);
                assert_eq!(
                    core(&h),
                    CORE_NATIVE_PERSON,
                    "extension never changes the native core mark"
                );
                timers::observe_migrations();
                h.store = Store::open(h.temp.path()).unwrap();
                assert!(
                    timers::take_migration_calls().is_empty(),
                    "each step runs once"
                );
            }
        }
    }
}
