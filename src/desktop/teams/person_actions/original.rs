//! A live native original, never a serialized actor or saved receipt.
use super::{
    prepared::{self, Prepared},
    required,
};
use crate::{
    config::Config,
    desktop::ui_authority::Validated,
    teams::{ops, store},
};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use zerus_client_core::desktop::{creation_clone::Scope, team_person_actions as wire};
use zerus_team_plan::Problem;

type Result<T> = std::result::Result<T, Problem>;

pub(super) struct Admission {
    authority: Validated,
    prepared: Arc<Prepared>,
    context: Context,
}
enum Context {
    Service {
        config: Box<Config>,
        scope: Scope,
        stopped: Arc<AtomicBool>,
    },
    #[cfg(test)]
    Fixture,
}
pub(crate) struct Evidence {
    intent: wire::Intent,
    authority: Validated,
    prepared: Arc<Prepared>,
    config: Option<Box<Config>>,
    stopped: Option<Arc<AtomicBool>>,
}

impl Admission {
    pub(super) fn new(
        authority: Validated,
        prepared: Arc<Prepared>,
        config: Config,
        scope: Scope,
        stopped: Arc<AtomicBool>,
    ) -> Self {
        Self {
            authority,
            prepared,
            context: Context::Service {
                config: Box::new(config),
                scope,
                stopped,
            },
        }
    }

    pub fn preflight(&self, who: &ops::Actor, request: &ops::Request) -> Result<()> {
        let intent = self
            .authority
            .team_person_action_intent()
            .ok_or_else(required)?;
        if !intent.valid() || !self.prepared.matches(who, request, intent) {
            return Err(required());
        }
        match &self.context {
            Context::Service {
                config,
                scope,
                stopped,
            } => {
                if stopped.load(Ordering::Acquire)
                    || &intent.scope != scope
                    || machine(config)? != intent.machine
                    || Prepared::resolve(config, self.prepared.plan.clone())?.request != *request
                {
                    return Err(required());
                }
            }
            #[cfg(test)]
            Context::Fixture => (),
        }
        self.authority
            .check_team_person_action()
            .map_err(|_| required())
    }

    pub fn consume(
        self,
        db: &Connection,
        state: &Path,
        who: &ops::Actor,
        request: &ops::Request,
    ) -> Result<Evidence> {
        if db.is_autocommit() {
            return Err(required());
        }
        self.preflight(who, request)?;
        let intent = self
            .authority
            .team_person_action_intent()
            .ok_or_else(required)?;
        let wire::StoreBinding::Existing { fingerprint } = &intent.store else {
            return Err(required());
        };
        if super::super::person_gesture::identity(db, state)? != *fingerprint {
            return Err(required());
        }
        let (config, stopped) = match self.context {
            Context::Service {
                config, stopped, ..
            } => {
                if crate::platform::canonical(&config.state).map_err(store::storage)? != state {
                    return Err(required());
                }
                (Some(config), Some(stopped))
            }
            #[cfg(test)]
            Context::Fixture => (None, None),
        };
        receipt_bound(db, &intent.plan.request_id)?;
        // Retain the one opaque original through facts, policy and writes.
        // Final consumption happens before commit and rolls back every row
        // if any intervening work outlives the original's deadline.
        self.authority
            .check_team_person_action()
            .map_err(|_| required())?;
        let intent = self
            .authority
            .team_person_action_intent()
            .ok_or_else(required)?
            .clone();
        Ok(Evidence {
            intent,
            authority: self.authority,
            prepared: self.prepared,
            config,
            stopped,
        })
    }

    #[cfg(test)]
    fn fixture(store: &store::Store, prepared: Arc<Prepared>) -> Self {
        use serde_json::json;
        let intent = wire::Intent {
            scope: Scope {
                configuration: "a".repeat(64),
                service_epoch: uuid::Uuid::new_v4().to_string(),
            },
            machine: uuid::Uuid::new_v4().to_string(),
            store: wire::StoreBinding::Existing {
                fingerprint: super::super::person_gesture::identity(
                    store.db.as_ref().unwrap(),
                    store.state(),
                )
                .unwrap(),
            },
            facts: prepared.facts(store.db.as_ref().unwrap()).unwrap().1,
            request: wire::digest(&prepared.request).unwrap(),
            plan: prepared.plan.reference().unwrap(),
        };
        Self {
            authority: Validated::fixture(&json!({"op":"team_person_action","intent":intent})),
            prepared,
            context: Context::Fixture,
        }
    }
}

impl Evidence {
    pub fn original(&self, db: &Connection) -> Result<()> {
        original(db, &self.intent)?;
        self.current()
    }
    pub fn fresh(&self, db: &Connection) -> Result<()> {
        let (_, current) = self.prepared.bound_facts(db, self.config.as_deref())?;
        if current != self.intent.facts {
            return Err(Problem::new(
                zerus_team_plan::problem::code::STALE_REVISION,
                "The reviewed team action changed. Read it again before confirming.",
            ));
        }
        self.current()
    }
    fn current(&self) -> Result<()> {
        if self
            .stopped
            .as_ref()
            .is_some_and(|s| s.load(Ordering::Acquire))
        {
            return Err(required());
        }
        self.authority
            .check_team_person_action()
            .map_err(|_| required())
    }
    pub fn record(self, db: &Connection) -> Result<()> {
        self.current()?;
        let intent = self
            .authority
            .team_person_action()
            .map_err(|_| required())?;
        db.execute(
            "INSERT INTO person_gestures(request_id,intent) VALUES(?1,?2)",
            params![intent.plan.request_id, store::encode(&intent)?],
        )
        .map_err(store::storage)?;
        Ok(())
    }
}

pub(super) fn machine(config: &Config) -> Result<String> {
    crate::state::lifecycle_receipt_machine(config).map_err(store::storage)
}

pub(super) fn receipt_bound(db: &Connection, id: &str) -> Result<()> {
    let bytes: Option<u64> = db.query_row(
        "SELECT length(CAST(actor AS BLOB))+length(CAST(content AS BLOB))+length(CAST(result AS BLOB)) FROM requests WHERE id=?1",
        [id], |r| r.get(0)).optional().map_err(store::storage)?;
    if bytes.is_some_and(|n| n > 524288) {
        return Err(required());
    }
    Ok(())
}

pub(super) fn original(db: &Connection, intent: &wire::Intent) -> Result<()> {
    let saved: Option<String> = db.query_row(
        "SELECT intent FROM person_gestures WHERE request_id=?1 AND length(CAST(intent AS BLOB))<=65536",
        [&intent.plan.request_id], |r| r.get(0)).optional().map_err(store::storage)?;
    if saved.as_deref() != Some(store::encode(intent)?.as_str()) {
        return Err(Problem::new(
            zerus_team_plan::problem::code::DUPLICATE,
            "The original native team action evidence is unavailable or different.",
        ));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::teams::ops::tests::{self as producers, Harness, TEAM};
    use zerus_client_core::teams::action::TeamAction;

    fn sample(h: &Harness) -> Arc<Prepared> {
        let revision = h.store.team(TEAM).unwrap().unwrap().revision;
        let request = producers::request(
            revision,
            ops::Operation::RenameTeam {
                name: "Native action rename".into(),
            },
        );
        let plan = wire::Plan {
            request_id: request.request_id.clone(),
            team: TEAM.into(),
            expected_revision: revision,
            action: wire::Action::Team {
                change: Box::new(TeamAction::RenameTeam {
                    name: "Native action rename".into(),
                }),
            },
        };
        Arc::new(Prepared {
            plan: Arc::new(plan),
            request,
        })
    }

    #[test]
    fn native_v2_model_renames_with_original_evidence_in_the_writer() {
        let mut h = Harness::new();
        let prepared = sample(&h);
        let admission = Admission::fixture(&h.store, prepared.clone());
        let intent = admission
            .authority
            .team_person_action_intent()
            .unwrap()
            .clone();
        let result = ops::apply_with_native(
            &mut h.store,
            &prepared::actor(),
            &prepared.request,
            1234,
            Some(admission.into()),
        )
        .unwrap();
        assert_eq!(result.revision, prepared.request.expected_revision + 1);
        assert_eq!(
            h.store.team(TEAM).unwrap().unwrap().name,
            "Native action rename"
        );
        let db = h.store.db.as_ref().unwrap();
        original(db, &intent).unwrap();
        assert_eq!(
            ops::replay(db, &prepared::actor(), &prepared.request)
                .unwrap()
                .unwrap()
                .unwrap(),
            result
        );
        let mut exact = Admission::fixture(&h.store, prepared.clone());
        exact.authority =
            Validated::fixture(&serde_json::json!({"op":"team_person_action","intent":intent}));
        assert_eq!(
            ops::apply_with_native(
                &mut h.store,
                &prepared::actor(),
                &prepared.request,
                1235,
                Some(exact.into())
            )
            .unwrap(),
            result
        );
        let changed = Admission::fixture(&h.store, prepared.clone());
        assert_eq!(
            ops::apply_with_native(
                &mut h.store,
                &prepared::actor(),
                &prepared.request,
                1235,
                Some(changed.into())
            )
            .unwrap_err()
            .code,
            zerus_team_plan::problem::code::DUPLICATE
        );
    }

    #[test]
    fn native_v2_model_binds_actor_payload_reference_and_request_digest() {
        for field in ["actor", "payload", "reference", "request"] {
            let mut h = Harness::new();
            let prepared = sample(&h);
            let mut admission = Admission::fixture(&h.store, prepared.clone());
            let mut actor = prepared::actor();
            let mut request = prepared.request.clone();
            match field {
                "actor" => actor = producers::person(),
                "payload" => {
                    request.operation = ops::Operation::RenameTeam {
                        name: "Unreviewed".into(),
                    }
                }
                _ => {
                    let mut intent = admission
                        .authority
                        .team_person_action_intent()
                        .unwrap()
                        .clone();
                    if field == "reference" {
                        intent.plan.sha256 = "f".repeat(64);
                    } else {
                        intent.request = "e".repeat(64);
                    }
                    admission.authority = Validated::fixture(
                        &serde_json::json!({"op":"team_person_action","intent":intent}),
                    );
                }
            }
            if field == "actor" {
                actor = ops::Actor::Person {
                    person: "local".into(),
                    origin: ops::PersonOrigin::CliInterim,
                };
            }
            assert_eq!(
                ops::apply_with_native(
                    &mut h.store,
                    &actor,
                    &request,
                    1234,
                    Some(admission.into())
                )
                .unwrap_err()
                .code,
                "person_gesture_required",
                "{field}"
            );
            assert_eq!(
                h.store.team(TEAM).unwrap().unwrap().revision,
                prepared.request.expected_revision
            );
        }
    }

    #[test]
    fn native_v2_model_expiry_after_writer_wait_changes_no_row() {
        let mut h = Harness::new();
        let prepared = sample(&h);
        let mut admission = Admission::fixture(&h.store, prepared.clone());
        let intent = admission.authority.team_person_action_intent().unwrap();
        admission.authority = Validated::fixture_expiring(
            &serde_json::json!({"op":"team_person_action","intent":intent}),
            std::time::Instant::now() - std::time::Duration::from_secs(1),
        );
        assert_eq!(
            ops::apply_with_native(
                &mut h.store,
                &prepared::actor(),
                &prepared.request,
                1234,
                Some(admission.into())
            )
            .unwrap_err()
            .code,
            "person_gesture_required"
        );
        assert_eq!(
            h.store.team(TEAM).unwrap().unwrap().revision,
            prepared.request.expected_revision
        );
    }

    #[test]
    fn native_v2_model_changed_facts_and_oversize_bodies_are_refused() {
        for oversized in [false, true] {
            let mut h = Harness::new();
            let prepared = sample(&h);
            let admission = Admission::fixture(&h.store, prepared.clone());
            // Named corruption only, after a real producer created the team.
            h.store
                .db
                .as_ref()
                .unwrap()
                .execute(
                    "UPDATE teams SET goal=?1 WHERE id=?2",
                    params![
                        if oversized {
                            "x".repeat(300_000)
                        } else {
                            "Changed review facts".into()
                        },
                        TEAM
                    ],
                )
                .unwrap();
            let problem = ops::apply_with_native(
                &mut h.store,
                &prepared::actor(),
                &prepared.request,
                1234,
                Some(admission.into()),
            )
            .unwrap_err();
            assert_eq!(
                problem.code,
                if oversized {
                    zerus_team_plan::problem::code::OVER_CAP
                } else {
                    zerus_team_plan::problem::code::STALE_REVISION
                }
            );
            assert_eq!(
                h.store.team(TEAM).unwrap().unwrap().revision,
                prepared.request.expected_revision
            );
            let count: u32 = h
                .store
                .db
                .as_ref()
                .unwrap()
                .query_row(
                    "SELECT count(*) FROM requests WHERE id=?1",
                    [&prepared.request.request_id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        }
    }
}
