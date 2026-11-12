use super::*;
use std::io::{IsTerminal, Read};

pub(super) fn set_key(config: &Config, args: &[String], dry: bool) -> Result<Value> {
    if dry || args.get(1).map(String::as_str) != Some("native-dsh") {
        return Err(Error::new(
            1,
            "Use account set-key native-dsh --json with an API key on stdin.",
        ));
    }
    if std::io::stdin().is_terminal() || !args.iter().any(|a| a == "--json") {
        return Err(Error::new(1, "API key input requires a private JSON pipe."));
    }
    let mut bytes = Vec::new();
    std::io::stdin().take(8193).read_to_end(&mut bytes)?;
    let input: Value =
        serde_json::from_slice(crate::state::without_bom(&bytes)).map_err(|_| Error::new(1, "Invalid API key request."))?;
    let key = input["api_key"].as_str().unwrap_or("");
    if bytes.len() > 8192
        || key.is_empty()
        || key.len() > 4096
        || key.chars().any(char::is_whitespace)
        || key.chars().any(char::is_control)
    {
        return Err(Error::new(
            1,
            "Enter a non-empty API key without spaces or line breaks.",
        ));
    }
    let label = input["label"].as_str();
    if let Some(label) = label {
        validate_label(label)?;
    }
    if !platform::available("dsh") {
        return Err(Error::new(
            1,
            "Install DeepSeek on this machine before saving its API key.",
        ));
    }
    let _lock = locked(config)?;
    let catalog_bytes = read(config)?;
    check_revision(&catalog_bytes, args)?;
    let mut catalog = decode(&catalog_bytes)?;
    let url = crate::state::dsh::native_ui_url("@account").map_err(|_| {
        Error::new(
            1,
            "Could not connect to the native DeepSeek host on this machine.",
        )
    })?;
    let mut command = Command::new("node");
    command.args([
        "--input-type=module",
        "-e",
        concat!(
            include_str!("../dsh/account-client.mjs"),
            "\n",
            include_str!("../dsh/account-key.mjs")
        ),
    ]);
    let output = inspection::capture(
        command,
        json!({"url":url,"api_key":key}).to_string().as_bytes(),
        20,
    )
    .map_err(|_| {
        Error::new(
            1,
            "DeepSeek did not confirm saving the API key. Refresh the account before retrying.",
        )
    })?;
    let result: Value = serde_json::from_slice(&output)
        .map_err(|_| Error::new(1, "Invalid DeepSeek save response."))?;
    if result["ok"] != true {
        return Err(Error::new(
            1,
            result["error"]
                .as_str()
                .unwrap_or("Could not save the DeepSeek API key."),
        ));
    }
    if let Some(label) = label {
        catalog
            .native_labels
            .insert("native-dsh".into(), label.into());
    }
    catalog.hidden_profiles.remove("native-dsh");
    save(config, &catalog)?;
    // Refresh the sanitized status cache after the native service confirms the write.
    let _ = inspection::inspect(
        config,
        &["inspect".into(), "native-dsh".into(), "--refresh".into()],
    );
    list(config)
}

/// A decimal amount as DeepSeek reports balances: an optional minus sign,
/// digits and an optional fraction, at most 40 characters.
fn decimal(amount: &str) -> bool {
    let digits = amount.strip_prefix('-').unwrap_or(amount);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, "1"));
    amount.len() <= 40
        && !whole.is_empty()
        && !fraction.is_empty()
        && whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
}

/// Projects the official `/user/balance` reply that `account-read.mjs`
/// attaches as `api_balance` into the shared `balances` rows: per currency a
/// `Total`, then its `Topped up` and `Granted` parts. `balance_available`
/// keeps DeepSeek's `is_available`. The raw reply never reaches the cache,
/// and an unusable one leaves the account's status and rows unchanged.
pub(super) fn project_api_balance(data: &mut Value) {
    let Some(reply) = data.as_object_mut().and_then(|d| d.remove("api_balance")) else {
        return;
    };
    let mut rows = Vec::new();
    let mut currencies = Vec::new();
    for info in reply["balance_infos"]
        .as_array()
        .into_iter()
        .flatten()
        .take(4)
    {
        let currency = info["currency"].as_str().unwrap_or_default();
        if !matches!(currency, "USD" | "CNY") || currencies.contains(&currency) {
            continue;
        }
        let amount = |field: &str| info[field].as_str().filter(|value| decimal(value));
        let Some(total) = amount("total_balance") else {
            continue;
        };
        currencies.push(currency);
        rows.push(json!({"kind":"Total","balance":total,"currency":currency}));
        for (field, kind) in [
            ("topped_up_balance", "Topped up"),
            ("granted_balance", "Granted"),
        ] {
            if let Some(part) = amount(field) {
                rows.push(json!({"kind":kind,"balance":part,"currency":currency}));
            }
        }
    }
    if rows.is_empty() {
        return;
    }
    data["balances"] = json!(rows);
    if let Some(available) = reply["is_available"].as_bool() {
        data["balance_available"] = json!(available);
    }
}

/// The private state file for one account profile and home under `dir`.
pub(super) fn state_path(config: &Config, dir: &str, id: &str, home: &Path) -> PathBuf {
    config.dir.join(dir).join(format!(
        "{:x}.json",
        Sha256::digest(format!("{}:{id}", home.display()).as_bytes())
    ))
}

/// An exclusive lock beside `path`, or None while another process holds it:
/// a skipped update loses nothing, since the next one starts from the same state.
pub(super) fn try_lock(path: &Path) -> Option<platform::FileLock> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    platform::private_open(&mut options);
    platform::no_follow(&mut options);
    let file = options.open(path.with_extension("lock")).ok()?;
    file.try_lock_exclusive().ok()?;
    Some(platform::FileLock::held(file))
}

/// A private regular JSON file of at most `limit` bytes; anything else reads as absent.
pub(super) fn read_private<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Option<T> {
    use std::io::Read;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    platform::no_follow(&mut options);
    let file = options.open(path).ok()?;
    if !file.metadata().ok()?.is_file() || !platform::verify_private_file(&file).ok()? {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit)
        .then(|| serde_json::from_slice(&bytes).ok())
        .flatten()
}

/// Replaces `path` atomically with a private file holding `value`.
pub(super) fn write_private(path: &Path, value: &impl Serialize) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error::new(1, "Invalid state path."))?;
    platform::private_dir(dir)?;
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    platform::make_private(file.as_file())?;
    let bytes =
        serde_json::to_vec(value).map_err(|_| Error::new(1, "Could not encode account state."))?;
    file.write_all(&bytes)?;
    platform::persist(file, path).map_err(|_| Error::new(1, "Could not save account state."))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_balance_becomes_total_and_parts_per_currency() {
        let mut data = json!({"status":"configured","api_key_configured":true,"windows":[],
            "identity":{"auth_method":"API key"},
            "api_balance":{"is_available":true,"balance_infos":[
                {"currency":"USD","total_balance":"6.20","granted_balance":"0.00","topped_up_balance":"6.20"},
                {"currency":"CNY","total_balance":"110.00","granted_balance":"10.00","topped_up_balance":"100.00"}]}});
        project_api_balance(&mut data);
        assert_eq!(
            data["balances"],
            json!([
                {"kind":"Total","balance":"6.20","currency":"USD"},
                {"kind":"Topped up","balance":"6.20","currency":"USD"},
                {"kind":"Granted","balance":"0.00","currency":"USD"},
                {"kind":"Total","balance":"110.00","currency":"CNY"},
                {"kind":"Topped up","balance":"100.00","currency":"CNY"},
                {"kind":"Granted","balance":"10.00","currency":"CNY"},
            ])
        );
        assert_eq!(data["balance_available"], json!(true));
        // An API key stays a configured key, not a signed-in subscription.
        assert_eq!(data["status"], json!("configured"));
        assert!(data.get("api_balance").is_none());
    }

    #[test]
    fn insufficient_balance_is_reported_with_its_amounts() {
        let mut data = json!({"status":"configured","api_balance":{"is_available":false,"balance_infos":[
            {"currency":"USD","total_balance":"-0.03","granted_balance":"0.00","topped_up_balance":"-0.03"}]}});
        project_api_balance(&mut data);
        assert_eq!(data["balance_available"], json!(false));
        assert_eq!(
            data["balances"][0],
            json!({"kind":"Total","balance":"-0.03","currency":"USD"})
        );
    }

    #[test]
    fn unusable_replies_leave_the_status_without_balances() {
        for reply in [
            json!(null),
            json!({"is_available":true}),
            json!({"is_available":true,"balance_infos":"6.20"}),
            json!({"is_available":true,"balance_infos":[{"currency":"EUR","total_balance":"6.20"}]}),
            json!({"is_available":true,"balance_infos":[{"currency":"USD","total_balance":6.2}]}),
            json!({"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"6,20"}]}),
            json!({"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"1e3"}]}),
            json!({"is_available":true,"balance_infos":[{"currency":"USD","total_balance":".5"}]}),
            json!({"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"5."}]}),
            json!({"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"-"}]}),
            json!({"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"1".repeat(41)}]}),
        ] {
            let mut data = json!({"status":"configured","api_balance":reply});
            project_api_balance(&mut data);
            assert_eq!(data, json!({"status":"configured"}), "{reply}");
        }
        let mut untouched = json!({"status":"unavailable","balances":[{"kind":"Wallet","balance":"1","currency":"USD"}]});
        let before = untouched.clone();
        project_api_balance(&mut untouched);
        assert_eq!(untouched, before);
    }

    #[test]
    fn malformed_parts_drop_only_themselves_and_duplicates_keep_the_first() {
        let mut data = json!({"status":"configured","api_balance":{"balance_infos":[
            {"currency":"USD","total_balance":"6.20","granted_balance":"n/a","topped_up_balance":"6.20"},
            {"currency":"USD","total_balance":"9.99","granted_balance":"0","topped_up_balance":"9.99"}]}});
        project_api_balance(&mut data);
        assert_eq!(
            data["balances"],
            json!([
                {"kind":"Total","balance":"6.20","currency":"USD"},
                {"kind":"Topped up","balance":"6.20","currency":"USD"},
            ])
        );
        // No is_available in the reply: no claim about it either way.
        assert!(data.get("balance_available").is_none());
    }

    #[test]
    fn production_balance_read_names_only_the_official_host() {
        let balance = include_str!("../dsh/account-balance.mjs");
        assert!(balance
            .contains("const DEEPSEEK_BALANCE_URL = 'https://api.deepseek.com/user/balance';"));
        assert!(balance.contains("redirect: 'error'"));
        // The account read never chooses a destination or timeout of its own.
        let read = include_str!("../dsh/account-read.mjs");
        assert_eq!(read.matches("apiBalance(").count(), 1);
        assert!(read.contains("await apiBalance(key);"));
    }

    /// The real balance script against a loopback server: never the network.
    #[cfg(unix)]
    mod balance_http {
        use super::*;
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::time::{Duration, Instant};

        const DRIVER: &str = r#"
let input = '';
for await (const chunk of process.stdin) input += chunk;
const request = JSON.parse(input);
let result;
try { result = {ok: await apiBalance(request.key, request.url, request.timeout)}; }
catch { result = {failed: true}; }
process.stdout.write(JSON.stringify(result));
"#;

        fn read_request(stream: &mut TcpStream) -> String {
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            String::from_utf8_lossy(&request).into_owned()
        }

        /// Answers one request with `response` and returns what it received.
        fn serve(response: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/user/balance", listener.local_addr().unwrap());
            let worker = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                let _ = stream.write_all(&response);
                request
            });
            (url, worker)
        }

        fn reply(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
            let mut bytes = format!(
                "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .into_bytes();
            bytes.extend_from_slice(body);
            bytes
        }

        fn run(url: &str, timeout_ms: u64) -> Value {
            let mut command = Command::new("node");
            command.args([
                "--input-type=module",
                "-e",
                &format!("{}\n{DRIVER}", include_str!("../dsh/account-balance.mjs")),
            ]);
            let input = json!({"key":"sk-test-balance","url":url,"timeout":timeout_ms});
            let output = inspection::capture(command, input.to_string().as_bytes(), 20)
                .expect("node runs the balance script");
            serde_json::from_slice(&output).unwrap()
        }

        #[test]
        fn sends_the_bearer_key_and_returns_only_public_balance_fields() {
            let body = br#"{"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"6.20","granted_balance":"0.00","topped_up_balance":"6.20"}],"user":"private"}"#;
            let (url, worker) = serve(reply("200 OK", "Content-Type: application/json\r\n", body));
            let result = run(&url, 5000);
            let request = worker.join().unwrap().to_ascii_lowercase();
            assert!(
                request.starts_with("get /user/balance http/1.1\r\n"),
                "{request}"
            );
            assert!(
                request.contains("\r\nauthorization: bearer sk-test-balance\r\n"),
                "{request}"
            );
            assert!(
                request.contains("\r\naccept: application/json\r\n"),
                "{request}"
            );
            assert_eq!(
                result,
                json!({"ok":{"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"6.20","granted_balance":"0.00","topped_up_balance":"6.20"}]}})
            );
        }

        #[test]
        fn rejected_malformed_and_oversized_replies_fail() {
            for response in [
                reply(
                    "401 Unauthorized",
                    "",
                    br#"{"error":{"message":"Authentication Fails"}}"#,
                ),
                reply("500 Internal Server Error", "", b"{}"),
                reply("200 OK", "", b"not json"),
                reply("200 OK", "", br#"{"is_available":true}"#),
                reply(
                    "200 OK",
                    "",
                    format!(r#"{{"balance_infos":[],"pad":"{}"}}"#, "x".repeat(70_000)).as_bytes(),
                ),
            ] {
                let (url, worker) = serve(response);
                assert_eq!(run(&url, 5000), json!({"failed":true}));
                worker.join().unwrap();
            }
        }

        #[test]
        fn a_redirect_is_refused_and_never_carries_the_key_elsewhere() {
            let elsewhere = TcpListener::bind("127.0.0.1:0").unwrap();
            elsewhere.set_nonblocking(true).unwrap();
            let location = format!("http://{}/user/balance", elsewhere.local_addr().unwrap());
            let (url, worker) = serve(reply(
                "302 Found",
                &format!("Location: {location}\r\n"),
                b"",
            ));
            assert_eq!(run(&url, 5000), json!({"failed":true}));
            worker.join().unwrap();
            assert!(matches!(
                elsewhere.accept(),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
            ));
        }

        #[test]
        fn a_silent_server_is_bounded_by_the_timeout() {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/user/balance", listener.local_addr().unwrap());
            let (done, finished) = std::sync::mpsc::channel::<()>();
            let worker = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                read_request(&mut stream);
                // Hold the connection open without answering until the run ends.
                let _ = finished.recv_timeout(Duration::from_secs(10));
            });
            let started = Instant::now();
            assert_eq!(run(&url, 300), json!({"failed":true}));
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{:?}",
                started.elapsed()
            );
            drop(done);
            worker.join().unwrap();
        }
    }
}
