use serde::{Deserialize, Serialize};
use zerus_desktop_ledger::task_controls::{
    Executor, Request, EXECUTOR_HOLDS, PAGE_ROWS, REQUEST_BYTES,
};
pub(crate) mod canonical;

/// Binds the complete frozen Start plan, including its acknowledgement mode.
/// This digest never supplies caller, floor, attempt or delivery authority.
pub(crate) fn start_plan_digest(plan: &impl Serialize) -> Result<String, String> {
    zerus_client_core::task_start::refusal::plan_digest(
        &serde_json::to_value(plan).map_err(|e| e.to_string())?,
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Begin {
    pub reference: String,
    pub attempt: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(
    tag = "operation",
    content = "input",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum Command {
    Begin(Begin),
    Plan(PlanRequest),
    Execute(Box<Plan>),
    Status(Status),
}
impl Command {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Begin(b) => {
                begin(&[b.reference.clone(), "--attempt".into(), b.attempt.clone()]).map(|_| ())
            }
            Self::Plan(p) => p.validate(),
            Self::Execute(p) => p.validate(),
            Self::Status(s) => s.validate(),
        }
    }
    pub fn identity(&self) -> (&'static str, Option<&str>, Option<&str>) {
        match self {
            Self::Begin(b) => ("begin_attempt", Some(&b.attempt), Some(&b.reference)),
            Self::Plan(p) => ("plan", Some(&p.request_id), None),
            Self::Execute(p) => ("execute", Some(&p.request.id), None),
            Self::Status(s) => (
                "status",
                match &s.target {
                    StatusScope::Request { request_id } => Some(request_id.as_str()),
                    _ => None,
                },
                None,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    Pause,
    Resume,
    Yield,
    ReleaseStop,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Target {
    Attempt {
        project: String,
        task: String,
        attempt: String,
    },
    Executor {
        executor: Executor,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlanRequest {
    pub schema: u32,
    pub request_id: String,
    pub action: Action,
    pub target: Target,
    pub observed_pauses: Vec<String>,
    pub observed_stops: Vec<String>,
}

/// This is captured by the owning admission, then checked against fresh local
/// evidence on execute. These serialized labels never authenticate a caller.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Caller {
    Person {},
    Executor { executor: Executor },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plan {
    pub schema: u32,
    pub request: Request,
    pub caller: Caller,
    pub configuration: String,
    pub store_identity: String,
    pub cli_image: String,
    pub loaded_adapter: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StatusScope {
    Request {
        request_id: String,
    },
    Executor {
        executor: Executor,
        after: Option<i64>,
        limit: usize,
    },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Status {
    pub schema: u32,
    pub target: StatusScope,
}

pub(crate) fn begin(args: &[String]) -> Result<Begin, String> {
    let Some(reference) = args.first().filter(|s| !s.starts_with('-')) else {
        return Err("begin-attempt requires REF --attempt UUID".into());
    };
    text(reference, 512)?;
    let mut origin = None;
    let mut json = false;
    let mut at = 1;
    while at < args.len() {
        match args[at].as_str() {
            "--attempt" if origin.is_none() && at + 1 < args.len() => {
                canonical(&args[at + 1])?;
                origin = Some(args[at + 1].clone());
                at += 2;
            }
            "--json" if !json => {
                json = true;
                at += 1;
            }
            _ => return Err("begin-attempt accepts one --attempt UUID and optional --json".into()),
        }
    }
    Ok(Begin {
        reference: reference.clone(),
        attempt: origin.ok_or("begin-attempt requires the original --attempt UUID")?,
    })
}

pub(crate) fn action(args: &[String]) -> Result<&str, String> {
    if args.len() != 2
        || args[1] != "--json"
        || !matches!(args[0].as_str(), "plan" | "execute" | "status")
    {
        return Err(
            "usage: hgs task control plan|execute|status --json (bounded JSON on stdin)".into(),
        );
    }
    Ok(&args[0])
}

pub(crate) fn read(action: &str, bytes: &[u8]) -> Result<Command, String> {
    if bytes.len() > REQUEST_BYTES {
        return Err("task-control request exceeds 16 KiB".into());
    }
    let parsed = match action {
        "plan" => {
            let p: PlanRequest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            p.validate()?;
            Command::Plan(p)
        }
        "execute" => {
            let p: Plan = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            p.validate()?;
            Command::Execute(Box::new(p))
        }
        "status" => {
            let p: Status = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
            p.validate()?;
            Command::Status(p)
        }
        _ => return Err("unknown task-control action".into()),
    };
    Ok(parsed)
}

impl PlanRequest {
    fn validate(&self) -> Result<(), String> {
        schema(self.schema)?;
        canonical(&self.request_id)?;
        observed(&self.observed_pauses)?;
        observed(&self.observed_stops)?;
        match &self.target {
            Target::Attempt {
                project,
                task,
                attempt,
            } if self.action != Action::ReleaseStop => {
                text(project, 256)?;
                canonical(task)?;
                canonical(attempt)?;
            }
            Target::Executor { executor }
                if self.action == Action::ReleaseStop
                    && self.observed_pauses.is_empty()
                    && !self.observed_stops.is_empty() =>
            {
                executor_key(executor)?
            }
            _ => return Err("task-control action and exact target disagree".into()),
        }
        Ok(())
    }
}
impl Plan {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "The service planner is connected in the next inactive native leaf"
        )
    )]
    pub fn seal(mut self) -> Result<Self, String> {
        // The service fills its current independently verified guards first.
        // This digest is only an immutable binding, never proof of those guards.
        self.request.admission = "0".repeat(64);
        self.request.admission = self.admission()?;
        self.validate()?;
        Ok(self)
    }
    pub fn admission(&self) -> Result<String, String> {
        self.validate_shape()?;
        let mut value = serde_json::to_value(self).map_err(|e| e.to_string())?;
        value["request"]
            .as_object_mut()
            .ok_or("Invalid task-control request")?
            .remove("admission");
        canonical::digest(&value)
    }
    fn validate(&self) -> Result<(), String> {
        if self.request.admission != self.admission()? {
            return Err("The original task-control Plan guards changed".into());
        }
        Ok(())
    }
    fn validate_shape(&self) -> Result<(), String> {
        schema(self.schema)?;
        self.request.validate().map_err(|e| e.to_string())?;
        if !matches!(
            self.request.action,
            zerus_desktop_ledger::task_controls::Action::Pause
                | zerus_desktop_ledger::task_controls::Action::Resume
                | zerus_desktop_ledger::task_controls::Action::Yield
                | zerus_desktop_ledger::task_controls::Action::ReleaseStop
        ) {
            return Err("unsupported task-control execute action".into());
        }
        if self.request.action != zerus_desktop_ledger::task_controls::Action::ReleaseStop
            && (self.request.slot.is_none() || self.request.turn.is_none())
        {
            return Err("task control requires its exact slot and native-turn guards".into());
        }
        canonical(&self.request.id)?;
        executor_key(&self.request.executor)?;
        observed(&self.request.observed_pauses)?;
        observed(&self.request.observed_stops)?;
        for value in [
            &self.configuration,
            &self.store_identity,
            &self.cli_image,
            &self.loaded_adapter,
        ] {
            digest(value)?;
        }
        if let Caller::Executor { executor } = &self.caller {
            executor_key(executor)?;
            if self.request.action != zerus_desktop_ledger::task_controls::Action::Yield
                || *executor != self.request.executor
            {
                return Err("this control requires a person or its exact yielding executor".into());
            }
        }
        // The records API checks the complete request again on reservation.
        // Execute additionally requires its actual caller/runtime and fresh
        // exact task/slot/native-turn evidence in the wiring leaf.
        Ok(())
    }
}
impl Status {
    fn validate(&self) -> Result<(), String> {
        schema(self.schema)?;
        match &self.target {
            StatusScope::Request { request_id } => canonical(request_id)?,
            StatusScope::Executor {
                executor,
                after,
                limit,
            } => {
                executor_key(executor)?;
                if *limit == 0 || *limit > PAGE_ROWS || after.is_some_and(|n| n <= 0) {
                    return Err("invalid task-control status page".into());
                }
            }
        }
        Ok(())
    }
}

fn schema(n: u32) -> Result<(), String> {
    if n == 1 {
        Ok(())
    } else {
        Err("unsupported task-control schema".into())
    }
}
fn canonical(s: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(s).is_ok_and(|id| id.to_string() == s) {
        Ok(())
    } else {
        Err("expected a canonical original UUID".into())
    }
}
fn text(s: &str, limit: usize) -> Result<(), String> {
    if !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control) {
        Ok(())
    } else {
        Err("invalid task-control identity".into())
    }
}
fn digest(s: &str) -> Result<(), String> {
    if s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        Ok(())
    } else {
        Err("invalid task-control proof digest".into())
    }
}
fn executor_key(e: &Executor) -> Result<(), String> {
    e.validate().map_err(|error| error.to_string())
}
fn observed(ids: &[String]) -> Result<(), String> {
    if ids.len() > EXECUTOR_HOLDS {
        return Err("too many observed holds".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for id in ids {
        canonical(id)?;
        if !seen.insert(id) {
            return Err("duplicate observed hold".into());
        }
    }
    Ok(())
}
