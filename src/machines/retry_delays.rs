//! How long this computer's desktop waits before reconnecting to a computer
//! whose connection failed: `reconnect.local.json` in the configuration
//! folder, `{"version":1,"delays_seconds":[FIRST,SECOND,THIRD,LATER]}`.
//!
//! A missing file means the fast defaults. An unreadable or invalid file is
//! kept as it is and reported; readers fall back to the defaults (or the
//! service keeps its last valid schedule) instead of failing. Only an explicit
//! `--set` or `--reset` replaces it, under the machines settings lock.
use super::lock;
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};
use zerus_client_core::stream::{checked_restart_delays, RESTART_DEFAULT_SECS};

pub const FILE: &str = "reconnect.local.json";
const VERSION: u32 = 1;
/// More than a valid file ever needs; a larger one is reported, not read.
const MAX_BYTES: u64 = 4096;
const RULE: &str =
    "Use four whole numbers of seconds from 1 to 3600, each at least the one before.";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u32,
    delays_seconds: Vec<u64>,
}

/// What the file holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Loaded {
    /// No file: the defaults apply.
    Missing,
    Valid([Duration; 4]),
    /// The file stays as it is; the message says why it is not used.
    Invalid(String),
}

impl Loaded {
    /// The schedule a reader without a previous one uses.
    pub fn delays_or_default(&self) -> [Duration; 4] {
        match self {
            Self::Valid(delays) => *delays,
            Self::Missing | Self::Invalid(_) => RESTART_DEFAULT_SECS.map(Duration::from_secs),
        }
    }
}

/// Reads the file without following a final symbolic link, never blocking on
/// a FIFO and never reading more than [`MAX_BYTES`].
pub fn load(dir: &Path) -> Loaded {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    platform::no_follow(&mut options);
    let file = match options.open(dir.join(FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(error) => return Loaded::Invalid(format!("{FILE} cannot be read: {error}")),
    };
    match file.metadata() {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Loaded::Invalid(format!("{FILE} is not a regular file.")),
        Err(error) => return Loaded::Invalid(format!("{FILE} cannot be read: {error}")),
    }
    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_BYTES + 1).read_to_end(&mut bytes) {
        return Loaded::Invalid(format!("{FILE} cannot be read: {error}"));
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Loaded::Invalid(format!("{FILE} is larger than {MAX_BYTES} bytes."));
    }
    decode(&bytes)
}

fn decode(bytes: &[u8]) -> Loaded {
    let stored: Stored = match serde_json::from_slice(bytes) {
        Ok(stored) => stored,
        Err(error) => return Loaded::Invalid(format!("{FILE} is not valid: {error}")),
    };
    if stored.version != VERSION {
        return Loaded::Invalid(format!(
            "{FILE} has version {}, this hgs reads version {VERSION}.",
            stored.version
        ));
    }
    match checked_restart_delays(&stored.delays_seconds) {
        Ok(delays) => Loaded::Valid(delays),
        Err(_) => Loaded::Invalid(format!("{FILE} is not valid. {RULE}")),
    }
}

fn seconds(delays: [Duration; 4]) -> [u64; 4] {
    delays.map(|delay| delay.as_secs())
}

/// The schedule as `hgs machine ls` and `retry-delays` report it: the
/// seconds in effect, the defaults, where they come from (`default`, `file`
/// or `invalid`) and, for an invalid file, why it is not used.
pub fn view(dir: &Path) -> Value {
    let loaded = load(dir);
    let (source, diagnostic) = match &loaded {
        Loaded::Missing => ("default", None),
        Loaded::Valid(_) => ("file", None),
        Loaded::Invalid(message) => ("invalid", Some(message.clone())),
    };
    json!({"delays_seconds":seconds(loaded.delays_or_default()),
        "defaults":RESTART_DEFAULT_SECS,"source":source,"diagnostic":diagnostic})
}

/// `hgs machine retry-delays [--set FIRST SECOND THIRD LATER | --reset]`.
pub fn dispatch(config: &Config, args: &[String], dry: bool) -> Result<i32> {
    let usage = || {
        Error::new(
            1,
            "usage: hgs machine retry-delays [--set FIRST SECOND THIRD LATER | --reset]",
        )
    };
    match args.first().map(String::as_str) {
        None => {}
        Some("--set") => {
            let values = args[1..]
                .iter()
                .map(|value| value.parse::<u64>().map_err(|_| Error::new(1, RULE)))
                .collect::<Result<Vec<_>>>()?;
            let delays = checked_restart_delays(&values).map_err(|_| Error::new(1, RULE))?;
            if !dry {
                set(config, Some(delays))?;
            }
        }
        Some("--reset") if args.len() == 1 => {
            if !dry {
                set(config, None)?;
            }
        }
        _ => return Err(usage()),
    }
    let mut out = view(&config.dir);
    out["dry_run"] = dry.into();
    println!("{out}");
    Ok(0)
}

/// Saves `delays`, or with None removes the file so the defaults apply.
/// Never replaces an invalid file except by an explicit reset.
fn set(config: &Config, delays: Option<[Duration; 4]>) -> Result<()> {
    let _lock = lock(config)?;
    let path = config.dir.join(FILE);
    let Some(delays) = delays else {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(platform::sync_dir(&config.dir)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        };
    };
    if let Loaded::Invalid(message) = load(&config.dir) {
        return Err(Error::new(
            1,
            format!("{message} Reset the reconnect timings to replace it."),
        ));
    }
    let stored = Stored {
        version: VERSION,
        delays_seconds: seconds(delays).to_vec(),
    };
    let mut output =
        serde_json::to_vec_pretty(&stored).map_err(|e| Error::new(1, e.to_string()))?;
    output.push(b'\n');
    let mut stage = tempfile::NamedTempFile::new_in(&config.dir)?;
    stage.write_all(&output)?;
    stage.as_file().sync_all()?;
    platform::persist(stage, path).map_err(|e| Error::new(1, e.to_string()))?;
    platform::sync_dir(&config.dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new().prefix(name).tempdir().unwrap()
    }

    #[test]
    fn a_missing_file_means_the_fast_defaults() {
        let dir = folder("retry-missing");
        assert_eq!(load(dir.path()), Loaded::Missing);
        let view = view(dir.path());
        assert_eq!(view["delays_seconds"], json!([1, 2, 5, 10]));
        assert_eq!(view["source"], "default");
        assert!(view["diagnostic"].is_null());
    }

    #[test]
    fn only_version_1_with_four_valid_seconds_is_used() {
        let dir = folder("retry-valid");
        let path = dir.path().join(FILE);
        fs::write(&path, r#"{"version":1,"delays_seconds":[2,4,8,30]}"#).unwrap();
        assert_eq!(
            load(dir.path()),
            Loaded::Valid([2, 4, 8, 30].map(Duration::from_secs))
        );
        for bad in [
            r#"{"version":2,"delays_seconds":[2,4,8,30]}"#,
            r#"{"version":1,"delays_seconds":[2,4,8]}"#,
            r#"{"version":1,"delays_seconds":[0,4,8,30]}"#,
            r#"{"version":1,"delays_seconds":[8,4,8,30]}"#,
            r#"{"version":1,"delays_seconds":[2,4,8,3601]}"#,
            r#"{"version":1,"delays_seconds":[2,4,8,30],"extra":true}"#,
            "not json",
        ] {
            fs::write(&path, bad).unwrap();
            let loaded = load(dir.path());
            assert!(matches!(loaded, Loaded::Invalid(_)), "{bad}");
            assert_eq!(
                loaded.delays_or_default(),
                [1, 2, 5, 10].map(Duration::from_secs)
            );
            let view = view(dir.path());
            assert_eq!(view["source"], "invalid");
            assert!(view["diagnostic"]
                .as_str()
                .is_some_and(|d| d.contains(FILE)));
            // Reading never repairs or removes what is there.
            assert_eq!(fs::read_to_string(&path).unwrap(), bad);
        }
    }

    #[test]
    fn an_oversized_file_is_reported_without_reading_it_all() {
        let dir = folder("retry-large");
        fs::write(dir.path().join(FILE), vec![b' '; 8192]).unwrap();
        assert!(matches!(load(dir.path()), Loaded::Invalid(message) if message.contains("larger")));
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_not_followed() {
        let dir = folder("retry-link");
        let target = dir.path().join("elsewhere.json");
        fs::write(&target, r#"{"version":1,"delays_seconds":[2,4,8,30]}"#).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join(FILE)).unwrap();
        assert!(matches!(load(dir.path()), Loaded::Invalid(_)));
    }
}
