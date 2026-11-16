//! Data resolution and the ordinary task producer for native team intake.
//! The desktop original owns admission; this module never constructs an actor
//! for a command or opens a session-delivery path.
use super::*;
use zerus_team_plan::{problem::code, Problem};
type NativeResult<T> = std::result::Result<T, Problem>;

pub(crate) fn project_name(config: &Config, id: &str) -> NativeResult<String> {
    crate::swarm::existing_project_name(config, id)
        .map_err(|error| Problem::new(code::INVALID_STATE, error.to_string()))
}

/// Called only after the native original has a durable Unknown reservation.
/// Placement, identity and creation fingerprints are the ordinary producer's.
/// An existing object is never adopted, even if its visible title matches.
pub(crate) fn new_document(
    store: &ProjectStore,
    project_name: &str,
    id: &str,
    title: &str,
    backlog: bool,
    now: i64,
) -> NativeResult<TaskDoc> {
    absent(store, id)?;
    let (document, created) = placed(store, |placement| {
        store
            .new_named_task_placed(
                placement,
                title,
                "",
                &[],
                if backlog { Stage::Backlog } else { Stage::Todo },
                Target::Any,
                Actor::Person,
                &Position::End,
                project_name,
                now,
                Some(id),
                false,
            )
            .map_err(fail)
    })
    .map_err(|error| Problem::new(code::STORAGE_FAILED, error.message))?;
    if !created {
        return Err(Problem::new(
            code::DUPLICATE,
            "The reserved task already exists. Read its original result.",
        ));
    }
    Ok(document)
}

pub(crate) fn absent(store: &ProjectStore, id: &str) -> NativeResult<()> {
    for suffix in ["loro", "updates", "managed", "cutover", "prepare", "kept"] {
        let path = store.dir().join("tasks").join(format!("{id}.{suffix}"));
        match std::fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Ok(_) => {
                return Err(Problem::new(
                    code::DUPLICATE,
                    "The reserved task identity already has evidence. Nothing was adopted.",
                ))
            }
            Err(error) => return Err(Problem::new(code::STORAGE_FAILED, error.to_string())),
        }
    }
    Ok(())
}
