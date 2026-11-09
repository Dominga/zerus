//! Whether an account's permission mode is shared by every computer that has
//! the account (the default) or kept per computer (zerus-f0uz8, owner
//! 2026-11-07). The desktop applies a shared mode on each of the account's
//! computers; agents on a computer read only that computer's accounts.json.
//! The scope lives beside accounts.json, which older hgs builds parse
//! strictly, so they never see it. It is presentation of the desktop's write
//! fan-out only: it grants nothing and changes no permission by itself.
use super::*;
use std::io::Read as _;

const FILE: &str = "account-scopes.json";
const LIMIT: u64 = 64 * 1024;
const PER_COMPUTER: &str = "computer";
const SHARED: &str = "account";

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scopes {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    permission_scopes: BTreeMap<String, String>,
}

/// The stored scopes, empty when there is no file; None when it is
/// unreadable, too large or malformed.
fn read_scopes(config: &Config) -> Option<Scopes> {
    let file = match platform::no_follow(fs::OpenOptions::new().read(true)).open(config.dir.join(FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Scopes::default()),
        Err(_) => return None,
    };
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > LIMIT {
        return None;
    }
    let scopes: Scopes = serde_json::from_slice(&bytes).ok()?;
    scopes.permission_scopes.values().all(|scope| scope == PER_COMPUTER).then_some(scopes)
}

/// Each listed row's `permission_scope`: `computer` or `account`. An
/// unreadable record reads as `computer`, so no permission change spreads.
pub(super) fn describe(config: &Config, rows: &mut [Value]) {
    let scopes = read_scopes(config);
    for row in rows {
        let id = row["id"].as_str().unwrap_or("");
        let per_computer = scopes.as_ref().is_none_or(|scopes| scopes.permission_scopes.contains_key(id));
        row["permission_scope"] = json!(if per_computer { PER_COMPUTER } else { SHARED });
    }
}

/// Whether `id`'s scope here is `expected` (`account` or `computer`); an
/// unreadable record is `computer`. Called under the catalog lock.
pub(super) fn is(config: &Config, id: &str, expected: &str) -> bool {
    let per_computer = read_scopes(config).is_none_or(|scopes| scopes.permission_scopes.contains_key(id));
    expected == if per_computer { PER_COMPUTER } else { SHARED }
}

/// `account permission-scope ID --scope account|computer [--revision REV]`.
pub(super) fn set(config: &Config, args: &[String], dry: bool) -> Result<Value> {
    let id = args.get(1).map(String::as_str).unwrap_or("");
    let scope = arg(args, "--scope")
        .filter(|scope| [SHARED, PER_COMPUTER].contains(scope))
        .ok_or_else(|| Error::new(1, "Choose --scope account or --scope computer."))?;
    let _lock = locked(config)?;
    let bytes = read(config)?;
    check_revision(&bytes, args)?;
    let catalog = decode(&bytes)?;
    if !native_id(id) && !catalog.profiles.contains_key(id) {
        return Err(Error::new(1, "Account profile does not exist."));
    }
    if id == "native-opencode" {
        return Err(Error::new(1, "OpenCode permissions are managed in its native configuration"));
    }
    let mut scopes = read_scopes(config)
        .ok_or_else(|| Error::new(1, "Invalid account-scopes.json; repair or remove it before changing permission scope."))?;
    // Forget profiles that no longer exist.
    scopes
        .permission_scopes
        .retain(|known, _| native_id(known) || catalog.profiles.contains_key(known));
    if scope == PER_COMPUTER {
        scopes.permission_scopes.insert(id.into(), PER_COMPUTER.into());
    } else {
        scopes.permission_scopes.remove(id);
    }
    if !dry {
        let mut f = tempfile::NamedTempFile::new_in(&config.dir)?;
        platform::make_private(f.as_file())?;
        f.write_all(&serde_json::to_vec_pretty(&scopes).map_err(|_| Error::new(1, "Cannot encode permission scopes."))?)?;
        f.as_file().sync_all()?;
        platform::persist(f, config.dir.join(FILE)).map_err(|e| Error::new(1, e.to_string()))?;
        platform::sync_dir(&config.dir)?;
    }
    list(config)
}
