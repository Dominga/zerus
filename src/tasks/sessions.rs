//! Resolve task links from one saved-binding snapshot per command. Labels never
//! establish identity once a task has an immutable session_id.
use super::*;

pub(super) fn identity(row: &Value) -> String {
    for key in ["launch_id", "run_id"] {
        if let Some(id) = row[key].as_str().filter(|id| !id.is_empty()) {
            return format!("{key}:{id}");
        }
    }
    String::new()
}

impl Machines {
    pub(super) fn target(
        &self,
        machine: String,
        name: &str,
        args: &[String],
    ) -> Result<SessionKey> {
        let mut key = SessionKey {
            machine,
            session: name.into(),
            session_id: option(args, "--session-id")?.unwrap_or_default().into(),
        };
        if key.session_id.len() > 200 || key.session_id.chars().any(char::is_control) {
            return Err(fail("invalid task session identity"));
        }
        if let Some(current) = self.resolve(&key) {
            key = current;
        } else {
            return Err(fail("the selected session identity is no longer available"));
        }
        Ok(key)
    }
    pub(super) fn named(&self, machine: String, name: &str) -> Result<SessionKey> {
        self.resolve(&SessionKey {
            machine,
            session: name.into(),
            ..Default::default()
        })
        .ok_or_else(|| fail("ambiguous task session identity; choose the session again"))
    }

    /// Unknown remote bindings remain hints for that computer to resolve. A
    /// known local ID with no unique binding refuses rather than using its name.
    pub(super) fn resolve(&self, key: &SessionKey) -> Option<SessionKey> {
        if key.machine != self.here {
            return Some(key.clone());
        }
        let matches = |row: &&Value| {
            if !key.session_id.is_empty() {
                identity(row) == key.session_id
            } else {
                row["name"].as_str() == Some(&key.session)
            }
        };
        let mut found = self.bindings.iter().filter(matches);
        if let Some(row) = found.next() {
            if found.next().is_some() {
                return None;
            }
            return Some(SessionKey {
                machine: key.machine.clone(),
                session: row["name"].as_str()?.into(),
                session_id: identity(row),
            });
        }
        // A legacy task has no ID. Only a unique retained original label can
        // recover an old rename, and an exact current name takes precedence.
        if key.session_id.is_empty() {
            let mut aliases = self.bindings.iter().filter(|row| {
                row["journal_name"].as_str() == Some(&key.session)
                    || row["legacy_shadow"]["name"].as_str() == Some(&key.session)
            });
            if let Some(row) = aliases.next() {
                if aliases.next().is_some() {
                    return None;
                }
                return Some(SessionKey {
                    machine: key.machine.clone(),
                    session: row["name"].as_str()?.into(),
                    session_id: identity(row),
                });
            }
            return Some(key.clone()); // existing name-only tasks remain readable
        }
        None
    }
}
