//! The person's disposition of one unknown attempt. No native notice,
//! receipt recovery or session effect belongs to this command.
use super::*;

struct Request {
    reference: Vec<String>,
    attempt: String,
    reason: String,
}

fn parse(args: &[String]) -> Result<Request> {
    let Some(reference) = args
        .first()
        .filter(|s| !s.starts_with('-') && !s.is_empty())
    else {
        return Err(fail("cancel-attempt requires REF --attempt UUID"));
    };
    let mut request = Request {
        reference: vec![reference.clone()],
        attempt: String::new(),
        reason: String::new(),
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut at = 1;
    while at < args.len() {
        let key = args[at].as_str();
        if !seen.insert(key) {
            return Err(fail("cancel-attempt refuses duplicate options"));
        }
        match key {
            "--attempt" | "--reason" | "--project" if at + 1 < args.len() => {
                let value = args[at + 1].clone();
                match key {
                    "--attempt" => request.attempt = value,
                    "--reason" => request.reason = value,
                    _ => request.reference.extend([key.into(), value]),
                }
                at += 2;
            }
            "--json" => at += 1,
            _ => {
                return Err(fail(
                    "cancel-attempt accepts REF --attempt UUID [--reason TEXT] [--project P] [--json]",
                ));
            }
        }
    }
    if !zerus_replica::ids::is_uuid(&request.attempt) {
        return Err(fail(
            "cancel-attempt requires --attempt with a canonical UUID",
        ));
    }
    if request.reason.len() > MAX_TEXT || request.reason.contains('\0') {
        return Err(fail("invalid attempt abandonment reason"));
    }
    Ok(request)
}

pub(super) fn execute(config: &Config, args: &[String]) -> Result<Value> {
    let request = parse(args)?;
    let machines = machines(config)?;
    // No delegation, --agent forwarding or team service identity can become
    // Person. Parsed text values are never reinterpreted as actor options.
    let who = actor(&machines.here, &[], false)?;
    if who != Actor::Person {
        return Err(fail(
            "only the person abandons an attempt whose delivery is unknown",
        ));
    }
    let reference = reference(config, &request.reference, &who)?;
    let store = store(config, &reference.project.0)?;
    let mut task = find(&store, &reference.task)?;
    // Project writer first, then the same team writer fence through save.
    // A terminal-team check without this retained fence races Resume.
    let _team_admission =
        crate::teams::tasks::abandon_admission(&config.state, &machines.here, &task)
            .map_err(|error| fail(format!("{}: {}", error.code, error.message)))?;
    let before = task.revision();
    attempts::pause_at(&request.attempt, "cancel_attempt");
    task.cancel_attempt(&request.attempt, &request.reason, &who, now())
        .map_err(fail)?;
    let repeated = task.revision() == before;
    if !repeated {
        store.save(&mut task).map_err(fail)?;
    }
    Ok(json!({
        // One bounded task row: the selected history is read separately.
        "task": view(&task, &reference.project.1, false, &machines)?,
        "attempt": {"id": request.attempt, "state": "cancelled",
            "delivery": "unknown", "already_cancelled": repeated}
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_scope_is_strict_and_text_is_never_an_actor_option() {
        let args = |tail: &[&str]| tail.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let id = "11111111-1111-4111-8111-111111111111";
        let request = parse(&args(&[
            "example/T-123456",
            "--attempt",
            id,
            "--reason",
            "--agent",
        ]))
        .unwrap();
        assert_eq!(request.reason, "--agent");
        assert_eq!(request.reference, ["example/T-123456"]);
        for bad in [
            args(&[]),
            args(&["T-123456"]),
            args(&["T-123456", "--attempt"]),
            args(&["T-123456", "--attempt", "latest"]),
            args(&["T-123456", "--attempt", id, "--attempt", id]),
            args(&["T-123456", "--attempt", id, "--agent", "someone"]),
            args(&["T-123456", "--attempt", id, "--recover-only"]),
        ] {
            assert!(parse(&bad).is_err(), "{bad:?}");
        }
    }
}
