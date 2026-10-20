//! WhiteWh fork: optional extensions, integrations with third-party tools that
//! stay outside the Zerus core. Each one is a cargo feature, off by default; a
//! build without it keeps the command and answers that the extension is absent.

#[cfg(feature = "socraticode")]
mod socraticode;

use crate::{cli::Result, config::Config};

/// `hgs socraticode …`: a folder's SocratiCode index (feature `socraticode`).
pub fn socraticode(config: &Config, args: &[String], dry: bool) -> Result<i32> {
    #[cfg(feature = "socraticode")]
    return socraticode::dispatch(config, args, dry);
    #[cfg(not(feature = "socraticode"))]
    {
        let _ = (config, args, dry);
        Err(crate::cli::Error::new(
            1,
            "This hgs was built without the SocratiCode extension (cargo feature `socraticode`).",
        ))
    }
}
