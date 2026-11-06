//! Task 68: project and session iron rules (Qt's IronRulesPanel over
//! HgsClient::rulesTransport). hgs owns them (`hgs [@host] rules get|set
//! --json`, iron-rules-v1, JSON on stdin); this module builds the guarded
//! calls and projects the replies, so the webview never parses either.
//!
//! Routing is Qt's: project rules live on this computer, whatever computer
//! the selected session runs on; a session's rules are read here with its
//! `host`, and written on its own computer. Every write carries the revision
//! it was read at and a request id; an uncertain write is never sent again by
//! itself: the panel reads the rules again, and the owner decides.
use crate::actions::{failure_detail, Exit, Invocation};
use crate::fleet::valid_alias;
use crate::text::Text;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

pub const MAX_RULES: usize = 20;
pub const MAX_LENGTH: usize = 300;
/// The help marker of a CLI that has `hgs rules`.
pub const MARKER: &str = "iron-rules-v1";
const READ_LIMIT: usize = 1024 * 1024;

/// Whose rules: a project of this computer's organization, or one exact
/// session (its run and conversation as the view saw them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum RulesTarget {
    Project {
        project: String,
    },
    Session {
        name: String,
        run_id: String,
        #[serde(default)]
        conversation_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RulesOp {
    Read,
    /// The whole new list, at the revision it was read at. `request_id`
    /// names an earlier uncertain attempt the owner sends again.
    Save {
        rules: Vec<String>,
        expected_revision: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesCall {
    /// The computer of the selected session; "" for this one.
    #[serde(default)]
    pub computer: String,
    pub target: RulesTarget,
    pub op: RulesOp,
}

/// The rules of one level as hgs reports them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct RulesView {
    pub rules: Vec<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RulesOutcome {
    Done {
        view: RulesView,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// The computer's hgs has no `rules`: existing rules are kept.
    Unsupported { detail: Text },
    /// Nothing was changed: refused, or never sent.
    Failed {
        detail: Text,
        #[serde(skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
    },
    /// The write may have been applied. Read again before anything else.
    Uncertain { detail: Text, request_id: String },
}

fn plain(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
}

/// One rule as hgs stores it: its whitespace collapsed.
pub fn simplified(rule: &str) -> String {
    rule.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Qt's `IronRules::problem`: a list hgs would refuse. Overflow is refused,
/// never truncated.
pub fn problem(rules: &[String]) -> Option<Text> {
    if rules.len() > MAX_RULES {
        return Some(crate::text!("Use at most 20 rules per level."));
    }
    if rules.iter().any(|rule| {
        let text = simplified(rule);
        text.is_empty()
            || rule.chars().count() > MAX_LENGTH
            || rule.chars().any(|c| c.is_control() && !c.is_whitespace())
    }) {
        return Some(crate::text!("Each rule must contain 1 to 300 characters."));
    }
    None
}

impl RulesCall {
    fn check(&self) -> Result<(), Text> {
        if !self.computer.is_empty() && !valid_alias(&self.computer) {
            return Err(crate::text!("Not a computer alias."));
        }
        match &self.target {
            RulesTarget::Project { project } if !plain(project, 200) => {
                Err(crate::text!("Choose a project first."))
            }
            RulesTarget::Session {
                name,
                run_id,
                conversation_id,
            } if !plain(name, 2048)
                || !plain(run_id, 2048)
                || conversation_id.len() > 2048
                || conversation_id.chars().any(char::is_control) =>
            {
                Err(crate::text!(
                    "This session has no running agent to hold rules."
                ))
            }
            _ => Ok(()),
        }
    }

    /// Where the call runs: "" for this computer.
    pub fn destination(&self) -> &str {
        match (&self.target, &self.op) {
            (RulesTarget::Session { .. }, RulesOp::Save { .. }) => &self.computer,
            _ => "",
        }
    }

    /// The JSON hgs reads on stdin.
    pub fn input(&self, request_id: Option<&str>) -> Value {
        let mut input = match &self.target {
            RulesTarget::Project { project } => json!({"scope":"project","project":project}),
            RulesTarget::Session {
                name,
                run_id,
                conversation_id,
            } => json!({"scope":"session","name":name,
                "expected_run_id":run_id,"expected_conversation_id":conversation_id}),
        };
        input["by"] = json!("human");
        match &self.op {
            RulesOp::Read => {
                if matches!(self.target, RulesTarget::Session { .. }) && !self.computer.is_empty() {
                    input["host"] = json!(self.computer);
                }
            }
            RulesOp::Save {
                rules,
                expected_revision,
                ..
            } => {
                input["rules"] = json!(rules
                    .iter()
                    .map(|rule| simplified(rule))
                    .collect::<Vec<_>>());
                input["expected_revision"] = json!(expected_revision);
                input["request_id"] = json!(request_id.unwrap_or_default());
            }
        }
        input
    }

    fn with_destination(&self, args: &[&str]) -> Vec<String> {
        let destination = self.destination();
        let mut out: Vec<String> = (!destination.is_empty())
            .then(|| format!("@{destination}"))
            .into_iter()
            .collect();
        out.extend(args.iter().map(|a| (*a).to_owned()));
        out
    }

    /// `hgs [@host] --help`, for the marker.
    pub fn probe(&self) -> Invocation {
        Invocation {
            args: self.with_destination(&["--help"]),
            stdin: None,
            timeout: Duration::from_secs(15),
            request_id: None,
        }
    }

    /// The checked call and its request id (writes only). A call that cannot
    /// be made gives its outcome instead.
    pub fn prepare(&self) -> Result<(Invocation, Option<String>), RulesOutcome> {
        let request_id = match &self.op {
            RulesOp::Read => None,
            RulesOp::Save { request_id, .. } => Some(
                request_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            ),
        };
        let failed = |detail: Text| RulesOutcome::Failed {
            detail,
            request_id: request_id.clone(),
        };
        if let Some(id) = &request_id {
            if !crate::teams::action::canonical_uuid(id) {
                return Err(failed(crate::text!("Not a request id.")));
            }
        }
        self.check().map_err(failed)?;
        if let RulesOp::Save { rules, .. } = &self.op {
            if let Some(problem) = problem(rules) {
                return Err(failed(problem));
            }
        }
        let op = if matches!(self.op, RulesOp::Read) {
            "get"
        } else {
            "set"
        };
        let input = self.input(request_id.as_deref());
        Ok((
            Invocation {
                args: self.with_destination(&["rules", op, "--json"]),
                stdin: Some(input.to_string().into_bytes()),
                timeout: Duration::from_secs(30),
                request_id: request_id.clone(),
            },
            request_id,
        ))
    }
}

/// The rules of a reply; None for a reply hgs would not have written.
pub fn parse(value: &Value) -> Option<RulesView> {
    let rules = value.get("rules")?.as_array()?;
    if rules.len() > MAX_RULES {
        return None;
    }
    let rules = rules
        .iter()
        .map(|rule| {
            rule.as_str()
                .filter(|r| plain(r, MAX_LENGTH * 4) && r.chars().count() <= MAX_LENGTH)
                .map(str::to_owned)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RulesView {
        rules,
        revision: value.get("revision").and_then(Value::as_u64).unwrap_or(0),
    })
}

fn unsupported() -> RulesOutcome {
    RulesOutcome::Unsupported {
        detail: crate::text!(
            "Update this computer's hgs to edit iron rules. Existing rules are kept."
        ),
    }
}

/// Sorts what the runner saw into an outcome. A write that may have run is
/// uncertain; a read never is.
pub fn classify(write: bool, request_id: Option<String>, exit: Exit) -> RulesOutcome {
    let uncertain = |detail: Text, request_id: Option<String>| match (write, request_id) {
        (true, Some(request_id)) => RulesOutcome::Uncertain { detail, request_id },
        (_, request_id) => RulesOutcome::Failed { detail, request_id },
    };
    match exit {
        Exit::Code {
            code: 0, stdout, ..
        } => {
            let view = serde_json::from_slice::<Value>(&stdout).ok();
            if view.as_ref().is_some_and(|v| v["unsupported"] == true) {
                return unsupported();
            }
            match view.as_ref().and_then(parse) {
                Some(view) => RulesOutcome::Done { view, request_id },
                None => uncertain(crate::text!("Invalid iron rules response."), request_id),
            }
        }
        exit @ (Exit::Code { .. } | Exit::StartFailed(_)) => RulesOutcome::Failed {
            detail: Text::verbatim(failure_detail(exit)),
            request_id,
        },
        exit => uncertain(Text::verbatim(failure_detail(exit)), request_id),
    }
}

/// Runs one call: the marker probe, then the call itself.
pub fn run(hgs: &Path, call: &RulesCall) -> RulesOutcome {
    let (invocation, request_id) = match call.prepare() {
        Ok(prepared) => prepared,
        Err(outcome) => return outcome,
    };
    match crate::run::run_limited(hgs, &call.probe(), &[], READ_LIMIT) {
        Ok(Exit::Code {
            code: 0, stdout, ..
        }) if String::from_utf8_lossy(&stdout).contains(MARKER) => {}
        Ok(Exit::Code { code: 0, .. }) => return unsupported(),
        Ok(other) => {
            return RulesOutcome::Failed {
                detail: Text::verbatim(failure_detail(other)),
                request_id,
            }
        }
        Err(_) => return unsupported(),
    }
    let write = matches!(call.op, RulesOp::Save { .. });
    match crate::run::run_limited(hgs, &invocation, &[], READ_LIMIT) {
        Ok(exit) => classify(write, request_id, exit),
        Err(_) => classify(
            write,
            request_id,
            Exit::Abnormal {
                stderr: "Iron rules reply is too large.".into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> RulesTarget {
        RulesTarget::Session {
            name: "claude/app/main".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
        }
    }
    fn save(rules: &[&str]) -> RulesOp {
        RulesOp::Save {
            rules: rules.iter().map(|r| (*r).to_owned()).collect(),
            expected_revision: 3,
            request_id: None,
        }
    }
    fn stdin(invocation: &Invocation) -> Value {
        serde_json::from_slice(invocation.stdin.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn project_rules_live_on_this_computer() {
        let call = RulesCall {
            computer: "studio".into(),
            target: RulesTarget::Project {
                project: "p-1".into(),
            },
            op: RulesOp::Read,
        };
        let (invocation, request_id) = call.prepare().unwrap();
        assert_eq!(invocation.args, ["rules", "get", "--json"]);
        assert_eq!(request_id, None);
        assert_eq!(
            stdin(&invocation),
            json!({"scope":"project","project":"p-1","by":"human"})
        );
        assert_eq!(call.probe().args, ["--help"]);
    }

    #[test]
    fn a_remote_session_is_read_here_and_written_on_its_computer() {
        let read = RulesCall {
            computer: "studio".into(),
            target: session(),
            op: RulesOp::Read,
        };
        let (invocation, _) = read.prepare().unwrap();
        assert_eq!(invocation.args, ["rules", "get", "--json"]);
        assert_eq!(stdin(&invocation)["host"], "studio");
        assert_eq!(stdin(&invocation)["expected_run_id"], "run-1");
        let write = RulesCall {
            computer: "studio".into(),
            target: session(),
            op: save(&["  Keep   tests green "]),
        };
        let (invocation, request_id) = write.prepare().unwrap();
        assert_eq!(invocation.args, ["@studio", "rules", "set", "--json"]);
        assert_eq!(write.probe().args, ["@studio", "--help"]);
        let input = stdin(&invocation);
        assert_eq!(input["rules"], json!(["Keep tests green"]));
        assert_eq!(input["expected_revision"], 3);
        assert_eq!(input["by"], "human");
        assert!(
            input.get("host").is_none(),
            "a write runs on the session's computer"
        );
        let request_id = request_id.unwrap();
        assert!(crate::teams::action::canonical_uuid(&request_id));
        assert_eq!(input["request_id"], request_id);
        assert_eq!(invocation.request_id.as_deref(), Some(request_id.as_str()));
    }

    #[test]
    fn a_retry_keeps_its_request_id_and_bad_calls_never_run() {
        let id = "11111111-1111-4111-8111-111111111111";
        let call = RulesCall {
            computer: String::new(),
            target: RulesTarget::Project {
                project: "p".into(),
            },
            op: RulesOp::Save {
                rules: vec!["a".into()],
                expected_revision: 0,
                request_id: Some(id.into()),
            },
        };
        assert_eq!(call.prepare().unwrap().1.as_deref(), Some(id));
        let refused = |call: RulesCall| matches!(call.prepare(), Err(RulesOutcome::Failed { .. }));
        assert!(refused(RulesCall {
            computer: "-oProxy=x".into(),
            ..call.clone()
        }));
        assert!(refused(RulesCall {
            target: RulesTarget::Project {
                project: String::new()
            },
            ..call.clone()
        }));
        assert!(refused(RulesCall {
            target: RulesTarget::Session {
                name: "s".into(),
                run_id: String::new(),
                conversation_id: String::new()
            },
            ..call.clone()
        }));
        assert!(refused(RulesCall {
            op: RulesOp::Save {
                rules: vec!["a".into()],
                expected_revision: 0,
                request_id: Some("nope".into())
            },
            ..call.clone()
        }));
        let many: Vec<&str> = vec!["rule"; 21];
        assert!(refused(RulesCall {
            op: save(&many),
            ..call.clone()
        }));
        assert!(refused(RulesCall {
            op: save(&["   "]),
            ..call.clone()
        }));
        assert!(refused(RulesCall {
            op: save(&[&"x".repeat(301)]),
            ..call
        }));
    }

    #[test]
    fn problems_match_qt() {
        assert_eq!(
            problem(&vec!["a".into(); 21]).unwrap().english(),
            "Use at most 20 rules per level."
        );
        assert_eq!(
            problem(&["".into()]).unwrap().english(),
            "Each rule must contain 1 to 300 characters."
        );
        assert_eq!(
            problem(&["a\u{7}".into()]).unwrap().english(),
            "Each rule must contain 1 to 300 characters."
        );
        assert!(problem(&vec!["x".repeat(300); 20]).is_none());
    }

    #[test]
    fn replies_are_projected_and_writes_that_may_have_run_are_uncertain() {
        let ok = |value: Value| Exit::Code {
            code: 0,
            stdout: value.to_string().into_bytes(),
            stderr: String::new(),
        };
        let done = classify(
            false,
            None,
            ok(json!({"scope":"project","rules":["One","Two"],"revision":4})),
        );
        assert_eq!(
            done,
            RulesOutcome::Done {
                view: RulesView {
                    rules: vec!["One".into(), "Two".into()],
                    revision: 4
                },
                request_id: None
            }
        );
        assert!(matches!(
            classify(false, None, ok(json!({"unsupported":true,"rules":[]}))),
            RulesOutcome::Unsupported { .. }
        ));
        let id = Some("11111111-1111-4111-8111-111111111111".to_owned());
        assert!(matches!(
            classify(true, id.clone(), Exit::TimedOut),
            RulesOutcome::Uncertain { .. }
        ));
        assert!(matches!(
            classify(true, id.clone(), ok(json!({"rules":"x"}))),
            RulesOutcome::Uncertain { .. }
        ));
        assert!(matches!(
            classify(false, None, Exit::TimedOut),
            RulesOutcome::Failed { .. }
        ));
        let refused = classify(
            true,
            id,
            Exit::Code {
                code: 1,
                stdout: vec![],
                stderr: "Session rules changed; read them again".into(),
            },
        );
        match refused {
            RulesOutcome::Failed { detail, .. } => {
                assert_eq!(detail.english(), "Session rules changed; read them again")
            }
            other => panic!("{other:?}"),
        }
        assert!(parse(&json!({"rules":vec!["r"; 21]})).is_none());
        assert!(parse(&json!({"rules":[1]})).is_none());
    }
}
