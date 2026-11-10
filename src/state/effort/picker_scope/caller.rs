//! Preserve the physical-machine guard already verified by the public CLI.
//! This data cannot create authority: the keeper rechecks the same existing
//! native catalog, without initializing, refreshing or saving it.
use super::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

type Object = (u64, u64, u32, u32, u32);
const CATALOG_BYTES: u64 = 128 * 1024 * 1024;
static ORIGINAL: Mutex<Remembered> = Mutex::new(Remembered {
    first: None,
    refused: None,
});

struct Remembered {
    first: Option<Arc<Captured>>,
    refused: Option<String>,
}

struct Captured {
    data: Machine,
    _configuration: Directory,
    _directory: Directory,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::state::effort::picker_scope) struct Machine {
    expected: String,
    configuration: PathBuf,
    configuration_object: Object,
    directory: PathBuf,
    object: Object,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::state::effort::picker_scope) enum Snapshot {
    Local,
    Guarded(Machine),
}

fn object(directory: &Directory) -> Result<Object> {
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(directory.descriptor(), &mut stat) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok((
        stat.st_dev as u64,
        stat.st_ino as u64,
        stat.st_mode as u32,
        stat.st_uid,
        stat.st_gid,
    ))
}

/// Called only AFTER the existing public --expect-machine/reference check.
/// A capture failure is remembered for a later ID self-exec, not used to alter
/// the existing guarded legacy/Settings dispositions that never use a frame.
pub(super) fn remember(config: &crate::config::Config, expected: &str) {
    let capture = (|| {
        let configuration = crate::platform::canonical(&config.dir).map_err(|e| e.to_string())?;
        let original_configuration = Directory::open(&configuration)?;
        let path =
            crate::platform::canonical(config.dir.join("swarm")).map_err(|e| e.to_string())?;
        let directory = Directory::open(&path)?;
        if !directory.private()? {
            return Err("Original guarded machine catalog is not private".into());
        }
        let data = Machine {
            expected: expected.into(),
            configuration,
            configuration_object: object(&original_configuration)?,
            directory: path,
            object: object(&directory)?,
        };
        // Retain the original namespaces until the frontend exits. The keeper
        // opens matching objects while its original endpoint is still live.
        Ok(Arc::new(Captured {
            data,
            _configuration: original_configuration,
            _directory: directory,
        }))
    })();
    let mut original = ORIGINAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if original.refused.is_some() {
        return;
    }
    match capture {
        Err(error) => original.refused = Some(error),
        Ok(captured) => match &original.first {
            None => original.first = Some(captured),
            Some(first) if first.data == captured.data => {} // retain FIRST actual descriptors
            Some(_) => {
                original.refused =
                    Some("Original guarded machine changed between public caller checks".into())
            }
        },
    }
}

impl Snapshot {
    pub(super) fn capture() -> Result<Self> {
        let original = ORIGINAL
            .lock()
            .map_err(|_| "Original machine guard is unavailable")?;
        if let Some(error) = &original.refused {
            return Err(error.clone());
        }
        Ok(original
            .first
            .as_ref()
            .map_or(Self::Local, |first| Self::Guarded(first.data.clone())))
    }
    pub(super) fn validate(&self) -> Result<()> {
        if let Self::Guarded(machine) = self {
            if !uuid::Uuid::parse_str(&machine.expected)
                .is_ok_and(|id| !id.is_nil() && id.to_string() == machine.expected)
                || !machine.configuration.is_absolute()
                || !machine.directory.is_absolute()
            {
                return Err("Invalid original machine guard".into());
            }
        }
        Ok(())
    }
    pub(super) fn hold(&self) -> Result<Held> {
        self.validate()?;
        match self {
            Self::Local => Ok(Held {
                snapshot: self.clone(),
                catalog: Mutex::new(None),
            }),
            Self::Guarded(_) => {
                let held = Held {
                    snapshot: self.clone(),
                    catalog: Mutex::new(None),
                };
                held.check()?;
                Ok(held)
            }
        }
    }
}

struct Catalog {
    configuration: Directory,
    directory: Directory,
    file: File,
    version: Value,
}
pub(super) struct Held {
    snapshot: Snapshot,
    catalog: Mutex<Option<Catalog>>,
}

impl Held {
    fn fence(machine: &Machine, configuration: &Directory, directory: &Directory) -> Result<()> {
        configuration.verify()?;
        directory.verify()?;
        let home = crate::platform::home().ok_or("Original native configuration is unavailable")?;
        let configured = crate::platform::canonical(crate::config::config_dir(&home))
            .map_err(|e| e.to_string())?;
        if configured != machine.configuration
            || crate::platform::canonical(configured.join("swarm")).map_err(|e| e.to_string())?
                != machine.directory
            || object(configuration)? != machine.configuration_object
            || object(directory)? != machine.object
            || !directory.private()?
        {
            return Err("Original guarded machine route or namespace changed".into());
        }
        Ok(())
    }
    pub(super) fn check(&self) -> Result<()> {
        // The private literal is not a substitute for the existing local floor.
        crate::state::session_operations()?;
        let Snapshot::Guarded(machine) = &self.snapshot else {
            return Ok(());
        };
        let home = crate::platform::home().ok_or("Original native configuration is unavailable")?;
        let configured = crate::platform::canonical(crate::config::config_dir(&home))
            .map_err(|e| e.to_string())?;
        if configured != machine.configuration {
            return Err("Original guarded machine configuration changed".into());
        }
        if crate::platform::canonical(configured.join("swarm")).map_err(|e| e.to_string())?
            != machine.directory
        {
            return Err("Original guarded machine catalog route changed".into());
        }
        let mut cached = self
            .catalog
            .lock()
            .map_err(|_| "Original machine catalog is unavailable")?;
        if let Some(catalog) = cached.as_ref() {
            catalog.configuration.verify()?;
            catalog.directory.verify()?;
            if object(&catalog.configuration)? != machine.configuration_object
                || object(&catalog.directory)? != machine.object
                || !catalog.directory.private()?
            {
                return Err("Original guarded machine namespace changed".into());
            }
            if version(&catalog.file)? == catalog.version
                && version(&catalog.directory.read_private("catalog.json")?)? == catalog.version
            {
                Self::fence(machine, &catalog.configuration, &catalog.directory)?;
                return Ok(());
            }
        }
        let configuration = Directory::open(&configured)?;
        let directory = Directory::open(&machine.directory)?;
        if object(&configuration)? != machine.configuration_object
            || !directory.private()?
            || object(&directory)? != machine.object
        {
            return Err("Original guarded machine namespace changed".into());
        }
        let file = directory.read_private("catalog.json")?;
        let before = version(&file)?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        if size > CATALOG_BYTES {
            return Err("Original machine catalog exceeds its bound".into());
        }
        let mut bytes = vec![0; size as usize];
        file.read_exact_at(&mut bytes, 0)
            .map_err(|e| e.to_string())?;
        if before != version(&file)? || before != version(&directory.read_private("catalog.json")?)?
        {
            return Err("Original guarded machine catalog changed while reading".into());
        }
        let actual = crate::swarm::existing_node_id(&bytes)
            .map_err(|_| "Original native machine identity is unavailable")?;
        if actual != machine.expected {
            return Err("Mobile target identity changed; refresh computers".into());
        }
        if before != version(&file)? || before != version(&directory.read_private("catalog.json")?)?
        {
            return Err("Original guarded machine catalog changed while validating".into());
        }
        Self::fence(machine, &configuration, &directory)?;
        // A normal atomic catalog save may refresh bytes under this SAME
        // namespace and expected UUID. This is current data, never new consent.
        *cached = Some(Catalog {
            configuration,
            directory,
            file,
            version: before,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
