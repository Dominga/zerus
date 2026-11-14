//! One command's words and its one JSON request on stdin. Every refusal names
//! the stable code a caller matches on, never a serde message.
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};
use zerus_team_plan::problem::code;
use zerus_team_plan::Problem;

/// Where each command group says how it is called.
pub(super) const USAGE: &str = "usage: hgs team <command> [--json]";

/// The words of a command, without the global output flag.
pub(super) fn words(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|word| word.as_str() != "--json")
        .cloned()
        .collect()
}

/// The options a command accepts: those that take a value and those that are
/// a plain switch. Anything else is refused instead of ignored.
pub(super) struct Options {
    values: BTreeMap<String, String>,
    switches: BTreeSet<String>,
    rest: Vec<String>,
}

impl Options {
    pub(super) fn parse(
        args: &[String],
        valued: &[&str],
        switches: &[&str],
    ) -> Result<Self, Problem> {
        let mut options = Self {
            values: BTreeMap::new(),
            switches: BTreeSet::new(),
            rest: Vec::new(),
        };
        let mut words = args.iter().map(String::as_str);
        while let Some(word) = words.next() {
            match word {
                _ if valued.contains(&word) => {
                    let value = words
                        .next()
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| usage(format!("{word} needs a value")))?;
                    if options.values.insert(word.into(), value.into()).is_some() {
                        return Err(usage(format!("{word} was given twice")));
                    }
                }
                _ if switches.contains(&word) => {
                    options.switches.insert(word.into());
                }
                _ if word.starts_with("--") => {
                    return Err(usage(format!("unknown option: {word}")));
                }
                _ => options.rest.push(word.into()),
            }
        }
        Ok(options)
    }

    pub(super) fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub(super) fn has(&self, name: &str) -> bool {
        self.switches.contains(name)
    }

    /// The one word this command requires after its own name.
    pub(super) fn required(&self, what: &str) -> Result<&str, Problem> {
        match self.rest.as_slice() {
            [one] => Ok(one),
            [] => Err(usage(format!("{what} is missing"))),
            _ => Err(usage(format!("unexpected words: {}", self.rest.join(" ")))),
        }
    }

    /// No word beyond those the options carry.
    pub(super) fn nothing_else(&self, what: &str) -> Result<(), Problem> {
        self.rest
            .is_empty()
            .then_some(())
            .ok_or_else(|| usage(format!("{what} takes no words")))
    }

    /// A page size and cursor, both bounded by the store's own limits.
    pub(super) fn page(&self) -> Result<(u32, Option<&str>), Problem> {
        let limit = match self.get("--limit") {
            None => 50,
            Some(text) => match text.parse::<u32>() {
                Ok(limit) if (1..=crate::teams::store::MAX_PAGE).contains(&limit) => limit,
                _ => {
                    return Err(Problem::new(
                        code::OVER_CAP,
                        format!(
                            "--limit must be between 1 and {}",
                            crate::teams::store::MAX_PAGE
                        ),
                    ))
                }
            },
        };
        Ok((limit, self.get("--after")))
    }
}

impl Options {
    /// A target team named on the command line, for the person's own reads.
    pub(super) fn team(&self) -> Result<Option<&str>, Problem> {
        match self.get("--team") {
            None => Ok(None),
            Some(id) => {
                let valid = uuid::Uuid::parse_str(id).is_ok_and(|parsed| parsed.to_string() == id);
                if !valid {
                    return Err(Problem::new(
                        code::INVALID_STATE,
                        "--team needs a lowercase canonical team ID",
                    ));
                }
                Ok(Some(id))
            }
        }
    }
}

pub(super) fn usage(text: impl Into<String>) -> Problem {
    Problem::new(code::INVALID_STATE, format!("{}; {USAGE}", text.into()))
}

pub(super) fn invalid(text: impl Into<String>) -> Problem {
    Problem::new(code::INVALID_STATE, text)
}

/// The request object on stdin. An empty stdin is refused before anything is
/// read twice, and a key the operation does not know is named.
pub(super) fn object(input: &str) -> Result<Map<String, Value>, Problem> {
    let text = input.trim();
    if text.is_empty() {
        return Err(invalid("This command reads one JSON request on stdin."));
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(invalid("The request must be one JSON object.")),
        Err(error) => Err(serde_problem(error.to_string())),
    }
}

/// One JSON value on stdin, for the catalog files and bundles.
pub(super) fn value(input: &str, what: &str) -> Result<Value, Problem> {
    let text = input.trim();
    if text.is_empty() {
        return Err(invalid(format!("This command reads one {what} on stdin.")));
    }
    serde_json::from_str(text).map_err(|error| serde_problem(error.to_string()))
}

/// A refusal shaped from serde's message, with the unknown key's own code.
pub(super) fn serde_problem(message: String) -> Problem {
    if let Some(rest) = message.strip_prefix("unknown field `") {
        if let Some(field) = rest.split('`').next() {
            return Problem::new(
                code::UNKNOWN_FIELD,
                format!("The request has an unknown field: {field}."),
            )
            .with_refs([field]);
        }
    }
    invalid(message)
}

/// The operation clock: milliseconds since the Unix epoch.
pub(super) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The five reads share a cursor token that is opaque to a person and is the
/// store's own cursor inside. It is never interpreted by the desktop.
pub(super) fn cursor_token(cursor: &crate::teams::store::Cursor) -> Result<String, Problem> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let bytes = serde_json::to_vec(cursor).map_err(|error| serde_problem(error.to_string()))?;
    Ok(format!("t1.{}", URL_SAFE_NO_PAD.encode(bytes)))
}

pub(super) fn parse_cursor(token: &str) -> Result<crate::teams::store::Cursor, Problem> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let refused = || invalid("The cursor does not belong to this list; read the first page.");
    let encoded = token.strip_prefix("t1.").ok_or_else(refused)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| refused())?;
    serde_json::from_slice(&bytes).map_err(|_| refused())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &str) -> Vec<String> {
        words.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_output_flag_is_never_a_command_word() {
        assert_eq!(
            words(&args("status --json --team x")),
            ["status", "--team", "x"]
        );
    }

    #[test]
    fn options_refuse_unknown_switches_and_repeated_values() {
        let options =
            Options::parse(&args("--bundle --team a"), &["--team"], &["--bundle"]).unwrap();
        assert!(options.has("--bundle"));
        assert_eq!(options.get("--team"), Some("a"));
        assert!(Options::parse(&args("--colour red"), &["--team"], &["--bundle"]).is_err());
        assert!(Options::parse(&args("--team a --team b"), &["--team"], &[]).is_err());
        assert!(Options::parse(&args("--team"), &["--team"], &[]).is_err());
    }

    #[test]
    fn a_team_id_is_a_lowercase_canonical_uuid() {
        let good = "00000000-0000-4000-8000-00000000beef";
        let options = Options::parse(&args(&format!("--team {good}")), &["--team"], &[]).unwrap();
        assert_eq!(options.team().unwrap(), Some(good));
        for bad in ["example", "00000000-0000-4000-8000-00000000BEEF"] {
            let options =
                Options::parse(&args(&format!("--team {bad}")), &["--team"], &[]).unwrap();
            assert!(options.team().is_err(), "{bad}");
        }
        let options = Options::parse(&args(""), &["--team"], &[]).unwrap();
        assert_eq!(options.team().unwrap(), None);
    }

    #[test]
    fn a_page_is_bounded_and_defaults_to_fifty() {
        let options = Options::parse(
            &args("--limit 200 --after t1.x"),
            &["--limit", "--after"],
            &[],
        )
        .unwrap();
        assert_eq!(options.page().unwrap(), (200, Some("t1.x")));
        for bad in ["0", "201", "many"] {
            let options =
                Options::parse(&args(&format!("--limit {bad}")), &["--limit"], &[]).unwrap();
            assert_eq!(options.page().unwrap_err().code, code::OVER_CAP, "{bad}");
        }
        let options = Options::parse(&args(""), &["--limit"], &[]).unwrap();
        assert_eq!(options.page().unwrap(), (50, None));
    }

    #[test]
    fn an_unknown_field_is_named_with_its_own_code() {
        let problem = serde_problem("unknown field `actor`, expected one of `team`".into());
        assert_eq!(problem.code, code::UNKNOWN_FIELD);
        assert_eq!(problem.refs, ["actor"]);
        assert_eq!(
            serde_problem("missing field `team`".into()).code,
            code::INVALID_STATE
        );
    }

    #[test]
    fn a_request_is_one_json_object_and_nothing_else() {
        assert!(object(r#"{"team":"x"}"#).is_ok());
        assert_eq!(object("").unwrap_err().code, code::INVALID_STATE);
        assert_eq!(object("[]").unwrap_err().code, code::INVALID_STATE);
        assert_eq!(object("{").unwrap_err().code, code::INVALID_STATE);
    }

    #[test]
    fn a_cursor_round_trips_and_only_for_its_own_list() {
        let cursor = crate::teams::store::Cursor {
            scope: "team:nodes:None".into(),
            ceiling: 9,
            after: 4,
            rank: "a4".into(),
        };
        let token = cursor_token(&cursor).unwrap();
        assert!(token.starts_with("t1."));
        assert_eq!(parse_cursor(&token).unwrap(), cursor);
        for bad in ["", "t1.", "t2.e30", "xx"] {
            assert!(parse_cursor(bad).is_err(), "{bad}");
        }
    }
}
