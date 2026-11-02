//! Native client preparation on the closed wire types. No callable transport
//! exists here until Root's authenticated Request and Responder APIs land.
use std::time::{Duration, Instant};
use zerus_client_core::desktop::notification_control::{
    Command, FocusReply, FocusState, NativeFocus, Scope, FOCUS_REFRESH_MS,
};

const WINDOWS: usize = 64;

/// Complete fresh native app-window observations, never webview focus or JS.
/// A known focused window proves Foreground even if another query failed.
/// Empty means an actual completed enumeration with no windows, not disconnect.
pub(super) fn aggregate(windows: impl IntoIterator<Item = Option<bool>>) -> FocusState {
    let mut unknown = false;
    for (index, window) in windows.into_iter().enumerate() {
        if index == WINDOWS {
            return FocusState::Unknown;
        }
        match window {
            Some(true) => return FocusState::Foreground,
            Some(false) => (),
            None => unknown = true,
        }
    }
    if unknown {
        FocusState::Unknown
    } else {
        FocusState::Background
    }
}

/// Scheduling only; the eventual worker must independently retain/admit its
/// original child and every pipe before using this result. This owns no native
/// cache revision: only the Responder may produce/invalidate that evidence.
#[derive(Default)]
pub(super) struct Schedule {
    last: Option<(NativeFocus, Instant)>,
    uncertain: bool,
}
impl Schedule {
    pub fn due(&self, current: &NativeFocus, now: Instant) -> bool {
        if !current.valid() {
            return false;
        }
        let Some((last, at)) = &self.last else {
            return true;
        };
        if current.connection_id != last.connection_id {
            return true;
        }
        if current.revision < last.revision
            || (current.revision == last.revision && current.state != last.state)
        {
            return false;
        }
        if (current.state == FocusState::Foreground && last.state == FocusState::Foreground)
            || (self.uncertain && current == last)
        {
            return now.saturating_duration_since(*at) >= Duration::from_millis(FOCUS_REFRESH_MS);
        }
        current != last
    }
    pub fn attempted(&mut self, observation: NativeFocus, now: Instant) {
        self.last = Some((observation, now));
        self.uncertain = false;
    }
    /// A lost reply needs a fresh observation/UUID, never replay or lookup.
    /// This changes timing only; it does not release the original resource gate.
    pub fn lost_reply(&mut self) {
        self.uncertain = true;
    }
    pub fn disconnected(&mut self) {
        self.last = None;
        self.uncertain = false;
    }
}

/// Echoed UUID/scope alone cannot bind a reply to the actual attached native
/// observation. This check supplies integrity only, never ownership/authority.
pub(super) fn reply_matches(
    reply: &FocusReply,
    command: &Command,
    scope: &Scope,
    attached: &NativeFocus,
) -> bool {
    reply.matches(command, scope) && &reply.observation == attached
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerus_client_core::desktop::notification_control::ABI;

    fn observation(state: FocusState, revision: u64) -> NativeFocus {
        NativeFocus {
            connection_id: "00000000-0000-4000-8000-000000000001".into(),
            revision,
            state,
        }
    }

    #[test]
    fn another_focused_native_window_keeps_app_foreground_and_query_failure_is_unknown() {
        assert_eq!(aggregate([Some(false), Some(true)]), FocusState::Foreground);
        assert_eq!(aggregate([None, Some(true)]), FocusState::Foreground);
        assert_eq!(aggregate([Some(false), None]), FocusState::Unknown);
        assert_eq!(
            aggregate([Some(false), Some(false)]),
            FocusState::Background
        );
        assert_eq!(aggregate([]), FocusState::Background);
        assert_eq!(
            aggregate(std::iter::repeat_n(Some(false), WINDOWS + 1)),
            FocusState::Unknown
        );
    }

    #[test]
    fn blur_and_unknown_bypass_renewal_delay_and_new_connections_do_not_inherit_timing() {
        let now = Instant::now();
        let mut schedule = Schedule::default();
        let front = observation(FocusState::Foreground, 1);
        assert!(schedule.due(&front, now));
        schedule.attempted(front.clone(), now);
        assert!(!schedule.due(&front, now + Duration::from_millis(1999)));
        assert!(schedule.due(&front, now + Duration::from_millis(2000)));
        let background = observation(FocusState::Background, 2);
        assert!(schedule.due(&background, now));
        schedule.attempted(background.clone(), now);
        assert!(!schedule.due(&background, now));
        schedule.lost_reply();
        assert!(!schedule.due(&background, now + Duration::from_millis(1999)));
        assert!(schedule.due(&background, now + Duration::from_millis(2000)));
        let unknown = observation(FocusState::Unknown, 3);
        assert!(schedule.due(&unknown, now));
        schedule.attempted(unknown, now);
        assert!(!schedule.due(&front, now + Duration::from_secs(3)));
        let mut replaced = observation(FocusState::Unknown, 0);
        replaced.connection_id = "00000000-0000-4000-8000-000000000002".into();
        assert!(schedule.due(&replaced, now));
        schedule.disconnected();
        assert!(schedule.due(&replaced, now));
        assert!(!schedule.due(&observation(FocusState::Foreground, 0), now));
    }

    #[test]
    fn reply_must_bind_the_exact_native_evidence_in_addition_to_its_uuid_and_scope() {
        let scope = Scope {
            configuration: "a".repeat(64),
            service_epoch: uuid::Uuid::new_v4().to_string(),
        };
        let command = Command::Focus {
            request_id: uuid::Uuid::new_v4().to_string(),
        };
        let attached = observation(FocusState::Foreground, 1);
        let mut reply = FocusReply {
            abi: ABI.into(),
            request_id: command.request_id().into(),
            scope: scope.clone(),
            observation: attached.clone(),
            ttl_ms: 5000,
        };
        assert!(reply_matches(&reply, &command, &scope, &attached));
        reply.observation.revision += 1;
        assert!(reply.matches(&command, &scope));
        assert!(!reply_matches(&reply, &command, &scope, &attached));
        reply.observation = attached.clone();
        reply.observation.connection_id = uuid::Uuid::new_v4().to_string();
        assert!(!reply_matches(&reply, &command, &scope, &attached));
        reply.observation = attached.clone();
        reply.ttl_ms = 5001;
        assert!(!reply_matches(&reply, &command, &scope, &attached));
    }
}
