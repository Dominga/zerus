use super::*;
use serde_json::json;
use std::{
    fs, io,
    os::fd::OwnedFd,
    process::{Command, Stdio},
};

const PROBE: &str = "state::terminating::writer_records::admission::source_io_tests::private_probe";
const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const RUN: &str = "22222222-2222-4222-8222-222222222222";
const REQUEST: &str = "33333333-3333-4333-8333-333333333333";
fn end() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
fn scope() -> Run {
    Run::new(SESSION, RUN).unwrap()
}
fn probe(case: &str) {
    let home = tempfile::tempdir().unwrap();
    reexec(case, home.path());
}
fn reexec(case: &str, home: &Path) {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            PROBE,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("HGS_CONFIG_DIR", home.join("config"))
        .env("HGS_STATE_DIR", home.join("state"))
        .env("HGS_TEST_SOURCE_IO", case);
    let (status, out, err) = platform::run_bounded(
        &mut command,
        [64 << 10; 2],
        Instant::now() + Duration::from_secs(15),
        &|| false,
    )
    .unwrap();
    assert!(
        status.success(),
        "{case}: {} {}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    );
    assert!(String::from_utf8_lossy(&out).contains("test result: ok. 1 passed; 0 failed;"));
}
fn gate(held: bool) {
    reexec(
        if held { "gate-held" } else { "gate-free" },
        storage::root().parent().unwrap(),
    );
}
fn wait(mut done: impl FnMut() -> bool) {
    let until = end();
    while !done() {
        assert!(
            Instant::now() < until,
            "original helper did not physically settle"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn command(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", script])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn finish(exchange: &mut SourceIoExchange) -> SourceIoSettled {
    let mut settled = None;
    wait(|| {
        settled = exchange.poll();
        settled.is_some()
    });
    assert!(exchange.poll().is_none());
    settled.unwrap()
}
fn save(at: &Path, record: &Value) {
    storage::atomic(at, &serde_json::to_string(record).unwrap()).unwrap();
}
fn ledger() -> PathBuf {
    storage::root()
        .join("terminating-writers")
        .join(SESSION)
        .join(RUN)
        .join("pending.json")
}
fn change_row() {
    let mut value: Value = serde_json::from_slice(&fs::read(ledger()).unwrap()).unwrap();
    value["pending"][0]["intent_sha256"] = json!("c".repeat(64));
    fs::write(ledger(), serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn one_paired_source_row_child_and_deadline_reach_the_original_input_once() {
    probe("echo");
    probe("id");
    probe("metadata");
}
#[test]
fn input_checks_source_on_both_sides_of_the_original_native_check() {
    for case in [
        "source-before",
        "source-during",
        "marker-during",
        "native-refusal",
        "row-during",
    ] {
        probe(case);
    }
}
#[test]
fn explicit_empty_handoff_has_the_same_source_brackets_and_no_replay() {
    for case in [
        "empty",
        "empty-source-before",
        "empty-source-during",
        "empty-marker-during",
        "empty-native-refusal",
        "empty-row-during",
    ] {
        probe(case);
    }
}
#[test]
fn pre_spawn_refusal_returns_the_same_pair_and_never_retires_its_row() {
    for case in [
        "admission-source",
        "admission-writer",
        "admission-row",
        "admission-expired",
    ] {
        probe(case);
    }
}
#[test]
fn handed_off_source_and_row_cannot_be_replaced_by_a_different_writer_or_budget() {
    probe("foreign-writer");
    probe("expired");
}
#[test]
fn a_withheld_original_child_never_runs_a_native_check_or_releases_input() {
    probe("withheld");
}
#[test]
fn source_and_native_loan_survive_exit_and_drop_until_original_output_eof() {
    probe("held-output");
    probe("drop-output");
}
#[test]
fn cancellation_keeps_source_and_native_loan_until_original_input_join() {
    probe("held-input");
}

#[test]
#[ignore = "bounded private source/Child reexec; no controller or real agent"]
fn private_probe() {
    let case = std::env::var("HGS_TEST_SOURCE_IO").unwrap();
    if matches!(case.as_str(), "gate-held" | "gate-free") {
        let store = Store::open(&scope(), end()).unwrap();
        assert_eq!(
            store
                .exclusive(Instant::now() + Duration::from_millis(50))
                .is_err(),
            case == "gate-held"
        );
        return;
    }
    drop(storage::lock(None).unwrap());
    let mut record = json!({"version":1,"name":"codex/fixture/source-io","session_id":SESSION,"run_id":RUN,"conversation_id":"original-conversation","agent":"codex","pid":4242,"process_start":"saved root correlation","supervisor":{"pid":4343,"start":"saved supervisor correlation"},"pane":"%123","terminal_name":"fixture-terminal","cwd":"/fixture/work","launch_dir":"/fixture/work","hgs":"/fixture/bin/hgs"});
    let at = if case == "id" {
        record["layout"] = json!("id");
        let at = storage::id_record_path(SESSION).unwrap();
        platform::private_dir(at.parent().unwrap()).unwrap();
        at
    } else {
        storage::label_record_path(record["name"].as_str().unwrap())
    };
    save(&at, &record);
    initialize(&scope(), end()).unwrap();
    let writer = storage::writer_until(end()).unwrap().unwrap();
    let deadline = if matches!(case.as_str(), "expired" | "admission-expired") {
        Instant::now() + Duration::from_millis(100)
    } else {
        end()
    };
    let request = Request::new(REQUEST, Kind::Input, &"a".repeat(64), &"b".repeat(64)).unwrap();
    let original = OriginalRequest::capture(&writer, &record, request, deadline)
        .unwrap()
        .reserve(&writer)
        .unwrap();
    let source = Arc::downgrade(&original.source);
    let loan = Arc::downgrade(original.reservation.lease.as_ref().unwrap());
    assert_eq!(original.reservation.entry.request, REQUEST);
    assert_eq!(original.reservation.until, deadline);
    assert_eq!(original.source.until, deadline);
    if case.starts_with("admission-") {
        let aux = storage::lock(Some(&storage::root().join("aux.lock"))).unwrap();
        match case.as_str() {
            "admission-source" => {
                record["conversation_id"] = json!("replacement");
                save(&at, &record);
            }
            "admission-row" => change_row(),
            "admission-expired" => wait(|| Instant::now() >= deadline),
            _ => {}
        }
        let (returned, failure) = match original.into_io(if case == "admission-writer" {
            &aux
        } else {
            &writer
        }) {
            Ok(_) => panic!("invalid pre-spawn source was admitted"),
            Err(refused) => refused,
        };
        assert_eq!(
            failure,
            if case == "admission-expired" {
                Failure::Expired
            } else {
                Failure::SourceChanged
            }
        );
        assert!(Arc::ptr_eq(&source.upgrade().unwrap(), &returned.source));
        assert!(Arc::ptr_eq(
            &loan.upgrade().unwrap(),
            returned.reservation.lease.as_ref().unwrap()
        ));
        assert_eq!(returned.reservation.entry.request, REQUEST);
        assert_eq!(returned.reservation.until, deadline);
        assert_eq!(pending(&scope(), end()).unwrap().len(), 1);
        drop(returned);
        assert_eq!(source.strong_count(), 0);
        assert_eq!(loan.strong_count(), 0);
        assert_eq!(pending(&scope(), end()).unwrap().len(), 1);
        return;
    }
    let admission = original
        .into_io(&writer)
        .unwrap_or_else(|(_, why)| panic!("{why:?}"));
    assert_eq!(
        source.strong_count(),
        2,
        "wrapper and original Core charge keep the SAME source"
    );
    if matches!(case.as_str(), "held-output" | "drop-output" | "held-input") {
        let (read, write) = io::pipe().unwrap();
        let mut read = Some(read);
        let mut write = Some(write);
        let mut cmd = command(if case == "held-input" {
            "exec /bin/sleep 30"
        } else {
            "read line"
        });
        if case == "held-input" {
            cmd.stdin(Stdio::from(read.as_ref().unwrap().try_clone().unwrap()));
        } else {
            cmd.stdout(Stdio::from(write.as_ref().unwrap().try_clone().unwrap()));
        }
        let mut child = cmd.spawn().unwrap();
        let pid = child.id();
        let birth = platform::process_identity(pid).unwrap().birth;
        if case == "held-input" {
            child.stdin = Some(OwnedFd::from(write.take().unwrap()).into());
        } else {
            child.stdout = Some(OwnedFd::from(read.take().unwrap()).into());
        }
        drop(cmd);
        let mut exchange = admission.attach(child);
        if case == "held-input" {
            exchange
                .send(&writer, vec![b'x'; 1 << 20], |_, until| until == deadline)
                .unwrap();
            exchange.cancel();
        } else {
            exchange
                .send(&writer, b"finish\n".to_vec(), |_, until| until == deadline)
                .unwrap();
        }
        wait(|| {
            assert!(exchange.poll().is_none());
            platform::process_identity(pid).is_none_or(|now| now.birth != birth)
        });
        gate(true);
        assert_eq!(pending(&scope(), end()).unwrap().len(), 1);
        assert_eq!(source.strong_count(), 2);
        if case == "drop-output" {
            drop(exchange);
            assert_eq!(
                source.strong_count(),
                1,
                "unfinished Core retirement retains the exact Source"
            );
            gate(true);
            // The remaining local pipe end is released below, before waiting
            // for source/lease release. No PID or replacement Child is adopted.
            drop(write.take());
            wait(|| source.strong_count() == 0 && loan.strong_count() == 0);
            gate(false);
            assert_eq!(pending(&scope(), end()).unwrap().len(), 1);
            return;
        }
        if case == "held-input" {
            drop(read.take());
        } else {
            drop(write.take());
        }
        let mut settled = finish(&mut exchange);
        assert_eq!(source.strong_count(), 1);
        if case == "held-input" {
            assert_eq!(settled.report().failure, Some(Failure::Cancelled));
            assert!(!settled.report().input_written);
        } else {
            assert!(settled.report().exit.success());
        }
        settled.retire(end()).unwrap();
        assert!(pending(&scope(), end()).unwrap().is_empty());
        drop(settled);
        assert_eq!(source.strong_count(), 0);
        return;
    }
    let child = command("exec /bin/cat").spawn().unwrap();
    let pid = child.id();
    let birth = platform::process_identity(pid).unwrap().birth;
    let mut exchange = if case == "withheld" {
        admission.attach_withheld(child)
    } else {
        admission.attach(child)
    };
    let is_empty = case.starts_with("empty");
    let success = matches!(case.as_str(), "echo" | "id" | "metadata" | "empty");
    if case.ends_with("source-before") {
        record["pid"] = json!(5252);
        save(&at, &record);
    }
    if case == "expired" {
        wait(|| Instant::now() >= deadline);
    }
    let aux = storage::lock(Some(&storage::root().join("aux.lock"))).unwrap();
    let mut checks = 0;
    let check = |original: &mut Child, until| {
        checks += 1;
        assert_eq!(original.id(), pid);
        assert_eq!(platform::process_identity(pid).unwrap().birth, birth);
        assert_eq!(until, deadline);
        if case.ends_with("source-during") {
            record["conversation_id"] = json!("replacement");
            save(&at, &record);
        }
        if case.ends_with("marker-during") {
            record["terminating"] = Value::Null;
            save(&at, &record);
        }
        if case.ends_with("row-during") {
            change_row();
        }
        if case == "metadata" {
            record["last_message"] = json!("original completion");
            save(&at, &record);
        }
        !case.ends_with("native-refusal")
    };
    let active_writer = if case == "foreign-writer" {
        &aux
    } else {
        &writer
    };
    let result = if is_empty {
        exchange.close_empty_input(active_writer, check)
    } else {
        exchange.send(active_writer, b"private original input".to_vec(), check)
    };
    let failure = if case == "expired" {
        Failure::Expired
    } else {
        Failure::SourceChanged
    };
    assert_eq!(result, if success { Ok(()) } else { Err(failure) });
    assert_eq!(
        checks,
        usize::from(
            !case.ends_with("source-before")
                && !matches!(case.as_str(), "foreign-writer" | "expired" | "withheld")
        )
    );
    assert!(exchange
        .send(&writer, b"replay".to_vec(), |_, _| panic!(
            "second input check"
        ))
        .is_err());
    let mut settled = finish(&mut exchange);
    assert_eq!(settled.report().input_written, success);
    assert_eq!(
        settled.report().stdout,
        if success && !is_empty {
            b"private original input".as_slice()
        } else {
            b""
        }
    );
    assert_eq!(
        settled.report().failure,
        if success { None } else { Some(failure) }
    );
    assert_eq!(
        pending(&scope(), end()).unwrap().len(),
        1,
        "physical exit never retires a row"
    );
    if case.ends_with("row-during") {
        assert!(settled.retire(end()).is_err());
    } else {
        settled.retire(end()).unwrap();
        assert!(pending(&scope(), end()).unwrap().is_empty());
    }
    drop(settled);
    assert_eq!(source.strong_count(), 0);
    assert_eq!(loan.strong_count(), 0);
}
