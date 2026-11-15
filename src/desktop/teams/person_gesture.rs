//! One native owner-card decision. All serialized values are evidence only;
//! the opaque original-child permit is consumed inside the Teams transaction.
use crate::{
    config::Config,
    teams::{
        ops,
        store::{self, rows, Store},
    },
};
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use zerus_client_core::desktop::{creation_clone::Scope, team_person as wire};
use zerus_team_plan::{model::Team, problem::code, Problem};

type Result<T> = std::result::Result<T, Problem>;

pub(crate) fn required() -> Problem {
    Problem::new("person_gesture_required", "Decide this owner card in the Zerus window. A command-line Person claim is not a person's gesture.")
}

fn actor() -> ops::Actor {
    ops::Actor::Person {
        person: "local".into(),
        origin: ops::PersonOrigin::DesktopUi,
    }
}

fn request(plan: &wire::Plan) -> ops::Request {
    ops::Request {
        request_id: plan.request_id.clone(),
        team: plan.team.clone(),
        expected_revision: plan.expected_revision,
        operation: ops::Operation::DecideCard {
            card: plan.card,
            decision: plan.decision.clone(),
        },
    }
}

/// No Clone/Deserialize or production constructor outside the native handler.
pub(crate) struct Admission {
    authority: crate::desktop::ui_authority::Validated,
    context: NativeContext,
}
enum NativeContext {
    Service {
        config: Box<Config>,
        scope: Scope,
    },
    #[cfg(test)]
    Fixture,
}
impl Admission {
    pub(crate) fn preflight(&self, who: &ops::Actor, expected: &ops::Request) -> Result<()> {
        let intent = self.authority.team_person_intent().ok_or_else(required)?;
        if !intent.valid()
            || who != &actor()
            || store::encode(expected)? != store::encode(&request(&intent.plan))?
        {
            return Err(required());
        }
        Ok(())
    }

    pub(crate) fn consume(
        self,
        db: &Connection,
        state: &Path,
        who: &ops::Actor,
        expected: &ops::Request,
    ) -> Result<wire::Intent> {
        let intent = self
            .authority
            .team_person_intent()
            .ok_or_else(required)?
            .clone();
        if !intent.valid()
            || who != &actor()
            || store::encode(expected)? != store::encode(&request(&intent.plan))?
        {
            return Err(required());
        }
        match &self.context {
            NativeContext::Service { config, scope } => {
                if &intent.scope != scope
                    || machine(config).map_err(store::storage)? != intent.machine
                    || crate::platform::canonical(&config.state).map_err(store::storage)? != state
                {
                    return Err(required());
                }
            }
            #[cfg(test)]
            NativeContext::Fixture => (),
        }
        if identity(db, state)? != intent.store {
            return Err(required());
        }
        let bytes: Option<u64> = db
            .query_row(
                "SELECT length(CAST(actor AS BLOB))+length(CAST(content AS BLOB))+length(CAST(result AS BLOB)) FROM requests WHERE id=?1",
                [&intent.plan.request_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(store::storage)?;
        if bytes.is_some_and(|n| n > 524288) {
            return Err(store::storage(
                "The original decision receipt exceeds its bound.",
            ));
        }
        // Revalidate the original after all potentially blocking identity
        // reads, while still holding the same Teams writer transaction.
        self.authority.team_person_decide().map_err(|_| required())
    }

    /// Unit fixtures still pass the same typed boundary, bindings and facts.
    /// This models an admitted native original; it is not native UI evidence.
    #[cfg(test)]
    pub(crate) fn fixture(store: &Store, expected: &ops::Request) -> Self {
        let ops::Operation::DecideCard { card, decision } = &expected.operation else {
            panic!("fixture only admits decide")
        };
        let plan = wire::Plan {
            request_id: expected.request_id.clone(),
            team: expected.team.clone(),
            expected_revision: expected.expected_revision,
            card: *card,
            decision: decision.clone(),
        };
        let db = store.db.as_ref().unwrap();
        let facts = facts(db, &plan).map_or_else(|_| "0".repeat(64), |(_, _, digest)| digest);
        let intent = wire::Intent {
            scope: Scope {
                configuration: "a".repeat(64),
                service_epoch: "11111111-1111-4111-8111-111111111111".into(),
            },
            machine: "22222222-2222-4222-8222-222222222222".into(),
            store: identity(db, store.state()).unwrap(),
            facts,
            plan,
        };
        Self {
            authority: crate::desktop::ui_authority::Validated::fixture(
                &json!({"op":"team_person_decide","intent":intent}),
            ),
            context: NativeContext::Fixture,
        }
    }
}

fn machine(config: &Config) -> anyhow::Result<String> {
    crate::state::lifecycle_receipt_machine(config).map_err(anyhow::Error::msg)
}

/// Check the opened SQLite object too: a pathname's device/inode alone cannot
/// authorize a connection which still points at a replaced database.
fn identity(db: &Connection, state: &Path) -> Result<String> {
    let path = state.join("teams/teams-v1.sqlite3");
    #[cfg(unix)]
    let id = {
        let id = crate::sqlite_file::private(&path).map_err(store::storage)?;
        crate::sqlite_file::bind_private(db, &path, id).map_err(store::storage)?;
        id
    };
    #[cfg(not(unix))]
    let id = crate::platform::file_identity(&path).ok_or_else(required)?;
    let nonce: String = db
        .query_row(
            "SELECT nonce FROM person_gesture_store WHERE id=1 AND length(CAST(nonce AS BLOB))=36",
            [],
            |r| r.get(0),
        )
        .map_err(store::storage)?;
    if !zerus_desktop_migration::canonical_uuid(&nonce) {
        return Err(required());
    }
    wire::digest(&(state, id.device, id.inode, nonce)).ok_or_else(required)
}

/// Size probes precede materialization and share the caller's transaction.
/// One team and one indexed card are read; no journal or history is scanned.
fn facts(db: &Connection, plan: &wire::Plan) -> Result<(Team, store::Card, String)> {
    let team_bytes: Option<u64> = db.query_row(
        "SELECT length(CAST(id AS BLOB))+length(CAST(project AS BLOB))+length(CAST(name AS BLOB))+length(CAST(goal AS BLOB))+length(CAST(home AS BLOB))+length(CAST(owner AS BLOB))+length(CAST(status AS BLOB))+length(CAST(lead AS BLOB))+length(CAST(settings AS BLOB))+length(CAST(bundle AS BLOB))+length(CAST(integration_branch AS BLOB))+length(CAST(base_commit AS BLOB)) FROM teams WHERE id=?1",
        [&plan.team], |r| r.get(0),
    ).optional().map_err(store::storage)?;
    let card_bytes: Option<u64> = db.query_row(
        "SELECT coalesce(length(CAST(kind AS BLOB)),0)+coalesce(length(CAST(actor AS BLOB)),0)+coalesce(length(CAST(text AS BLOB)),0)+coalesce(length(CAST(state AS BLOB)),0)+coalesce(length(CAST(decision AS BLOB)),0)+coalesce(length(CAST(decided_actor AS BLOB)),0)+coalesce(length(CAST(decisions AS BLOB)),0)+coalesce(length(CAST(notes AS BLOB)),0)+coalesce(length(CAST(parameters AS BLOB)),0) FROM cards WHERE team=?1 AND seq=?2",
        params![plan.team, plan.card], |r| r.get(0),
    ).optional().map_err(store::storage)?;
    if !team_bytes.is_some_and(|n| n <= 262144) || !card_bytes.is_some_and(|n| n <= 49152) {
        return Err(Problem::new(
            code::INVALID_STATE,
            "The exact owner card is missing or exceeds its review bound.",
        ));
    }
    let team = rows::team(db, &plan.team)?.ok_or_else(required)?;
    let card = rows::card(db, &plan.team, plan.card)?.ok_or_else(required)?;
    let digest = wire::digest(&(&team, &card)).ok_or_else(required)?;
    Ok((team, card, digest))
}

pub(crate) fn fresh(db: &Connection, intent: &wire::Intent) -> Result<()> {
    let (_, _, current) = facts(db, &intent.plan)?;
    if current != intent.facts {
        return Err(Problem::new(
            code::STALE_REVISION,
            "The owner card changed after native confirmation. Read it again.",
        ));
    }
    Ok(())
}

pub(crate) fn record(db: &Connection, intent: &wire::Intent) -> Result<()> {
    db.execute(
        "INSERT INTO person_gestures(request_id,intent) VALUES(?1,?2)",
        params![intent.plan.request_id, store::encode(intent)?],
    )
    .map_err(store::storage)?;
    Ok(())
}

pub(crate) fn original(db: &Connection, intent: &wire::Intent) -> Result<()> {
    let saved: Option<String> = db.query_row(
        "SELECT intent FROM person_gestures WHERE request_id=?1 AND length(CAST(intent AS BLOB))<=65536",
        [&intent.plan.request_id], |r| r.get(0),
    ).optional().map_err(store::storage)?;
    if saved.as_deref() != Some(store::encode(intent)?.as_str()) {
        return Err(Problem::new(
            code::DUPLICATE,
            "The original native decision evidence is unavailable or different.",
        ));
    }
    Ok(())
}

fn scope(desktop: &crate::desktop::store::Store) -> anyhow::Result<Scope> {
    Ok(desktop.db.query_row(
        "SELECT configuration,epoch FROM service WHERE id=1 AND length(CAST(configuration AS BLOB))=64 AND length(CAST(epoch AS BLOB))=36",
        [],
        |r| {
            Ok(Scope {
                configuration: r.get(0)?,
                service_epoch: r.get(1)?,
            })
        },
    )?)
}

/// Read-only review and original receipt lookup. Neither endpoint constructs
/// Admission or calls an operation writer, including on absent/corrupt rows.
pub(in crate::desktop) fn query(
    config: &Config,
    desktop: &crate::desktop::store::Store,
    value: &Value,
) -> Value {
    read(config, desktop, value).unwrap_or_else(|_| json!({"ok":false,"code":"team_person_evidence_unavailable","message":"The exact native owner-card evidence is unavailable. Nothing was replayed."}))
}

fn read(
    config: &Config,
    desktop: &crate::desktop::store::Store,
    value: &Value,
) -> anyhow::Result<Value> {
    #[derive(Deserialize)]
    #[serde(tag = "op", deny_unknown_fields)]
    enum Query {
        #[serde(rename = "team_person_prepare")]
        Prepare { scope: Scope, plan: wire::Plan },
        #[serde(rename = "team_person_result")]
        Result { intent: wire::Intent },
    }
    (|| -> anyhow::Result<Value> {
        let query: Query = serde_json::from_value(value.clone())?;
        let current = scope(desktop)?;
        let store = Store::person_evidence(&config.state).map_err(anyhow::Error::msg)?;
        let db = store.db.as_ref().context("Teams store missing")?;
        let tx = db.unchecked_transaction()?;
        match query {
            Query::Prepare { scope, plan } => {
                ensure!(
                    scope == current && plan.valid(),
                    "native review scope changed"
                );
                let (team, card, facts) = facts(&tx, &plan).map_err(anyhow::Error::msg)?;
                ops::authority(&actor(), &request(&plan).operation, &team)
                    .map_err(anyhow::Error::msg)?;
                ensure!(
                    team.revision == plan.expected_revision
                        && card.state == "open"
                        && card.decisions.contains(&plan.decision),
                    "owner card changed"
                );
                let review = wire::Review {
                    intent: wire::Intent {
                        scope,
                        machine: machine(config)?,
                        store: identity(&tx, store.state()).map_err(anyhow::Error::msg)?,
                        facts,
                        plan,
                    },
                    team_name: team.name,
                    card_kind: card.kind,
                    card_text: card.text,
                    card_parameters: card.parameters,
                    card_notes: card.notes.iter().map(ops::ReportNote::sentence).collect(),
                };
                ensure!(review.valid(), "owner review exceeds bounds");
                Ok(json!({"ok":true,"review":review}))
            }
            Query::Result { intent } => {
                ensure!(
                    intent.valid()
                        && intent.scope.configuration == current.configuration
                        && intent.machine == machine(config)?
                        && intent.store
                            == identity(&tx, store.state()).map_err(anyhow::Error::msg)?,
                    "original decision scope changed"
                );
                original(&tx, &intent).map_err(anyhow::Error::msg)?;
                let size: u64 = tx.query_row(
                    "SELECT length(CAST(actor AS BLOB))+length(CAST(content AS BLOB))+length(CAST(result AS BLOB)) FROM requests WHERE id=?1",
                    [&intent.plan.request_id],
                    |r| r.get(0),
                )?;
                ensure!(size <= 524288, "original decision receipt exceeds bounds");
                let saved = ops::replay(&tx, &actor(), &request(&intent.plan))
                    .map_err(anyhow::Error::msg)?
                    .context("original decision receipt missing")?;
                Ok(json!({"intent":intent,"reply":reply(&intent.plan, saved)}))
            }
        }
    })()
}

fn reply(plan: &wire::Plan, result: Result<ops::Reply>) -> Value {
    match result {
        Ok(reply) => {
            let mut value = serde_json::to_value(reply).expect("fixed Teams reply");
            value["actor"] = json!(actor());
            value["team"] = json!(plan.team);
            value["request_id"] = json!(plan.request_id);
            value
        }
        Err(problem) => {
            json!({"ok":false,"code":problem.code,"message":problem.message,"request_id":plan.request_id})
        }
    }
}

/// Only the authenticated native frontend lane calls this. The permit stays
/// opaque until the common kernel owns the Teams SQLite writer transaction.
pub(in crate::desktop) fn frontend(
    config: &Config,
    desktop: &crate::desktop::store::Store,
    authority: crate::desktop::ui_authority::Validated,
) -> Value {
    let id = authority.id().to_owned();
    if !cfg!(unix) {
        return json!({"ok":false,"code":"person_gesture_required","message":"Native Teams Person decisions are unavailable on this platform.","request_id":id});
    }
    let result = (|| -> Result<Value> {
        let scope = scope(desktop).map_err(store::storage)?;
        let intent = authority.team_person_intent().ok_or_else(required)?.clone();
        let plan = intent.plan.clone();
        let mut store = Store::open(&config.state)?;
        let admission = Admission {
            authority,
            context: NativeContext::Service {
                config: Box::new(config.clone()),
                scope,
            },
        };
        let result = ops::apply_with_gesture(
            &mut store,
            &actor(),
            &request(&plan),
            chrono::Utc::now().timestamp_millis(),
            Some(admission),
        );
        Ok(json!({"intent":intent,"reply":reply(&plan, result)}))
    })();
    result.unwrap_or_else(|p| json!({"ok":false,"code":p.code,"message":p.message,"request_id":id}))
}

#[cfg(all(test, unix))]
mod tests;
