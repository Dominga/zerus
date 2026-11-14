//! Read Kimi's own cache decision inputs. No guessed TTLs, network or settings writes.
use super::*;
use std::{fs, io::{Read, Seek, SeekFrom}, path::Path};

const CONFIG_BYTES: u64 = 64 * 1024;
const WIRE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rule { cache_duration: f64, min_tokens_to_hint: f64 }
impl Rule {
    fn read(value: &Value, model: &str, at: f64) -> Option<Self> {
        if value["version"] != 1 || value["config"]["version"] != 1 { return None; }
        let fetched = value["fetchedAt"].as_f64().filter(|n| n.is_finite())?;
        // ClientConfigClient.readDiskCache: cached configs are fresh for one day.
        if at * 1000.0 - fetched >= 86_400_000.0 { return None; }
        let rule = &value["config"]["config"][model];
        Some(Self {
            cache_duration: rule["cache_duration"].as_f64().filter(|n| n.is_finite())?,
            min_tokens_to_hint: rule["min_tokens_to_hint"].as_f64().filter(|n| n.is_finite())?,
        })
    }
}

fn bytes(path: &Path, limit: u64) -> Option<Vec<u8>> {
    keep_warm::read_regular(path,limit).ok()?
}
fn rule(home: &Path, model: &str, at: f64) -> Option<Rule> {
    let entries = fs::read_dir(home.join("cache/client-configs")).ok()?.take(65).collect::<Vec<_>>();
    if entries.len() > 64 { return None; }
    let mut found = None;
    for entry in entries {
        let entry = entry.ok()?; let name = entry.file_name(); let name = name.to_str()?;
        if !name.ends_with("%3A%2Fclient_configs%3Aestimated_cache_duration.json") { continue; }
        let value: Value = serde_json::from_slice(&bytes(&entry.path(), CONFIG_BYTES)?).ok()?;
        if value["fetchedAt"].as_f64().is_some_and(|fetched| at * 1000.0 - fetched >= 86_400_000.0) { continue; }
        let next = Rule::read(&value, model, at)?;
        // Region is not inferred from filenames or account labels. Different
        // fresh region rules are ambiguous and never authorize compaction.
        if found.is_some_and(|old| old != next) { return None; }
        found = Some(next);
    }
    found
}

/// Native 2.6.2 can persist profile.bind without llm.request. Resolve its
/// exact alias through Kimi's own configured upstream model, never alias text.
fn configured_model(home: &Path, alias: &str) -> Option<String> {
    let data = bytes(&home.join("config.toml"),1024*1024)?;
    let config = std::str::from_utf8(&data).ok()?.parse::<toml_edit::DocumentMut>().ok()?;
    let entry = config.get("models")?.get(alias)?;
    let model = entry.get("overrides").and_then(|overrides|overrides.get("model"))
        .or_else(||entry.get("model"))?.as_str()?;
    (!model.is_empty()).then(||model.to_owned())
}

#[derive(Debug, PartialEq)]
struct Snapshot { model: String, tokens: f64 }
fn snapshot(record: &Value) -> Option<Snapshot> {
    let current_model = effort::native_model(record);
    let alias = current_model.as_str();
    // wire_paths has a bounded legacy discovery budget. Require complete
    // directory coverage before using its uniqueness result as cache evidence.
    let count = fs::read_dir(questions::kimi_home(record).join("sessions")).ok()?
        .take(2001).try_fold(0usize, |count,entry| entry.map(|_|count+1)).ok()?;
    if count > 2000 { return None; }
    let paths = questions::wire_paths(record).ok()?;
    let [path] = paths.as_slice() else { return None; };
    let mut options = fs::OpenOptions::new(); options.read(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let mut file = crate::platform::no_follow(&mut options).open(path).ok()?;
    let (before, metadata) = crate::platform::FileIdentity::with_metadata(&file).ok()?;
    if !metadata.is_file() { return None; }
    let length = metadata.len();
    let offset = length.saturating_sub(WIRE_BYTES);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut data = Vec::new(); (&mut file).take(WIRE_BYTES + 1).read_to_end(&mut data).ok()?;
    let (after,metadata) = crate::platform::FileIdentity::with_metadata(&file).ok()?;
    if before != after || metadata.len() != length || crate::platform::FileIdentity::at(path).ok()?.0 != before { return None; }
    if data.len() as u64 != length - offset || !data.ends_with(b"\n") { return None; }
    enum Route { Configured(Option<String>), Observed(Option<String>) }
    let mut route = Route::Configured((!alias.is_empty()).then(||alias.to_owned()));
    let mut tokens = None;
    for (i,line) in data.split(|b| *b == b'\n').enumerate() {
        if (offset > 0 && i == 0) || line.is_empty() { continue; }
        let event: Value = serde_json::from_slice(line).ok()?;
        if event["agentId"] != "main" { continue; }
        if ["profile.bind","config.update"].contains(&string(&event,"type"))
            && (event.get("modelAlias").is_some() || event["type"] == "profile.bind") {
            route = Route::Configured(event["modelAlias"].as_str().filter(|s| !s.is_empty()).map(str::to_owned));
        }
        if event["type"] == "llm.request" && event["kind"] == "loop" {
            let route_alias = event["modelAlias"].as_str().filter(|s| !s.is_empty());
            route = Route::Observed(route_alias.filter(|candidate| alias.is_empty() || *candidate == alias)
                .and_then(|_|event["model"].as_str().filter(|s| !s.is_empty()).map(str::to_owned)));
        }
        if ["token_counting.measured","token_counting.truncated","token_counting.rebased","token_counting.turn_recorded"].contains(&string(&event,"type")) {
            tokens = event["tokens"].as_f64().filter(|n| n.is_finite() && *n >= 0.0);
        }
        if event["type"] == "context.clear" { tokens = Some(0.0); }
    }
    // A profile can precede the bounded tail in a long conversation. The
    // existing effective-model reader also observes the live native footer.
    // Resolve at most once, rather than rereading config for every wire event.
    let model = match route {
        Route::Configured(route_alias) => route_alias.as_deref().filter(|candidate|alias.is_empty() || *candidate == alias)
            .and_then(|candidate|configured_model(&questions::kimi_home(record),candidate)),
        Route::Observed(model) => model,
    };
    Some(Snapshot { model: model?, tokens: tokens? })
}
fn expired(rule: Option<Rule>, last_active: Option<f64>, tokens: Option<f64>, at: f64) -> bool {
    let (Some(rule), Some(last), Some(tokens)) = (rule, last_active, tokens) else { return false; };
    at.is_finite() && last.is_finite() && tokens.is_finite()
        && at - last > rule.cache_duration && tokens >= rule.min_tokens_to_hint
}

pub(super) fn decision(record: &Value) -> Option<Value> {
    if record["agent"] != "kimi" || string(record,"conversation_id").is_empty() { return None; }
    let binding = account_binding(record);
    let config = crate::config::Config::load().ok()?;
    if !crate::accounts::kimi_compact_on_expiry(&config, string(&binding,"id"), Path::new(string(&binding,"home"))).ok()? { return None; }
    let sample = snapshot(record)?; let at = now();
    let native_rule = rule(Path::new(string(&binding,"home")), &sample.model, at)?;
    let last = keep_warm::last_turn_at(record)?;
    expired(Some(native_rule), Some(last), Some(sample.tokens), at).then(|| json!({
        "status":"reserved","source":"kimi_estimated_cache_duration","model":sample.model,
        "run_id":record["run_id"],"conversation_id":record["conversation_id"],
        "cache_duration":native_rule.cache_duration,"min_tokens_to_hint":native_rule.min_tokens_to_hint,
        "last_active_at":last,"tokens":sample.tokens,"checked_at":at
    }))
}

// This private custodian spans the unlocked native hook wait. It never grants
// another hgs process authority to type, and a dead sender does not lock out
// manual recovery. An unreadable birth stays conservative while the PID lives.
const PREPARING: &str = "kimi_pre_send";
pub(super) fn active(record: &Value) -> Option<&str> {
    let marker = &record[PREPARING];
    let pid = u32::try_from(marker["pid"].as_u64()?).ok()?;
    if !crate::platform::process_alive(pid) { return None; }
    let birth = process_start(pid);
    if !birth.is_empty() && marker["process_start"].as_str().is_some_and(|s| !s.is_empty() && s != birth) { return None; }
    marker["request_id"].as_str()
}
pub(super) fn same_identity(original: &Value, current: &Value) -> bool {
    ["session_id","agent","run_id","pane","pid","process_start","conversation_id","expected_id","launch_dir","agent_home","account_id","model"]
        .iter().all(|key| current[*key] == original[*key])
}
pub(super) fn owned(record: &Value) -> bool {
    active(record).is_some() && record[PREPARING]["pid"] == std::process::id()
        && record[PREPARING]["process_start"] == process_start(std::process::id())
}
pub(super) fn reserve(record: &mut Value, request: &str) -> Result<()> {
    record[PREPARING] = json!({"request_id":request,"pid":std::process::id(),
        "process_start":process_start(std::process::id())});
    write(record)
}
pub(super) fn release(record: &mut Value, request: &str, outcome: &Value) -> Result<()> {
    if !owned(record) || record[PREPARING]["request_id"] != request { return Err("Pre-send compaction custodian changed; no message was typed".into()); }
    record.as_object_mut().ok_or("Invalid session record")?.remove(PREPARING);
    let kept = record["last_send_compaction"]["kept_message"].clone();
    record["last_send_compaction"] = outcome.clone();
    record["last_send_compaction"]["kept_message"] = kept;
    write(record)
}

/// Only the ordinary send's reserved receipt calls this. No receipt replay can
/// enter here, so even an uncertain compact is never attempted twice.
pub(super) fn compact(record: &Value, request_id: &str) -> Value {
    let id = uuid::Uuid::new_v4().to_string();
    let result = compact_context::submit(string(record,"name"), compact_context::Request {
        request_id: id.clone(), expected_run_id: string(record,"run_id").into(),
        expected_conversation_id: string(record,"conversation_id").into(),
    }, Some(record));
    let started = std::time::Instant::now();
    let mut outcome = json!({"request_id":request_id,"compact_request_id":id,"status":"unknown"});
    if let Err(reason) = result {
        outcome["reason"] = json!(reason);
        return outcome;
    }
    loop {
        match read_run(string(record,"name"),string(record,"run_id")) {
            Ok(Some(current)) => {
                if !same_identity(record,&current) || !owned(&current) {
                    outcome["reason"] = json!("Session identity or pre-send custodian changed"); break;
                }
                let compact = &current["compact_context_request"];
                if compact["request_id"] != id { outcome["reason"] = json!("Compaction identity changed"); break; }
                match string(compact,"status") {
                    "completed" if compact["superseded_at"].is_null() => {
                        // PostCompact can precede the final native repaint.
                        // Observe readiness within the same bound; never type
                        // the message merely because the completion hook ran.
                        if input::checked_ready(string(record,"name"),string(record,"run_id"),string(record,"conversation_id"))
                            .and_then(|r|input::checked_terminal(&r,true)).is_ok() {
                            outcome["status"] = json!("completed"); break;
                        }
                    }
                    "cancelled" | "failed" | "uncertain" => {
                        outcome["status"] = compact["status"].clone();
                        outcome["reason"] = json!("Native compaction did not confirm success"); break;
                    }
                    _ => {}
                }
            }
            _ => { outcome["reason"] = json!("Session run disappeared during compaction"); break; }
        }
        if started.elapsed() >= std::time::Duration::from_secs(20) {
            outcome["status"] = json!("timeout"); outcome["reason"] = json!("Compaction did not complete within 20 seconds"); break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    outcome
}

/// The payload stays in its original receipt if the ordinary guards refuse
/// after compaction. The usual delivery-not-confirmed error keeps the person's
/// composer draft; inspection also provides this bounded copy for manual send.
pub(super) fn refused(path: &Path, reason: String) -> String {
    let Some(data) = bytes(path,512*1024) else { return reason; };
    let Ok(mut receipt) = serde_json::from_slice::<Value>(&data) else {return reason;};
    if !receipt["pre_send_compaction"].is_object() {return reason;}
    let reason = format!("Delivery not confirmed after pre-send compaction: {reason}. Your original message is kept; clear Terminal manually, then send it by hand. Do not replay this request automatically");
    receipt["delivery"] = json!("not_confirmed");
    receipt["uncertain_reason"] = json!(reason);
    let _ = receipt_index::persist(path,&receipt);
    reason
}

#[cfg(test)] mod tests {
    use super::*;
    fn envelope() -> Value { json!({"version":1,"fetchedAt":1000000,
        "config":{"version":1,"config":{"k3":{"cache_duration":60,"min_tokens_to_hint":100}}}}) }
    #[test] fn profile_bind_resolves_the_configured_model_without_llm_trace_events() {
        let dir=tempfile::tempdir().unwrap();
        let record=json!({"agent":"kimi","agent_home":dir.path(),"conversation_id":"fixture-cid","model":"Alias"});
        let path=dir.path().join("sessions/workspace/fixture-cid/agents/main/wire.jsonl");fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path,format!("{}\n{}\n",json!({"type":"profile.bind","agentId":"main","modelAlias":"Alias"}),
            json!({"type":"token_counting.turn_recorded","agentId":"main","tokens":277,"length":4,"turnId":2}))).unwrap();
        assert!(snapshot(&record).is_none(),"missing model mapping never guesses from the alias");
        let config=dir.path().join("config.toml");
        fs::write(&config,"[models.Alias]\nprovider = \"fixture\"\nmodel = \"upstream-k3\"\n").unwrap();
        assert_eq!(snapshot(&record),Some(Snapshot{model:"upstream-k3".into(),tokens:277.0}));
        fs::write(&config,"models = { Alias = { model = \"upstream-k3\", provider = \"fixture\" } }\n").unwrap();
        assert_eq!(snapshot(&record).unwrap().model,"upstream-k3");
        fs::write(&config,"[models.Alias]\nmodel = \"base\"\n[models.Alias.overrides]\nmodel = \"overridden\"\n").unwrap();
        assert_eq!(snapshot(&record).unwrap().model,"overridden");
        let padded=format!("{}\n{}\n",json!({"type":"fixture.padding","agentId":"main","text":"x".repeat(WIRE_BYTES as usize+100)}),
            json!({"type":"token_counting.turn_recorded","agentId":"main","tokens":300}));
        fs::write(&path,padded).unwrap();assert_eq!(snapshot(&record),Some(Snapshot{model:"overridden".into(),tokens:300.0}));
        fs::write(&config,"[models.Other]\nmodel = \"wrong\"\n").unwrap();assert!(snapshot(&record).is_none());
    }
    #[test] fn current_native_tokens_and_upstream_model_are_bound_to_the_cid() {
        let dir = tempfile::tempdir().unwrap();
        let record = json!({"agent":"kimi","agent_home":dir.path(),"conversation_id":"fixture-cid","model":"K3"});
        let path=dir.path().join("sessions/workspace/fixture-cid/agents/main/wire.jsonl");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let events = [json!({"type":"llm.request","agentId":"main","kind":"loop","modelAlias":"K3","model":"k3"}),
            json!({"type":"token_counting.measured","agentId":"main","tokens":1000}),
            json!({"type":"token_counting.turn_recorded","agentId":"main","tokens":1100}),
            json!({"type":"token_counting.measured","agentId":"child","tokens":99999})];
        let mut body = events.iter().map(|event|format!("{event}\n")).collect::<String>();
        fs::write(&path,&body).unwrap();
        assert_eq!(snapshot(&record),Some(Snapshot {model:"k3".into(),tokens:1100.0}));
        let mut missing=record.clone();missing.as_object_mut().unwrap().remove("model");
        assert_eq!(snapshot(&missing),snapshot(&record),"the native route supplies a default model alias");
        let no_alias=body.replace("\"modelAlias\":\"K3\",", "");
        fs::write(&path,no_alias).unwrap();assert!(snapshot(&missing).is_none());fs::write(&path,&body).unwrap();
        for (kind,count) in [("token_counting.truncated",30),("token_counting.rebased",40)] {
            body.push_str(&format!("{}\n",json!({"type":kind,"agentId":"main","tokens":count})));
            fs::write(&path,&body).unwrap();assert_eq!(snapshot(&record).unwrap().tokens,f64::from(count));
        }
        // A profile change after the request invalidates the upstream route.
        body.push_str(&format!("{}\n",json!({"type":"profile.bind","agentId":"main","modelAlias":"Other"})));
        fs::write(&path,&body).unwrap();assert!(snapshot(&record).is_none());
        fs::write(&path,"{partial").unwrap();assert!(snapshot(&record).is_none());
    }
    #[test] fn cached_regions_must_agree_and_missing_fresh_rules_skip() {
        let dir=tempfile::tempdir().unwrap();let folder=dir.path().join("cache/client-configs");fs::create_dir_all(&folder).unwrap();
        let a=folder.join("mainland-cn%3A%2Fclient_configs%3Aestimated_cache_duration.json");
        let b=folder.join("global%3A%2Fclient_configs%3Aestimated_cache_duration.json");
        fs::write(&a,envelope().to_string()).unwrap();assert!(rule(dir.path(),"k3",1000.0).is_some());
        fs::write(&b,envelope().to_string()).unwrap();assert!(rule(dir.path(),"k3",1000.0).is_some());
        let mut other=envelope();other["config"]["config"]["k3"]["cache_duration"]=json!(1);
        fs::write(&b,other.to_string()).unwrap();assert!(rule(dir.path(),"k3",1000.0).is_none());
        other["config"]["config"]=json!({});fs::write(&b,other.to_string()).unwrap();assert!(rule(dir.path(),"k3",1000.0).is_none());
    }
    #[test] fn matches_the_native_rule_and_missing_inputs_skip() {
        let rule = Rule::read(&envelope(), "k3", 1000.0);
        assert!(expired(rule,Some(900.0),Some(100.0),1000.0));
        assert!(!expired(rule,Some(940.0),Some(100.0),1000.0));
        assert!(!expired(rule,Some(900.0),Some(99.0),1000.0));
        for (r,last,tokens) in [(None,Some(0.0),Some(1000.0)),(rule,None,Some(1000.0)),(rule,Some(0.0),None)] {
            assert!(!expired(r,last,tokens,1000.0));
        }
        assert!(Rule::read(&envelope(),"absent",1000.0).is_none());
        assert!(Rule::read(&envelope(),"k3",87400.0).is_none());
        for key in ["cache_duration","min_tokens_to_hint"] {
            let mut malformed=envelope();malformed["config"]["config"]["k3"].as_object_mut().unwrap().remove(key);
            assert!(Rule::read(&malformed,"k3",1000.0).is_none());
        }
    }
}
