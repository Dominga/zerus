//! Sealed admission shared by the ordinary Teams writer and native originals.
//! A serialized actor or saved result never constructs this value.
use super::{super::person_gesture as card, required};
use crate::teams::{ops, store};
use rusqlite::Connection;
use std::path::Path;

type Result<T> = std::result::Result<T, zerus_team_plan::Problem>;

pub(crate) struct Permit(Original);
enum Original {
    Card(card::Admission),
    #[cfg(test)]
    Fixture {
        actor: ops::Actor,
        request: Box<ops::Request>,
    },
}
pub(crate) enum Evidence {
    Card(zerus_client_core::desktop::team_person::Intent),
    #[cfg(test)]
    Fixture,
}
impl From<card::Admission> for Permit {
    fn from(original: card::Admission) -> Self {
        Self(Original::Card(original))
    }
}
impl Permit {
    /// Canonical input binding is checked before first-store setup as well as
    /// under the writer lock. Only the latter consumes the native original.
    pub(crate) fn preflight(&mut self, actor: &ops::Actor, request: &ops::Request) -> Result<()> {
        match &self.0 {
            Original::Card(original) => original.preflight(actor, request),
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
    ) -> Result<Evidence> {
        if db.is_autocommit() {
            return Err(required());
        }
        match self.0 {
            Original::Card(original) => original
                .consume(db, state, actor, request)
                .map(Evidence::Card),
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
impl Evidence {
    pub(crate) fn original(&self, db: &Connection) -> Result<()> {
        match self {
            Self::Card(intent) => card::original(db, intent),
            #[cfg(test)]
            Self::Fixture => Ok(()),
        }
    }
    pub(crate) fn fresh(&self, db: &Connection) -> Result<()> {
        match self {
            Self::Card(intent) => card::fresh(db, intent),
            #[cfg(test)]
            Self::Fixture => Ok(()),
        }
    }
    pub(crate) fn record(&self, db: &Connection) -> Result<()> {
        match self {
            Self::Card(intent) => card::record(db, intent),
            #[cfg(test)]
            Self::Fixture => Ok(()),
        }
    }
}
