//! Only data is staged here. No actor, admission or operation writer exists in
//! this cache; the native action original has its separate typed consumer.
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use zerus_client_core::desktop::{creation_clone::Scope, team_person_actions as wire};

const PLANS: usize = 64;
const KEEP: Duration = Duration::from_secs(600);

pub(super) struct Cache {
    scope: Scope,
    rows: BTreeMap<String, Row>,
}
struct Row {
    reference: wire::Reference,
    created: Instant,
    pieces: Vec<Option<Vec<u8>>>,
    body: Option<Arc<[u8]>>,
}
impl Cache {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            rows: BTreeMap::new(),
        }
    }

    /// At most 64 plans of the existing 256 KiB request bound: 16 MiB of
    /// retained payload, plus bounded routing metadata. Only the completed
    /// plan being read is decoded; no typed GUI history is cached.
    pub fn stage(&mut self, piece: wire::Piece) -> Result<bool> {
        self.stage_at(piece, Instant::now())
    }

    fn stage_at(&mut self, piece: wire::Piece, now: Instant) -> Result<bool> {
        ensure!(
            piece.valid() && piece.scope == self.scope,
            "native plan staging scope changed"
        );
        self.rows
            .retain(|_, row| now.saturating_duration_since(row.created) < KEEP);
        let bytes = piece.decoded().context("invalid native plan piece")?;
        let id = piece.plan.request_id.clone();
        if !self.rows.contains_key(&id) {
            ensure!(self.rows.len() < PLANS, "native plan staging is full");
            self.rows.insert(
                id.clone(),
                Row {
                    reference: piece.plan.clone(),
                    created: now,
                    pieces: vec![None; (piece.plan.bytes as usize).div_ceil(wire::PIECE_BYTES)],
                    body: None,
                },
            );
        }
        let row = self.rows.get_mut(&id).expect("checked staged plan");
        ensure!(
            row.reference == piece.plan,
            "native plan identity was already used"
        );
        let index = piece.ordinal as usize;
        if let Some(body) = &row.body {
            let offset = index * wire::PIECE_BYTES;
            ensure!(
                body.get(offset..offset + bytes.len()) == Some(bytes.as_slice()),
                "native plan piece changed"
            );
            return Ok(true);
        }
        let slot = row
            .pieces
            .get_mut(index)
            .context("native plan piece range")?;
        if let Some(previous) = slot {
            ensure!(previous == &bytes, "native plan piece changed");
        } else {
            *slot = Some(bytes);
        }
        if row.pieces.iter().any(Option::is_none) {
            return Ok(false);
        }
        let body: Vec<u8> = row
            .pieces
            .iter()
            .flat_map(|piece| piece.as_ref().expect("complete plan").iter().copied())
            .collect();
        let plan: wire::Plan = serde_json::from_slice(&body)?;
        ensure!(
            plan.reference().as_ref() == Some(&row.reference),
            "native plan digest or content changed"
        );
        row.body = Some(body.into());
        row.pieces.clear();
        Ok(true)
    }

    pub fn get(&self, reference: &wire::Reference) -> Result<Arc<wire::Plan>> {
        self.get_at(reference, Instant::now())
    }

    /// Retiring data cannot renew or revoke an admitted original; the one
    /// worker already owns its immutable plan. Receipt reads need no cache.
    pub fn discard(&mut self, reference: &wire::Reference) {
        if self
            .rows
            .get(&reference.request_id)
            .is_some_and(|row| &row.reference == reference)
        {
            self.rows.remove(&reference.request_id);
        }
    }

    fn get_at(&self, reference: &wire::Reference, now: Instant) -> Result<Arc<wire::Plan>> {
        ensure!(reference.valid(), "invalid native plan reference");
        let row = self
            .rows
            .get(&reference.request_id)
            .filter(|row| {
                &row.reference == reference && now.saturating_duration_since(row.created) < KEEP
            })
            .context("the original native plan is unavailable")?;
        let bytes = row.body.as_ref().context("the native plan is incomplete")?;
        let plan: wire::Plan = serde_json::from_slice(bytes)?;
        ensure!(
            plan.reference().as_ref() == Some(reference),
            "native plan content changed"
        );
        Ok(Arc::new(plan))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use zerus_client_core::teams::action::TeamAction;

    // Literal request DTOs only. No canonical Team or Task row is invented.
    fn plan(large: bool) -> wire::Plan {
        let change = if large {
            TeamAction::SubmitDraftPhase {
                phase: 1,
                draft: zerus_team_plan::draft::DraftInput {
                    title: "A large reviewed draft".into(),
                    barrier: false,
                    nodes: (1..=8)
                        .map(|id| zerus_team_plan::draft::DraftNode {
                            id,
                            title: format!("Task {id}"),
                            text: "x".repeat(12_000),
                            criteria: vec!["Tests pass".into()],
                            role: "developer".into(),
                            pipeline: "reviewed".into(),
                            deps: vec![],
                            assignee: None,
                            writes: vec![],
                            estimate_minutes: 1,
                            kind: zerus_team_plan::model::NodeKind::Task,
                        })
                        .collect(),
                },
            }
        } else {
            TeamAction::RenameTeam {
                name: "A reviewed name".into(),
            }
        };
        wire::Plan {
            request_id: uuid::Uuid::new_v4().to_string(),
            team: uuid::Uuid::new_v4().to_string(),
            expected_revision: 2,
            action: wire::Action::Team {
                change: Box::new(change),
            },
        }
    }
    fn scope() -> Scope {
        Scope {
            configuration: "a".repeat(64),
            service_epoch: uuid::Uuid::new_v4().to_string(),
        }
    }
    fn pieces(scope: &Scope, plan: &wire::Plan) -> Vec<wire::Piece> {
        let reference = plan.reference().unwrap();
        serde_json::to_vec(plan)
            .unwrap()
            .chunks(wire::PIECE_BYTES)
            .enumerate()
            .map(|(ordinal, bytes)| wire::Piece {
                request_id: uuid::Uuid::new_v4().to_string(),
                scope: scope.clone(),
                plan: reference.clone(),
                ordinal: ordinal as u16,
                payload: base64::engine::general_purpose::STANDARD.encode(bytes),
            })
            .collect()
    }

    #[test]
    fn large_valid_plans_use_small_exact_pieces_and_need_complete_coverage() {
        let scope = scope();
        let plan = plan(true);
        let reference = plan.reference().unwrap();
        assert!(reference.bytes > 64 << 10);
        let mut cache = Cache::new(scope.clone());
        let mut input = pieces(&scope, &plan);
        for piece in &input {
            assert!(piece.valid());
            assert!(serde_json::to_vec(piece).unwrap().len() < 64 << 10);
        }
        let last = input.pop().unwrap();
        for piece in input {
            assert!(!cache.stage(piece).unwrap());
            assert!(cache.get(&reference).is_err());
        }
        assert!(cache.stage(last.clone()).unwrap());
        assert_eq!(*cache.get(&reference).unwrap(), plan);
        assert!(
            cache.stage(last).unwrap(),
            "exact data duplicates are inert"
        );
    }

    #[test]
    fn scope_content_and_request_identity_cannot_replace_an_existing_plan() {
        let scope = scope();
        let plan = plan(false);
        let mut cache = Cache::new(scope.clone());
        let original = pieces(&scope, &plan).remove(0);
        assert!(cache.stage(original.clone()).unwrap());
        let mut foreign = original.clone();
        foreign.scope.service_epoch = uuid::Uuid::new_v4().to_string();
        assert!(cache.stage(foreign).is_err());
        let mut altered = plan.clone();
        altered.action = wire::Action::Team {
            change: Box::new(TeamAction::Pause {}),
        };
        assert!(cache.stage(pieces(&scope, &altered).remove(0)).is_err());
        let mut changed = original;
        changed.payload =
            base64::engine::general_purpose::STANDARD
                .encode(vec![b'x'; changed.plan.bytes as usize]);
        assert!(cache.stage(changed).is_err());
        assert_eq!(*cache.get(&plan.reference().unwrap()).unwrap(), plan);
    }

    #[test]
    fn staged_plans_are_bounded_and_expired_data_is_never_an_original() {
        let scope = scope();
        let mut cache = Cache::new(scope.clone());
        let now = Instant::now();
        let mut first = None;
        for _ in 0..PLANS {
            let plan = plan(false);
            first.get_or_insert_with(|| plan.reference().unwrap());
            cache
                .stage_at(pieces(&scope, &plan).remove(0), now)
                .unwrap();
        }
        let next = pieces(&scope, &plan(false)).remove(0);
        assert!(cache.stage_at(next.clone(), now).is_err());
        let after = now + KEEP;
        assert!(cache.get_at(&first.unwrap(), after).is_err());
        assert!(cache.stage_at(next, after).unwrap());
        assert_eq!(cache.rows.len(), 1);
    }
}
