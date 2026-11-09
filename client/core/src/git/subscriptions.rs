//! Native-mode read interests on the already owned watch supervisor. The
//! service mode uses the existing details adapter's equivalent Pong fence.
use super::*;
use crate::CatalogState;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use zerus_watch_protocol::{topic, Body, Control, Frame, Ping, Subscription};

enum Phase {
    Fence { token: String, until: Instant },
    Active,
    Refused,
}
#[derive(Default)]
pub(crate) struct Subscriptions {
    wanted: Vec<String>,
    paths: BTreeMap<String, Phase>,
}
impl Subscriptions {
    pub fn wanted(&mut self, keys: Vec<String>) {
        self.wanted = keys;
    }
    pub fn disconnected(&mut self, state: &mut CatalogState, computer: &ComputerId) -> Vec<Change> {
        let mut changes = vec![];
        for path in std::mem::take(&mut self.paths).keys() {
            state.git_mut().unfollow(path);
            changes.push(Change::Git {
                computer: computer.clone(),
                key: path.clone(),
                update: GitUpdate::Unavailable {
                    code: "source_offline".into(),
                },
            });
        }
        changes
    }
    pub fn reconcile(
        &mut self,
        state: &mut CatalogState,
        computer: &ComputerId,
        now: Instant,
    ) -> (Vec<Control>, Vec<Change>) {
        let cap = state
            .hello()
            .filter(|h| h.topics.iter().any(|t| t == topic::GIT))
            .map_or(0, |h| h.limits.git_subscriptions.min(VISIBLE_FOLDERS));
        let mut desired = BTreeSet::new();
        if state.live() {
            for key in &self.wanted {
                if let Some(path) = state.rows().get(key).and_then(path) {
                    if desired.len() < cap {
                        desired.insert(path.to_owned());
                    }
                }
            }
        }
        let mut controls = vec![];
        let mut changes = vec![];
        let gone = self
            .paths
            .keys()
            .filter(|p| !desired.contains(*p))
            .cloned()
            .collect::<Vec<_>>();
        for path in gone {
            if !matches!(self.paths.remove(&path), Some(Phase::Refused)) {
                controls.push(Control::Unsubscribe(spec(&path)));
            }
            state.git_mut().unfollow(&path);
            changes.push(Change::Git {
                computer: computer.clone(),
                key: path,
                update: GitUpdate::Unavailable {
                    code: "git_interest_retired".into(),
                },
            });
        }
        for path in desired {
            if self.paths.contains_key(&path) {
                continue;
            }
            let token = uuid::Uuid::new_v4().to_string();
            controls.push(Control::Unsubscribe(spec(&path)));
            controls.push(Control::Ping(Ping {
                id: Some(Value::String(token.clone())),
            }));
            self.paths.insert(
                path,
                Phase::Fence {
                    token,
                    until: now + Duration::from_secs(15),
                },
            );
        }
        for (path, phase) in &mut self.paths {
            if matches!(phase, Phase::Fence { until, .. } if now >= *until) {
                *phase = Phase::Refused;
                changes.push(Change::Git {
                    computer: computer.clone(),
                    key: path.clone(),
                    update: GitUpdate::Unavailable {
                        code: "git_fence_timeout".into(),
                    },
                });
            }
        }
        (controls, changes)
    }
    pub fn frame(
        &mut self,
        state: &mut CatalogState,
        computer: &ComputerId,
        frame: &Frame,
        now: Instant,
    ) -> (Vec<Control>, Vec<Change>) {
        if matches!(frame.body, Body::Hello(_)) {
            return (vec![], self.disconnected(state, computer));
        }
        let (mut controls, changes) = self.reconcile(state, computer, now);
        if let Body::Pong(pong) = &frame.body {
            for (path, phase) in &mut self.paths {
                if matches!(phase, Phase::Fence { token, until }
                    if now < *until && pong.id.as_ref().and_then(Value::as_str) == Some(token.as_str()))
                {
                    *phase = Phase::Active;
                    state.git_mut().follow(path);
                    controls.push(Control::Subscribe(spec(path)));
                    break;
                }
            }
        }
        let refusal = match &frame.body {
            Body::Error(e) if e.topic.as_deref() == Some(topic::GIT) => Some(e.key.as_deref()),
            Body::Gap(g) if g.topic == topic::GIT => Some(g.key.as_deref()),
            Body::State(s) if s.topic == topic::GIT && GitStatus::parse(&s.value).is_none() => {
                Some(Some(s.key.as_str()))
            }
            _ => None,
        };
        if let Some(key) = refusal {
            for (path, phase) in &mut self.paths {
                if key.is_none_or(|key| key == path) && !matches!(phase, Phase::Refused) {
                    *phase = Phase::Refused;
                    state.git_mut().unfollow(path);
                    controls.push(Control::Unsubscribe(spec(path)));
                }
            }
        }
        (controls, changes)
    }
}
fn spec(path: &str) -> Subscription {
    Subscription {
        topic: topic::GIT.into(),
        path: Some(path.into()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn frame(value: Value) -> Frame {
        let mut v = value;
        v["v"] = json!(1);
        v["seq"] = json!(1);
        serde_json::from_value(v).unwrap()
    }
    fn state(cap: usize) -> CatalogState {
        let mut state = CatalogState::new();
        state.apply(&ComputerId::local(), &frame(json!({"t":"hello","stream":"fixture","host":"fixture",
            "hgs_version":"fixture","heartbeat_ms":10000,"max_frame_bytes":1048576,
            "topics":["catalog","git"],"limits":zerus_watch_protocol::Limits { git_subscriptions:cap, ..Default::default() }})));
        state.apply(
            &ComputerId::local(),
            &frame(
                json!({"t":"snapshot","topic":"catalog","page":0,"last":true,"cursor":0,"items":[
                    {"key":"run:a","row":{"name":"codex/fixture/a","cwd":"/fixture/a"}},
                    {"key":"run:b","row":{"name":"codex/fixture/b","cwd":"/fixture/b"}},
                    {"key":"run:same","row":{"name":"codex/fixture/same","cwd":"/fixture/a"}}
                ]}),
            ),
        );
        state
    }
    fn pong(controls: &[Control]) -> Frame {
        let Some(ping) = controls.iter().find_map(|c| match c {
            Control::Ping(p) => Some(p),
            _ => None,
        }) else {
            panic!("original fence");
        };
        frame(json!({"t":"pong","id":ping.id}))
    }
    #[test]
    fn current_native_paths_need_the_original_pong_and_respect_the_hello_cap() {
        let mut state = state(1);
        let mut s = Subscriptions::default();
        let computer = ComputerId::local();
        let now = Instant::now();
        s.wanted(vec!["run:a".into(), "run:same".into(), "run:b".into()]);
        let (controls, _) = s.reconcile(&mut state, &computer, now);
        assert_eq!(controls.len(), 2);
        assert!(matches!(
            &controls[..],
            [Control::Unsubscribe(_), Control::Ping(_)]
        ));
        assert!(state.git_mut().keys().next().is_none());
        let (controls, _) = s.frame(&mut state, &computer, &pong(&controls), now);
        assert!(
            matches!(&controls[..], [Control::Subscribe(spec)] if spec.path.as_deref() == Some("/fixture/a"))
        );
        assert_eq!(state.git_mut().keys().count(), 1);
        // Removing the old row precedes admission of the replacement path.
        s.wanted(vec!["run:b".into()]);
        let (controls, _) = s.reconcile(&mut state, &computer, now);
        assert!(
            matches!(&controls[..], [Control::Unsubscribe(old), Control::Unsubscribe(_), Control::Ping(_)] if old.path.as_deref() == Some("/fixture/a"))
        );
        assert!(state.git_mut().keys().next().is_none());
        let (controls, _) = s.frame(
            &mut state,
            &computer,
            &pong(&controls[1..]),
            now + Duration::from_secs(16),
        );
        assert!(controls.is_empty()); // An expired Pong never renews its deadline.
    }
    #[test]
    fn failed_or_gapped_routes_unsubscribe_once_without_an_automatic_retry() {
        for body in [
            json!({"t":"error","topic":"git","key":"/fixture/a","code":"git_status_failed","message":"fixture","fatal":false}),
            json!({"t":"gap","topic":"git","key":"/fixture/a","reason":"fixture"}),
            json!({"t":"state","topic":"git","key":"/fixture/a","rev":1,"value":{"state":"ok"}}),
        ] {
            let mut state = state(64);
            let computer = ComputerId::local();
            let now = Instant::now();
            let mut s = Subscriptions::default();
            s.wanted(vec!["run:a".into()]);
            let (ping, _) = s.reconcile(&mut state, &computer, now);
            s.frame(&mut state, &computer, &pong(&ping), now);
            let (controls, _) = s.frame(&mut state, &computer, &frame(body), now);
            assert!(matches!(&controls[..], [Control::Unsubscribe(_)]));
            assert!(state.git_mut().keys().next().is_none());
            assert!(s.reconcile(&mut state, &computer, now).0.is_empty());
            s.disconnected(&mut state, &computer);
            assert!(matches!(
                &s.reconcile(&mut state, &computer, now).0[..],
                [Control::Unsubscribe(_), Control::Ping(_)]
            ));
        }
    }
}
