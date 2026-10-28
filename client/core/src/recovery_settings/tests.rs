// Mirrors tray/tests/test_sessionswindow.cpp sharedRecoverySyncCatchesUpOfflinePeer
// and recoverySettingsValidateAndSaveForSelectedMachine.
use super::*;

fn map(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap()
}

fn latest() -> Map<String, Value> {
    map(json!({
        "version": 3, "revision": 4, "writer": "local-writer", "enabled": false, "service": true,
        "network": true, "rate_limit": true, "session_limit": true, "session_limit_mode": "interval",
        "delays": [15, 30, -1], "schedules": {"network": [5, 0], "session_limit": [600, 0]},
    }))
}

fn envelope(policy: &Map<String, Value>) -> Map<String, Value> {
    map(json!({"order": [policy["revision"], policy["writer"]], "policy": policy}))
}

fn calls(effects: &[Effect]) -> Vec<Call> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Call(call) => Some(call.clone()),
            Effect::Persist(_) => None,
        })
        .collect()
}

/// Answers every call: `get` with the machine's policy, `sync` taking the
/// incoming one when newer, as hgs does; an offline machine fails.
struct Machines {
    policies: BTreeMap<String, Map<String, Value>>,
    offline: Vec<String>,
    persisted: Vec<String>,
}

impl Machines {
    fn run(&mut self, sync: &mut RecoverySync, mut effects: Vec<Effect>) {
        while let Some(effect) = effects.pop() {
            let call = match effect {
                Effect::Call(call) => call,
                Effect::Persist(text) => {
                    self.persisted.push(text);
                    continue;
                }
            };
            let reply = if self.offline.contains(&call.host) {
                Err("ssh: connect to host mac port 22: Connection refused".to_owned())
            } else {
                let current = self.policies[&call.host].clone();
                if call.op == Op::Sync
                    && compare(&envelope(&call.data), &envelope(&current)) == Ordering::Greater
                {
                    self.policies.insert(call.host.clone(), call.data.clone());
                }
                Ok(envelope(&self.policies[&call.host]))
            };
            effects.extend(sync.answered(call.id, reply));
        }
    }
}

#[test]
fn an_offline_peer_catches_up_and_a_newer_peer_wins() {
    let mut older = latest();
    older.insert("revision".into(), json!(2));
    older.insert("writer".into(), json!("peer"));
    let mut machines = Machines {
        policies: [(String::new(), latest()), ("mac".to_owned(), older)]
            .into_iter()
            .collect(),
        offline: vec!["mac".into()],
        persisted: Vec::new(),
    };
    let mut sync = RecoverySync::new("");
    let effects = sync.set_peers(vec!["mac".into()]);
    assert_eq!(calls(&effects).len(), 2);
    machines.run(&mut sync, effects);
    assert!(!sync.busy());
    assert_eq!(sync.envelope()["policy"], json!(latest()));
    assert!(sync.status().contains("mac"), "{}", sync.status());
    let form = form(sync.envelope()).unwrap();
    assert_eq!(form.session_limit_mode, "interval");
    assert_eq!(form.delays["session_limit"], "600, 0");
    assert_eq!(machines.persisted.len(), 1);

    machines.offline.clear();
    let effects = sync.refresh();
    machines.run(&mut sync, effects);
    assert_eq!(machines.policies["mac"], latest());
    assert_eq!(
        sync.status(),
        "Synchronized across 2 machines. Changes from any Zerus are shared."
    );

    let mut newer = latest();
    newer.insert("revision".into(), json!(5));
    newer.insert("writer".into(), json!("remote-writer"));
    newer.insert("service".into(), json!(false));
    machines.policies.insert("mac".into(), newer.clone());
    let effects = sync.refresh();
    machines.run(&mut sync, effects);
    assert_eq!(machines.policies[""], newer);

    let stale = envelope(&latest());
    assert!(sync.save(latest(), &stale).is_empty());
    assert!(sync.error().contains("another Zerus"));
}

#[test]
fn the_form_loads_qts_defaults_and_saves_the_base_policy_with_the_changes() {
    let base = map(
        json!({"order": [7, "writer"], "policy": {"version": 2, "revision": 7, "writer": "writer",
        "enabled": false, "enabled_at": 0, "service": true, "network": true, "rate_limit": true,
        "delays": [15, 30, 60, 300]}}),
    );
    let mut form = form(&base).unwrap();
    assert!(!form.enabled);
    assert_eq!(form.delays["network"], "15, 30, 60, 300, -1");
    assert!(form.rules["session_limit"]);
    assert_eq!(form.session_limit_mode, "reset");
    assert_eq!(form.delays["session_limit"], "300, 0");

    for mode in ["reset", "interval"] {
        form.enabled = true;
        form.session_limit_mode = mode.into();
        if mode == "interval" {
            form.delays.insert("session_limit".into(), "600, 0".into());
        }
        form.delays.insert("network".into(), "0".into());
        assert!(policy(&base, &form)
            .unwrap_err()
            .starts_with("Use 1–16 values"));
        form.delays.insert("network".into(), "5, 20, 0".into());
        let saved = policy(&base, &form).unwrap();
        assert_eq!(saved["enabled"], true);
        assert_eq!(saved["revision"], 7);
        assert_eq!(saved["writer"], "writer");
        assert_eq!(saved["enabled_at"], 0);
        assert_eq!(saved["version"], 3);
        assert_eq!(saved["session_limit"], true);
        assert_eq!(saved["session_limit_mode"], mode);
        let session = if mode == "interval" { 600 } else { 300 };
        assert_eq!(saved["schedules"]["session_limit"], json!([session, 0]));
        assert_eq!(saved["schedules"]["network"], json!([5, 20, 0]));
        assert_eq!(saved["schedules"]["service"], json!([15, 30, 60, 300, -1]));
    }
}

#[test]
fn delays_follow_qts_rule() {
    let valid = |text: &str| delays(text).is_some();
    assert!(valid("15, 30, 60, 300, -1"));
    assert!(valid("300, 0"));
    assert!(valid("86400"));
    assert!(valid(" +5 ,7"));
    assert!(!valid("0"));
    assert!(!valid("-1, 5"));
    assert!(!valid("15, 0, 30"));
    assert!(!valid("86401"));
    assert!(!valid("15,"));
    assert!(!valid(""));
    assert!(!valid("x"));
    assert!(!valid(&vec!["5"; 17].join(", ")));
    assert!(valid(&vec!["5"; 16].join(", ")));
}

#[test]
fn a_save_goes_to_one_reachable_machine_and_reports_its_failure() {
    let mut sync = RecoverySync::new(&Value::Object(envelope(&latest())).to_string());
    assert_eq!(sync.envelope()["order"], json!([4, "local-writer"]));
    let base = sync.envelope().clone();
    let effects = sync.save(latest(), &base);
    let call = calls(&effects).remove(0);
    assert_eq!((call.host.as_str(), call.op), ("", Op::Set));
    assert!(!sync.can_save());
    assert!(sync.save(latest(), &base).is_empty());
    assert_eq!(
        sync.error(),
        "Wait for synchronization to finish before saving."
    );
    sync.answered(
        call.id,
        Err("Recovery settings changed on this machine; reload them".into()),
    );
    assert_eq!(
        sync.error(),
        "Recovery settings changed on this machine; reload them"
    );

    // A control-only computer saves to a reachable peer, and without one cannot.
    let mut control = RecoverySync::new(&Value::Object(envelope(&latest())).to_string());
    let effects = control.set_local_sessions(false);
    assert!(effects.is_empty());
    assert_eq!(
        control.status(),
        "Add a remote machine in Machines to configure automatic recovery; this computer runs no agents."
    );
    control.set_local_sessions_offered(true);
    assert!(control
        .status()
        .starts_with("To configure automatic recovery, enable local sessions"));
    assert!(control.save(latest(), &base).is_empty());
    assert_eq!(
        control.error(),
        "Connect a remote machine to save recovery settings."
    );
    let effects = control.set_peers(vec!["mac".into(), String::new(), "mac".into()]);
    let read = calls(&effects);
    assert_eq!(read.len(), 1);
    control.answered(read[0].id, Ok(envelope(&latest())));
    assert_eq!(
        control.status(),
        "Synchronized with 1 machine. Changes from any Zerus are shared."
    );
    let call = calls(&control.save(latest(), &base)).remove(0);
    assert_eq!((call.host.as_str(), call.op), ("mac", Op::Set));
}

#[test]
fn an_older_hgs_is_named_and_a_stored_policy_must_be_whole() {
    let mut sync = RecoverySync::new(r#"{"order": [1], "policy": {}}"#);
    assert!(sync.envelope().is_empty());
    assert!(form(sync.envelope()).is_none());
    let call = calls(&sync.refresh()).remove(0);
    sync.answered(call.id, Ok(map(json!({"enabled": true}))));
    assert!(sync.status().starts_with("Waiting for this machine."));
    assert!(!sync.can_save());
}

#[test]
fn calls_and_answers_read_as_hgs_client_reads_them() {
    let call = Call {
        id: 1,
        host: "mac".into(),
        op: Op::Sync,
        data: latest(),
    };
    let invocation = invocation(&call);
    assert_eq!(invocation.args, ["@mac", "recovery", "sync"]);
    assert_eq!(invocation.timeout, Duration::from_secs(25));
    let sent: Value = serde_json::from_slice(invocation.stdin.as_ref().unwrap()).unwrap();
    assert_eq!(sent, json!(latest()));
    let local = invocation_of(Op::Get);
    assert_eq!(local.args, ["recovery", "get"]);
    assert_eq!(local.timeout, Duration::from_secs(10));
    assert!(local.stdin.is_none());

    assert_eq!(
        answer(Exit::TimedOut).unwrap_err(),
        "Recovery request timed out. Reload its state before trying again."
    );
    assert_eq!(
        answer(Exit::Code {
            code: 1,
            stdout: Vec::new(),
            stderr: "  ".into()
        })
        .unwrap_err(),
        "Could not update recovery. Reload its state."
    );
    assert_eq!(
        answer(Exit::Code {
            code: 1,
            stdout: Vec::new(),
            stderr: " reload them \n".into()
        })
        .unwrap_err(),
        "reload them"
    );
    assert_eq!(
        answer(Exit::Code {
            code: 0,
            stdout: b"[1]".to_vec(),
            stderr: String::new()
        })
        .unwrap_err(),
        "Invalid recovery response"
    );
    assert_eq!(
        answer(Exit::Code {
            code: 0,
            stdout: br#"{"order":[1,"w"],"policy":{}}"#.to_vec(),
            stderr: String::new()
        })
        .unwrap()["order"],
        json!([1, "w"])
    );
}

fn invocation_of(op: Op) -> Invocation {
    invocation(&Call {
        id: 1,
        host: String::new(),
        op,
        data: Map::new(),
    })
}
