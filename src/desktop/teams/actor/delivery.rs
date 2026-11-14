use super::*;
use crate::teams::{ops::HandoutOutcome, store::EffectIntent};
use super::ports::{Availability, Fact};
use serde::{Deserialize, Serialize};
use zerus_team_plan::model::MemberState;

/// Why `member` left the active set, when the actor's read shows it
/// (zerus-63rru.85, round 3 after zerus-63rru.87): the store's own member
/// state says it (paused, stopped, unbound, or the row is gone), or the
/// actor's observation read the binding as decisively ended (Gone, Changed,
/// StartedFresh). The second case is recorded in the store before any
/// settlement, so a settlement is always decided from store state and never
/// from a request's reason. An observation that cannot decide (NoHandle,
/// Unavailable) is unknown and stays the unreadable-member way, so it is not
/// a departure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Left {
    /// The store's member row already says it; the ops name the cause.
    Store,
    /// The binding ended in the actor's own read; the store still holds it.
    Binding,
}

fn left(
    store: &Store,
    ports: &mut impl Ports,
    team: &Team,
    member: &str,
) -> Result<Option<Left>> {
    let Some(m) = store.member(&team.id, member)? else {
        // A missing member is gone too: a removal already settled its row.
        return Ok(Some(Left::Store));
    };
    if m.state != MemberState::Active || m.session.is_none() {
        return Ok(Some(Left::Store));
    }
    Ok(matches!(
        ports.availability(team, &m)?,
        Availability::Unreadable(Fact::Gone | Fact::Changed | Fact::StartedFresh)
    )
    .then_some(Left::Binding))
}
#[derive(Serialize, Deserialize)]
struct Delivery {
    node: u32,
    member: String,
    id: String,
    kind: String,
    intent: Option<Request>,
    outcome: Option<HandoutOutcome>,
    receipt: Option<Request>,
    gated: bool,
    #[serde(default)]
    not_started: bool,
    #[serde(default)]
    reads: u8,
    #[serde(default)]
    nonstarts: u32,
    /// The refusal is the terminal gate's: the member's terminal holds
    /// input (zerus-63rru.79). An older build reads a plain refusal.
    #[serde(default)]
    waiting: bool,
    /// Which dialog the gate knew, when one did (zerus-63rru.83).
    #[serde(default)]
    dialog: Option<String>,
    /// The refusal is the project's own limit: a fact about the team
    /// (zerus-63rru.23). An older build reads a plain refusal.
    #[serde(default)]
    project: bool,
    /// This delivery is the probe that may end the team's project fact.
    #[serde(default)]
    probe: bool,
    /// The outcome came from the command's own decoded answer (never from a
    /// recovery fabrication), which is what decides whether a probe ends the
    /// fact (zerus-63rru.23, the review's P2).
    #[serde(default)]
    command: bool,
    /// The stamp of the member's row when that refusal was observed.
    #[serde(default)]
    stamp: Option<String>,
}
impl Runtime {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn send(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        assignment: (&Node, &Member),
        kind: &str,
        probe: bool,
        now: i64,
    ) -> Result<()> {
        let (node, member) = assignment;
        let id = uuid::Uuid::new_v4().to_string();
        let op = if kind == "reopen" {
            Operation::ReopenIntent {
                node: node.id,
                member: member.id.clone(),
                effect_request_id: id.clone(),
            }
        } else {
            Operation::HandoutIntent {
                node: node.id,
                member: member.id.clone(),
                effect_request_id: id.clone(),
            }
        };
        let (actor, intent) = request(store, &team.id, op)?;
        let mut d = Delivery {
            node: node.id,
            member: member.id.clone(),
            id: id.clone(),
            kind: kind.into(),
            intent: Some(intent),
            outcome: None,
            receipt: None,
            gated: false,
            not_started: false,
            reads: 0,
            nonstarts: 0,
            waiting: false,
            dialog: None,
            stamp: None,
            project: false,
            probe,
            command: false,
        };
        let key = format!("delivery:{id}");
        self.ledger.reserve(
            &team.id,
            &key,
            serde_json::to_value(&d).map_err(crate::teams::store::storage)?,
        )?;
        boundary("reserved");
        // This frame may attempt an original. A durably known non-start
        // permits a later fresh admission; an unknown original never does.
        let intent = d.intent.as_ref().expect("reserved");
        if let Err(error) = ops::apply(store, &actor, intent, now) {
            if store.request_refused(&team.id, &intent.request_id)? {
                if let Some(row) = self.ledger.get(&team.id, &key)? {
                    self.ledger.remove(&row)?;
                }
            }
            return Err(error);
        }
        boundary("intent");
        d.intent = None;
        self.invoke_delivery(store, ports, team, &key, d, (node, member), now)
    }
    #[allow(clippy::too_many_arguments)]
    fn invoke_delivery(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        key: &str,
        mut d: Delivery,
        assignment: (&Node, &Member),
        now: i64,
    ) -> Result<()> {
        if d.not_started {
            // Consume the proof durably before attempting a new original. If
            // this result is lost, recovery is uncertain and cannot send.
            d.not_started = false;
            self.put(&team.id, key, &d)?;
            boundary("retry_armed");
        }
        let result = ports.deliver(team, assignment.0, assignment.1, &d.id);
        boundary("effect");
        match result {
            ports::DeliveryResult::ReadLater => {
                d.command = true;
                self.put(&team.id, key, &d)?;
                self.defer(store, team, key, "", now)
            }
            ports::DeliveryResult::Stale => {
                self.defer_delivery(store, ports, team, (key, assignment.0), d, now)
            }
            ports::DeliveryResult::Outcome(outcome) => {
                d.outcome = Some(outcome);
                d.command = true;
                self.put(&team.id, key, &d)?;
                boundary("outcome");
                self.settle(store, team, key, d, now)
            }
            // Refused before any input because the member's terminal holds
            // input: recorded as a refusal, with that cause and the member's
            // row as it is now, both kept before anything settles.
            ports::DeliveryResult::TerminalWaiting { reason, dialog } => {
                d.stamp = ports.row_stamp(team, assignment.1).ok().flatten();
                d.waiting = true;
                d.dialog = dialog;
                d.command = true;
                d.outcome = Some(HandoutOutcome::Refused { reason });
                self.put(&team.id, key, &d)?;
                boundary("outcome");
                self.settle(store, team, key, d, now)
            }
            // Refused before any input because the project's own limits
            // refuse it: recorded as a refusal, with that cause, both kept
            // before anything settles.
            ports::DeliveryResult::ProjectLimit { reason } => {
                d.project = true;
                d.command = true;
                d.outcome = Some(HandoutOutcome::Refused { reason });
                self.put(&team.id, key, &d)?;
                boundary("outcome");
                self.settle(store, team, key, d, now)
            }
            ports::DeliveryResult::NotStarted { reason } => {
                d.command = true;
                d.not_started = true;
                d.nonstarts = d.nonstarts.saturating_add(1);
                if d.nonstarts >= 5 {
                    // Five non-starts reach the retry table's fifteen-minute
                    // tier (0, 1, 3, 7, 15 minutes). Free the reservation so
                    // the kernel can choose another member on a later pass.
                    d.outcome = Some(HandoutOutcome::Refused {
                        reason: reason.clone(),
                    });
                }
                self.put(&team.id, key, &d)?;
                boundary("not_started");
                if d.outcome.is_some() {
                    self.settle(store, team, key, d, now)
                } else {
                    self.failure(store, team, key, "", &problem(&reason), now)
                }
            }
        }
    }
    fn defer_delivery(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        target: (&str, &Node),
        mut d: Delivery,
        now: i64,
    ) -> Result<()> {
        // The CLI ran, but proved no delivery started. Persist that proof
        // before observing; an uncertain original never gets it.
        let (key, node) = target;
        d.not_started = true;
        self.put(&team.id, key, &d)?;
        let task = node
            .task
            .as_ref()
            .ok_or_else(|| problem("Stale delivery has no task"))?;
        let observed = ports.observe(store, task);
        // Even a failed read must not leave the delivery itself ungated.
        self.defer(store, team, key, "", now)?;
        observed
    }
    pub(super) fn recover(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        now: i64,
    ) -> Result<()> {
        let mut budget = 4;
        for e in self
            .open_effects(store, &team.id)?
            .into_iter()
            .filter(|e| e.outcome.is_none())
        {
            let key = format!("delivery:{}", e.request_id);
            if self.ledger.get(&team.id, &key)?.is_none() {
                self.put(&team.id, &key, &Delivery::interrupted(e))?;
            }
            self.recover_row(store, ports, team, &key, now, &mut budget)?;
        }
        let after = self
            .load::<String>(&team.id, "delivery_cursor")?
            .unwrap_or_default();
        let rows = self.ledger.page(&team.id, &after)?;
        let next = if rows.len() == 200 {
            rows.last().expect("full page").key.clone()
        } else {
            String::new()
        };
        for row in rows {
            if row.key.starts_with("delivery:") {
                self.recover_row(store, ports, team, &row.key, now, &mut budget)?;
            }
        }
        self.put(&team.id, "delivery_cursor", &next)?;
        Ok(())
    }
    fn recover_row(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        key: &str,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        if !self.due(store, team, key, "", now)? {
            return Ok(());
        }
        if let Err(error) = self.recover_delivery(store, ports, team, key, now, budget) {
            // A stale revision is a lost race, not a failure: another
            // request settled the row first, and the next pass re-reads
            // (zerus-63rru.96).
            if error.code == code::STALE_REVISION {
                return Ok(());
            }
            self.failure(store, team, key, "", &error, now)?;
        }
        Ok(())
    }
    fn recover_delivery(
        &mut self,
        store: &mut Store,
        ports: &mut impl Ports,
        team: &Team,
        key: &str,
        now: i64,
        budget: &mut u8,
    ) -> Result<()> {
        let Some(row) = self.ledger.get(&team.id, key)? else {
            return Ok(());
        };
        let mut d: Delivery =
            serde_json::from_value(row.data.clone()).map_err(crate::teams::store::storage)?;
        if let Some(intent) = &d.intent {
            let actor = Actor::Service {
                person: team.owner.clone(),
                team: team.id.clone(),
            };
            match ops::apply(store, &actor, intent, now) {
                Ok(_) => d.intent = None,
                Err(error) => {
                    if store.request_refused(&team.id, &intent.request_id)? {
                        self.recovered(store, team, key, now)?;
                        self.ledger.remove(&row)?;
                        return Ok(());
                    }
                    return Err(error);
                }
            }
        }
        if d.outcome.is_none() && !store.effect_reserved(&team.id, &d.id)? {
            // The store settled this effect before any outcome reached the
            // row: a removal's hand-back (zerus-63rru.77), a reopen or an
            // override. No admission lets it out again, so the row ends now:
            // nothing is delivered, replayed or recorded, no failure stays.
            self.recovered(store, team, key, now)?;
            self.ledger.remove_input(&team.id, &d.id)?;
            self.ledger.remove(&row)?;
            return Ok(());
        }
        if d.outcome.is_none() && store.effect_reserved(&team.id, &d.id)? {
            if let Some(left) = left(store, ports, team, &d.member)? {
                // A reservation for a member that left the active set
                // (paused, stopped, unbound) is settled at the next pass
                // instead of being replayed through the non-start tiers
                // (zerus-63rru.85): the store supersedes the effect and
                // hands the node back to the lead with one item, and nothing
                // here delivers, replays or fails it.
                if left == Left::Binding {
                    // The store still holds the binding the actor's own read
                    // found ended. Record the departure first, so the
                    // settlement is decided from store state and not from a
                    // reason the operation cannot check (zerus-63rru.85
                    // round 3).
                    let (actor, unbind) = request(
                        store,
                        &team.id,
                        Operation::UnbindMember {
                            member: d.member.clone(),
                        },
                    )?;
                    if let Err(error) = ops::apply(store, &actor, &unbind, now) {
                        if store.request_refused(&team.id, &unbind.request_id)? {
                            // The store moved between the read and the write:
                            // the next pass reads it again.
                            return Ok(());
                        }
                        return Err(error);
                    }
                }
                let (actor, request) = request(
                    store,
                    &team.id,
                    Operation::SettleLeftHandout {
                        node: d.node,
                        reason: None,
                    },
                )?;
                #[cfg(test)]
                if STALE_RACE.with(|c| c.get()) {
                    return Err(Problem::new(code::STALE_REVISION, "stale"));
                }
                if let Err(error) = ops::apply(store, &actor, &request, now) {
                    if store.request_refused(&team.id, &request.request_id)? {
                        // The member returned to the active set between the
                        // read and the settlement, which writes nothing: the
                        // next pass reads the store again.
                        return Ok(());
                    }
                    return Err(error);
                }
                self.recovered(store, team, key, now)?;
                self.ledger.remove(&row)?;
                return Ok(());
            }
        }
        if d.not_started && d.outcome.is_none() {
            if *budget == 0 || ports.stopped() || team.status != TeamStatus::Running {
                return Ok(());
            }
            let node = store
                .node(&team.id, d.node)?
                .ok_or_else(|| problem("Deferred delivery node disappeared"))?;
            let member = store
                .member(&team.id, &d.member)?
                .ok_or_else(|| problem("Deferred delivery member disappeared"))?;
            *budget -= 1;
            return self.invoke_delivery(store, ports, team, key, d, (&node, &member), now);
        }
        if d.outcome.is_none() {
            let node = store.node(&team.id, d.node)?.ok_or_else(|| problem("Delivery node disappeared"))?;
            let member = store.member(&team.id, &d.member)?.ok_or_else(|| problem("Delivery member disappeared"))?;
            if member.agent == "dsh" && d.reads < 3 && *budget > 0 && !ports.stopped() {
                d.reads += 1;
                *budget -= 1;
                self.put(&team.id, key, &d)?;
                match ports.recover_delivery(team, &node, &member, &d.id) {
                    Ok(Some(outcome)) => d.outcome = Some(outcome),
                    Ok(None) | Err(_) if d.reads < 3 => {
                        self.defer(store, team, key, "", now)?;
                        return Ok(());
                    }
                    _ => {}
                }
            } else if member.agent == "dsh" && d.reads < 3 {
                return Ok(());
            }
        }
        if d.outcome.is_none() {
            d.outcome=Some(HandoutOutcome::Uncertain{reason:"The service restarted or lost its original result; this delivery will not be sent again.".into()});
            self.put(&team.id, key, &d)?;
        }
        self.settle(store, team, key, d, now)
    }
    fn settle(
        &mut self,
        store: &mut Store,
        team: &Team,
        key: &str,
        mut d: Delivery,
        now: i64,
    ) -> Result<()> {
        if !d.gated {
            // A refusal of an effect the store has already settled (a
            // removal's hand-back, which told the lead) is no failure of the
            // node's hand-out (zerus-63rru.77), and neither is a refusal of
            // the terminal gate (zerus-63rru.79) nor of the project's own
            // limit (zerus-63rru.23).
            let refused = match &d.outcome {
                Some(HandoutOutcome::Refused { reason })
                    if !d.waiting
                        && !d.project
                        && store.effect_reserved(&team.id, &d.id)? =>
                {
                    Some(reason)
                }
                _ => None,
            };
            if let Some(reason) = refused {
                let revision = store
                    .task_observation(&team.id, d.node)?
                    .map_or(String::new(), |o| o.revision);
                self.failure(
                    store,
                    team,
                    &format!("handout:{}", d.node),
                    &revision,
                    &problem(reason),
                    now,
                )?;
            }
            // A refusal of the terminal gate is not a failed hand-out: the
            // member's terminal is not free (zerus-63rru.79).
            if d.waiting {
                self.terminal_refused(
                    &team.id,
                    &d.member,
                    d.stamp.clone(),
                    d.dialog.clone(),
                    now,
                )?;
            }
            // A refusal of the project's own limit is not a failed hand-out:
            // it is a fact about the team (zerus-63rru.23).
            if d.project {
                let reason = match &d.outcome {
                    Some(HandoutOutcome::Refused { reason }) => reason.clone(),
                    _ => String::new(),
                };
                self.project_limit_refused(store, team, &d.id, &reason, now)?;
            } else if d.probe
                && d.command
                && !d.not_started
                && matches!(
                    d.outcome,
                    Some(HandoutOutcome::Submitted { .. } | HandoutOutcome::Refused { .. })
                )
                && self.ledger.get(&team.id, super::project_limit::KEY)?.is_some()
            {
                // The probe's own answer, and the project admitted: only a
                // submitted delivery or the command's own refusal with
                // another cause ends the fact (zerus-63rru.23, the review's
                // P2). An uncertain probe and any non-start, the converted
                // fifth included, keep it; the next probe waits as usual.
                self.project_limit_ended(store, team, now)?;
            }
            // A submitted delivery: the member's terminal took input.
            if matches!(d.outcome, Some(HandoutOutcome::Submitted { .. })) {
                self.terminal_free(&team.id, &d.member)?;
            }
            // A hand-back is settled by the store at once; nothing retries it.
            if matches!(
                d.outcome,
                Some(HandoutOutcome::Submitted { .. } | HandoutOutcome::HandedBack { .. })
            ) {
                self.recovered(store, team, &format!("handout:{}", d.node), now)?;
            }
            d.gated = true;
            self.put(&team.id, key, &d)?;
        }
        if d.receipt.is_none() {
            let outcome = d.outcome.clone().expect("durable outcome");
            let op = if d.kind == "reopen" {
                Operation::ReopenOutcome {
                    effect_request_id: d.id.clone(),
                    outcome,
                }
            } else {
                Operation::HandoutOutcome {
                    effect_request_id: d.id.clone(),
                    outcome,
                }
            };
            d.receipt = Some(request(store, &team.id, op)?.1);
            self.put(&team.id, key, &d)?;
        }
        let actor = Actor::Service {
            person: team.owner.clone(),
            team: team.id.clone(),
        };
        match ops::apply(store, &actor, d.receipt.as_ref().expect("reserved"), now) {
            Ok(_) => {
                boundary("recorded");
                // The effect now durably forbids another offer. Full inputs
                // need not accumulate after their actor delivery settles.
                self.ledger.remove_input(&team.id, &d.id)?;
                if matches!(team.status, TeamStatus::Running | TeamStatus::Paused) {
                    self.recovered(store, team, key, now)?;
                }
                if let Some(row) = self.ledger.get(&team.id, key)? {
                    self.ledger.remove(&row)?;
                }
            }
            Err(e) if e.code == code::STALE_REVISION => {
                d.receipt = None;
                self.put(&team.id, key, &d)?;
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }
    /// Settle only durable proof that no child started, before terminal
    /// cancellation. Unknown originals remain evidence and are never retried.
    pub(super) fn terminal_deliveries(
        &mut self,
        store: &mut Store,
        team: &Team,
        now: i64,
    ) -> Result<bool> {
        if !matches!(team.status, TeamStatus::Stopped | TeamStatus::Archived) {
            return Ok(true);
        }
        let after = self
            .load::<String>(&team.id, "terminal_delivery_cursor")?
            .unwrap_or_default();
        let rows = self.ledger.prefix_page(&team.id, "delivery:", &after)?;
        let next = if rows.len() == 200 {
            rows.last().unwrap().key.clone()
        } else {
            String::new()
        };
        for row in rows {
            let mut d: Delivery =
                serde_json::from_value(row.data.clone()).map_err(crate::teams::store::storage)?;
            if !d.not_started {
                // Round 2 (zerus-63rru.102): the terminal team settles a
                // hosted delivery whose actor outcome was never recorded
                // only from a durable outcome it already holds. The row's
                // persisted outcome, or the hosted child's retained reply
                // (read without any offer; nothing is delivered, replayed
                // or offered again), records through the service's own
                // outcome writer as a fact only - no new hand-out, no new
                // card, no status change - then settles, which removes the
                // input as on a running team. With neither, the delivery
                // stays unknown: keep the row and the input, never replay.
                // The settle below removes the input, but it does not complete
                // the Zerus task attempt that only the hosted child writes
                // after retaining its reply (hosted.rs:212-223). If the child
                // stopped between those two writes and the team is terminal, the
                // attempt stays unknown: every later send or start of that task
                // is refused, and nothing is replayed. --recover-only cannot settle
                // it on a terminal team, and no person command cancels that exact
                // attempt yet (zerus-iysrw, zerus-63rru.125).
                let outcome = if d.outcome.is_some() {
                    d.outcome.clone()
                } else {
                    #[cfg(any(target_os = "linux", target_os = "macos"))]
                    {
                        self.retained_outcome(team, &d)?
                    }
                    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
                    {
                        None
                    }
                };
                if let Some(outcome) = outcome {
                    d.outcome = Some(outcome);
                    d.gated = true;
                    self.put(&team.id, &row.key, &d)?;
                    // The fact is durable before it is applied: a crash here
                    // must leave the recorded outcome and its gate for the
                    // next pass, not a row that only looks unknown.
                    boundary("terminal_outcome");
                    self.settle(store, team, &row.key, d, now)?;
                }
                continue;
            }
            if d.outcome.is_none() {
                d.outcome = Some(HandoutOutcome::Refused {
                    reason: "Delivery did not start before the team ended".into(),
                });
                d.gated = true;
                self.put(&team.id, &row.key, &d)?;
            }
            self.settle(store, team, &row.key, d, now)?;
        }
        self.put(&team.id, "terminal_delivery_cursor", &next)?;
        Ok(next.is_empty())
    }
    /// The hosted child's retained reply as a durable outcome, read without
    /// any offer (zerus-63rru.102 round 2). Only a submitted, refused,
    /// handed-back or cutting-over answer counts; an uncertain or pending
    /// reply settles nothing, and a missing input settles nothing. An
    /// uncertain reply is the accepted limit (round 3): the delivery stays
    /// unknown, the row and its retained task text stay, and the open effect
    /// keeps the terminal prune blocked for that team. It is never replayed.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn retained_outcome(&self, team: &Team, d: &Delivery) -> Result<Option<HandoutOutcome>> {
        let Some(row) = self
            .ledger
            .input(&team.id, &format!("hosted-input:{}", d.id))?
        else {
            return Ok(None);
        };
        if row.data["reply"].is_null() {
            return Ok(None);
        }
        let raw =
            serde_json::to_vec(&row.data["reply"]).map_err(crate::teams::store::storage)?;
        Ok(match super::native::decode_delivery(&raw, &d.id) {
            HandoutOutcome::Submitted { .. }
            | HandoutOutcome::Refused { .. }
            | HandoutOutcome::HandedBack { .. }
            | HandoutOutcome::CuttingOver { .. } => {
                Some(super::native::decode_delivery(&raw, &d.id))
            }
            HandoutOutcome::Uncertain { .. } => None,
        })
    }
    pub(super) fn open_effects(&self, store: &Store, team: &str) -> Result<Vec<EffectIntent>> {
        let mut out = Vec::new();
        let mut cursor = None;
        for _ in 0..4 {
            let page = store.open_effects(team, 200, cursor.as_ref())?;
            out.extend(page.rows);
            cursor = page.next;
            if cursor.is_none() {
                return Ok(out);
            }
        }
        Err(problem("Open effect page bound exceeded"))
    }
}
impl Delivery {
    fn interrupted(e: EffectIntent) -> Self {
        Self {
            node: e.node,
            member: e.member,
            id: e.request_id,
            kind: e.kind,
            intent: None,
            outcome: None,
            receipt: None,
            gated: false,
            not_started: false,
            reads: 0,
            nonstarts: 0,
            waiting: false,
            dialog: None,
            stamp: None,
            project: false,
            probe: false,
            command: false,
        }
    }
}

#[cfg(test)]
thread_local! {pub(super) static CRASH: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };}
#[cfg(test)]
thread_local! {pub(super) static STALE_RACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };}
fn boundary(name: &str) {
    #[cfg(test)]
    CRASH.with(|c| {
        if c.get() == Some(name) {
            c.set(None);
            panic!("fixture restart at {name}");
        }
    });
    #[cfg(not(test))]
    let _ = name;
}
