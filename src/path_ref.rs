//! Versioned lexical path identity. This module performs no filesystem access.
//!
//! A canonical spelling is not evidence of file ownership, containment after
//! symlink resolution, or permission to access a file. Native I/O stays native.

use anyhow::{bail, ensure, Context, Result};
use canonicalpath_rust as cp;
use serde::{Deserialize, Serialize};
use std::io::{self, Read};

const SCHEMA_VERSION: u32 = 1;
const MAX_REQUEST_BYTES: u64 = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathFlavor {
    Windows,
    Posix,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PathRef {
    pub schema_version: u32,
    pub machine_id: String,
    pub environment_id: String,
    pub path_flavor: PathFlavor,
    pub native_path: String,
    pub canonical_path: String,
    pub workspace_id: Option<String>,
    pub mapping_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathInput {
    pub machine_id: String,
    pub environment_id: String,
    pub path_flavor: PathFlavor,
    pub native_path: String,
    pub workspace_id: Option<String>,
}

/// An owner-selected drive-mount alias for one workspace and one WSL user.
/// Merely recognizing `/mnt/c` never creates this association.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WslMapping {
    pub mapping_id: String,
    pub workspace_id: String,
    pub machine_id: String,
    pub windows_environment_id: String,
    pub wsl_environment_id: String,
    pub distribution: String,
    pub user: String,
    pub mount_root: String,
    pub windows_root: String,
}

impl PathRef {
    pub fn describe(input: PathInput, mapping: Option<&WslMapping>) -> Result<Self> {
        input.validate()?;
        let canonical_path = match mapping {
            Some(mapping) => mapping.canonical_for(&input)?,
            None => native_identity(&input.native_path, input.path_flavor)?,
        };
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            machine_id: input.machine_id,
            environment_id: input.environment_id,
            path_flavor: input.path_flavor,
            native_path: input.native_path,
            canonical_path,
            workspace_id: input.workspace_id,
            mapping_id: mapping.map(|mapping| mapping.mapping_id.clone()),
        })
    }

    /// Recompute identity; do not trust the canonical value in serialized input.
    pub fn validate(&self, mapping: Option<&WslMapping>) -> Result<()> {
        ensure!(
            self.schema_version == SCHEMA_VERSION,
            "unsupported PathRef schema"
        );
        ensure!(
            self.mapping_id.as_deref() == mapping.map(|m| m.mapping_id.as_str()),
            "PathRef mapping does not match the supplied mapping"
        );
        let expected = Self::describe(self.input(), mapping)?;
        ensure!(
            self == &expected,
            "PathRef canonical identity does not match its native path"
        );
        Ok(())
    }

    fn input(&self) -> PathInput {
        PathInput {
            machine_id: self.machine_id.clone(),
            environment_id: self.environment_id.clone(),
            path_flavor: self.path_flavor,
            native_path: self.native_path.clone(),
            workspace_id: self.workspace_id.clone(),
        }
    }
}

impl PathInput {
    fn validate(&self) -> Result<()> {
        identifier(&self.machine_id, "machine_id")?;
        identifier(&self.environment_id, "environment_id")?;
        if let Some(workspace) = &self.workspace_id {
            identifier(workspace, "workspace_id")?;
        }
        Ok(())
    }
}

impl WslMapping {
    fn validate(&self) -> Result<String> {
        for (name, value) in [
            ("mapping_id", &self.mapping_id),
            ("workspace_id", &self.workspace_id),
            ("machine_id", &self.machine_id),
            ("windows_environment_id", &self.windows_environment_id),
            ("wsl_environment_id", &self.wsl_environment_id),
            ("distribution", &self.distribution),
            ("user", &self.user),
        ] {
            identifier(value, name)?;
        }
        ensure!(
            !self.distribution.contains([':', '/', '\\']) && !self.user.contains([':', '/', '\\']),
            "distribution and user must not contain namespace separators"
        );
        ensure!(
            self.wsl_environment_id == format!("wsl:{}:{}", self.distribution, self.user),
            "wsl_environment_id must be wsl:<distribution>:<user>"
        );
        ensure!(
            self.windows_environment_id != self.wsl_environment_id,
            "Windows and WSL environment identities must differ"
        );
        ensure!(!self.mount_root.is_empty(), "mount_root must be explicit");
        let root = native_identity(&self.windows_root, PathFlavor::Windows)?;
        // to_wsl validates the mount configuration and rejects UNC/device roots.
        cp::to_wsl(&root, &self.options())?;
        Ok(root)
    }

    fn options(&self) -> cp::WslOptions {
        cp::WslOptions {
            enabled: true,
            mount_root: self.mount_root.clone(),
        }
    }

    fn canonical_for(&self, input: &PathInput) -> Result<String> {
        let root = self.validate()?;
        ensure!(
            input.machine_id == self.machine_id,
            "mapping belongs to another machine"
        );
        ensure!(
            input.workspace_id.as_deref() == Some(self.workspace_id.as_str()),
            "mapping belongs to another workspace"
        );
        let canonical = if input.environment_id == self.windows_environment_id {
            ensure!(
                input.path_flavor == PathFlavor::Windows,
                "Windows mapping requires Windows grammar"
            );
            native_identity(&input.native_path, PathFlavor::Windows)?
        } else if input.environment_id == self.wsl_environment_id {
            ensure!(
                input.path_flavor == PathFlavor::Posix,
                "WSL mapping requires POSIX grammar"
            );
            // Parse native POSIX first, retaining literal backslashes. Only this
            // explicit alias is allowed to reinterpret the configured drive mount.
            let posix = native_identity(&input.native_path, PathFlavor::Posix)?;
            let canonical = cp::normalize_with_options(
                &posix,
                &cp::NormalizeOptions {
                    wsl: self.options(),
                    trim_outer_whitespace: false,
                    ..Default::default()
                },
            )?;
            ensure!(
                is_drive_root(&canonical),
                "WSL path is not under a configured drive mount"
            );
            cp::to_win32(&canonical).context("WSL path has no lossless Windows representation")?;
            ensure!(
                cp::to_wsl(&canonical, &self.options())? == posix,
                "WSL path does not round-trip through the configured drive mount"
            );
            canonical
        } else {
            bail!("mapping belongs to another environment");
        };
        cp::relative(&root, &canonical).context("path is outside the mapped workspace")?;
        Ok(canonical)
    }

    /// Map only a verified reference from the exact source namespace. This does
    /// not probe mounts, create directories, or authorize filesystem access.
    pub fn map(&self, path: &PathRef) -> Result<PathRef> {
        let supplied = path.mapping_id.as_ref().map(|_| self);
        path.validate(supplied)?;
        let canonical = self.canonical_for(&path.input())?;
        let (environment_id, path_flavor, native_path) =
            if path.environment_id == self.windows_environment_id {
                (
                    self.wsl_environment_id.clone(),
                    PathFlavor::Posix,
                    cp::to_wsl(&canonical, &self.options())?,
                )
            } else {
                (
                    self.windows_environment_id.clone(),
                    PathFlavor::Windows,
                    cp::to_win32(&canonical)?,
                )
            };
        PathRef::describe(
            PathInput {
                machine_id: self.machine_id.clone(),
                environment_id,
                path_flavor,
                native_path,
                workspace_id: Some(self.workspace_id.clone()),
            },
            Some(self),
        )
    }
}

fn identifier(value: &str, name: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "{name} must be a nonempty identifier of at most 256 bytes without control characters"
    );
    Ok(())
}

fn is_drive_root(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":/"
}

fn native_identity(raw: &str, flavor: PathFlavor) -> Result<String> {
    let canonical = cp::normalize_with_options(
        raw,
        &cp::NormalizeOptions {
            source_host: match flavor {
                PathFlavor::Windows => "windows",
                PathFlavor::Posix => "posix",
            }
            .to_owned(),
            // Never trim a real filename or decode percent escapes implicitly.
            trim_outer_whitespace: false,
            ..Default::default()
        },
    )?;
    match flavor {
        PathFlavor::Windows => {
            ensure!(
                is_drive_root(&canonical) || canonical.starts_with("//"),
                "Windows PathRef requires an absolute drive or UNC path"
            );
            cp::to_win32(&canonical)?;
        }
        PathFlavor::Posix => {
            ensure!(
                canonical.starts_with('/') && !canonical.starts_with("//"),
                "POSIX PathRef requires an absolute native path"
            );
        }
    }
    Ok(canonical)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DescribeRequest {
    path: PathInput,
    mapping: Option<WslMapping>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceRequest {
    path: PathRef,
    mapping: Option<WslMapping>,
}

pub fn run(args: &[String]) -> Result<()> {
    ensure!(
        args.len() == 1 && matches!(args[0].as_str(), "describe" | "validate" | "map"),
        "usage: hgs path-ref <describe|validate|map> < request.json (see docs/path-identity.md)"
    );
    let mut input = Vec::new();
    io::stdin()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut input)?;
    ensure!(
        input.len() as u64 <= MAX_REQUEST_BYTES,
        "path-ref request exceeds 64 KiB"
    );
    let path = match args[0].as_str() {
        "describe" => {
            let request: DescribeRequest = serde_json::from_slice(&input)?;
            PathRef::describe(request.path, request.mapping.as_ref())?
        }
        "validate" => {
            let request: ReferenceRequest = serde_json::from_slice(&input)?;
            request.path.validate(request.mapping.as_ref())?;
            request.path
        }
        "map" => {
            let request: ReferenceRequest = serde_json::from_slice(&input)?;
            request
                .mapping
                .context("map requires an explicit mapping")?
                .map(&request.path)?
        }
        _ => unreachable!(),
    };
    println!("{}", serde_json::to_string_pretty(&path)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(flavor: PathFlavor, path: &str) -> PathInput {
        PathInput {
            machine_id: "workstation-a".into(),
            environment_id: match flavor {
                PathFlavor::Windows => "windows-native",
                PathFlavor::Posix => "wsl:Ubuntu-24.04:alice",
            }
            .into(),
            path_flavor: flavor,
            native_path: path.into(),
            workspace_id: Some("orbit".into()),
        }
    }

    fn mapping() -> WslMapping {
        WslMapping {
            mapping_id: "orbit-drive".into(),
            workspace_id: "orbit".into(),
            machine_id: "workstation-a".into(),
            windows_environment_id: "windows-native".into(),
            wsl_environment_id: "wsl:Ubuntu-24.04:alice".into(),
            distribution: "Ubuntu-24.04".into(),
            user: "alice".into(),
            mount_root: "/mnt".into(),
            windows_root: r"C:\Projects\Orbit".into(),
        }
    }

    #[test]
    fn explicit_grammars_preserve_case_unicode_spaces_and_percent() {
        let path = PathRef::describe(
            input(PathFlavor::Windows, r"C:\Projects\Orbit\..\Orbit\Мир %20\"),
            None,
        )
        .unwrap();
        assert_eq!(path.canonical_path, "c:/Projects/Orbit/Мир %20");
        let posix =
            PathRef::describe(input(PathFlavor::Posix, "/home/alice/space /a\\b"), None).unwrap();
        assert_eq!(posix.canonical_path, "/home/alice/space /a\\b");
        assert_eq!(posix.native_path, "/home/alice/space /a\\b");
    }

    #[test]
    fn mount_syntax_does_not_imply_a_windows_identity() {
        let posix =
            PathRef::describe(input(PathFlavor::Posix, "/mnt/c/Projects/Orbit"), None).unwrap();
        assert_eq!(posix.canonical_path, "/mnt/c/Projects/Orbit");
        assert_eq!(posix.mapping_id, None);
    }

    #[test]
    fn mapping_preserves_trailing_posix_spaces_and_requires_mount_case_roundtrip() {
        let map = mapping();
        let raw = "/mnt/c/Projects/Orbit/final space ";
        let path = PathRef::describe(input(PathFlavor::Posix, raw), Some(&map)).unwrap();
        assert_eq!(path.canonical_path, "c:/Projects/Orbit/final space ");
        let windows = map.map(&path).unwrap();
        assert_eq!(windows.native_path, "C:\\Projects\\Orbit\\final space ");
        assert_eq!(map.map(&windows).unwrap().native_path, raw);
        assert!(PathRef::describe(
            input(PathFlavor::Posix, "/mnt/C/Projects/Orbit"),
            Some(&map)
        )
        .is_err());
    }

    #[test]
    fn drive_mapping_round_trips_with_explicit_namespace_and_workspace() {
        let map = mapping();
        let windows = PathRef::describe(
            input(PathFlavor::Windows, r"C:\Projects\Orbit\My Game\世界"),
            None,
        )
        .unwrap();
        let wsl = map.map(&windows).unwrap();
        assert_eq!(wsl.native_path, "/mnt/c/Projects/Orbit/My Game/世界");
        assert_eq!(wsl.canonical_path, windows.canonical_path);
        assert_eq!(wsl.environment_id, "wsl:Ubuntu-24.04:alice");
        let restored = map.map(&wsl).unwrap();
        assert_eq!(restored.native_path, windows.native_path);
        assert_eq!(restored.canonical_path, windows.canonical_path);
        restored.validate(Some(&map)).unwrap();
    }

    #[test]
    fn custom_mounts_and_root_mount_are_explicit() {
        for (mount, native) in [
            ("/drives", "/drives/c/Projects/Orbit"),
            ("/", "/c/Projects/Orbit"),
        ] {
            let mut map = mapping();
            map.mount_root = mount.into();
            let path = PathRef::describe(input(PathFlavor::Posix, native), Some(&map)).unwrap();
            assert_eq!(path.canonical_path, "c:/Projects/Orbit");
        }
        for mount in ["relative", "//host", "C:/", "", "/bad\0root"] {
            let mut map = mapping();
            map.mount_root = mount.into();
            assert!(map.validate().is_err(), "{mount:?}");
        }
    }

    #[test]
    fn namespaces_workspaces_and_distro_users_cannot_be_silently_merged() {
        let map = mapping();
        let base = input(PathFlavor::Windows, r"C:\Projects\Orbit");
        let mut foreign = base.clone();
        foreign.machine_id = "workstation-b".into();
        assert!(PathRef::describe(foreign, Some(&map)).is_err());
        let mut foreign = base.clone();
        foreign.environment_id = "wsl:Ubuntu-24.04:bob".into();
        assert!(PathRef::describe(foreign, Some(&map)).is_err());
        let mut foreign = base;
        foreign.workspace_id = Some("another-project".into());
        assert!(PathRef::describe(foreign, Some(&map)).is_err());
        let mut changed = mapping();
        changed.user = "bob".into();
        assert!(changed.validate().is_err());
    }

    #[test]
    fn mapping_rejects_prefix_siblings_escaping_parents_and_posix_only_names() {
        let map = mapping();
        for raw in [
            "/mnt/c/Projects/Orbit-evil/file",
            "/mnt/c/Projects/Orbit/../../outside",
            "/home/alice/Projects/Orbit",
            "/mnt/c/Projects/Orbit/a\\b",
            "/mnt/c/../d/Projects/Orbit",
        ] {
            assert!(
                PathRef::describe(input(PathFlavor::Posix, raw), Some(&map)).is_err(),
                "{raw}"
            );
        }
    }

    #[test]
    fn unc_works_for_native_identity_but_not_implicit_wsl_drive_mounts() {
        for raw in [r"\\server\share\Folder", r"\\?\UNC\server\share\Folder"] {
            let path = PathRef::describe(input(PathFlavor::Windows, raw), None).unwrap();
            assert_eq!(path.canonical_path, "//server/share/Folder");
            assert!(mapping().map(&path).is_err());
        }
    }

    #[test]
    fn relative_paths_wrong_grammar_and_nul_fail() {
        for raw in ["relative", "C:relative", "/mnt/c/path", "C:\\bad\0path"] {
            assert!(PathRef::describe(input(PathFlavor::Windows, raw), None).is_err());
        }
        for raw in ["relative", "C:\\path", "//server/share", "/bad\0path"] {
            assert!(PathRef::describe(input(PathFlavor::Posix, raw), None).is_err());
        }
        let path = PathRef::describe(input(PathFlavor::Posix, "/C:/literal"), None).unwrap();
        assert_eq!(path.canonical_path, "/C:/literal");
    }

    #[test]
    fn serialized_refs_require_recomputation_and_known_schema() {
        let mut path =
            PathRef::describe(input(PathFlavor::Windows, r"C:\Projects\Orbit"), None).unwrap();
        path.validate(None).unwrap();
        path.canonical_path = "c:/Other".into();
        assert!(path.validate(None).is_err());
        path.schema_version = 2;
        assert!(path.validate(None).is_err());
        let map = mapping();
        let mapped =
            PathRef::describe(input(PathFlavor::Windows, r"C:\Projects\Orbit"), Some(&map))
                .unwrap();
        assert!(mapped.validate(None).is_err());
    }
}
