//! Sealed admission shared by the ordinary Teams writer and native originals.
//! A serialized actor or saved result never constructs this value.
use super::{super::person_gesture as card, required};
use crate::teams::{ops, store::phase_controls::task_custody::TaskCustody};
#[cfg(test)]
use crate::teams::store;
use rusqlite::Connection;
use std::path::Path;

type Result<T> = std::result::Result<T, zerus_team_plan::Problem>;

pub(crate) struct Permit<'a>(Original<'a>);
enum Original<'a> {
    Card(Box<card::Admission>),
    Action(Box<super::original::Admission>),
    Intake(Box<super::intake::Admission<'a>>),
    #[cfg(test)]
    Fixture {
        actor: ops::Actor,
        request: Box<ops::Request>,
    },
}
pub(crate) enum Evidence<'a> {
    Card(zerus_client_core::desktop::team_person::Intent),
    Action(Box<super::original::Evidence>),
    Intake(Box<super::intake::Evidence<'a>>),
    #[cfg(test)]
    Fixture,
}
impl From<card::Admission> for Permit<'_> {
    fn from(original: card::Admission) -> Self {
        Self(Original::Card(Box::new(original)))
    }
}
impl From<super::original::Admission> for Permit<'_> {
    fn from(original: super::original::Admission) -> Self {
        Self(Original::Action(Box::new(original)))
    }
}
impl<'a> From<super::intake::Admission<'a>> for Permit<'a> {
    fn from(original: super::intake::Admission<'a>) -> Self {
        Self(Original::Intake(Box::new(original)))
    }
}
impl<'a> Permit<'a> {
    pub(crate) fn prepare_store(&mut self, state: &Path) -> Result<()> {
        match &mut self.0 {
            Original::Action(original) => original.prepare_store(state),
            _ => Ok(()),
        }
    }
    /// Canonical input binding is checked before first-store setup as well as
    /// under the writer lock. Only the latter consumes the native original.
    pub(crate) fn preflight(&mut self, actor: &ops::Actor, request: &ops::Request) -> Result<()> {
        match &self.0 {
            Original::Card(original) => original.preflight(actor, request),
            Original::Action(original) => original.preflight(actor, request),
            Original::Intake(original) => original.preflight(actor, request),
            #[cfg(test)]
            Original::Fixture {
                actor: expected_actor,
                request: expected,
            } => {
                if actor != expected_actor || store::encode(request)? != store::encode(expected)? {
                    return Err(required());
                }
                Ok(())
            }
        }
    }
    pub(crate) fn consume(
        self,
        db: &Connection,
        state: &Path,
        actor: &ops::Actor,
        request: &ops::Request,
    ) -> Result<Evidence<'a>> {
        if db.is_autocommit() {
            return Err(required());
        }
        match self.0 {
            Original::Card(original) => original
                .consume(db, state, actor, request)
                .map(Evidence::Card),
            Original::Action(original) => original
                .consume(db, state, actor, request)
                .map(|evidence| Evidence::Action(Box::new(evidence))),
            Original::Intake(original) => original
                .consume(db, state, actor, request)
                .map(|evidence| Evidence::Intake(Box::new(evidence))),
            #[cfg(test)]
            Original::Fixture {
                actor: expected_actor,
                request: expected,
            } => {
                if actor != &expected_actor || store::encode(request)? != store::encode(&expected)?
                {
                    return Err(required());
                }
                Ok(Evidence::Fixture)
            }
        }
    }
    /// Explicit model for existing operation fixtures. These still use the
    /// product writer; this constructor is not evidence of a native gesture.
    #[cfg(test)]
    pub(crate) fn fixture(actor: &ops::Actor, request: &ops::Request) -> Self {
        Self(Original::Fixture {
            actor: actor.clone(),
            request: Box::new(request.clone()),
        })
    }
}
impl Evidence<'_> {
    pub(crate) fn intake(&self) -> bool {
        matches!(self, Self::Intake(_))
    }
    pub(crate) fn before_run(&mut self, db: &Connection, state: &Path) -> Result<u64> {
        match self {
            Self::Intake(original) => original.before_run(db, state),
            _ => Ok(0),
        }
    }
    pub(crate) fn original(&self, db: &Connection) -> Result<()> {
        match self {
            Self::Card(intent) => card::original(db, intent),
            Self::Action(original) => original.original(db),
            Self::Intake(_) => Err(required()),
            #[cfg(test)]
            Self::Fixture => Ok(()),
        }
    }
    pub(crate) fn fresh(
        &self,
        db: &Connection,
        custody: Option<&Result<TaskCustody>>,
    ) -> Result<()> {
        match self {
            Self::Card(intent) => card::fresh(db, intent),
            Self::Action(original) => original.fresh_with_phase_resources(db, custody),
            Self::Intake(original) => original.fresh(db),
            #[cfg(test)]
            Self::Fixture => Ok(()),
        }
    }
    pub(crate) fn record(self, db: &Connection, result: &Result<ops::Reply>) -> Result<()> {
        match self {
            Self::Card(intent) => card::record(db, &intent),
            Self::Action(original) => original.record(db),
            Self::Intake(original) => original.record(db, result),
            #[cfg(test)]
            Self::Fixture => Ok(()),
        }
    }
}
