//! zerus-63rru.74: a team name, role ID and member ID refuse control and
//! bidirectional characters at admission, through the real store.
use super::*;

const BIDIRECTIONAL: [char; 9] = [
    '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}',
    '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
];

fn with_name(name: &str) -> Operation {
    let mut create = create();
    if let Operation::Create { name: team_name, .. } = &mut create {
        *team_name = name.into();
    }
    create
}
fn refused(h: &mut Harness, name: &str) -> Problem {
    h.run(&person(), with_name(name)).unwrap_err()
}

#[test]
fn each_forbidden_character_is_refused_in_a_team_name() {
    let mut cases: Vec<char> = BIDIRECTIONAL.to_vec();
    cases.extend(['\u{0007}', '\u{0085}', '\u{2028}', '\u{2029}']);
    for ch in cases {
        let mut h = Harness::empty();
        let problem = refused(&mut h, &format!("Team{ch}name"));
        assert_eq!(problem.code, code::INVALID_STATE, "{ch:?}: {problem:?}");
        assert!(
            problem.message.contains(&format!("U+{:04X}", ch as u32)),
            "{ch:?}: {problem:?}"
        );
        assert!(
            problem.message.contains("The team name"),
            "{ch:?}: {problem:?}"
        );
    }
}

#[test]
fn a_valid_name_with_non_ascii_letters_is_accepted() {
    let mut h = Harness::empty();
    h.run(&person(), with_name("Команда Ångström 東京")).unwrap();
    assert_eq!(
        h.store.team(TEAM).unwrap().unwrap().name,
        "Команда Ångström 東京"
    );
}

#[test]
fn role_and_template_ids_outside_the_set_are_refused() {
    for bad in ["Role!", "role id", ""] {
        let mut h = Harness::empty();
        let mut create = create();
        if let Operation::Create { bundle, .. } = &mut create {
            bundle.roles[0].id = bad.into();
        }
        let problem = h.run(&person(), create).unwrap_err();
        assert_eq!(problem.code, code::INVALID_STATE, "{bad:?}: {problem:?}");
        assert!(problem.message.contains("The role ID"), "{bad:?}: {problem:?}");
    }
    // A template ID outside the set.
    let mut h = Harness::empty();
    let mut create = create();
    if let Operation::Create { bundle, .. } = &mut create {
        bundle.template.id = "Bad template!".into();
    }
    let problem = h.run(&person(), create).unwrap_err();
    assert_eq!(problem.code, code::INVALID_STATE, "{problem:?}");
    assert!(problem.message.contains("The template ID"), "{problem:?}");
}

#[test]
fn an_existing_team_with_such_a_name_keeps_working() {
    let mut h = Harness::new();
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE teams SET settings=settings WHERE id=?1",
            rusqlite::params![TEAM],
        )
        .unwrap();
    // The value existed before the check: set the stored name the way an
    // older build left it, then every read and operation works unchanged.
    h.store
        .db
        .as_ref()
        .unwrap()
        .execute(
            "UPDATE teams SET name=?2 WHERE id=?1",
            rusqlite::params![TEAM, "Team\u{0007}\u{202D}name"],
        )
        .unwrap();
    let team = h.store.team(TEAM).unwrap().unwrap();
    assert_eq!(team.name, "Team\u{0007}\u{202D}name");
    h.run(&person(), Operation::Pause).unwrap();
    h.run(&person(), Operation::Resume).unwrap();
    let node = h.add(&person(), 1);
    assert_eq!(h.store.node(TEAM, node).unwrap().unwrap().id, node);
}

/// Round 2, P2: the plan kernel's own list and the replica's agree for
/// every character from U+0000 to U+10FFFF, so the two rules never drift.
#[test]
fn the_replica_and_the_plan_kernel_agree_for_every_character() {
    for point in 0u32..=0x10FFFF {
        let Some(ch) = char::from_u32(point) else {
            continue;
        };
        assert_eq!(
            zerus_replica::forbidden_char(&ch.to_string()).is_some(),
            zerus_team_plan::forbidden_char(&ch.to_string()).is_some(),
            "U+{point:04X}"
        );
    }
}
