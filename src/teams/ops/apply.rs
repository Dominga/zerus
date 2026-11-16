use super::*;
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
enum Saved {
    Success(Reply),
    Refused(Problem),
}

/// Every new mutation, its revision and its receipt share one write transaction.
/// Saved receipt replay is a read and never renews permission to cause effects.
pub fn apply(store: &mut Store, actor: &Actor, request: &Request, now: i64) -> Result<Reply> {
    apply_with_config(store, actor, request, now, None)
}

/// Native configuration supplies the placement reader, never a request path.
/// Repository and provider reads run under the transaction after receipt replay.
pub fn apply_with_config(
    store: &mut Store,
    actor: &Actor,
    request: &Request,
    now: i64,
    config: Option<&crate::config::Config>,
) -> Result<Reply> {
    apply_with_context(store, actor, request, now, config, &|team| {
        let config = config.ok_or_else(|| {
            invalid("A trusted local repository resolver is required to advance the base.")
        })?;
        crate::desktop::teams::repository(config, team)
    })
}

/// Tests supply a private repository through the same transaction and guards.
#[cfg(test)]
pub(crate) fn apply_with_repository(
    store: &mut Store,
    actor: &Actor,
    request: &Request,
    now: i64,
    repository: &dyn Fn(&Team) -> Result<crate::teams::integrate::Repo>,
) -> Result<Reply> {
    apply_with_context(store, actor, request, now, None, repository)
}

fn apply_with_context(
    store: &mut Store,
    actor: &Actor,
    request: &Request,
    now: i64,
    config: Option<&crate::config::Config>,
    repository: &dyn Fn(&Team) -> Result<crate::teams::integrate::Repo>,
) -> Result<Reply> {
    apply_inputs(store, actor, request, now, config, repository, None)
}

pub(crate) fn apply_with_gesture(
    store: &mut Store,
    actor: &Actor,
    request: &Request,
    now: i64,
    admission: Option<crate::desktop::teams::person_gesture::Admission>,
) -> Result<Reply> {
    // This closed V1 intent is only DecideCard. It never resolves a repository.
    apply_inputs(
        store,
        actor,
        request,
        now,
        None,
        &|_| {
            Err(invalid(
                "A trusted local repository resolver is required to advance the base.",
            ))
        },
        admission,
    )
}

fn apply_inputs(
    store: &mut Store,
    actor: &Actor,
    request: &Request,
    now: i64,
    config: Option<&crate::config::Config>,
    repository: &dyn Fn(&Team) -> Result<crate::teams::integrate::Repo>,
    admission: Option<crate::desktop::teams::person_gesture::Admission>,
) -> Result<Reply> {
    validate_request(request, now)?;
    if matches!(actor, Actor::Person { .. })
        && matches!(request.operation, Operation::DecideCard { .. })
        && admission.is_none()
    {
        return Err(crate::desktop::teams::person_gesture::required());
    }
    store.refresh()?;
    prepare_first(store, actor, request)?;
    let state = store.state().to_owned();
    let task_custody = phase_controls::prepare(store, request);
    let tx = store
        .db
        .as_mut()
        .ok_or_else(missing)?
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(crate::teams::store::begin_error)?;
    store::execution_fence(&tx)?;
    let gesture = admission
        .map(|admission| admission.consume(&tx, &state, actor, request))
        .transpose()?;
    if let Some(intent) = &gesture {
        if let Some(saved) = replay(&tx, actor, request)? {
            crate::desktop::teams::person_gesture::original(&tx, intent)?;
            return saved;
        }
        // Bound the native point-read bodies before the common kernel reads
        // its team. The snapshot cannot change under this writer transaction.
        crate::desktop::teams::person_gesture::fresh(&tx, intent)?;
    }
    let current = rows::team(&tx, &request.team)?;
    let team = current
        .clone()
        .map_or_else(|| validation::creation_team(request, actor), Ok)?;
    verify_actor(&tx, actor, &team)?;
    if let Some(saved) = replay(&tx, actor, request)? {
        return saved;
    }
    authority(actor, &request.operation, &team)?;
    tx.execute_batch("SAVEPOINT operation;").map_err(storage)?;
    let mut ctx = Context {
        db: &tx,
        state: &state,
        actor,
        team: &team,
        op: &request.operation,
        request_id: &request.request_id,
        task_custody: task_custody.as_ref(),
        now,
        reply: Reply {
            ok: true,
            revision: 0,
            member: None,
            node: None,
            item: None,
        },
        event: None,
        superseded: None,
    };
    let result = run(&mut ctx, request, current.is_some(), config, repository);
    // A storage failure is not a saved decision. Dropping the transaction
    // rolls back partial work; the same request ID can be processed again.
    if matches!(&result, Err(problem) if problem.code == code::STORAGE_FAILED) {
        return result;
    }
    save(&tx, actor, request, &result)?;
    if let Some(intent) = &gesture {
        crate::desktop::teams::person_gesture::record(&tx, intent)?;
    }
    tx.commit().map_err(storage)?;
    result
}
fn validate_request(request: &Request, now: i64) -> Result<()> {
    uuid(&request.request_id)?;
    uuid(&request.team)?;
    if now < 0 {
        return Err(invalid("Time must be nonnegative."));
    }
    text(&encode(request)?, 262_144)
}
fn prepare_first(store: &mut Store, actor: &Actor, request: &Request) -> Result<()> {
    if store.db.is_some() {
        return Ok(());
    }
    let team = validation::creation_team(request, actor)?;
    authority(actor, &request.operation, &team)?;
    validation::validate_create_before_store(&team)?;
    if request.expected_revision != 0 {
        return Err(stale());
    }
    store.ensure()
}
fn verify_actor(db: &Connection, actor: &Actor, team: &Team) -> Result<()> {
    if let Actor::Session {
        team: id,
        member,
        session,
        ..
    } = actor
    {
        let valid = rows::member(db, id, member)?.is_some_and(|m| {
            m.state != MemberState::Removed && m.session.as_ref() == Some(session)
        });
        if id != &team.id || !valid {
            return Err(Problem::new(
                code::NOT_ALLOWED,
                "The exact session binding is no longer current.",
            ));
        }
    }
    Ok(())
}
pub(crate) fn replay(
    db: &Connection,
    actor: &Actor,
    request: &Request,
) -> Result<Option<Result<Reply>>> {
    let saved: Option<(String, String, String)> = db
        .query_row(
            "SELECT actor,content,result FROM requests WHERE id=?1",
            [&request.request_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(storage)?;
    let Some((who, content, result)) = saved else {
        return Ok(None);
    };
    if who != encode(actor)? || content != encode(request)? {
        return Err(Problem::new(
            code::DUPLICATE,
            "Request ID was used with different content or authority.",
        ));
    }
    Ok(Some(match decode(&result)? {
        Saved::Success(r) => Ok(r),
        Saved::Refused(p) => Err(p),
    }))
}
fn run(
    ctx: &mut Context<'_>,
    request: &Request,
    existed: bool,
    config: Option<&crate::config::Config>,
    repository: &dyn Fn(&Team) -> Result<crate::teams::integrate::Repo>,
) -> Result<Reply> {
    if ctx.team.revision != request.expected_revision {
        return Err(stale());
    }
    if existed
        && matches!(
            ctx.op,
            Operation::Create { .. } | Operation::StartPlanning { .. }
        )
    {
        return Err(invalid("Team already exists."));
    }
    if matches!(ctx.op, Operation::AdvanceBase { .. }) {
        base::advance(ctx, repository)?;
    } else {
        dispatch(ctx, config)?;
    }
    ctx.reply.revision = ctx
        .team
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| over("Revision is exhausted."))?;
    ctx.db
        .execute(
            "UPDATE teams SET revision=?2 WHERE id=?1",
            params![ctx.team.id, ctx.reply.revision],
        )
        .map_err(storage)?;
    journal::append(ctx, request)?;
    rows::archive_decided(ctx.db, &ctx.team.id, &encode(ctx.actor)?)?;
    Ok(ctx.reply.clone())
}
fn save(db: &Connection, actor: &Actor, request: &Request, result: &Result<Reply>) -> Result<()> {
    let saved = match result {
        Ok(r) => {
            db.execute_batch("RELEASE operation;").map_err(storage)?;
            Saved::Success(r.clone())
        }
        Err(p) => {
            db.execute_batch("ROLLBACK TO operation; RELEASE operation;")
                .map_err(storage)?;
            Saved::Refused(p.clone())
        }
    };
    db.execute(
        "INSERT INTO requests(id,team,actor,content,result) VALUES(?1,?2,?3,?4,?5)",
        params![
            request.request_id,
            request.team,
            encode(actor)?,
            encode(request)?,
            encode(&saved)?
        ],
    )
    .map_err(storage)?;
    Ok(())
}
fn dispatch(ctx: &mut Context<'_>, config: Option<&crate::config::Config>) -> Result<()> {
    match ctx.op {
        Operation::SetPhaseBarrier { .. }
        | Operation::PausePhase { .. }
        | Operation::ResumePhase { .. }
        | Operation::MovePhase { .. }
        | Operation::SetPhaseAcceptance { .. } => phase_controls::write(ctx),
        Operation::AdvanceBase { .. } => unreachable!("handled with the trusted repository reader"),
        Operation::ReconcilePhaseReview { .. } => phase_review::review(ctx),
        Operation::SendBackPhase { .. } => rework::send_back(ctx),
        Operation::SubmitPhaseCorrections { .. } | Operation::EditPhaseCorrections { .. } => {
            rework::write(ctx)
        }
        Operation::ApprovePhaseCorrections { .. } => rework::approve(ctx),
        Operation::RejectPhaseCorrections { .. } => rework::reject(ctx),
        Operation::UndoPhaseCorrectionRejection { .. } => rework::undo(ctx),
        Operation::ReconcileCorrectionReview { .. } => rework::review(ctx),
        Operation::ApprovePhase { .. } => publication::approve(ctx),
        Operation::RejectPhase { .. } => publication::reject(ctx),
        Operation::UndoRejectPhase { .. } => publication::undo(ctx),
        Operation::ReconcilePublication { .. } => publication::publish(ctx),
        Operation::PublicationTaskProblem { .. } => publication::problem(ctx),
        Operation::ReconcilePhase { .. } => publication::reconcile(ctx),
        Operation::AcceptPhase { .. } => publication::accept(ctx),
        Operation::SubmitDraftPhase { .. } | Operation::EditDraftPhase { .. } => drafts::write(ctx),
        Operation::AddMember { .. } => editing::add_member(ctx),
        Operation::SwapMember { .. } => editing::swap_member(ctx),
        Operation::RenameTeam { .. } => editing::rename(ctx),
        Operation::SetGoal { .. } => editing::goal(ctx),
        Operation::Create { .. } => team::create(ctx),
        Operation::StartPlanning { .. } => planning::start(ctx),
        Operation::Pause => team::pause(ctx),
        Operation::Resume => team::resume(ctx),
        Operation::Stop => team::stop(ctx),
        Operation::Archive => team::archive(ctx),
        Operation::SetValidationConfig { .. } => provider_settings::select(ctx, config),
        Operation::ChangeSettings { .. } => team::change_settings(ctx),
        Operation::SetMemberSession { .. } => members::set_session(ctx),
        Operation::SetMemberHandle { .. } => members::set_handle(ctx),
        Operation::SetMemberState { .. } => members::set_state(ctx),
        Operation::PauseMember { .. } => members::pause(ctx),
        Operation::ResumeMember { .. } => members::resume(ctx),
        Operation::StopMember { .. } => members::stop(ctx),
        Operation::ArchiveMember { .. } => members::archive(ctx),
        Operation::AddTask { .. } => queue::add(ctx),
        Operation::Admit { .. } => queue::admit(ctx),
        Operation::Route { .. } => queue::route(ctx),
        Operation::SetDeps { .. } => dependencies::set(ctx),
        Operation::Hold { .. } => queue::hold(ctx),
        Operation::Release { .. } => queue::release(ctx),
        Operation::ObserveTask { .. } => queue::observe(ctx),
        Operation::HandoutIntent { .. } => handout::intent(ctx),
        Operation::HandoutOutcome { .. } => handout::outcome(ctx),
        Operation::ReopenIntent { .. } => handout::reopen_intent(ctx),
        Operation::ReopenOutcome { .. } => handout::reopen_outcome(ctx),
        Operation::RecordReport { .. } => handout::report(ctx),
        Operation::StageTaskIntent { .. } => stage_task::intent(ctx),
        Operation::StageTaskOutcome { .. } => stage_task::outcome(ctx),
        Operation::Verdict { .. } => pipeline::verdict(ctx),
        Operation::Accept { .. } => decisions::accept(ctx),
        Operation::Return { .. } => decisions::return_task(ctx),
        Operation::Reassign { .. } => decisions::reassign(ctx),
        Operation::Cancel { .. } => decisions::cancel(ctx),
        Operation::RecordIntegration { .. } => decisions::integration(ctx),
        Operation::RecordIntegrationConflict { .. } => decisions::conflict(ctx),
        Operation::Ask { .. } => talk::ask(ctx),
        Operation::Reply { .. } => talk::reply(ctx),
        Operation::OpenCard { .. } => talk::open_card(ctx),
        Operation::CloseCard { .. } => talk::close_card(ctx),
        Operation::DecideCard { .. } => talk::decide_card(ctx),
        Operation::FireTimer { .. } => timers::fire(ctx),
        Operation::RecordKeyClear { .. } => timers::key_clear(ctx),
        Operation::RecordLateSession { .. } => timers::late_session(ctx),
        Operation::RecordLateReport { .. } => handout::late_report(ctx),
        Operation::RecordCancel { .. } => timers::record_cancel(ctx),
        Operation::ReconcileLeadQueue => lead_queue::reconcile(ctx),
        Operation::ReconcileUnstaffed => unstaffed::reconcile(ctx),
        Operation::EndStaleUnstaffed => unstaffed::end_stale(ctx),
        Operation::SettleLeftHandout { .. } => members::settle_left(ctx),
        Operation::UnbindMember { .. } => members::unbind(ctx),
        Operation::TellPausedStarted { .. } => members::tell_paused_started(ctx),
        Operation::ClosePausedStarted { .. } => members::close_paused_started(ctx),
    }
}
