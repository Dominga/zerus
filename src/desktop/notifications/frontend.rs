//! Exact Linux frontend custody for the maintained service installation.
//! Selection reads files only. The Linux launcher owns its original Child and
//! must separately require the target-navigation capability before launch.
use crate::platform::{self, FileIdentity, ProcessIdentity};
use anyhow::{ensure, Context, Result};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Child,
};

#[derive(Clone, Debug, PartialEq)]
struct Stamp {
    identity: FileIdentity,
    uid: u32,
    mode: u32,
    bytes: u64,
    changed: (i64, i64),
}
impl Stamp {
    fn of(metadata: &Metadata) -> Self {
        Self {
            identity: FileIdentity {
                device: metadata.dev(),
                inode: metadata.ino(),
                modified: metadata.mtime(),
                modified_nanos: metadata.mtime_nsec(),
            },
            uid: metadata.uid(),
            mode: metadata.mode(),
            bytes: metadata.len(),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}
fn owned(metadata: &Metadata) -> bool {
    let user = unsafe { libc::geteuid() };
    (metadata.uid() == 0 || metadata.uid() == user) && metadata.mode() & 0o022 == 0
}
struct Pinned {
    path: PathBuf,
    file: File,
    stamp: Stamp,
}
impl Pinned {
    fn open(path: PathBuf, executable: bool) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && owned(&metadata),
            "unsafe installed frontend file"
        );
        ensure!(
            !executable || metadata.mode() & 0o111 != 0,
            "frontend entry is not executable"
        );
        let result = Self {
            path,
            stamp: Stamp::of(&metadata),
            file,
        };
        result.check()?;
        Ok(result)
    }
    fn check(&self) -> Result<()> {
        let path = fs::symlink_metadata(&self.path)?;
        ensure!(
            path.is_file()
                && owned(&path)
                && Stamp::of(&path) == self.stamp
                && Stamp::of(&self.file.metadata()?) == self.stamp,
            "installed frontend changed"
        );
        Ok(())
    }
}
struct Directory {
    path: PathBuf,
    file: File,
    identity: (u64, u64, u32, u32),
}
impl Directory {
    fn identity(metadata: &Metadata) -> (u64, u64, u32, u32) {
        (
            metadata.dev(),
            metadata.ino(),
            metadata.uid(),
            metadata.mode(),
        )
    }
    fn open(path: PathBuf) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_dir() && owned(&metadata),
            "unsafe frontend directory"
        );
        let result = Self {
            path,
            file,
            identity: Self::identity(&metadata),
        };
        result.check()?;
        Ok(result)
    }
    fn check(&self) -> Result<()> {
        let path = fs::symlink_metadata(&self.path)?;
        ensure!(
            path.is_dir()
                && Self::identity(&path) == self.identity
                && Self::identity(&self.file.metadata()?) == self.identity,
            "frontend directory changed"
        );
        Ok(())
    }
}
enum Origin {
    RunningService,
    #[cfg(test)]
    Fixture,
}

/// Non-Clone/non-serde. Only select() binds the actual running service image;
/// no unscoped path, PATH lookup or build-info creates this handle.
pub(in crate::desktop) struct SelectedFrontend {
    cli: Pinned,
    image: Pinned,
    wrapper: Option<Pinned>,
    setting: Option<Pinned>,
    setting_path: PathBuf,
    directories: Vec<Directory>,
    origin: Origin,
    configuration: String,
}
/// Observation of the caller's ORIGINAL retained Child, not a later PID lookup.
pub(in crate::desktop) struct ChildObservation {
    pid: u32,
    identity: ProcessIdentity,
}
impl ChildObservation {
    pub fn pid(&self) -> u32 {
        self.pid
    }
    pub fn birth(&self) -> &str {
        &self.identity.birth
    }
}
impl SelectedFrontend {
    /// Off actor, bind the root's explicit maintained-image choice under the
    /// SAME installed-image scope used by inbound frontend authentication.
    pub fn select(scope: &crate::desktop::ui_authority::Scope, image: &Path) -> Result<Self> {
        let installed = scope
            .installed_frontend(image)
            .context("frontend is not a maintained installed image")?;
        let cli = std::env::current_exe()?.canonicalize()?;
        Self::capture(
            cli,
            installed.to_owned(),
            scope.configuration.clone(),
            Origin::RunningService,
        )
    }
    fn capture(
        cli: PathBuf,
        image_path: PathBuf,
        configuration: String,
        origin: Origin,
    ) -> Result<Self> {
        ensure!(
            cli.is_absolute() && image_path.is_absolute(),
            "frontend selection must be absolute"
        );
        ensure!(
            image_path.file_name().is_some_and(|v| v == "hgs-tray"),
            "unrecognized frontend image"
        );
        let bin = image_path.parent().context("missing frontend bin")?;
        ensure!(
            bin.file_name().is_some_and(|v| v == "bin"),
            "frontend is not in a maintained bin layout"
        );
        let prefix = bin
            .parent()
            .context("missing frontend installation prefix")?;
        let mut directories = vec![
            Directory::open(prefix.to_owned())?,
            Directory::open(bin.to_owned())?,
        ];
        let image = Pinned::open(image_path.clone(), true)?;
        let setting_path = prefix.join("share/hgs/local-webkit.json");
        let (setting, wrapper) = match fs::symlink_metadata(&setting_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(e) => return Err(e.into()),
            Ok(_) => {
                directories.push(Directory::open(prefix.join("share/hgs"))?);
                directories.push(Directory::open(prefix.join("libexec"))?);
                (
                    Some(Pinned::open(setting_path.clone(), false)?),
                    Some(Pinned::open(prefix.join("libexec/hgs-tray-launch"), true)?),
                )
            }
        };
        let result = Self {
            cli: Pinned::open(cli, true)?,
            image,
            wrapper,
            setting,
            setting_path,
            directories,
            origin,
            configuration,
        };
        result.revalidate()?;
        Ok(result)
    }
    pub fn image(&self) -> &Path {
        &self.image.path
    }
    pub fn configuration(&self) -> &str {
        &self.configuration
    }
    pub fn service_image(&self) -> &Path {
        &self.cli.path
    }
    pub fn revalidate_for_launch(&self, launch: &super::activation::Launch) -> Result<()> {
        ensure!(
            self.configuration == launch.configuration(),
            "frontend belongs to another service configuration"
        );
        self.revalidate()
    }
    /// Preserve the maintained local-WebKit entry when selected by its existing
    /// setting. The setting's data is never an identity or permission grant.
    pub fn entry(&self) -> &Path {
        self.wrapper.as_ref().unwrap_or(&self.image).path.as_path()
    }
    pub fn revalidate(&self) -> Result<()> {
        for directory in &self.directories {
            directory.check()?;
        }
        self.cli.check()?;
        match self.origin {
            Origin::RunningService => ensure!(
                platform::image_identity() == Some(self.cli.stamp.identity),
                "running service image changed"
            ),
            #[cfg(test)]
            Origin::Fixture => (),
        }
        self.image.check()?;
        if let Some(setting) = &self.setting {
            setting.check()?;
        } else {
            ensure!(
                matches!(fs::symlink_metadata(&self.setting_path),Err(e) if e.kind()==std::io::ErrorKind::NotFound),
                "frontend launcher selection changed"
            );
        }
        if let Some(wrapper) = &self.wrapper {
            wrapper.check()?;
        }
        Ok(())
    }
    /// None while a maintained wrapper still owns the original process. The
    /// launcher keeps this same Child and deadline; never adopt another PID.
    pub fn observe_original_child(&self, child: &mut Child) -> Result<Option<ChildObservation>> {
        ensure!(child.try_wait()?.is_none(), "frontend child already exited");
        self.revalidate()?;
        let pid = child.id();
        let identity =
            platform::process_identity(pid).context("frontend child identity unavailable")?;
        ensure!(
            identity.parent == std::process::id(),
            "frontend is not the original direct child"
        );
        let image = platform::file_identity(&PathBuf::from(format!("/proc/{pid}/exe")));
        if image != Some(self.image.stamp.identity) {
            return Ok(None);
        }
        let observation = ChildObservation { pid, identity };
        self.revalidate_child(child, &observation)?;
        Ok(Some(observation))
    }
    pub fn revalidate_child(&self, child: &mut Child, original: &ChildObservation) -> Result<()> {
        ensure!(
            child.id() == original.pid && child.try_wait()?.is_none(),
            "frontend child already exited or changed"
        );
        self.revalidate()?;
        ensure!(
            platform::process_identity(original.pid).as_ref() == Some(&original.identity)
                && platform::file_identity(&PathBuf::from(format!("/proc/{}/exe", original.pid)))
                    == Some(self.image.stamp.identity),
            "original frontend image changed"
        );
        ensure!(
            child.try_wait()?.is_none(),
            "frontend child exited during observation"
        );
        self.revalidate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
