//! Select the desktop service CLI separately from the ordinary agent runner.
//! A returned path routes a command; it is not frontend or notification authority.
use std::path::{Path, PathBuf};

#[cfg(any(test, target_os = "macos"))]
const UNAVAILABLE: &str = "paired_desktop_cli_unavailable";

/// Closed observations of the same original helper; none is permission or a
/// reason to replay. No PID, code detail, path or native error text is exported.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalHelperFailure {
    Unavailable,
    ChildExited,
    #[cfg(feature = "macos-native-peer")]
    Native(crate::macos_notify::peer::installation::Refusal),
}

/// DATA from this connection's genuine own-original-child observation. It is
/// not a lease, a stop permit or a substitute for fresh kernel peer evidence.
/// There is no public constructor or deserializer.
#[cfg(target_os = "macos")]
pub struct OriginalHelperIdentity {
    pid: u32,
    parent: u32,
    birth: String,
    audit: [u32; 8],
}
#[cfg(target_os = "macos")]
impl OriginalHelperIdentity {
    pub fn parent(&self) -> u32 {
        self.parent
    }
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn birth(&self) -> &str {
        &self.birth
    }
    pub fn audit_token(&self) -> &[u32; 8] {
        &self.audit
    }
}
#[cfg(all(target_os = "macos", feature = "macos-native-peer"))]
impl From<crate::macos_notify::peer::installation::Refusal> for OriginalHelperFailure {
    fn from(error: crate::macos_notify::peer::installation::Refusal) -> Self {
        match error {
            crate::macos_notify::peer::installation::Refusal::ChildExited => Self::ChildExited,
            error => Self::Native(error),
        }
    }
}
#[cfg(target_os = "macos")]
impl OriginalHelperFailure {
    /// Unit-only native refusals project to fixed text; no SDK message or
    /// serialized identity crosses into caller error details.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "original_child_unavailable",
            Self::ChildExited => "original_child_exited",
            #[cfg(feature = "macos-native-peer")]
            Self::Native(error) => {
                use crate::macos_notify::peer::{installation::Refusal as R, Refusal as P};
                match error {
                    R::UnsupportedLayout => "original_child_layout",
                    R::Metadata => "original_child_metadata",
                    R::Signature => "original_child_signature",
                    R::SelfChanged => "original_child_self_changed",
                    R::GenerationChanged => "original_child_generation_changed",
                    R::Child => "original_child_identity",
                    R::ChildExited => "original_child_exited",
                    R::Peer(P::RequirementSize) => "original_child_requirement_size",
                    R::Peer(P::RequirementNul) => "original_child_requirement_nul",
                    R::Peer(P::RequirementInvalid) => "original_child_requirement_invalid",
                    R::Peer(P::AuditUnavailable) => "original_child_audit_unavailable",
                    R::Peer(P::AuditMalformed) => "original_child_audit_malformed",
                    R::Peer(P::LiveCodeUnavailable) => "original_child_code_unavailable",
                    R::Peer(P::LiveCodeRejected) => "original_child_code_rejected",
                    R::Peer(P::PeerChanged) => "original_child_peer_changed",
                }
            }
        }
    }
}

/// Off the UI thread. A paired Mac app uses only its verified own bundled hgs;
/// a broken purported pair stays unavailable, never falling through to PATH or
/// an explicit ordinary-CLI override. Source/unpaired and other platforms retain
/// the caller's ordinary selection. Recheck for each new service exchange.
pub fn select(ordinary: &Path) -> Result<PathBuf, &'static str> {
    let connection = Connection::select(ordinary)?;
    connection.executable().map(Path::to_owned)
}

/// One service connection's CLI routing snapshot, never an effect permission.
/// The expensive native signature/seal verification runs once at construction.
/// Later pages compare the fixed generation metadata and refuse any drift.
pub struct Connection {
    executable: PathBuf,
    #[cfg(all(target_os = "macos", feature = "macos-native-peer"))]
    native: Option<crate::macos_notify::peer::installation::Selection>,
}
impl Connection {
    /// Route only the helper from this retained genuine Mac selection. An
    /// ordinary/unpaired connection has no original-helper route. This is
    /// routing freshness, not live child evidence or effect permission: call
    /// observe_original_helper on EACH original child before private input.
    #[cfg(target_os = "macos")]
    pub fn original_helper_executable(&self) -> Result<&Path, &'static str> {
        #[cfg(feature = "macos-native-peer")]
        {
            self.native
                .as_ref()
                .ok_or(UNAVAILABLE)?
                .service_executable_if_unchanged()
                .map_err(|_| UNAVAILABLE)
        }
        #[cfg(not(feature = "macos-native-peer"))]
        Err(UNAVAILABLE)
    }

    /// Mac own-original-child observation only, before taking/writing stdin.
    /// No caller-supplied PID, path, signature or reusable permission. Ordinary
    /// routing has no native selection and refuses this operation. On failure
    /// input is closed; the SAME Child stays with its caller for bounded
    /// stop/reap/pipe custody. A success is point-in-time, not an exec fence or
    /// G1 effect admission. Retain actual resources past any outer timeout.
    #[cfg(target_os = "macos")]
    pub fn observe_original_helper(
        &self,
        child: &mut std::process::Child,
    ) -> Result<(), &'static str> {
        self.observe_original_helper_diagnostic(child)
            .map_err(|_| UNAVAILABLE)
    }

    /// The same check/close-on-refusal, with its closed failure category kept
    /// for the original caller's diagnostic. Performs no extra observation.
    #[cfg(target_os = "macos")]
    pub fn observe_original_helper_diagnostic(
        &self,
        child: &mut std::process::Child,
    ) -> Result<(), OriginalHelperFailure> {
        #[cfg(feature = "macos-native-peer")]
        let result = self
            .native
            .as_ref()
            .ok_or(OriginalHelperFailure::Unavailable)
            .and_then(|native| {
                native
                    .observe_original_helper(child)
                    .map_err(OriginalHelperFailure::from)
            });
        #[cfg(not(feature = "macos-native-peer"))]
        let result = Err(OriginalHelperFailure::Unavailable);
        if result.is_err() {
            drop(child.stdin.take());
        }
        result
    }
    /// Preserve the exact kernel identity observed alongside live code, for
    /// binding the original helper to a later host-local custody socket. The
    /// consumer must freshly verify that socket's token and current exec
    /// generation. This DATA cannot reconstruct consent or child custody.
    #[cfg(target_os = "macos")]
    pub fn observe_original_helper_identity(
        &self,
        child: &mut std::process::Child,
    ) -> Result<OriginalHelperIdentity, OriginalHelperFailure> {
        #[cfg(feature = "macos-native-peer")]
        let result = self
            .native
            .as_ref()
            .ok_or(OriginalHelperFailure::Unavailable)
            .and_then(|native| {
                native
                    .observe_original_helper_identity(child)
                    .map_err(OriginalHelperFailure::from)
            })
            .map(|identity| OriginalHelperIdentity {
                pid: identity.pid(),
                parent: identity.parent(),
                birth: identity.birth().into(),
                audit: *identity.audit_token(),
            });
        #[cfg(not(feature = "macos-native-peer"))]
        let result = Err(OriginalHelperFailure::Unavailable);
        if result.is_err() {
            drop(child.stdin.take());
        }
        result
    }
    pub fn select(ordinary: &Path) -> Result<Self, &'static str> {
        #[cfg(target_os = "macos")]
        {
            let executable = std::env::current_exe().map_err(|_| UNAVAILABLE)?;
            #[cfg(feature = "macos-native-peer")]
            let mut native = None;
            let selected = select_mac(&executable, ordinary, || {
                #[cfg(feature = "macos-native-peer")]
                {
                    let selection = crate::macos_notify::peer::installation::select_own()
                        .map_err(|_| UNAVAILABLE)?;
                    let path = selection.service_executable().to_owned();
                    native = Some(selection);
                    Ok(path)
                }
                #[cfg(not(feature = "macos-native-peer"))]
                Err(UNAVAILABLE)
            })?;
            Ok(Self {
                executable: selected,
                #[cfg(feature = "macos-native-peer")]
                native,
            })
        }
        #[cfg(not(target_os = "macos"))]
        Ok(Self {
            executable: ordinary.into(),
        })
    }
    /// Off the UI thread. Cheap routing freshness only: this cannot replace
    /// the host factory's full native revalidation or peer/child checks.
    pub fn executable(&self) -> Result<&Path, &'static str> {
        #[cfg(all(target_os = "macos", feature = "macos-native-peer"))]
        if let Some(native) = &self.native {
            return native
                .service_executable_if_unchanged()
                .map_err(|_| UNAVAILABLE);
        }
        Ok(&self.executable)
    }
}

#[cfg(any(test, target_os = "macos"))]
fn select_mac(
    executable: &Path,
    ordinary: &Path,
    verify_own: impl FnOnce() -> Result<PathBuf, &'static str>,
) -> Result<PathBuf, &'static str> {
    let fallback = || Ok(ordinary.to_owned());
    // This shape/presence check distinguishes legacy unpaired source installs.
    // It establishes NO trust; only select_own may select a purported pair.
    let Some(macos) = executable.parent() else {
        return fallback();
    };
    let Some(contents) = macos.parent() else {
        return fallback();
    };
    let Some(bundle) = contents.parent() else {
        return fallback();
    };
    if !executable.is_absolute()
        || executable.file_name().is_none_or(|v| v != "hgs-tray")
        || macos.file_name().is_none_or(|v| v != "MacOS")
        || contents.file_name().is_none_or(|v| v != "Contents")
        || bundle.extension().is_none_or(|v| v != "app")
    {
        return fallback();
    }
    let candidate = macos.join("hgs");
    match std::fs::symlink_metadata(&candidate) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A local pair whose helper disappeared must not become a legacy
            // install. Presence only triggers verification, never supplies trust.
            let record = contents.join("Resources/zerus-local-pair.json");
            match std::fs::symlink_metadata(record) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return fallback(),
                Err(_) => return Err(UNAVAILABLE),
                Ok(_) => {}
            }
        }
        Err(_) => return Err(UNAVAILABLE),
        Ok(_) => {}
    }
    let verified = verify_own()?;
    if verified != candidate {
        return Err(UNAVAILABLE);
    }
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_routing_requires_native_own_selection_and_ignores_the_ordinary_override() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zerus-relay-{}-{nonce}", std::process::id()));
        let code = root.join("Example.app/Contents/MacOS/hgs-tray");
        std::fs::create_dir_all(code.parent().unwrap()).unwrap();
        let bundled = code.with_file_name("hgs");
        let ordinary = root.join("stable/hgs");
        // No hgs: legacy source/unpaired app keeps the existing resolver.
        assert_eq!(
            select_mac(&code, &ordinary, || panic!("unpaired")),
            Ok(ordinary.clone())
        );
        std::fs::write(
            &bundled,
            b"synthetic code; native verification is MODELLED below",
        )
        .unwrap();
        assert_eq!(
            select_mac(&code, &ordinary, || Ok(bundled.clone())),
            Ok(bundled.clone())
        );
        assert_eq!(
            select_mac(&code, &ordinary, || Err(UNAVAILABLE)),
            Err(UNAVAILABLE)
        );
        assert_eq!(
            select_mac(&code, &ordinary, || Ok(ordinary.clone())),
            Err(UNAVAILABLE)
        );
        std::fs::remove_file(&bundled).unwrap();
        let record = code
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Resources/zerus-local-pair.json");
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(record, b"synthetic, not authority").unwrap();
        assert_eq!(
            select_mac(&code, &ordinary, || Err(UNAVAILABLE)),
            Err(UNAVAILABLE)
        );
        assert_eq!(
            select_mac(&root.join("target/hgs-tray"), &ordinary, || panic!(
                "source"
            )),
            Ok(ordinary.clone())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn other_platforms_keep_the_callers_exact_selection() {
        assert_eq!(
            select(Path::new("explicit-fixture-hgs")),
            Ok("explicit-fixture-hgs".into())
        );
    }
}
