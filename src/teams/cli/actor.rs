//! Map a proved caller to its team. The proof itself lives in [`super::caller`]
//! so that task intake uses the same one; this module only reads the persisted
//! member bindings and refuses a command whose actor kind is wrong.
use crate::teams::ops::{Actor, PersonOrigin};
use crate::teams::store::Store;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use zerus_team_plan::model::SessionKey;
use zerus_team_plan::problem::code;
use zerus_team_plan::Problem;

pub(super) use crate::teams::caller::Caller;

/// The one person identifier Zerus knows today.
pub(super) const PERSON: &str = "local";

/// The actor of this command and the team it is bound to, if any.
pub(super) struct Identity {
    pub(super) actor: Actor,
    pub(super) team: Option<String>,
    pub(super) member: Option<String>,
}

/// The actor this caller acts as, after the team's persisted session bindings
/// are consulted. Only an exact `(machine, session_id)` binding maps a session
/// to a member; a name never does.
pub(super) fn resolve(store: &Store, caller: &Caller) -> Result<Identity, Problem> {
    match caller {
        Caller::Person => Ok(Identity {
            actor: Actor::Person {
                person: PERSON.into(),
                origin: PersonOrigin::CliInterim,
            },
            team: None,
            member: None,
        }),
        Caller::Refused(problem) => Err(problem.clone()),
        Caller::Session(key) => match bound_member(store, key)? {
            Some((team, member)) => {
                let owner = store
                    .team(&team)?
                    .map_or_else(|| PERSON.to_owned(), |team| team.owner);
                Ok(Identity {
                    actor: Actor::Session {
                        person: owner,
                        team: team.clone(),
                        member: member.clone(),
                        session: key.clone(),
                    },
                    team: Some(team),
                    member: Some(member),
                })
            }
            None => Ok(Identity {
                actor: Actor::Other {
                    person: PERSON.into(),
                    session: key.clone(),
                },
                team: None,
                member: None,
            }),
        },
    }
}

/// The member a session key is bound to, by identity alone.
pub(super) fn bound_member(
    store: &Store,
    key: &SessionKey,
) -> Result<Option<(String, String)>, Problem> {
    let Some(db) = store.db.as_ref() else {
        return Ok(None);
    };
    db.query_row(
        "SELECT team,id FROM members WHERE machine=?1 AND session_id=?2 LIMIT 2",
        rusqlite::params![key.machine, key.session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(|error| {
        Problem::new(
            code::INVALID_STATE,
            format!("Cannot read member bindings: {error}"),
        )
    })
}

/// The actor as every reply shows it. A person's reply names the limits of the
/// caller proofs (`caller::limits`).
pub(super) fn view(actor: &Actor) -> Value {
    match actor {
        Actor::Person { person, origin } => {
            let mut view = json!({
                "kind": "person",
                "person": person,
                "origin": match origin {
                    PersonOrigin::DesktopUi => "desktop_ui",
                    PersonOrigin::CliInterim => "cli_interim",
                },
                "interim": matches!(origin, PersonOrigin::CliInterim),
            });
            if matches!(origin, PersonOrigin::CliInterim) {
                view["note"] = json!(crate::teams::caller::limits());
            }
            view
        }
        Actor::Session {
            person,
            team,
            member,
            ..
        } => json!({"kind": "session", "person": person, "team": team, "member": member}),
        Actor::Other { person, session } => json!({
            "kind": "other",
            "person": person,
            "session_id": session.session_id,
        }),
        Actor::Service { person, team } => {
            json!({"kind": "service", "person": person, "team": team})
        }
    }
}

/// The team a read or operation is about: named, or the caller's own team.
pub(super) fn target_team(
    store: &Store,
    caller: &Identity,
    named: Option<&str>,
) -> Result<String, Problem> {
    if let Some(id) = named {
        if store.team(id)?.is_none() {
            return Err(Problem::new(code::NOT_FOUND, "Team does not exist.").with_refs([id]));
        }
        return Ok(id.to_owned());
    }
    caller.team.clone().ok_or_else(|| {
        Problem::new(
            code::NOT_FOUND,
            "Name the team with --team TEAM, or run from its lead or member session.",
        )
    })
}

/// Leads and the person read the team's lists; a member and any other session
/// do not. Returns the caller's member ID for a lead, `None` for the person.
pub(super) fn lead_reader(
    store: &Store,
    caller: &Identity,
    team: &str,
) -> Result<Option<String>, Problem> {
    match &caller.actor {
        Actor::Person { .. } => Ok(None),
        Actor::Session {
            team: session_team,
            member,
            ..
        } => {
            // A lead is a lead of one team. A member ID is unique only inside
            // its own team, so the proved session's team must be the target.
            if session_team != team {
                return Err(not_allowed("This session does not belong to that team."));
            }
            let lead = store
                .team(team)?
                .map(|team| team.lead)
                .ok_or_else(|| Problem::new(code::NOT_FOUND, "Team does not exist."))?;
            if member == &lead {
                Ok(Some(member.clone()))
            } else {
                Err(not_allowed(
                    "Only the team's lead or the person reads this list.",
                ))
            }
        }
        _ => Err(not_allowed(
            "Only the team's lead or the person reads this list.",
        )),
    }
}

/// A member session and nothing else, for `current` and `ask`.
pub(super) fn member_session(caller: &Identity, what: &str) -> Result<(), Problem> {
    match &caller.actor {
        Actor::Session { .. } => Ok(()),
        _ => Err(not_allowed(format!(
            "{what} runs only from a member session."
        ))),
    }
}

/// Catalog and template commands change the person's own configuration.
pub(super) fn person_only(caller: &Identity, what: &str) -> Result<(), Problem> {
    match &caller.actor {
        Actor::Person { .. } => Ok(()),
        _ => Err(not_allowed(format!("{what} is the person's own command."))),
    }
}

pub(super) fn not_allowed(text: impl Into<String>) -> Problem {
    Problem::new(code::NOT_ALLOWED, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_session_runs_a_member_command() {
        let session = Actor::Session {
            person: PERSON.into(),
            team: "team".into(),
            member: "m1".into(),
            session: SessionKey {
                machine: "m".into(),
                session_id: "launch_id:l".into(),
            },
        };
        assert!(member_session(
            &Identity {
                actor: session,
                team: Some("team".into()),
                member: Some("m1".into()),
            },
            "current"
        )
        .is_ok());
        let other = Actor::Other {
            person: PERSON.into(),
            session: SessionKey {
                machine: "m".into(),
                session_id: "run_id:r".into(),
            },
        };
        assert!(member_session(
            &Identity {
                actor: other,
                team: None,
                member: None,
            },
            "current"
        )
        .is_err());
    }
}
