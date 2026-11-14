//! OpenCode retains a refusal-only check beside the shared DeepSeek proof.
use super::*;
use crate::state::session_graph::Registered;
use ancestry::{Ancestor, Process};

fn row(pid: u32, birth: Option<&str>) -> Ancestor {
    Ancestor {
        pid,
        identity: Process { parent: pid + 1, birth: birth.map(Into::into) },
        claim: None,
        top: false,
    }
}

#[test]
fn an_opencode_service_counts_when_it_is_not_younger_than_its_file() {
    let mut top = row(11, Some("b11"));
    top.top = true;
    let chain = [row(10, Some("b10")), top];
    let service = Registered { pid: 11, written: 100.0 };
    for started in [90.0, 100.5] {
        assert_eq!(hosted(&chain, &service, |_| Some(("b11".into(), started))),
            Ok(Some(Host { pid: 11, birth: "b11".into() })));
    }
    assert_eq!(hosted(&chain, &service, |_| Some(("b11".into(), 102.0))), Ok(None));
    assert_eq!(hosted(&chain, &Registered { pid: 12, written: 100.0 }, |_| None), Ok(None));
    assert_eq!(hosted(&chain, &service, |_| None), Err(Refusal::Birth));
    assert_eq!(hosted(&chain, &service, |_| Some(("reused".into(), 90.0))), Err(Refusal::Birth));
    assert_eq!(host_refusal(Refusal::Birth), host_unreadable());
}

#[test]
fn opencode_walks_only_when_its_service_is_registered_and_fails_closed() {
    assert_eq!(opencode_ancestor_read(|| None, || panic!("no service"), |_| None), Ok(None));
    let service = || Some(Registered { pid: 11, written: 100.0 });
    assert_eq!(opencode_ancestor_read(service, || Err(unreadable()), |_| None), Err(Refusal::Walk));
    assert_eq!(host_refusal(Refusal::Walk), unreadable());
    assert_eq!(opencode_ancestor_read(service, || Ok(vec![row(11, Some("b11"))]),
        |_| Some(("b11".into(), 90.0))), Ok(Some(Host { pid: 11, birth: "b11".into() })));
}

#[cfg(unix)]
#[test]
fn deepseek_refusals_retain_the_shared_cause_and_recovery_text() {
    use crate::state::dsh::Refusal as Shared;
    for cause in [
        Shared::TooManyFolders("/fixture/hosts".into()),
        Shared::TooManyEntries("/fixture/hosts".into()),
        Shared::UnreadableRecord("/fixture/host.identity.json".into()),
        Shared::BirthlessRecord("/fixture/host.pid".into()),
        Shared::AncestryBound, Shared::UnreadableBirth(11),
        Shared::UnreadableAncestry, Shared::IsolatedPidNamespace,
    ] {
        let expected = cause.sentence();
        let problem = host_refusal(Refusal::DeepSeek(cause));
        assert_eq!(problem.code, code::INVALID_STATE);
        assert_eq!(problem.message, expected);
    }
}

#[test]
fn only_sessions_of_a_resident_host_are_host_served() {
    assert!(host_served_agent("dsh") && host_served_agent("opencode"));
    for agent in ["claude", "codex", "kimi", "", "DSH"] {
        assert!(!host_served_agent(agent), "{agent}");
    }
}

/// The record rules of one host folder, read by the reader itself.
#[cfg(unix)]
mod records {
    use crate::state::session_graph::{caller_host_folder, written_boot, Named};
    use serde_json::{json, Value};

    const THIS: &str = "11111111-1111-4111-8111-111111111111";
    const OTHER: &str = "22222222-2222-4222-8222-222222222222";

    fn folder(record: &Value, host_pid: Option<u32>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("host.identity.json"), record.to_string()).unwrap();
        if let Some(pid) = host_pid {
            std::fs::write(dir.path().join("host.pid"), pid.to_string()).unwrap();
        }
        dir
    }

    fn running(boot: &str) -> Value {
        json!({"state":"running","version":2,"pid":4242,"birth":"77","pid_file":[1,2,3,4],"boot":boot})
    }

    fn named(pid: u32, birth: Option<&str>) -> Named {
        Named {
            pid,
            birth: birth.map(Into::into),
        }
    }

    /// The writer's boot read is the reader's: a record stamped with it is
    /// this boot's, here as in every real-process test.
    #[test]
    fn a_record_stamped_with_the_writers_boot_is_this_boots() {
        let Some(boot) = written_boot() else {
            eprintln!("skipped: this computer's boot cannot be read");
            return;
        };
        let dir = folder(&running(&boot), Some(4242));
        let got = caller_host_folder(dir.path(), Some(&boot)).unwrap();
        assert_eq!(got, vec![named(4242, Some("77"))]);
    }

    /// When this boot cannot be read, no record is set aside for its boot.
    #[test]
    fn an_unread_boot_sets_nothing_aside() {
        let dir = folder(&running(OTHER), Some(4242));
        assert_eq!(
            caller_host_folder(dir.path(), None).unwrap(),
            vec![named(4242, Some("77"))]
        );
        let got = caller_host_folder(dir.path(), Some(THIS)).unwrap();
        assert_eq!(got, Vec::new(), "another boot, with its own host.pid");
    }

    /// Only the form the writer stamps can set a record aside; a boot in any
    /// other form is unknown, and the record stays named.
    #[test]
    fn only_the_writers_boot_form_sets_a_record_aside() {
        let lettered = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        let dir = folder(&running(lettered), None);
        let got = caller_host_folder(dir.path(), Some(THIS)).unwrap();
        assert_eq!(got, Vec::new(), "the written form of another boot");
        for form in [
            lettered.to_uppercase(),
            format!("{{{lettered}}}"),
            lettered.replace('-', ""),
            "garbage".to_owned(),
        ] {
            let dir = folder(&running(&form), None);
            let got = caller_host_folder(dir.path(), Some(THIS)).unwrap();
            assert_eq!(got, vec![named(4242, Some("77"))], "{form}");
        }
    }

    /// A starting record from another boot sets aside the `host.pid` it
    /// publishes as its PID file; a different `host.pid` stays named.
    #[test]
    fn an_old_boot_starting_record_covers_its_own_host_pid() {
        let starting = |pid_file: u32| {
            json!({"state":"starting","version":2,"attempt":"33333333-3333-4333-8333-333333333333",
                "boot":OTHER,"launcher":{"pid":1u32 << 30,"birth":"1"},"phase":"spawning",
                "pid_file":[pid_file,[1,2,3,4]],"previous":null})
        };
        let dir = folder(&starting(5151), Some(5151));
        assert_eq!(
            caller_host_folder(dir.path(), Some(THIS)).unwrap(),
            Vec::new()
        );
        let dir = folder(&starting(5151), Some(6161));
        assert_eq!(
            caller_host_folder(dir.path(), Some(THIS)).unwrap(),
            vec![named(6161, None)]
        );
        let dir = folder(&starting(5151), Some(5151));
        assert_eq!(
            caller_host_folder(dir.path(), Some(OTHER)).unwrap(),
            vec![named(5151, None)],
            "this boot's own starting record names its PID file once"
        );
    }
}
