//! Opt-in RAM-only accounting. No owner payload reaches a journal or HTTP.
use super::*;
use std::cell::Cell;
use std::collections::VecDeque;
use std::io::Read;

#[path = "../../../../../../src/state/public_listing.rs"]
mod public_listing;

type Checked<T> = std::result::Result<T, &'static str>;
type Responses = Rc<RefCell<BTreeMap<String, VecDeque<NativeResult>>>>;
thread_local! {
    static PLAINTEXT: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

pub(super) fn capture(plain: &[u8]) -> bool {
    PLAINTEXT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(value) = slot.as_mut() {
            *value = plain.to_vec();
            true
        } else {
            false
        }
    })
}

struct NativeAccounting {
    listing: Value,
    process: Option<crate::native::Process>,
    responses: Responses,
    retained_bytes: Cell<usize>,
    counts: RefCell<BTreeMap<&'static str, usize>>,
    failed: Cell<bool>,
}

fn category(argv: &[String]) -> Option<&'static str> {
    let argv = if argv.first().is_some_and(|v| v == "--expect-machine") {
        argv.get(2..)?
    } else {
        argv
    };
    let words: Vec<_> = argv.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["--help"] => Some("help"),
        ["mobile", "identity", "--json"] => Some("identity"),
        ["ls", "--json", "--local"] | ["ls", "--json", "--local", "--no-archives"] => {
            Some("listing")
        }
        ["swarm", "get"] => Some("projects"),
        ["task", "digests", "--json"] => Some("task_digests"),
        ["account", "ls"] => Some("accounts"),
        ["account", "inspect", _] => Some("account_refresh"),
        ["archives", "--json"] => Some("archives"),
        ["inspect", _] | ["inspect", _, "--session-id", _] => Some("attention"),
        _ => None,
    }
}

impl Native for NativeAccounting {
    async fn call(&self, call: Call) -> NativeResult {
        let Some(kind) = category(&call.argv) else {
            self.failed.set(true);
            return Err(NativeError::Failed(
                "accounting refused a non-allowlisted call",
            ));
        };
        *self.counts.borrow_mut().entry(kind).or_default() += 1;
        if kind == "listing" {
            return Ok(Output::Json(self.listing.clone()));
        }
        let key = encoded(&json!([call.argv, call.payload, call.json_output]))
            .map_err(|_| NativeError::Failed("accounting call encoding failed"))?;
        let key = String::from_utf8(key)
            .map_err(|_| NativeError::Failed("accounting call encoding failed"))?;
        if let Some(process) = &self.process {
            let result = process.call(call).await;
            let bytes = match &result {
                Ok(Output::Json(value)) => encoded(value).map(|v| v.len()).unwrap_or(usize::MAX),
                Ok(Output::Text(value)) => value.len(),
                Err(_) => 0,
            };
            let total = self.retained_bytes.get().saturating_add(bytes);
            if total > 64 * 1024 * 1024 || self.responses.borrow().len() >= 64 {
                self.failed.set(true);
                return Err(NativeError::Failed("accounting response budget exceeded"));
            }
            self.retained_bytes.set(total);
            self.responses
                .borrow_mut()
                .entry(key)
                .or_default()
                .push_back(result.clone());
            result
        } else {
            let result = self
                .responses
                .borrow_mut()
                .get_mut(&key)
                .and_then(VecDeque::pop_front);
            match result {
                Some(result) => result,
                None => {
                    self.failed.set(true);
                    Err(NativeError::Failed("accounting replay did not match"))
                }
            }
        }
    }
}

fn strip(value: &Value) -> Checked<Value> {
    let mut value = value.clone();
    let rows = value["sessions"].as_array_mut().ok_or("invalid sessions")?;
    public_listing::project(rows);
    Ok(value)
}

fn identities(value: &Value) -> Checked<BTreeMap<String, Value>> {
    let mut ids = BTreeMap::new();
    for row in value["sessions"].as_array().ok_or("invalid sessions")? {
        let name = row["name"]
            .as_str()
            .filter(|v| !v.is_empty())
            .ok_or("invalid identity")?;
        // A display name can be shared by a current launch and its archives,
        // or by a saved binding and a genuinely different native launch.
        // This is only an equality/count oracle, never action authority.
        let identity = json!([
            name,
            row["session_id"],
            row["run_id"],
            row["conversation_id"],
            row["archive_id"]
        ]);
        let key = serde_json::to_string(&identity).map_err(|_| "identity encoding failed")?;
        if ids.insert(key, identity).is_some() {
            return Err("duplicate identity");
        }
    }
    Ok(ids)
}

fn measure(value: &Value) -> Checked<Value> {
    let bytes = encoded(value).map_err(|_| "encoding failed")?;
    Ok(json!({"bytes":bytes.len(),"sha256":zerus_relay::json::digest(&bytes)}))
}

fn oracle(value: &Value) -> Checked<Value> {
    let mut value = value.clone();
    for row in value["sessions"].as_array_mut().ok_or("invalid sessions")? {
        let row = row.as_object_mut().ok_or("invalid row")?;
        // Independent allowed-difference oracle, not the production field loop.
        for key in [
            "recovery_user_text",
            "recovery_user_at",
            "recovery_success_at",
            "recovery_cancel_at",
            "keep_warm_prompt_at",
            "model_options",
        ] {
            row.remove(key);
        }
    }
    Ok(value)
}

struct Metadata<'a> {
    machine: &'a str,
    name: &'a str,
    binding: &'a str,
    via: &'a Value,
    wall: f64,
}

async fn augmented(
    f: &Fixture,
    native: NativeAccounting,
    metadata: &Metadata<'_>,
) -> Checked<(Value, Value, Value)> {
    let Metadata {
        machine,
        name,
        binding,
        via,
        wall,
    } = *metadata;
    let journal = Journal::open(
        &f._temporary.path().join(if native.process.is_some() {
            "original"
        } else {
            "projected"
        }),
        "accounting-private",
        wall,
    )
    .map_err(|_| "private journal unavailable")?;
    journal
        .encrypted_init(wall)
        .map_err(|_| "private journal unavailable")?;
    let connector = Rc::new(Connector::new(
        native,
        journal,
        Settings {
            interval: 1.0,
            binding: binding.into(),
            default_timeout: 5.0,
            clock_scale: 1.0,
        },
        Box::new(move || wall),
    ));
    let snapshot =
        Connector::<NativeAccounting>::encrypted_guarded(machine.into(), connector.snapshot())
            .await
            .map_err(|_| "snapshot augmentation failed")?;
    connector.stopping.set();
    if connector.native.failed.get() {
        return Err("native allowlist or replay refused");
    }
    let engine = Engine {
        connector: &connector,
        relay: &f.relay,
        private_dir: &f.private_dir,
        profile: RefCell::new(Rc::new(
            workspace::load_active(&f.private_dir).map_err(|_| "private profile unavailable")?,
        )),
        machine: RefCell::new(Some(machine.into())),
        published: RefCell::default(),
        authorization: tokio::sync::Mutex::new(()),
        native_edits_slot: true,
    };
    let profile =
        workspace::load_active(&f.private_dir).map_err(|_| "private profile unavailable")?;
    PLAINTEXT.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
    let _intercepted = engine
        .snapshot(
            &profile,
            machine,
            &profile.identity.device.device_id,
            name,
            snapshot.clone(),
            via.clone(),
        )
        .await;
    let plain = PLAINTEXT
        .with(|slot| slot.borrow_mut().take())
        .ok_or("missing accounting cut")?;
    if plain.is_empty() {
        return Err("engine did not reach the accounting cut");
    }
    let computer = serde_json::from_slice(&plain).map_err(|_| "invalid accounting cut")?;
    let calls = json!(*connector.native.counts.borrow());
    Ok((snapshot, computer, calls))
}

fn section_accounting(computer: &Value) -> Checked<Value> {
    let snapshot = &computer["snapshot"];
    let rows = snapshot["sessions"].as_array().ok_or("invalid sessions")?;
    let mut current = 0usize;
    let mut archived = 0usize;
    let mut current_bytes = 0usize;
    let mut archived_bytes = 0usize;
    let mut archived_max_bytes = 0usize;
    for row in rows {
        let bytes = encoded(row).map_err(|_| "section encoding failed")?.len();
        if alerts::archived(row) {
            archived += 1;
            archived_bytes += bytes;
            archived_max_bytes = archived_max_bytes.max(bytes);
        } else {
            current += 1;
            current_bytes += bytes;
        }
    }
    let array_size = |count: usize, sum: usize| 2 + sum + count.saturating_sub(1);
    let session_bytes = encoded(&snapshot["sessions"])
        .map_err(|_| "section encoding failed")?
        .len();
    if session_bytes != array_size(rows.len(), current_bytes + archived_bytes) {
        return Err("section byte reconciliation failed");
    }
    let archive_delta = session_bytes - array_size(current, current_bytes);
    let snapshot_bytes = encoded(snapshot)
        .map_err(|_| "section encoding failed")?
        .len();
    let mut properties = BTreeMap::new();
    for key in [
        "sessions",
        "mobile_archive",
        "mobile_projects",
        "mobile_accounts",
        "mobile_capabilities",
    ] {
        let value = snapshot.get(key);
        let mut without = snapshot.clone();
        without
            .as_object_mut()
            .ok_or("invalid snapshot")?
            .remove(key);
        let delta = snapshot_bytes
            - encoded(&without)
                .map_err(|_| "section encoding failed")?
                .len();
        properties.insert(key,json!({"present":value.is_some(),"value_bytes":value.map(encoded).transpose().map_err(|_| "section encoding failed")?.map(|v|v.len()),"encoded_property_delta":delta}));
    }
    let number = |value: &Value| value.as_u64().map(Value::from).unwrap_or(Value::Null);
    let boolean = |value: &Value| value.as_bool().map(Value::from).unwrap_or(Value::Null);
    let count = |value: &Value| value.as_array().map(|v| v.len());
    let bytes = encoded(computer)
        .map_err(|_| "section encoding failed")?
        .len();
    Ok(
        json!({"current_rows":{"count":current,"row_sum_bytes":current_bytes,"standalone_array_bytes":array_size(current,current_bytes)},
        "archived_rows":{"count":archived,"row_sum_bytes":archived_bytes,"standalone_array_bytes":array_size(archived,archived_bytes),"max_row_bytes":archived_max_bytes,"array_removal_delta":archive_delta},
        "sessions_array_bytes":session_bytes,"snapshot_properties":properties,
        "archive_preview":{"available":boolean(&snapshot["mobile_archive"]["available"]),"loaded":number(&snapshot["mobile_archive"]["loaded"]),"total":number(&snapshot["mobile_archive"]["total"]),"partial":boolean(&snapshot["mobile_archive"]["partial"])},
        "projects":{"available":boolean(&snapshot["mobile_projects"]["available"]),"count":count(&snapshot["mobile_projects"]["projects"])},
        "accounts":{"available":boolean(&snapshot["mobile_accounts"]["available"]),"count":count(&snapshot["mobile_accounts"]["accounts"])},
        "capabilities":{"operations":count(&snapshot["mobile_capabilities"]["operations"]),"features":count(&snapshot["mobile_capabilities"]["features"])},
        "without_archive_rows_only_bytes":bytes-archive_delta,
        "without_archive_rows_only_slack":1048576i64-(bytes-archive_delta) as i64,
        "counterfactual_note":"Arithmetic only; original archive metadata is unchanged. No rows are removed or delivered."}),
    )
}

// A fixed public DTO vocabulary from source/private synthetic public rows, never
// a list of keys discovered in an owner's payload. Values are never emitted.
const PUBLIC_ROW_FIELDS: &[&str] = &[
    "account_home",
    "account_id",
    "action_reasons",
    "activity",
    "activity_detail",
    "activity_summary",
    "agent",
    "allowed_actions",
    "archive_id",
    "archived_at",
    "attached",
    "attention_id",
    "attention_signature",
    "attention_started",
    "clients",
    "cmd",
    "compaction_started",
    "conversation_id",
    "conversation_state",
    "coordination_wait",
    "created",
    "current_tool",
    "cwd",
    "cwd_canonical",
    "cwd_source",
    "effort",
    "effort_change_reason",
    "effort_change_supported",
    "effort_options",
    "effort_scope",
    "fork_parent_id",
    "fork_reason",
    "fork_source_name",
    "fork_supported",
    "git_branch",
    "git_common_dir",
    "git_detached",
    "git_metadata_state",
    "git_root",
    "git_worktree",
    "git_worktree_name",
    "goal",
    "goal_source",
    "human_request",
    "keep_warm",
    "last_event_at",
    "last_message",
    "mobile_project_id",
    "mobile_unread_attention",
    "mobile_unread_reply",
    "model",
    "model_options",
    "name",
    "notification_preferences",
    "optional_questions",
    "optional_questions_complete",
    "parent",
    "pending_effort",
    "pending_model",
    "pending_questions",
    "pending_settings_id",
    "phase",
    "process_state",
    "process_work",
    "project",
    "prompt",
    "provider_error",
    "provider_errors",
    "provider_status_at",
    "question_routing",
    "read",
    "recovery",
    "recovery_capability",
    "recovery_error_class",
    "reply_at",
    "reply_id",
    "resumable",
    "run_id",
    "runtime_state",
    "schedules",
    "session_id",
    "session_usage",
    "settings_apply_when",
    "settings_change_notice",
    "settings_change_reason",
    "settings_change_supported",
    "settings_scope",
    "state",
    "status_detail",
    "subagent_active_count",
    "subagent_completed_count",
    "subagent_count",
    "subagent_counts_complete",
    "subagent_previews",
    "subagent_source",
    "subagent_total_count",
    "subagents",
    "tag",
    "terminal_reason",
    "terminal_supported",
    "title",
    "tool_detail",
    "tracked",
    "turn_started",
    "unread_attention",
    "unread_reply",
    "worktree_owner_id",
];

fn public_field_accounting(computer: &Value) -> Checked<Value> {
    let rows = computer["snapshot"]["sessions"]
        .as_array()
        .ok_or("invalid sessions")?;
    let mut current = BTreeMap::new();
    let mut archived = BTreeMap::new();
    for key in PUBLIC_ROW_FIELDS {
        let mut counts = [(0usize, 0usize, 0usize); 2];
        for row in rows {
            let Some(value) = row.get(*key) else {
                continue;
            };
            let group = usize::from(alerts::archived(row));
            let bytes = encoded(value).map_err(|_| "field encoding failed")?.len();
            let overhead = encoded(key).map_err(|_| "field encoding failed")?.len()
                + 1
                + usize::from(row.as_object().is_some_and(|v| v.len() > 1));
            counts[group].0 += 1;
            counts[group].1 += bytes;
            counts[group].2 += bytes + overhead;
        }
        let metric = |(present, value_bytes, property_delta)| json!({"present":present,"value_bytes":value_bytes,"encoded_property_delta":property_delta});
        current.insert(*key, metric(counts[0]));
        archived.insert(*key, metric(counts[1]));
    }
    Ok(json!({"current":current,"archived":archived,
        "note":"Fixed public field names only; each property delta is independent, not a proposal to omit the field."}))
}

async fn account(input: Value) -> Checked<Value> {
    let raw = input["listing_json"]
        .as_str()
        .ok_or("listing_json is required")?;
    if raw.len() > crate::native::MAX_BYTES {
        return Err("native input exceeds bound");
    }
    let original: Value = serde_json::from_str(raw).map_err(|_| "invalid native JSON")?;
    let name = input["computer_name"]
        .as_str()
        .filter(|v| v.len() <= 4096)
        .ok_or("computer_name is required")?;
    let binding = input["binding"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or("binding is required")?;
    let via = input.get("via").cloned().unwrap_or(Value::Null);
    let projected = strip(&original)?;
    if projected != oracle(&original)? || identities(&projected)? != identities(&original)? {
        return Err("public semantic equality failed");
    }
    let mut fields = BTreeMap::new();
    for key in public_listing::SELECTED_ONLY_FIELDS {
        let mut count = 0;
        let mut delta = 0;
        for row in original["sessions"].as_array().ok_or("invalid sessions")? {
            if row.get(key).is_some() {
                count += 1;
                let mut changed = row.clone();
                changed.as_object_mut().ok_or("invalid row")?.remove(key);
                delta += encoded(row).map_err(|_| "encoding failed")?.len()
                    - encoded(&changed).map_err(|_| "encoding failed")?.len();
            }
        }
        fields.insert(key, json!({"present":count,"encoded_delta":delta}));
    }
    let native = std::env::var("ZERUS_ACCOUNTING_NATIVE").map_err(|_| "native path is required")?;
    let process = crate::native::Process::new(native, 5.0);
    let machine = process
        .call(Call::new(["mobile", "identity", "--json"]))
        .await
        .map_err(|_| "native identity unavailable")?
        .json();
    let machine = machine["machine_id"]
        .as_str()
        .filter(|v| crate::pyvalue::canonical_uuid(v))
        .ok_or("native identity unavailable")?;
    let responses = Responses::default();
    let make = |listing, process| NativeAccounting {
        listing,
        process,
        responses: Rc::clone(&responses),
        retained_bytes: Cell::new(0),
        counts: RefCell::default(),
        failed: Cell::new(false),
    };
    let f = Fixture::new(); // Synthetic profile only. No owner input is stored here.
    let wall = crate::now();
    let metadata = Metadata {
        machine,
        name,
        binding,
        via: &via,
        wall,
    };
    let (s2_original, s3_original, calls) =
        augmented(&f, make(original.clone(), Some(process)), &metadata).await?;
    let (s2, s3, replayed) = augmented(&f, make(projected.clone(), None), &metadata).await?;
    if strip(&s2_original)? != s2 || identities(&s2_original)? != identities(&s2)? {
        return Err("augmented semantic equality failed");
    }
    let mut expected = s3_original.clone();
    expected["snapshot"] = strip(&expected["snapshot"])?;
    if expected != s3 {
        return Err("computer semantic equality failed");
    }
    let plain = encoded(&s3).map_err(|_| "encoding failed")?;
    let slack = 1024i64 * 1024 - plain.len() as i64;
    let profile =
        workspace::load_active(&f.private_dir).map_err(|_| "private profile unavailable")?;
    let computer =
        Identity::from_record(&profile.identity).map_err(|_| "synthetic identity failed")?;
    let mut envelope_measure = Value::Null;
    if slack >= 0 {
        let header = Engine::<NativeAccounting>::header(
            &profile,
            f.phone.public(),
            &profile.identity.device.device_id,
            uuid::Uuid::new_v4().to_string(),
            PayloadKind::Snapshot,
            OperationClass::Read,
            ((wall as u64) / 300 + 12) * 300,
        );
        let envelope = crypto::seal(&computer, f.phone.public(), header, &plain)
            .map_err(|_| "synthetic seal failed")?;
        let opened = crypto::open(
            &f.phone,
            &profile.identity.device,
            &envelope.header,
            &envelope,
        )
        .map_err(|_| "synthetic open failed")?;
        if opened.as_slice() != plain.as_slice() {
            return Err("synthetic open equality failed");
        }
        envelope_measure = measure(&json!({"computer_id":machine,"envelope":envelope}))?;
    }
    Ok(
        json!({"ok":true,"source_mode":"test-only RAM accounting; fresh connector caches; no journal/delivery past Engine encoding",
        "sessions":identities(&original)?.len(),"augmented_rows":s2["sessions"].as_array().ok_or("invalid sessions")?.len(),
        "augmented_identity_count":identities(&s2)?.len(),"raw_native_bytes":raw.len(),"present_keys":fields,
        "s0_native":measure(&original)?,"s1_projected":measure(&projected)?,
        "s2_original_augmented":measure(&s2_original)?,"s2_projected_augmented":measure(&s2)?,
        "s3_original_computer":measure(&s3_original)?,"s3_projected_computer":measure(&s3)?,
        "s4_synthetic_envelope":envelope_measure,"plaintext_limit":1048576,"plaintext_slack":slack,
        "sections_original":section_accounting(&s3_original)?,"sections_projected":section_accounting(&s3)?,
        "public_fields_projected":public_field_accounting(&s3)?,
        "semantic_equality_outside_six":true,"identity_equality":true,"native_calls":calls,
        "replayed_calls":replayed,"synthetic_encryption_identity":true,
        "archive_available":s2["mobile_archive"]["available"],"projects_available":s2["mobile_projects"]["available"],
        "accounting_only_not_original_service_restoration":true}),
    )
}

#[tokio::test]
#[ignore = "explicit root-reviewed RAM-only stdin accounting; never an automatic owner probe"]
async fn account_complete_native_body() {
    let future = async {
        let mut input = Vec::new();
        std::io::stdin()
            .take(crate::native::MAX_PAYLOAD as u64 + 1)
            .read_to_end(&mut input)
            .map_err(|_| "stdin failed")?;
        if input.len() > crate::native::MAX_PAYLOAD {
            return Err("accounting input exceeds bound");
        }
        let input = serde_json::from_slice(&input).map_err(|_| "invalid accounting input")?;
        account(input).await
    };
    let result = tokio::task::LocalSet::new().run_until(future).await;
    // No Debug/panic of native values or errors: only fixed error classes.
    let report = match result {
        Ok(value) => value,
        Err(code) => json!({"ok":false,"error_class":code}),
    };
    println!(
        "IU768_ACCOUNTING:{}",
        serde_json::to_string(&report).expect("aggregate JSON")
    );
}

#[test]
fn accounting_identity_oracle_keeps_archive_and_native_name_collisions() {
    let rows = json!({"sessions":[
        {"name":"codex/example/shared","session_id":"sid","run_id":"run","state":"running"},
        {"name":"codex/example/shared","session_id":"sid","run_id":"old","archive_id":"archive-a","state":"archived"},
        {"name":"codex/example/shared","session_id":"sid","run_id":"old","archive_id":"archive-b","state":"archived"},
        {"name":"codex/example/shared","launch_id":"untracked-native","state":"running"}
    ]});
    assert_eq!(identities(&rows).unwrap().len(), 4);
    let mut duplicate = rows.clone();
    duplicate["sessions"]
        .as_array_mut()
        .unwrap()
        .push(rows["sessions"][0].clone());
    assert!(identities(&duplicate).is_err());
    duplicate["sessions"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["state"] = json!("different-status");
    assert!(
        identities(&duplicate).is_err(),
        "status is not a distinct native identity"
    );
}

#[test]
fn field_metrics_never_report_payload_discovered_keys_or_values() {
    let computer = json!({"snapshot":{"sessions":[{"name":"private-name-not-output", "model_options":[{"id":"private-model-value-not-output"}],"owner_dynamic_secret_key":"private-value-not-output"}]}});
    let report = public_field_accounting(&computer).unwrap();
    assert_eq!(report["current"]["model_options"]["present"], 1);
    let raw = encoded(&report).unwrap();
    let text = std::str::from_utf8(&raw).unwrap();
    for forbidden in [
        "private-name-not-output",
        "private-model-value-not-output",
        "owner_dynamic_secret_key",
        "private-value-not-output",
    ] {
        assert!(!text.contains(forbidden));
    }
}
