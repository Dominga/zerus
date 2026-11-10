//! Public quota snapshots only. Shared by CLI processes on this owning computer.
use super::*;
use std::io::Read;
use std::time::{Duration, Instant};

const INTERVAL: u64 = 60;
const FRESH_INTERVAL: u64 = 300;
const UNKNOWN_RETRY: u64 = 300;
const ACCOUNTS: usize = 256;
const MAX_CACHE: u64 = 1024 * 1024;

pub(super) enum Reply {
    Usage(Value),
    RateLimited(u64),
    Unavailable,
}

// Fresh native identity is part of every key, including the UUID route. OAuth
// bearer/refresh tokens and expiry timestamps never participate in this key.
fn identity(value: &Value) -> Value {
    let mut public = json!({});
    for field in [
        "email",
        "organization",
        "organization_id",
        "plan",
        "auth_method",
    ] {
        public[field] = json!(value[field]
            .as_str()
            .unwrap_or("")
            .chars()
            .filter(|c| !c.is_control())
            .take(256)
            .collect::<String>());
    }
    public
}

pub(super) fn same_identity(a: &Value, b: &Value) -> bool {
    !a["email"].as_str().unwrap_or("").is_empty() && identity(a) == identity(b)
}

pub(super) fn key(home: &Path, observed: &Value, credentials: &Value, signed_in: bool) -> String {
    let saved = inspection::read_json(&home.join(".claude.json"));
    let account = &saved["oauthAccount"];
    let email = observed["email"].as_str().unwrap_or("");
    let uuid = account["accountUuid"].as_str().unwrap_or("");
    let organization = observed["organization_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| account["organizationUuid"].as_str().unwrap_or(""));
    let namespace = if signed_in
        && !email.is_empty()
        && account["emailAddress"].as_str() == Some(email)
        && uuid::Uuid::parse_str(uuid).is_ok()
        && (organization.is_empty() || uuid::Uuid::parse_str(organization).is_ok())
    {
        format!("account:{uuid}:{organization}")
    } else {
        format!("profile:{}", home.display())
    };
    let auth = &credentials["claudeAiOauth"];
    let mut metadata = json!({});
    // Only stable account markers: scope/tier/expiry changes accompanying a
    // token rotation must not discard this account's cooldown either.
    for field in ["accountUuid", "organizationUuid"] {
        metadata[field] = json!(auth[field]
            .as_str()
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
            .unwrap_or(""));
    }
    let credential_fingerprint = format!("{:x}", Sha256::digest(metadata.to_string().as_bytes()));
    format!(
        "{:x}",
        Sha256::digest(
            json!([
                "claude-usage-v2",
                namespace,
                identity(observed),
                credential_fingerprint
            ])
            .to_string()
            .as_bytes()
        )
    )
}

// Opening must itself be bounded: a byte/read timeout cannot protect a FIFO's
// open handshake. The held handle must be a private regular file, never a link.
fn open_regular(path: &Path, lock: bool) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(!lock).write(lock).create(lock).truncate(false);
    platform::private_open(&mut options);
    platform::no_follow(&mut options);
    let file = options.open(path)?;
    if !file.metadata()?.is_file() || !platform::verify_private_file(&file)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Account cache must be a private regular file.",
        ));
    }
    Ok(file)
}

pub(super) fn read_cache(path: &Path) -> Result<Value> {
    let file = match open_regular(path, false) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Null),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CACHE + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CACHE as usize {
        return Err(Error::new(1, "Account cache is too large."));
    }
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::new(1, "Account cache is unreadable."))
}

fn snapshot(value: &Value) -> Value {
    let windows: Vec<_> = value["windows"]
        .as_array().into_iter().flatten().take(4)
        .filter_map(|w| {
            let id = w["id"].as_str()?;
            let (label, minutes) = match id {
                "five_hour" => ("", 300),
                "seven_day" => ("", 10080),
                "seven_day_sonnet" => ("Sonnet", 10080),
                "seven_day_opus" => ("Opus", 10080),
                _ => return None,
            };
            let used = w["used_percent"].as_f64().filter(|v| v.is_finite() && *v >= 0.0)?;
            let reset = w["resets_at"].as_str().map(|s| {
                Value::String(s.chars().filter(|c| !c.is_control()).take(160).collect())
            }).or_else(|| w["resets_at"].as_u64().map(Value::from)).unwrap_or(Value::Null);
            Some(json!({"id":id,"label":label,"window_minutes":minutes,"used_percent":used,"resets_at":reset}))
        }).collect();
    let status = match value["status"].as_str() {
        Some("ok") => "ok",
        Some("rate_limited") => "rate_limited",
        _ => "unavailable",
    };
    let checked_at = (!windows.is_empty())
        .then(|| value["checked_at"].as_u64())
        .flatten();
    json!({"status":status,"windows":windows,"checked_at":checked_at,
        "attempted_at":value["attempted_at"].as_u64(),"retry_at":value["retry_at"].as_u64()})
}

fn ready_at(data: &Value) -> u64 {
    let interval = if data["status"] == "ok" {
        FRESH_INTERVAL
    } else {
        INTERVAL
    };
    data["retry_at"].as_u64().unwrap_or(0).max(
        data["attempted_at"]
            .as_u64()
            .map(|t| t.saturating_add(interval))
            .unwrap_or(0),
    )
}

fn store(dir: &Path, entries: &serde_json::Map<String, Value>) -> Result<()> {
    let mut f = tempfile::NamedTempFile::new_in(dir)?;
    platform::make_private(f.as_file())?;
    let bytes = Value::Object(entries.clone()).to_string().into_bytes();
    if bytes.len() > MAX_CACHE as usize {
        return Err(Error::new(1, "Claude usage cache is too large."));
    }
    f.write_all(&bytes)?;
    platform::persist(f, dir.join("claude-usage.json"))
        .map_err(|_| Error::new(1, "Could not cache Claude usage."))?;
    Ok(())
}

/// All callers, including explicit Refresh, share this minimum interval and
/// persisted server cooldown. The lock spans the bounded request, so two GUI
/// clients cannot issue the same account request concurrently. No history grows.
pub(super) fn get(
    config_dir: &Path,
    key: &str,
    seed: &Value,
    clock: impl FnOnce() -> u64,
    fetch: impl FnOnce() -> Reply,
) -> Result<Value> {
    let dir = config_dir.join("account-status");
    private_dir(&dir)?;
    let file = open_regular(&dir.join("claude-usage.lock"), true)?;
    let started = Instant::now();
    while file.try_lock_exclusive().is_err() {
        if started.elapsed() >= Duration::from_secs(8) {
            return Err(Error::new(
                1,
                "Claude usage is already updating; try again shortly.",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _lock = platform::FileLock::held(file);
    let now = clock();
    let path = dir.join("claude-usage.json");
    let mut entries = match read_cache(&path)? {
        Value::Null => serde_json::Map::new(),
        Value::Object(entries) => entries,
        _ => return Err(Error::new(1, "Claude usage cache is unreadable.")),
    };
    if entries.len() > ACCOUNTS {
        return Err(Error::new(1, "Claude usage cache has too many accounts."));
    }
    let mut data = snapshot(entries.get(key).unwrap_or(seed));
    if ready_at(&data) > now {
        return Ok(data);
    }
    if !entries.contains_key(key) && entries.len() == ACCOUNTS {
        let oldest = entries
            .iter()
            .filter(|(_, v)| ready_at(v) <= now)
            .min_by_key(|(_, v)| v["attempted_at"].as_u64().unwrap_or(0))
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            entries.remove(&oldest);
        } else {
            return Err(Error::new(
                1,
                "Claude usage cache is busy; wait for its cooldowns.",
            ));
        }
    }
    // Reserve a conservative cooldown before the request. A killed inspector
    // cannot cause another process to repeat an unknown request immediately.
    data["attempted_at"] = json!(now);
    data["retry_at"] = json!(now.saturating_add(UNKNOWN_RETRY));
    data["status"] = json!("unavailable");
    entries.insert(key.into(), data.clone());
    store(&dir, &entries)?;
    match fetch() {
        Reply::Usage(usage) => {
            let mut windows = Vec::new();
            for (id, label, minutes) in [
                ("five_hour", "", 300),
                ("seven_day", "", 10080),
                ("seven_day_sonnet", "Sonnet", 10080),
                ("seven_day_opus", "Opus", 10080),
            ] {
                let w = &usage[id];
                if let Some(window) = inspection::window(
                    id,
                    label,
                    w["utilization"].as_f64(),
                    Some(minutes),
                    w["resets_at"].clone(),
                ) {
                    windows.push(window);
                }
            }
            if !windows.is_empty() {
                data["windows"] = json!(windows);
                data["checked_at"] = json!(now);
                data["status"] = json!("ok");
            }
            data["retry_at"] = Value::Null;
        }
        Reply::RateLimited(retry) => {
            data["status"] = json!("rate_limited");
            data["retry_at"] = json!(retry.max(now.saturating_add(INTERVAL)));
        }
        Reply::Unavailable => {}
    }
    data = snapshot(&data);
    entries.insert(key.into(), data.clone());
    store(&dir, &entries)?;
    Ok(data)
}

pub(super) fn retry_after(headers: &str, now: u64) -> u64 {
    // curl can report a proxy CONNECT response before the final HTTP response.
    let final_headers = headers.rsplit("HTTP/").next().unwrap_or(headers);
    final_headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .filter_map(|(_, value)| {
            let value = value.trim();
            value
                .parse::<u64>()
                .ok()
                .map(|s| now.saturating_add(s))
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc2822(value)
                        .ok()
                        .and_then(|d| u64::try_from(d.timestamp()).ok())
                })
        })
        .max()
        .unwrap_or_else(|| now.saturating_add(UNKNOWN_RETRY))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn usage() -> Reply {
        Reply::Usage(json!({"five_hour":{"utilization":42,"resets_at":"2030-01-01T00:00:00Z"}}))
    }

    #[test]
    fn cooldown_keeps_sample_age_across_refresh_restart_and_account_selection() {
        let dir = crate::host::staging::private_tempdir();
        let first = get(dir.path(), "a", &Value::Null, || 100, usage).unwrap();
        assert_eq!(first["checked_at"], 100);
        assert_eq!(
            get(
                dir.path(),
                "a",
                &Value::Null,
                || 399,
                || panic!("healthy sample fetched before five minutes")
            )
            .unwrap(),
            first
        );
        let limited = get(
            dir.path(),
            "a",
            &Value::Null,
            || 401,
            || Reply::RateLimited(501),
        )
        .unwrap();
        assert_eq!(limited["windows"], first["windows"]);
        assert_eq!(limited["checked_at"], 100);
        assert_eq!(limited["retry_at"], 501);
        assert_eq!(limited["status"], "rate_limited");
        // A new call reads disk, as a second GUI process/manual Refresh would.
        let cached = get(
            dir.path(),
            "a",
            &Value::Null,
            || 500,
            || panic!("cooldown bypassed"),
        )
        .unwrap();
        assert_eq!(cached, limited);
        assert_eq!(
            get(dir.path(), "b", &Value::Null, || 200, usage).unwrap()["status"],
            "ok"
        );
        let next = get(dir.path(), "a", &Value::Null, || 501, usage).unwrap();
        assert_eq!(next["checked_at"], 501);
        assert!(next["retry_at"].is_null());
    }

    #[test]
    fn concurrent_inspectors_coalesce_and_first_429_keeps_the_old_profile_sample() {
        let dir = crate::host::staging::private_tempdir();
        let count = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let count = Arc::clone(&count);
                let path = dir.path();
                scope.spawn(move || {
                    let data = get(
                        path,
                        "a",
                        &Value::Null,
                        || 100,
                        || {
                            count.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(30));
                            usage()
                        },
                    )
                    .unwrap();
                    assert_eq!(data["checked_at"], 100);
                });
            }
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let old =
            json!({"status":"ok","checked_at":50,"windows":[{"id":"five_hour","used_percent":30}]});
        let limited = get(
            dir.path(),
            "legacy",
            &old,
            || 200,
            || Reply::RateLimited(300),
        )
        .unwrap();
        assert_eq!(limited["checked_at"], 50);
        assert_eq!(limited["windows"][0]["used_percent"], 30.0);
        assert_eq!(limited["status"], "rate_limited");
    }

    #[test]
    fn retry_after_delta_date_proxy_and_invalid_headers() {
        assert_eq!(
            retry_after("HTTP/1.1 429\r\nRetry-After: 100\r\n", 1000),
            1100
        );
        assert_eq!(
            retry_after(
                "HTTP/1.1 429\r\nretry-after: Sun, 06 Nov 1994 08:49:37 GMT\r\n",
                1
            ),
            784111777
        );
        assert_eq!(
            retry_after(
                "HTTP/1.1 200\r\nRetry-After: 999\r\n\r\nHTTP/2 429\r\nRetry-After: 12\r\n",
                1000
            ),
            1012
        );
        assert_eq!(retry_after("Retry-After: nope\r\n", 1000), 1300);
        assert_eq!(retry_after("", 1000), 1300);
    }

    #[test]
    fn bounded_cache_never_evicts_an_active_cooldown_and_projects_no_private_fields() {
        let dir = crate::host::staging::private_tempdir();
        let cache = dir.path().join("account-status");
        private_dir(&cache).unwrap();
        let mut entries = serde_json::Map::new();
        for i in 0..ACCOUNTS {
            entries.insert(i.to_string(), json!({"retry_at":300,"attempted_at":100}));
        }
        store(&cache, &entries).unwrap();
        assert!(get(
            dir.path(),
            "extra",
            &Value::Null,
            || 200,
            || panic!("cache overflow request")
        )
        .is_err());
        let data = get(dir.path(), "extra", &Value::Null, || 301, || Reply::Usage(json!({
            "accessToken":"synthetic-secret", "five_hour":{"utilization":40,"resets_at":"x".repeat(1000),"private":"synthetic-secret"}
        }))).unwrap();
        assert_eq!(data["windows"][0]["resets_at"].as_str().unwrap().len(), 160);
        let saved = fs::read_to_string(cache.join("claude-usage.json")).unwrap();
        assert!(!saved.contains("synthetic-secret"));
        assert_eq!(
            serde_json::from_str::<Value>(&saved)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            ACCOUNTS
        );
    }

    #[test]
    fn same_verified_native_account_shares_a_key_but_other_accounts_do_not() {
        let a = crate::host::staging::private_tempdir();
        let b = crate::host::staging::private_tempdir();
        let account = json!({"oauthAccount":{"accountUuid":"00000000-0000-4000-8000-000000000001","emailAddress":"person@example.test"}});
        for dir in [&a, &b] {
            fs::write(dir.path().join(".claude.json"), account.to_string()).unwrap();
        }
        let identity = json!({"email":"person@example.test"});
        assert_eq!(
            key(a.path(), &identity, &Value::Null, true),
            key(b.path(), &identity, &Value::Null, true)
        );
        assert_ne!(
            key(a.path(), &identity, &Value::Null, true),
            key(
                b.path(),
                &json!({"email":"other@example.test"}),
                &Value::Null,
                true
            )
        );
        assert_ne!(
            key(a.path(), &identity, &Value::Null, false),
            key(b.path(), &identity, &Value::Null, false)
        );
    }

    #[test]
    fn observed_identity_and_public_credential_metadata_bind_even_without_uuid() {
        let dir = crate::host::staging::private_tempdir();
        let alpha = json!({"email":"alpha@example.test","organization":"First","plan":"Max","auth_method":"claude.ai"});
        let auth = json!({"claudeAiOauth":{"accessToken":"synthetic-alpha","refreshToken":"synthetic-refresh","expiresAt":1,"subscriptionType":"max","rateLimitTier":"tier1","scopes":["usage","profile"]}});
        let first = key(dir.path(), &alpha, &auth, true);
        let mut rotated = auth.clone();
        rotated["claudeAiOauth"]["accessToken"] = json!("synthetic-rotated");
        rotated["claudeAiOauth"]["refreshToken"] = json!("synthetic-refresh-rotated");
        rotated["claudeAiOauth"]["expiresAt"] = json!(999999);
        assert_eq!(first, key(dir.path(), &alpha, &rotated, true));
        for field in [
            "email",
            "organization",
            "organization_id",
            "plan",
            "auth_method",
        ] {
            let mut other = alpha.clone();
            other[field] = json!("different");
            assert_ne!(first, key(dir.path(), &other, &auth, true), "{field}");
        }
        rotated["claudeAiOauth"]["rateLimitTier"] = json!("tier2");
        rotated["claudeAiOauth"]["scopes"] = json!(["profile"]);
        assert_eq!(first, key(dir.path(), &alpha, &rotated, true));
        rotated["claudeAiOauth"]["accountUuid"] = json!("00000000-0000-4000-8000-000000000002");
        assert_ne!(first, key(dir.path(), &alpha, &rotated, true));
        assert!(!same_identity(
            &alpha,
            &json!({"email":"beta@example.test"})
        ));
        assert!(!same_identity(&json!({}), &json!({})));
    }
}
