//! An account's color and emoji mark (zerus-sa9qn.6), kept with the account
//! on its computer so every desktop and phone shows the same mark. The marks
//! live beside accounts.json, which older hgs builds parse strictly, so they
//! never see them and keep working. Presentation only: a mark grants nothing.
//!
//! `hgs account ls` rows carry `mark` (`color` and `emoji`, null when unset)
//! on every computer that keeps marks; session rows of `hgs ls` carry
//! `account_mark` for their recorded account when one is set.
use super::*;
use std::io::Read as _;

const FILE: &str = "account-marks.json";
const LIMIT: u64 = 64 * 1024;
/// An emoji grapheme as the desktop accepts it: at most 32 UTF-16 units.
const MAX_EMOJI_UNITS: usize = 32;

/// One profile's mark. Unknown fields from a newer hgs are kept on write.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Mark {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    color: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    emoji: String,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
}
#[derive(Default, Serialize, Deserialize)]
struct Marks {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    marks: BTreeMap<String, Mark>,
    #[serde(flatten)]
    other: BTreeMap<String, Value>,
}

/// A `#rrggbb` color in lowercase.
fn color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A short emoji: no control or space characters, not plain ASCII, at most
/// 32 UTF-16 units. Clients check that it is one emoji grapheme.
fn emoji(value: &str) -> bool {
    !value.is_empty()
        && value.encode_utf16().count() <= MAX_EMOJI_UNITS
        && !value.is_ascii()
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// The stored marks, empty when there is no file; None when it is
/// unreadable, too large or malformed.
fn read_marks(dir: &Path) -> Option<Marks> {
    let file = match platform::no_follow(fs::OpenOptions::new().read(true)).open(dir.join(FILE)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(Marks::default()),
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
    serde_json::from_slice(&bytes).ok()
}

/// The shown value of a stored mark: valid fields only.
fn shown(mark: &Mark) -> (Option<&str>, Option<&str>) {
    (
        Some(mark.color.as_str()).filter(|value| color(value)),
        Some(mark.emoji.as_str()).filter(|value| emoji(value)),
    )
}

/// Each listed account row's `mark`. An unreadable record shows no marks.
pub(super) fn describe(config: &Config, rows: &mut [Value]) {
    let marks = read_marks(&config.dir).unwrap_or_default();
    for row in rows {
        let (color, emoji) = marks.marks.get(row["id"].as_str().unwrap_or("")).map(shown).unwrap_or_default();
        row["mark"] = json!({"color": color, "emoji": emoji});
    }
}

/// Each set mark by profile ID as `{color?, emoji?}`, for session rows; empty
/// when there are none or the record is unreadable. One bounded read.
pub(crate) fn by_profile() -> BTreeMap<String, Value> {
    let Some(marks) = Config::load().ok().and_then(|config| read_marks(&config.dir)) else {
        return BTreeMap::new();
    };
    marks
        .marks
        .iter()
        .filter_map(|(id, mark)| {
            let (color, emoji) = shown(mark);
            let mut value = serde_json::Map::new();
            if let Some(color) = color {
                value.insert("color".into(), json!(color));
            }
            if let Some(emoji) = emoji {
                value.insert("emoji".into(), json!(emoji));
            }
            (!value.is_empty()).then(|| (id.clone(), Value::Object(value)))
        })
        .collect()
}

/// `session["account_mark"]` from its `account_id`, when that account has one.
pub(crate) fn decorate(sessions: &mut [Value]) {
    if sessions.is_empty() {
        return;
    }
    let marks = by_profile();
    if marks.is_empty() {
        return;
    }
    for session in sessions {
        if let Some(mark) = session["account_id"].as_str().and_then(|id| marks.get(id)) {
            session["account_mark"] = mark.clone();
        }
    }
}

/// A requested field: Some("") clears it, None leaves it as it is.
fn requested(args: &[String], name: &str, valid: fn(&str) -> bool, message: &str) -> Result<Option<String>> {
    let Some(value) = arg(args, name) else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() || value == "none" {
        return Ok(Some(String::new()));
    }
    let value = if name == "--color" { value.to_ascii_lowercase() } else { value.to_owned() };
    if !valid(&value) {
        return Err(Error::new(1, message));
    }
    Ok(Some(value))
}

/// `account mark ID [--color #RRGGBB|none] [--emoji EMOJI|none] [--if-unset] [--revision REV]`.
/// `--if-unset` sets only the fields this computer has no value for: a
/// desktop moving its own choices here never replaces the computer's.
pub(super) fn set(config: &Config, args: &[String], dry: bool) -> Result<Value> {
    let id = args.get(1).map(String::as_str).unwrap_or("");
    let new_color = requested(args, "--color", color, "Choose a color as #RRGGBB, or none.")?;
    let new_emoji = requested(args, "--emoji", emoji, "Choose one emoji, or none.")?;
    if new_color.is_none() && new_emoji.is_none() {
        return Err(Error::new(1, "Choose --color or --emoji."));
    }
    let if_unset = args.iter().any(|a| a == "--if-unset");
    let _lock = locked(config)?;
    let bytes = read(config)?;
    check_revision(&bytes, args)?;
    let catalog = decode(&bytes)?;
    if !native_id(id) && !catalog.profiles.contains_key(id) {
        return Err(Error::new(1, "Account profile does not exist."));
    }
    let mut marks = read_marks(&config.dir)
        .ok_or_else(|| Error::new(1, "Invalid account-marks.json; repair or remove it before changing account marks."))?;
    // Forget profiles that no longer exist.
    marks.marks.retain(|known, _| native_id(known) || catalog.profiles.contains_key(known));
    let mark = marks.marks.entry(id.into()).or_default();
    for (field, value) in [(&mut mark.color, new_color), (&mut mark.emoji, new_emoji)] {
        if let Some(value) = value {
            if !(if_unset && !field.is_empty()) {
                *field = value;
            }
        }
    }
    if mark.color.is_empty() && mark.emoji.is_empty() && mark.other.is_empty() {
        marks.marks.remove(id);
    }
    // What is written must read back: the same cap as the reader, checked
    // before the original is replaced (zerus-etuiw).
    let bytes = serde_json::to_vec(&marks).map_err(|_| Error::new(1, "Cannot encode account marks."))?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::new(1, "Account marks would exceed 64 KiB on this computer; nothing was changed."));
    }
    if !dry {
        let mut f = tempfile::NamedTempFile::new_in(&config.dir)?;
        platform::make_private(f.as_file())?;
        f.write_all(&bytes)?;
        f.as_file().sync_all()?;
        platform::persist(f, config.dir.join(FILE)).map_err(|e| Error::new(1, e.to_string()))?;
        platform::sync_dir(&config.dir)?;
    }
    list(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lowercase_hex_colors_and_short_non_ascii_emoji_are_marks() {
        assert!(color("#2b97ff"));
        for bad in ["#2B97FF", "2b97ff", "#2b97f", "#2b97ffa", "#ggggggg", ""] {
            assert!(!color(bad), "{bad}");
        }
        for good in ["🦊", "🇺🇦", "👩‍💻", "1️⃣", "①"] {
            assert!(emoji(good), "{good}");
        }
        for bad in ["", "A", ":)", "🦊 🦊", "🦊\n", &"🦊".repeat(17)] {
            assert!(!emoji(bad), "{bad}");
        }
    }

    #[test]
    fn session_rows_carry_their_accounts_valid_mark_only() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(FILE),
            r##"{"marks":{"work":{"color":"#2b97ff","emoji":"🦊","later":1},"bad":{"color":"red"}},"version":2}"##,
        )
        .unwrap();
        let marks = read_marks(dir.path()).unwrap();
        assert_eq!(shown(&marks.marks["work"]), (Some("#2b97ff"), Some("🦊")));
        assert_eq!(shown(&marks.marks["bad"]), (None, None));
        // A newer hgs's fields survive a rewrite.
        let written = serde_json::to_value(&marks).unwrap();
        assert_eq!(written["version"], 2);
        assert_eq!(written["marks"]["work"]["later"], 1);
        fs::write(dir.path().join(FILE), "[").unwrap();
        assert!(read_marks(dir.path()).is_none());
        fs::write(dir.path().join(FILE), vec![b' '; LIMIT as usize + 1]).unwrap();
        assert!(read_marks(dir.path()).is_none());
    }
}
