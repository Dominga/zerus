//! Closed schema 1: points only. Even a supplied zero is not measured work.
use super::*;
use std::collections::BTreeSet;
fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}
fn time(t: i64) -> bool {
    (0..=9_007_199_254_740_991).contains(&t)
}
fn bounded(s: &str, cap: usize) -> bool {
    !s.is_empty() && s.len() <= cap && !s.contains('\0')
}
pub fn key(k: &ObservedUnitKey) -> bool {
    uuid(&k.team)
        && uuid(&k.home)
        && k.node > 0
        && uuid(&k.task.uuid)
        && bounded(&k.task.project, 256)
        && k.publication
            .as_ref()
            .is_none_or(|p| uuid(&p.id) && p.draft_generation > 0)
        && k.effect.as_ref().is_none_or(|id| uuid(id))
        && k.attempt.as_ref().is_none_or(|a| {
            uuid(&a.id)
                && a.generation > 0
                && a.fingerprint.len() == 64
                && a.fingerprint
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        && k.binding.as_ref().is_none_or(|b| {
            bounded(&b.member, 128)
                && uuid(&b.session.machine)
                && bounded(&b.session.session_id, 256)
                && b.native_incarnation.is_none()
                && b.run_id.as_ref().is_none_or(|s| bounded(s, 128))
                && b.conversation_id.as_ref().is_none_or(|s| bounded(s, 128))
        })
        && serde_json::to_vec(k).is_ok_and(|b| b.len() <= MAX_UNIT_KEY_BYTES)
}
pub fn fact(f: &TimingFact) -> bool {
    let source = &f.source;
    f.schema==SCHEMA&&key(&f.key)&&f.seq>0&&f.unit>0&&f.unit<=f.seq
        &&f.seq<=9_007_199_254_740_991&&source.schema==SCHEMA&&bounded(&source.identity,512)
        &&source.team_revision<=9_007_199_254_740_991&&f.implementation_observed_ms.is_none()
        &&f.snapshot.native_model.is_none()&&f.snapshot.title.len()<=200
        &&f.snapshot.role.as_ref().is_none_or(|s|bounded(s,128))
        &&f.snapshot.configured_model.as_ref().is_none_or(|s|bounded(s,256))
        &&time(f.observed_at_ms)&&f.event_at_ms.is_none_or(time)&&f.last_verified_ms.is_none_or(time)
        &&f.reasons.len()<=32&&f.break_before // schema 1 promises no continuous interval
        &&source.observer_epoch.as_ref().map_or_else(||f.reasons.contains(&Reason::ObserverEpochUnavailable),|s|uuid(s))
        &&source.replica_generation.as_ref().is_none_or(|s|bounded(s,128))
        &&match source.kind {
            SourceKind::TeamJournal => source.event_id.as_ref().is_some_and(|s|uuid(s))&&source.event_sequence.is_some_and(|s|s>0)&&f.event_at_ms.is_some()
                &&source.task_revision.is_none()&&source.replica_generation.is_none()&&source.replica_offset.is_none()
                &&f.snapshot.role.is_none()&&f.snapshot.configured_model.is_none()
                &&matches!(f.category,Category::DeliveryReserved|Category::DeliverySubmitted|Category::DeliveryRefused|Category::DeliveryUnknown),
            SourceKind::TaskDocument => source.event_id.is_none()&&source.event_sequence.is_none()&&f.event_at_ms.is_none()
                &&(if f.category==Category::Gap {source.task_revision.is_none()} else {source.task_revision.as_ref().is_some_and(|s|bounded(s,4096))})
                &&matches!(f.category,Category::TaskBacklog|Category::TaskTodo|Category::TaskInProgress|Category::TaskInReview|Category::TaskDone|Category::TaskCancelled|Category::Gap|Category::Watermark),
        }
        &&serde_json::to_vec(f).is_ok_and(|b|b.len()<=MAX_FACT_BYTES)
}
pub fn page(p: &ObservedTimingPage) -> bool {
    let w = &p.scope.window;
    let mut ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    p.schema == SCHEMA
        && uuid(&p.scope.team)
        && uuid(&p.scope.home)
        && p.capabilities == Capabilities::default()
        && p.rows.len() <= MAX_FACTS
        && p.cohort.units.len() <= MAX_FACTS
        && p.source_revision <= 9_007_199_254_740_991
        && p.fact_ceiling <= 9_007_199_254_740_991
        && (1..=MAX_WINDOW_MS).contains(&w.window_ms)
        && p.as_of_ms == p.last_verified_ms
        && p.as_of_ms == w.to_ms
        && match (w.from_ms, w.to_ms) {
            (None, None) => true,
            (Some(a), Some(b)) => time(a) && time(b) && a <= b && (b - a) as u64 <= w.window_ms,
            _ => false,
        }
        && p.stream_id.as_ref().is_none_or(|s| uuid(s))
        && p.reasons.len() <= 32
        && p.next.as_ref().is_none_or(|s| {
            !s.is_empty()
                && s.len() <= MAX_CURSOR_BYTES
                && !s.chars().any(|c| c.is_control() || c.is_whitespace())
        })
        && p.more == p.next.is_some()
        && (!p.cohort.more || p.truncated)
        && p.cohort.units.iter().all(|u| {
            key(&u.key)
                && u.id > 0
                && u.id <= p.fact_ceiling
                && ids.insert(u.id)
                && keys.insert(serde_json::to_string(&u.key).unwrap_or_default())
                && u.key.team == p.scope.team
                && u.key.home == p.scope.home
                && u.key.phase == p.scope.phase
                && p.scope.node.is_none_or(|n| n == u.key.node)
        })
        && p.rows.windows(2).all(|a| a[0].seq < a[1].seq)
        && p.rows.iter().all(|f| {
            fact(f)
                && f.seq <= p.fact_ceiling
                && f.source.team_revision <= p.source_revision
                && p.cohort
                    .units
                    .iter()
                    .any(|u| u.id == f.unit && u.key == f.key)
        })
        && serde_json::to_vec(p).is_ok_and(|b| b.len() <= MAX_PAGE_BYTES)
}
