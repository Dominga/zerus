//! A requester-bound native READ. This never becomes an issued Grant receipt.
use super::*;
use zerus_client_core::desktop::ownership::observation::{self as wire, Query};

type Resources = Arc<dyn Send + Sync>;

/// No Clone/Deserialize/public constructor. Only this module's checked native
/// original exchange constructs an observation; it cannot satisfy granted().
pub(crate) struct NativeCurrent {
    observation: wire::Observation,
    cancel: Arc<AtomicBool>,
    #[cfg(not(windows))]
    helper: PathBuf,
    #[cfg(windows)]
    image_id: zerus_client_core::desktop::windows::image::FileId,
}
impl NativeCurrent {
    pub(crate) fn status(&self) -> &Status {
        &self.observation.current
    }
    #[cfg(not(windows))]
    pub(crate) fn helper(&self) -> &Path {
        &self.helper
    }
    #[cfg(windows)]
    pub(crate) fn image_id(&self) -> zerus_client_core::desktop::windows::image::FileId {
        self.image_id
    }
    pub(crate) fn matches(&self, status: &Status, epoch: &str, generation: u64) -> bool {
        !self.cancel.load(Ordering::Acquire)
            && self.observation.query.matches(status, epoch, generation)
    }
}

impl Controller {
    /// Blocking native-worker read only. The mode reader supplies its SAME
    /// retained route/image resources and rechecks its historical target on
    /// both sides; no intent, receipt, grant or positive startup flag is made.
    pub(crate) fn observe_current(
        self: &Arc<Self>,
        query: Query,
        cancel: Arc<AtomicBool>,
        resources: Resources,
    ) -> Result<NativeCurrent, &'static str> {
        if self.mode != Mode::Service || !query.valid() {
            return Err("ownership_observation_unavailable");
        }
        let mut job = self
            .admit_with(
                None,
                Some(query.configuration.clone()),
                Some(cancel.clone()),
                |s| {
                    (!s.intent.as_ref().is_some_and(|i| !i.complete) && s.retired_native.is_none())
                        .then_some(None)
                },
            )
            .ok_or("ownership_observation_unavailable")?;
        job.deadline = Some(Instant::now() + Duration::from_secs(3));
        let route = Arc::new(relay::Connection::select(&self.hgs)?);
        let helper = route.executable()?.to_owned();
        #[cfg(windows)]
        let image = Arc::new(
            zerus_client_core::desktop::windows::image::PinnedImage::open(&helper)
                .map_err(|_| "ownership_identity_unavailable")?,
        );
        #[cfg(windows)]
        let image_id = image.id();
        // The help probe is also an original read. Its incomplete child/pipe
        // lane must retain the parent's mode charge and image pins too.
        job.resources = Arc::new((
            job.resources.clone(),
            resources.clone(),
            #[cfg(windows)]
            image.clone(),
        ));
        let help = job.call(&route, &["--help"], vec![])?;
        if !std::str::from_utf8(&help).ok().is_some_and(|s| {
            [wire::HELP_MARKER, ui_requests::HELP_MARKER]
                .iter()
                .all(|m| s.lines().any(|line| line.trim() == *m))
        }) {
            return Err("ownership_observation_unsupported");
        }
        if job.stopped() {
            return Err("ownership_stopped");
        }
        let responder = self
            .responder
            .as_ref()
            .ok_or("ownership_identity_unavailable")?;
        let registration = responder
            .prepare_ui_request(
                uuid::Uuid::new_v4().to_string(),
                Request::DesktopOwnershipObserve {
                    query: Box::new(query.clone()),
                },
            )
            .map_err(|_| "ownership_identity_unavailable")?;
        let input = serde_json::to_vec(registration.envelope())
            .map_err(|_| "invalid_ownership_observation")?;
        let held: Resources = Arc::new((
            resources,
            job.resources.clone(),
            self.clone(),
            route.clone(),
            registration.resources(),
            #[cfg(windows)]
            image.clone(),
        ));
        let mut call = invocation(&["desktop", "frontend-request", "--json"], input);
        job.bound(&mut call)?;
        let bound = Arc::new(AtomicBool::new(false));
        let original_bound = bound.clone();
        #[cfg(target_os = "macos")]
        let original_route = route.clone();
        let result = run_owned_observed(
            &helper,
            &call,
            &[],
            &cancel,
            move |command| {
                let mut child = command.spawn()?;
                #[cfg(windows)]
                let image_current = image.matches_process(&child).is_ok();
                #[cfg(not(windows))]
                let image_current = true;
                #[cfg(target_os = "macos")]
                let relay_current = original_route.observe_original_helper(&mut child).is_ok();
                #[cfg(not(target_os = "macos"))]
                let relay_current = true;
                if image_current && relay_current && registration.bind_original(&child) {
                    original_bound.store(true, Ordering::Release);
                } else {
                    drop(child.stdin.take());
                }
                Ok(child) // Never release a rejected original or its pipes at spawn.
            },
            held,
            |error, stderr| job.failure("ownership.observe", error, stderr),
        )
        .map_err(|_| "ownership_observation_unavailable")?;
        route.executable()?;
        if job.stopped() || !bound.load(Ordering::Acquire) || result.len() > super::wire::BYTES {
            return Err("ownership_observation_unavailable");
        }
        let observation: wire::Observation =
            serde_json::from_slice(&result).map_err(|_| "invalid_ownership_observation")?;
        if !observation.matches(&query) {
            return Err("invalid_ownership_observation");
        }
        Ok(NativeCurrent {
            observation,
            cancel,
            #[cfg(not(windows))]
            helper,
            #[cfg(windows)]
            image_id,
        })
    }
}
