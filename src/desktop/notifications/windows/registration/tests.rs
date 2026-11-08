//! In-memory registry only. No real HKCU or native activation is used.
use super::*;
#[derive(Default)]
struct Fake {
    keys: BTreeMap<String, Contents>,
    effects: usize,
    fail_at: Option<usize>,
}
impl Registry for Fake {
    fn read(&mut self, path: &str) -> Result<Option<Contents>, &'static str> {
        Ok(self.keys.get(path).cloned())
    }
    fn create_new(
        &mut self,
        path: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<(), &'static str> {
        self.effects += 1;
        if self.fail_at == Some(self.effects) {
            return Err("injected_create_failure");
        }
        if self.keys.contains_key(path) {
            return Err("create_race_kept");
        }
        self.keys.insert(
            path.into(),
            Contents {
                values: values.clone(),
                children: BTreeSet::new(),
            },
        );
        if let Some((parent, name)) = path.rsplit_once('\\') {
            if let Some(key) = self.keys.get_mut(parent) {
                key.children.insert(name.into());
            }
        }
        Ok(())
    }
    fn remove_exact(&mut self, path: &str, contents: &Contents) -> Result<(), &'static str> {
        self.effects += 1;
        if self.fail_at == Some(self.effects) {
            return Err("injected_remove_failure");
        }
        if self.keys.get(path) != Some(contents) {
            return Err("changed_key_kept");
        }
        self.keys.remove(path);
        if let Some((parent, name)) = path.rsplit_once('\\') {
            if let Some(key) = self.keys.get_mut(parent) {
                key.children.remove(name);
            }
        }
        Ok(())
    }
}
fn plan() -> Plan {
    Plan::new(&"a".repeat(64),"S-1-5-21-1000",r#""C:\Example\hgs.exe" --desktop-notification-activation --config-dir C:\Example\config --state-dir C:\Example\state"#).unwrap()
}
#[test]
fn install_and_remove_are_explicit_idempotent_and_exactly_owned() {
    let p = plan();
    let mut r = Fake::default();
    assert_eq!(
        p.apply(&mut r, "status"),
        Ok(Reply {
            registered: false,
            changed: false
        })
    );
    assert_eq!(r.effects, 0);
    assert_eq!(
        p.apply(&mut r, "install"),
        Ok(Reply {
            registered: true,
            changed: true
        })
    );
    assert_eq!(r.effects, 3);
    assert_eq!(
        p.apply(&mut r, "install"),
        Ok(Reply {
            registered: true,
            changed: false
        })
    );
    assert_eq!(r.effects, 3);
    assert_eq!(
        p.apply(&mut r, "remove"),
        Ok(Reply {
            registered: false,
            changed: true
        })
    );
    assert!(r.keys.is_empty());
    assert_eq!(r.effects, 6);
    assert_eq!(
        p.apply(&mut r, "remove"),
        Ok(Reply {
            registered: false,
            changed: false
        })
    );
    assert_eq!(r.effects, 6);
}
#[test]
fn foreign_partial_and_extra_values_refuse_before_any_mutation() {
    let p = plan();
    let mut full = Fake::default();
    p.apply(&mut full, "install").unwrap();
    for edit in 0..4 {
        let mut r = Fake {
            keys: full.keys.clone(),
            ..Default::default()
        };
        match edit {
            0 => {
                r.keys.remove(&p.keys[1].0);
            }
            1 => {
                r.keys
                    .get_mut(&p.keys[2].0)
                    .unwrap()
                    .values
                    .insert("Other".into(), "kept".into());
            }
            2 => {
                r.keys
                    .get_mut(&p.keys[0].0)
                    .unwrap()
                    .values
                    .insert(CONFIGURATION_VALUE.into(), "b".repeat(64));
            }
            _ => {
                r.keys
                    .get_mut(&p.keys[0].0)
                    .unwrap()
                    .children
                    .insert("Other".into());
            }
        }
        let before = r.keys.clone();
        for operation in ["status", "install", "remove"] {
            assert!(p.apply(&mut r, operation).is_err());
            assert_eq!(r.effects, 0);
            assert_eq!(r.keys, before);
        }
    }
}
#[test]
fn creation_or_removal_failure_keeps_partial_state_and_never_auto_replays() {
    let p = plan();
    for fail_at in 1..=3 {
        let mut r = Fake {
            fail_at: Some(fail_at),
            ..Default::default()
        };
        assert!(p.apply(&mut r, "install").unwrap_err().uncertain);
        let before = r.keys.clone();
        if !before.is_empty() {
            assert!(p.apply(&mut r, "status").is_err());
            assert_eq!(r.keys, before);
        }
    }
    for fail_at in 4..=6 {
        let mut r = Fake::default();
        p.apply(&mut r, "install").unwrap();
        r.fail_at = Some(fail_at);
        assert!(p.apply(&mut r, "remove").unwrap_err().uncertain);
        let before = r.keys.clone();
        if fail_at != 4 {
            assert!(p.apply(&mut r, "install").is_err());
            assert_eq!(r.keys, before);
        }
    }
}
#[test]
fn service_accounts_and_unbounded_or_malformed_identity_are_refused() {
    for sid in ["S-1-5-18", "S-1-5-19", "S-1-5-20", "S-1-5-80-99", "unknown"] {
        assert!(Plan::new(&"a".repeat(64), sid, "command").is_err());
    }
    assert!(Plan::new(&"A".repeat(64), "S-1-5-21-1000", "command").is_err());
    assert!(Plan::new(&"a".repeat(64), "S-1-5-21-1000", &"x".repeat(32767)).is_err());
    let p = plan();
    let mut r = Fake::default();
    assert!(p.apply(&mut r, "run").is_err());
    assert!(r.keys.is_empty());
}

#[test]
fn removal_receipt_needs_original_configuration_request_epoch_and_all_drain_fields() {
    let id = uuid::Uuid::new_v4().to_string();
    let epoch = uuid::Uuid::new_v4().to_string();
    let configuration = "a".repeat(64);
    let receipt = serde_json::json!({"request_id":id,"configuration":configuration,"writer_epoch":epoch,"accepted":true,"durable":true,"revoked":true,"checkpointed":true});
    assert!(settled_revoke(&receipt, &id, &configuration, &epoch));
    for field in ["accepted", "durable", "revoked", "checkpointed"] {
        let mut changed = receipt.clone();
        changed[field] = false.into();
        assert!(!settled_revoke(&changed, &id, &configuration, &epoch));
    }
    for field in ["request_id", "configuration", "writer_epoch"] {
        let mut changed = receipt.clone();
        changed[field] = "changed".into();
        assert!(!settled_revoke(&changed, &id, &configuration, &epoch));
    }
    assert!(!settled_revoke(
        &serde_json::json!({"accepted":true}),
        &id,
        &configuration,
        &epoch
    ));
    assert!(!settled_revoke(
        &serde_json::json!({"applicable":false,"handoff":"none","revoked":true,"nothing_written":true}),
        &id,
        &configuration,
        &epoch
    ));
}

#[test]
fn a_key_appearing_after_preflight_is_preserved_instead_of_overwritten() {
    struct Race(Fake);
    impl Registry for Race {
        fn read(&mut self, path: &str) -> Result<Option<Contents>, &'static str> {
            self.0.read(path)
        }
        fn create_new(
            &mut self,
            path: &str,
            values: &BTreeMap<String, String>,
        ) -> Result<(), &'static str> {
            self.0.keys.insert(
                path.into(),
                Contents {
                    values: [("Other".into(), "kept".into())].into_iter().collect(),
                    children: BTreeSet::new(),
                },
            );
            self.0.create_new(path, values)
        }
        fn remove_exact(&mut self, _: &str, _: &Contents) -> Result<(), &'static str> {
            panic!("create race must not clean up")
        }
    }
    let p = plan();
    let mut r = Race(Fake::default());
    assert!(p.apply(&mut r, "install").unwrap_err().uncertain);
    assert_eq!(r.0.keys[&p.keys[0].0].values["Other"], "kept");
    assert_eq!(r.0.effects, 1);
}

#[test]
fn one_formatter_writes_and_selects_the_exact_registered_command() {
    let command = expected_command(
        r"C:\Example\hgs.exe",
        r"C:\Example\config",
        r"C:\Example\state",
    )
    .unwrap();
    assert_eq!(
        command,
        r#""C:\Example\hgs.exe" --desktop-notification-activation --config-dir C:\Example\config --state-dir C:\Example\state"#
    );
    assert!(Plan::new(&"a".repeat(64), "S-1-5-21-1000", &command).is_ok());
    // Spaces and non-ASCII paths quote exactly as the registry stores them.
    let spaced = expected_command(
        r"C:\Users\Тима Example\.local\bin\hgs.exe",
        r"C:\Users\Тима Example\.config\hgs",
        r"\\?\C:\Users\Тима Example\state dir",
    )
    .unwrap();
    assert_eq!(
        spaced,
        r#""C:\Users\Тима Example\.local\bin\hgs.exe" --desktop-notification-activation --config-dir "C:\Users\Тима Example\.config\hgs" --state-dir "\\?\C:\Users\Тима Example\state dir""#
    );
    assert!(expected_command(r"\\server\share\hgs.exe", r"\\server\share\c", r"D:/s").is_ok());
    for (case, image, config, state) in [
        ("relative image", r"hgs.exe", r"C:\c", r"C:\s"),
        ("relative config", r"C:\hgs.exe", r"config", r"C:\s"),
        ("drive-relative state", r"C:\hgs.exe", r"C:\c", r"C:state"),
        ("quoted image", r#"C:\a"b\hgs.exe"#, r"C:\c", r"C:\s"),
        ("control character", r"C:\hgs.exe", "C:\\c\n", r"C:\s"),
        ("empty", "", r"C:\c", r"C:\s"),
    ] {
        assert_eq!(
            expected_command(image, config, state),
            Err("registration_command_invalid"),
            "{case}"
        );
    }
    let long = format!(r"C:\{}", "x".repeat(32767));
    assert_eq!(
        expected_command(r"C:\hgs.exe", &long, r"C:\s"),
        Err("registration_command_too_large")
    );
}

#[test]
fn only_exact_arguments_under_another_quoted_image_name_another_image() {
    let image = r"C:\Users\Example\.local\bin\hgs.exe";
    let expected = expected_command(image, r"C:\c", r"C:\s").unwrap();
    let other = expected_command(r"C:\Other copy\hgs.exe", r"C:\c", r"C:\s").unwrap();
    assert!(names_other_image(&other, &expected, image));
    assert!(
        !names_other_image(&expected, &expected, image),
        "the same image is not other"
    );
    for (case, registered) in [
        (
            "other config",
            expected_command(r"C:\Other\hgs.exe", r"C:\d", r"C:\s").unwrap(),
        ),
        (
            "other state",
            expected_command(r"C:\Other\hgs.exe", r"C:\c", r"C:\t").unwrap(),
        ),
        ("extra argument", format!("{other} --extra")),
        ("unquoted image", other.replacen('"', "", 2)),
        (
            "empty image",
            other.replacen(r"C:\Other copy\hgs.exe", "", 1),
        ),
        ("nothing", String::new()),
        (
            "quoted relative image",
            other.replacen(r"C:\Other copy\hgs.exe", r"Other copy\hgs.exe", 1),
        ),
        (
            "control character in the image",
            other.replacen(r"C:\Other copy\hgs.exe", "C:\\Other\tcopy\\hgs.exe", 1),
        ),
    ] {
        assert!(!names_other_image(&registered, &expected, image), "{case}");
    }
}

/// zerus-5wmh.66.3.30.8.5.1: only an exact earlier image of THIS configuration
/// and state is a previous image; the parse is a byte-for-byte round trip.
#[test]
fn only_an_exact_round_trip_for_the_current_folders_is_a_previous_image() {
    let image = r"C:\Users\Example\.local\bin\hgs.exe";
    let (config, state) = (r"C:\Users\Тима Example\.config\hgs", r"C:\s");
    let earlier = r"C:\Old copy\hgs.exe";
    let registered = expected_command(earlier, config, state).unwrap();
    assert_eq!(
        previous_image(&registered, image, config, state).as_deref(),
        Some(earlier)
    );
    let current = expected_command(image, config, state).unwrap();
    for (case, command) in [
        ("the same image", current.clone()),
        (
            "other config",
            expected_command(earlier, r"C:\d", state).unwrap(),
        ),
        (
            "other state",
            expected_command(earlier, config, r"C:\t").unwrap(),
        ),
        ("extra argument", format!("{registered} --extra")),
        ("trailing space", format!("{registered} ")),
        ("unquoted image", registered.replacen('"', "", 2)),
        ("empty image", registered.replacen(earlier, "", 1)),
        (
            "relative image",
            registered.replacen(earlier, r"Old copy\hgs.exe", 1),
        ),
        (
            "control character",
            registered.replacen(earlier, "C:\\Old\tcopy\\hgs.exe", 1),
        ),
        ("nothing", String::new()),
    ] {
        assert!(
            previous_image(&command, image, config, state).is_none(),
            "{case}"
        );
    }
}

/// One change applied to an otherwise exact earlier registration.
type Edit<'a> = Box<dyn Fn(&mut Fake) + 'a>;

fn earlier_plan() -> Plan {
    Plan::new(&"a".repeat(64),"S-1-5-21-1000",r#""C:\Old copy\hgs.exe" --desktop-notification-activation --config-dir C:\Example\config --state-dir C:\Example\state"#).unwrap()
}

#[test]
fn an_exact_previous_image_registration_is_selected_and_removed_before_a_new_install() {
    let (current, earlier) = (plan(), earlier_plan());
    let mut r = Fake::default();
    earlier.apply(&mut r, "install").unwrap();
    assert_eq!(
        current.inspect(&mut r),
        Err("foreign_or_incomplete_notification_registration_kept")
    );
    let effects = r.effects;
    let previous = current
        .previous(
            &mut r,
            r"C:\Example\hgs.exe",
            r"C:\Example\config",
            r"C:\Example\state",
        )
        .unwrap()
        .expect("the exact earlier registration");
    assert_eq!(r.effects, effects, "selection is read-only");
    assert_eq!(previous.keys, earlier.keys);
    assert_eq!(
        previous.apply(&mut r, "remove").unwrap(),
        Reply {
            registered: false,
            changed: true
        }
    );
    assert!(r.keys.is_empty());
    assert!(current.apply(&mut r, "install").unwrap().registered);
}

#[test]
fn a_foreign_partial_older_or_other_configuration_registration_is_no_previous_image() {
    let (current, earlier) = (plan(), earlier_plan());
    let mut full = Fake::default();
    earlier.apply(&mut full, "install").unwrap();
    let class = &earlier.keys[0].0;
    let server = &earlier.keys[1].0;
    let application = &earlier.keys[2].0;
    let edits: Vec<(&str, Edit<'_>)> = vec![
        (
            "other configuration hash",
            Box::new(|r: &mut Fake| {
                for path in [class, application] {
                    r.keys
                        .get_mut(path)
                        .unwrap()
                        .values
                        .insert(CONFIGURATION_VALUE.into(), "b".repeat(64));
                }
            }),
        ),
        (
            "older ABI marker",
            Box::new(|r: &mut Fake| {
                r.keys
                    .get_mut(class)
                    .unwrap()
                    .values
                    .insert(ABI_VALUE.into(), "desktop-notification-v0".into());
            }),
        ),
        (
            "foreign activator",
            Box::new(|r: &mut Fake| {
                r.keys.get_mut(application).unwrap().values.insert(
                    "CustomActivator".into(),
                    "{00000000-0000-0000-0000-000000000000}".into(),
                );
            }),
        ),
        (
            "foreign display name",
            Box::new(|r: &mut Fake| {
                r.keys
                    .get_mut(application)
                    .unwrap()
                    .values
                    .insert("DisplayName".into(), "Other".into());
            }),
        ),
        (
            "extra value",
            Box::new(|r: &mut Fake| {
                r.keys
                    .get_mut(server)
                    .unwrap()
                    .values
                    .insert("Other".into(), "kept".into());
            }),
        ),
        (
            "extra key",
            Box::new(|r: &mut Fake| {
                r.keys
                    .get_mut(class)
                    .unwrap()
                    .children
                    .insert("Other".into());
            }),
        ),
        (
            "partial set",
            Box::new(|r: &mut Fake| {
                r.keys.remove(application);
            }),
        ),
        (
            "other state folder in the command",
            Box::new(|r: &mut Fake| {
                r.keys.get_mut(server).unwrap().values.insert(
                    String::new(),
                    expected_command(
                        r"C:\Old copy\hgs.exe",
                        r"C:\Example\config",
                        r"C:\Other\state",
                    )
                    .unwrap(),
                );
            }),
        ),
    ];
    for (case, edit) in edits {
        let mut r = Fake {
            keys: full.keys.clone(),
            ..Default::default()
        };
        edit(&mut r);
        let before = r.keys.clone();
        assert_eq!(
            current
                .previous(
                    &mut r,
                    r"C:\Example\hgs.exe",
                    r"C:\Example\config",
                    r"C:\Example\state"
                )
                .unwrap()
                .map(|p| p.keys),
            None,
            "{case}"
        );
        assert_eq!((r.effects, &r.keys), (0, &before), "{case}");
    }
    // Nothing registered: nothing to select.
    assert!(current
        .previous(
            &mut Fake::default(),
            r"C:\Example\hgs.exe",
            r"C:\Example\config",
            r"C:\Example\state"
        )
        .unwrap()
        .is_none());
}

/// Counts admissions and confirmations; either can be made to refuse.
struct FakeGate {
    admit: Result<(), &'static str>,
    confirm: Result<(), &'static str>,
    admitted: std::cell::Cell<usize>,
    confirmed: std::cell::Cell<usize>,
}
impl FakeGate {
    fn new(admit: Result<(), &'static str>, confirm: Result<(), &'static str>) -> Self {
        Self {
            admit,
            confirm,
            admitted: 0.into(),
            confirmed: 0.into(),
        }
    }
}
impl Gate for FakeGate {
    fn admit(&self) -> anyhow::Result<()> {
        self.admitted.set(self.admitted.get() + 1);
        self.admit.map_err(|reason| anyhow::anyhow!(reason))
    }
    fn confirm(&self) -> anyhow::Result<()> {
        self.confirmed.set(self.confirmed.get() + 1);
        self.confirm.map_err(|reason| anyhow::anyhow!(reason))
    }
}

const FOLDERS: (&str, &str, &str) = (
    r"C:\Example\hgs.exe",
    r"C:\Example\config",
    r"C:\Example\state",
);

fn with_earlier() -> Fake {
    let mut r = Fake::default();
    earlier_plan().apply(&mut r, "install").unwrap();
    r.effects = 0;
    r
}

#[test]
fn a_previous_image_is_removed_only_through_an_admitted_gate_then_install_is_separate() {
    let mut r = with_earlier();
    let gate = std::rc::Rc::new(FakeGate::new(Ok(()), Ok(())));
    struct Shared(std::rc::Rc<FakeGate>);
    impl Gate for Shared {
        fn admit(&self) -> anyhow::Result<()> {
            self.0.admit()
        }
        fn confirm(&self) -> anyhow::Result<()> {
            self.0.confirm()
        }
    }
    let shared = Shared(gate.clone());
    let (reply, previous) = run(&plan(), &mut r, "remove", FOLDERS, || Ok(Some(shared))).unwrap();
    assert!(previous);
    assert_eq!(
        reply,
        Reply {
            registered: false,
            changed: true
        }
    );
    assert!(r.keys.is_empty());
    assert_eq!((gate.admitted.get(), gate.confirmed.get()), (1, 1));
    let (installed, previous) =
        run(&plan(), &mut r, "install", FOLDERS, || Ok(None::<FakeGate>)).unwrap();
    assert!(installed.registered && !previous);
}

#[test]
fn a_refusing_guard_leaves_a_valid_previous_registration_untouched() {
    for refusal in ["guard", "admit"] {
        let mut r = with_earlier();
        let before = r.keys.clone();
        let result = run(&plan(), &mut r, "remove", FOLDERS, || {
            if refusal == "guard" {
                anyhow::bail!("current rollback proof refused")
            }
            Ok(Some(FakeGate::new(Err("exclusion changed"), Ok(()))))
        });
        assert!(result.is_err(), "{refusal}");
        assert_eq!((r.effects, &r.keys), (0, &before), "{refusal}");
    }
}

#[test]
fn install_and_status_never_fall_back_to_a_previous_image() {
    for operation in ["install", "status"] {
        let mut r = with_earlier();
        let before = r.keys.clone();
        let error = run(&plan(), &mut r, operation, FOLDERS, || {
            Ok(Some(FakeGate::new(Ok(()), Ok(()))))
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("foreign_or_incomplete_notification_registration_kept"),
            "{operation}: {error}"
        );
        assert_eq!((r.effects, &r.keys), (0, &before), "{operation}");
    }
}

#[test]
fn a_partial_previous_removal_is_unknown_and_never_falls_back_repeats_or_installs() {
    let mut r = with_earlier();
    r.fail_at = Some(2);
    let error = run(&plan(), &mut r, "remove", FOLDERS, || {
        Ok(Some(FakeGate::new(Ok(()), Ok(()))))
    })
    .unwrap_err();
    assert!(error.to_string().contains("outcome unknown"), "{error}");
    assert_eq!(
        r.effects, 2,
        "one removal, then the failing one; nothing more"
    );
    let partial = r.keys.clone();
    assert!(!partial.is_empty() && partial.len() < 3);
    for operation in ["remove", "install", "status"] {
        let result = run(&plan(), &mut r, operation, FOLDERS, || {
            Ok(Some(FakeGate::new(Ok(()), Ok(()))))
        });
        assert!(result.is_err(), "{operation}");
        assert_eq!((r.effects, &r.keys), (2, &partial), "{operation}");
    }
}

#[test]
fn a_confirmation_failure_after_the_effect_is_reported_not_retried() {
    let mut r = with_earlier();
    let error = run(&plan(), &mut r, "remove", FOLDERS, || {
        Ok(Some(FakeGate::new(Ok(()), Err("exclusion changed"))))
    })
    .unwrap_err();
    assert!(error.to_string().contains("exclusion changed"), "{error}");
    assert!(r.keys.is_empty(), "the effect happened");
    assert_eq!(r.effects, 3, "three exact removals, no retry");
}
