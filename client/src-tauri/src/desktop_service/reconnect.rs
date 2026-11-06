//! Only read transport failures retry automatically. Compatibility and native
//! authority refusals keep their existing explicit recovery paths.
use super::*;

#[derive(Default)]
pub(super) struct Backoff(usize);

impl Backoff {
    pub fn reset(&mut self) {
        self.0 = 0;
    }
    pub fn next(&mut self, code: &str, healthy: bool) -> Option<Duration> {
        if !matches!(
            code,
            "service_unavailable"
                | "service_request_unconfirmed"
                | "service_stale"
                | "service_resnapshot_required"
                | "service_epoch_changed"
                | "service_transport_busy"
                | "catalog_unavailable"
                | "details_unavailable"
        ) {
            return None;
        }
        if healthy {
            self.reset();
        }
        const MILLIS: [u64; 5] = [250, 500, 1000, 2000, 5000];
        let delay = Duration::from_millis(MILLIS[self.0]);
        self.0 = (self.0 + 1).min(MILLIS.len() - 1);
        Some(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_to_a_cap_and_reset_only_after_a_healthy_connection() {
        let mut backoff = Backoff::default();
        for millis in [250, 500, 1000, 2000, 5000, 5000, 5000] {
            assert_eq!(
                backoff.next("service_unavailable", false),
                Some(Duration::from_millis(millis))
            );
        }
        assert_eq!(
            backoff.next("service_stale", true),
            Some(Duration::from_millis(250))
        );
        backoff.reset();
        assert_eq!(
            backoff.next("service_request_unconfirmed", false),
            Some(Duration::from_millis(250))
        );
    }

    #[test]
    fn compatibility_authority_and_unverified_custody_never_auto_retry() {
        for code in [
            "incompatible_service_cli",
            "incompatible_service",
            "invalid_service_frame",
            "replay_transport_unavailable",
            "ownership_rearm_required",
            "manual_refresh_required",
            "original_service_changed",
            "service_transport_unverified",
            "stopped",
        ] {
            assert!(Backoff::default().next(code, false).is_none(), "{code}");
        }
    }
}
