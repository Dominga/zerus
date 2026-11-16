//! One live compound intake. A saved reservation is only an Unknown result;
//! it cannot construct the borrowed custodian below or resume a task write.
use super::{
    original,
    prepared::{self, Prepared},
    required,
};
use crate::{
    config::Config,
    desktop::ui_authority::Validated,
    teams::{
        ops,
        store::{self, person_actions as ledger, Store},
        tasks::native,
    },
};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};
use zerus_client_core::desktop::{creation_clone::Scope, team_person_actions as wire};
use zerus_replica::{ProjectStore, TaskDoc};
use zerus_team_plan::{model::TaskRef, problem::code, Problem};
type Result<T> = std::result::Result<T, Problem>;

fn task(request: &ops::Request) -> Result<&TaskRef> {
    match &request.operation {
        ops::Operation::AddTask { task, .. } => Ok(task),
        _ => Err(required()),
    }
}
fn projects(config: &Config, project: &str) -> Result<ProjectStore> {
    ProjectStore::open_within(
        &config.state.join("replica"),
        project,
        Duration::from_millis(300),
    )
    .map_err(store::storage)?
    .ok_or_else(|| {
        Problem::new(
            code::STORE_BUSY,
            "The task project is busy. Nothing was sent.",
        )
    })
}
fn load(project: &ProjectStore, id: &str) -> Result<TaskDoc> {
    let document = project
        .admit_until(Instant::now() + Duration::from_secs(5))
        .ok_or_else(|| Problem::new(code::STORE_BUSY, "The exact task could not be read."))?
        .load_complete(id)
        .map_err(store::storage)?;
    if document.view().map_err(store::storage)?.project != project.project_id() {
        return Err(Problem::new(
            code::INVALID_STATE,
            "The task document belongs to another project. Nothing was adopted.",
        ));
    }
    Ok(document)
}

fn check_document(plan: &wire::Plan, document: &TaskDoc) -> Result<()> {
    let wire::Action::Intake { revision, .. } = &plan.action else {
        return Err(required());
    };
    if &document.revision() != revision {
        return Err(Problem::new(
            code::STALE_REVISION,
            "The task changed. Read it again before confirming.",
        ));
    }
    Ok(())
}

/// Read preparation never reserves a UUID, stages a plan or invokes a writer.
/// New-card preparation does not create a project/task store.
pub(super) fn review(config: &Config, scope: &Scope, draft: wire::TaskDraft) -> Result<Value> {
    if !draft.valid() {
        return Err(required());
    }
    let mut draft = draft;
    let project = match &draft.action {
        wire::Action::NewTask { project, .. } | wire::Action::Intake { project, .. } => project,
        _ => return Err(required()),
    };
    crate::tasks::native_team::project_name(config, project)?;
    // Canonical task read and project custody precede the Teams snapshot.
    // An unset draft clock requests preparation only, never execution.
    let mut held = if let wire::Action::Intake {
        project,
        task,
        revision,
        ..
    } = &mut draft.action
    {
        let project = projects(config, project)?;
        let document = load(&project, task)?;
        if revision.is_empty() {
            *revision = document.revision();
        }
        if *revision != document.revision() {
            return Err(Problem::new(
                code::STALE_REVISION,
                "The task changed. Read it again before confirming.",
            ));
        }
        Some((project, document))
    } else {
        None
    };
    let store = Store::person_evidence(&config.state)?;
    let db = store.db.as_ref().ok_or_else(required)?;
    let tx = db.unchecked_transaction().map_err(store::storage)?;
    let initial = Prepared::resolve(config, Arc::new(draft.plan(0).ok_or_else(required)?))?;
    let (team, _) = initial.facts(&tx)?;
    let team = team.ok_or_else(required)?;
    let plan = Arc::new(draft.plan(team.revision).ok_or_else(required)?);
    let prepared = Prepared::resolve(config, plan.clone())?;
    let machine = original::machine(config)?;
    native::check(&store, &config.state, &machine, &prepared.request)?;
    let (_, facts) = prepared.bound_facts(&tx, Some(config))?;
    let reference = task(&prepared.request)?;
    let mut details = vec![
        ("Project".into(), json!(reference.project).to_string()),
        ("Task identity".into(), json!(reference.uuid).to_string()),
    ];
    match &plan.action {
        wire::Action::NewTask { title, backlog, .. } => {
            details.push(("Title".into(), json!(title).to_string()));
            details.push((
                "Stage".into(),
                json!(if *backlog { "backlog" } else { "todo" }).to_string(),
            ));
        }
        wire::Action::Intake { role, pipeline, .. } => {
            let (_, document) = held.as_mut().ok_or_else(required)?;
            check_document(&plan, document)?;
            native::prepare(&store, &config.state, &machine, &prepared.request, document)?;
            details.push((
                "Title".into(),
                json!(document.view().map_err(store::storage)?.title).to_string(),
            ));
            details.push((
                "Stage".into(),
                json!(document.stage().map_err(store::storage)?.as_str()).to_string(),
            ));
            details.push(("Role".into(), json!(role).to_string()));
            details.push(("Pipeline".into(), json!(pipeline).to_string()));
        }
        _ => return Err(required()),
    }
    let review = wire::Review {
        intent: wire::Intent {
            scope: scope.clone(),
            machine,
            store: wire::StoreBinding::Existing {
                fingerprint: super::super::person_gesture::identity(db, store.state())?,
            },
            facts,
            request: wire::digest(&prepared.request).ok_or_else(required)?,
            plan: plan.reference().ok_or_else(required)?,
        },
        team_name: team.name,
        title: "Confirm team action".into(),
        subject: "Send task to team".into(),
        details,
    };
    if !review.valid() {
        return Err(required());
    }
    Ok(json!({"ok":true,"plan":plan,"review":review}))
}

struct Custody {
    project: ProjectStore,
    state: std::path::PathBuf,
    observation_pending: Option<Problem>,
    document: Option<TaskDoc>,
    existing: bool,
    saved: bool,
    previous: Option<native::Previous>,
    intent: wire::Intent,
    request: ops::Request,
    settlement: Option<(u32, ops::TaskObservation)>,
    now: i64,
}
impl Custody {
    fn restore(&mut self) -> Result<()> {
        if !self.saved {
            return Ok(());
        }
        if let (Some(document), Some(previous)) = (&mut self.document, &self.previous) {
            if native::restore(document, previous, self.now)? {
                self.project.save(document).map_err(store::storage)?
            }
        }
        Ok(())
    }
    fn reply(&self, result: &Result<ops::Reply>) -> Value {
        let reference = task(&self.request).expect("checked intake operation");
        let exists = self.existing || self.saved;
        let mut reply = json!({"ok":exists,"request_id":self.request.request_id,"actor":prepared::actor(),"task_exists":exists,"team":{"id":self.request.team,"queued":result.is_ok(),"node":result.as_ref().ok().and_then(|r|r.node)}});
        if let Some(document) = self.document.as_ref().filter(|_| exists) {
            // Keep the private original result bounded independently of task
            // descriptions, attachments, retained attempts and histories.
            reply["task"] = json!({"uuid":reference.uuid,"project":reference.project,"title":document.view().map(|v|v.title).unwrap_or_default(),"revision":document.revision()});
        }
        if let Some(problem) = &self.observation_pending {
            reply["observation_pending"] = json!(problem);
        }
        if let Err(problem) = result {
            reply["team"]["error"] = json!(problem);
            reply["code"] = json!(problem.code);
            reply["message"] = json!(problem.message)
        }
        reply
    }
    fn finish(&mut self, db: &Connection, result: &Result<ops::Reply>) -> Result<()> {
        if result.is_err() {
            self.restore()?
        }
        let reply = self.reply(result);
        ledger::settle(db, &self.intent, &reply)?;
        Ok(())
    }
}

pub(crate) struct Admission<'a> {
    original: original::Evidence,
    custody: &'a mut Custody,
}
pub(crate) struct Evidence<'a>(Admission<'a>);
impl<'a> Admission<'a> {
    pub(crate) fn preflight(&self, actor: &ops::Actor, request: &ops::Request) -> Result<()> {
        if actor != &prepared::actor() || request != &self.custody.request {
            return Err(required());
        }
        self.original.current()
    }
    pub(crate) fn consume(
        self,
        db: &Connection,
        state: &Path,
        actor: &ops::Actor,
        request: &ops::Request,
    ) -> Result<Evidence<'a>> {
        self.preflight(actor, request)?;
        if db.is_autocommit() {
            return Err(required());
        }
        let wire::StoreBinding::Existing { fingerprint } = &self.custody.intent.store else {
            return Err(required());
        };
        if &super::super::person_gesture::identity(db, state)? != fingerprint {
            return Err(required());
        }
        let saved = ledger::original(db, &self.custody.intent)?;
        if saved.result.is_some() || saved.request != self.custody.request {
            return Err(required());
        }
        Ok(Evidence(self))
    }
}
impl Evidence<'_> {
    pub(crate) fn fresh(&self, db: &Connection) -> Result<()> {
        self.0.original.fresh(db)
    }
    pub(crate) fn before_run(&mut self, db: &Connection, state: &Path) -> Result<u64> {
        let Some((node, observation)) = self.0.custody.settlement.take() else {
            return Ok(0);
        };
        ops::observe_native_intake(
            db,
            state,
            &self.0.custody.request,
            node,
            observation,
            self.0.custody.now,
        )?;
        Ok(1)
    }
    pub(crate) fn record(self, db: &Connection, result: &Result<ops::Reply>) -> Result<()> {
        if let Ok(reply) = result {
            let node = reply.node.ok_or_else(required)?;
            let custody = &mut *self.0.custody;
            let document = custody.document.as_ref().ok_or_else(required)?;
            if let Some(observation) = native::saved_observation(
                db,
                &custody.request.team,
                node,
                document,
                &custody.intent.machine,
            )? {
                let mut observed = custody.request.clone();
                // Exactly the revision this original just wrote, never a
                // current-name/current-revision retry after another writer.
                observed.expected_revision = reply.revision;
                if let Err(problem) = ops::observe_native_intake(
                    db,
                    &custody.state,
                    &observed,
                    node,
                    observation,
                    custody.now,
                ) {
                    if problem.code == code::STORAGE_FAILED {
                        return Err(problem);
                    }
                    custody.observation_pending = Some(problem);
                }
            }
        }
        self.0.custody.finish(db, result)?;
        #[cfg(all(test, unix))]
        test_point(&BEFORE_CONSUME, self.0.custody)?;
        self.0.original.record(db)
    }
}

pub(super) fn execute(
    config: &Config,
    scope: Scope,
    stopped: Arc<AtomicBool>,
    authority: Validated,
    prepared: Arc<Prepared>,
) -> Result<Value> {
    let request = prepared.request.clone();
    let reference = task(&request)?.clone();
    let intent = authority
        .team_person_action_intent()
        .ok_or_else(required)?
        .clone();
    let admission =
        original::Admission::new(authority, prepared.clone(), config.clone(), scope, stopped);
    admission.preflight(&prepared::actor(), &request)?;
    let project_name = crate::tasks::native_team::project_name(config, &reference.project)?;
    let project = projects(config, &reference.project)?;
    let mut document = match &prepared.plan.action {
        wire::Action::NewTask { .. } => {
            crate::tasks::native_team::absent(&project, &reference.uuid)?;
            None
        }
        wire::Action::Intake { .. } => {
            let document = load(&project, &reference.uuid)?;
            Some(document)
        }
        _ => return Err(required()),
    };
    let mut store = Store::open(&config.state)?;
    // Measure canonical bodies before the ordinary intake readers decode them.
    prepared.facts(store.db.as_ref().ok_or_else(required)?)?;
    let mut refusal = document
        .as_ref()
        .and_then(|document| check_document(&prepared.plan, document).err());
    if refusal.is_none() {
        refusal = native::check(&store, &config.state, &intent.machine, &request).err();
    }
    let settlement = if refusal.is_none() {
        document
            .as_ref()
            .map(|document| native::settlement(&store, &request.team, document))
            .transpose()?
            .flatten()
    } else {
        None
    };
    let previous = if refusal.is_none() {
        document
            .as_mut()
            .map(|document| {
                native::prepare(&store, &config.state, &intent.machine, &request, document)
            })
            .transpose()?
    } else {
        None
    };
    let held_state = store.state().to_owned();
    let mut custody = Custody {
        project,
        state: held_state.clone(),
        observation_pending: None,
        existing: document.is_some(),
        saved: false,
        document,
        previous,
        intent: intent.clone(),
        request: request.clone(),
        settlement,
        now: chrono::Utc::now().timestamp_millis(),
    };
    let tx = store
        .db
        .as_mut()
        .ok_or_else(required)?
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(store::begin_error)?;
    store::execution_fence(&tx)?;
    let original = admission.consume(&tx, &held_state, &prepared::actor(), &request)?;
    let fresh = original.fresh(&tx);
    if let Err(problem) = fresh {
        refusal = Some(problem);
    }
    if let Some(problem) = &refusal {
        if matches!(
            problem.code.as_str(),
            code::STORAGE_FAILED | code::STORE_BUSY
        ) {
            return Err(problem.clone());
        }
    }
    ledger::reserve(
        &tx,
        &ledger::Header {
            intent: intent.clone(),
            request: request.clone(),
            task: reference.clone(),
        },
    )?;
    if let Some(problem) = refusal {
        // A current, exact native original may record its closed refusal.
        // Stale facts grant no document/queue custody and cause no task write.
        custody.finish(&tx, &Err(problem))?;
        original.record_refusal(&tx)?;
        tx.commit().map_err(store::storage)?;
        let reply = ledger::original(store.db.as_ref().ok_or_else(required)?, &intent)?
            .result
            .ok_or_else(required)?;
        return Ok(json!({"intent":intent,"reply":reply}));
    }
    original.current()?;
    tx.commit().map_err(store::storage)?;
    // Only this live original continues the durable Unknown. Its saved row
    // cannot construct another custodian or restart a document-side write.
    let result = (|| -> Result<ops::Reply> {
        #[cfg(all(test, unix))]
        test_point(&AFTER_RESERVE, &custody)?;
        original.current()?;
        if let wire::Action::NewTask { title, backlog, .. } = &prepared.plan.action {
            let mut document = crate::tasks::native_team::new_document(
                &custody.project,
                &project_name,
                &reference.uuid,
                title,
                *backlog,
                custody.now,
            )?;
            custody.previous = Some(native::prepare(
                &store,
                &config.state,
                &intent.machine,
                &request,
                &mut document,
            )?);
            custody.document = Some(document);
        }
        #[cfg(all(test, unix))]
        test_point(&BEFORE_SAVE, &custody)?;
        original.current()?;
        custody
            .project
            .save(custody.document.as_mut().ok_or_else(required)?)
            .map_err(store::storage)?;
        custody.saved = true;
        #[cfg(all(test, unix))]
        test_point(&AFTER_SAVE, &custody)?;
        let now = custody.now;
        ops::apply_with_native_context(
            &mut store,
            &prepared::actor(),
            &request,
            now,
            Some(config),
            &|team| crate::desktop::teams::repository(config, team),
            Admission {
                original,
                custody: &mut custody,
            }
            .into(),
        )
    })();
    if matches!(&result,Err(problem) if problem.code==code::STORAGE_FAILED) {
        return Ok(
            json!({"intent":intent,"reply":{"ok":false,"code":"team_person_action_unknown","request_id":request.request_id,"message":"The original task intake is unconfirmed. Read its original result; it will not be sent again."}}),
        );
    }
    // A result tentatively written inside the operation is not evidence of
    // commit. Final proof consumption may have rolled that transaction back.
    // Only its durable original row can decide the answer now.
    if ledger::original(store.db.as_ref().ok_or_else(required)?, &intent)?
        .result
        .is_none()
    {
        // Known refusal before the common writer recorded a result. Close the
        // original as metadata only; never run the operation a second time.
        let tx = store
            .db
            .as_mut()
            .ok_or_else(required)?
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(store::begin_error)?;
        if let wire::StoreBinding::Existing { fingerprint } = &intent.store {
            if &super::super::person_gesture::identity(&tx, &held_state)? != fingerprint {
                return Err(required());
            }
        } else {
            return Err(required());
        }
        custody.finish(&tx, &result)?;
        tx.commit().map_err(store::storage)?;
    }
    let reply = ledger::original(store.db.as_ref().ok_or_else(required)?, &intent)?
        .result
        .ok_or_else(required)?;
    Ok(json!({"intent":intent,"reply":reply}))
}

#[cfg(all(test, unix))]
type Hook = std::cell::RefCell<Option<Box<dyn FnOnce(&Custody) -> Result<()>>>>;
#[cfg(all(test, unix))]
thread_local! {
    static AFTER_RESERVE:Hook=const {std::cell::RefCell::new(None)};
    static BEFORE_CONSUME:Hook=const {std::cell::RefCell::new(None)};
    static BEFORE_SAVE:Hook=const {std::cell::RefCell::new(None)};
    static AFTER_SAVE:Hook=const {std::cell::RefCell::new(None)};
}
#[cfg(all(test, unix))]
fn test_point(point: &'static std::thread::LocalKey<Hook>, custody: &Custody) -> Result<()> {
    match point.with(|hook| hook.borrow_mut().take()) {
        Some(hook) => hook(custody),
        None => Ok(()),
    }
}
#[cfg(all(test, unix))]
mod tests;
