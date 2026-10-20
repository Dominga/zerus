//! WhiteWh fork: a Windows computer as an SSH peer.
//!
//! Windows OpenSSH runs a remote command through `cmd.exe`, not a POSIX shell,
//! and a Windows computer keeps its CLI at `%USERPROFILE%\.local\bin\hgs.exe`
//! (`hgs session-host install`). `hgs machine check` finds out which kind of
//! computer an alias names and remembers it in `machine-platforms.json`
//! (beside `machines.local.json`, which the desktop rewrites whole); every
//! remote hgs command line is then built for that computer's shell.
use crate::{
    cli::{Error, Result},
    config::Config,
    platform,
};
use std::{collections::BTreeMap, fs, io::Write};

const FILE: &str = "machine-platforms.json";
const REMOTE_HGS: &str = r#""%USERPROFILE%\.local\bin\hgs.exe""#;

fn read(config: &Config) -> BTreeMap<String, String> {
    fs::read(config.dir.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Whether `alias` names a Windows computer, as its last check found.
pub fn is_windows(config: &Config, alias: &str) -> bool {
    read(config).get(alias).is_some_and(|platform| platform == "windows")
}

/// Records what a check found: a Windows computer, or a POSIX one (forgotten).
pub fn record(config: &Config, alias: &str, windows: bool) -> Result<()> {
    let mut platforms = read(config);
    let changed = if windows {
        platforms.insert(alias.into(), "windows".into()).as_deref() != Some("windows")
    } else {
        platforms.remove(alias).is_some()
    };
    if !changed {
        return Ok(());
    }
    fs::create_dir_all(&config.dir)?;
    let mut stage = tempfile::NamedTempFile::new_in(&config.dir)?;
    let mut bytes = serde_json::to_vec_pretty(&platforms).map_err(|e| Error::new(1, e.to_string()))?;
    bytes.push(b'\n');
    stage.write_all(&bytes)?;
    stage.as_file().sync_all()?;
    platform::persist(stage, config.dir.join(FILE)).map_err(|e| Error::new(1, e.to_string()))?;
    Ok(())
}

/// One argument for `cmd.exe /c`, read back by hgs.exe with the Microsoft C
/// rules: always quoted, backslashes before the closing quote doubled. `cmd`
/// expands `%` even inside quotes and ends a quoted run at `"`, so arguments
/// holding either (or a line break) are refused rather than altered.
pub fn quote(argument: &str) -> Result<String> {
    if argument.contains(['%', '"', '\r', '\n', '\0']) {
        return Err(Error::new(
            1,
            "This argument cannot be passed to a Windows computer: it contains %, \" or a line break.",
        ));
    }
    let trailing = argument.len() - argument.trim_end_matches('\\').len();
    Ok(format!("\"{argument}{}\"", "\\".repeat(trailing)))
}

/// `cmd /c` drops the first and the last quote of a line that starts with one,
/// so the whole line travels inside one more pair.
pub fn wrap(line: &str) -> String {
    format!("\"{line}\"")
}

/// The remote command line that runs hgs with `args` on a Windows computer.
pub fn command(args: &[String]) -> Result<String> {
    let mut line = REMOTE_HGS.to_owned();
    for argument in args {
        line.push(' ');
        line.push_str(&quote(argument)?);
    }
    Ok(wrap(&line))
}

/// The remote command line that runs hgs with `args` on `alias`, whichever
/// kind of computer it is.
pub fn hgs(config: &Config, alias: &str, args: &[String]) -> Result<String> {
    if is_windows(config, alias) {
        command(args)
    } else {
        Ok(format!("~/.local/bin/hgs {}", platform::join(args)))
    }
}

/// `cmd.exe` lines of the check report: the same keys the POSIX probe prints.
pub const PROBE: &str = r#"echo.& echo __HGS_MACHINE__& echo os=%OS%& echo arch=%PROCESSOR_ARCHITECTURE%& (if exist "%USERPROFILE%\.local\bin\hgs.exe" ("%USERPROFILE%\.local\bin\hgs.exe" --version) else (echo.))"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_reach_hgs_exe_unchanged() {
        assert_eq!(quote("codex/Temp/ImageWorks").unwrap(), "\"codex/Temp/ImageWorks\"");
        assert_eq!(quote(r"D:\Work\x y").unwrap(), r#""D:\Work\x y""#);
        assert_eq!(quote(r"D:\").unwrap(), r#""D:\\""#);
        assert_eq!(quote("a & b | c > d").unwrap(), "\"a & b | c > d\"");
        for refused in ["50%", "say \"hi\"", "two\nlines"] {
            assert!(quote(refused).is_err(), "{refused}");
        }
        assert_eq!(
            command(&["ls".into(), "--local".into(), "--json".into()]).unwrap(),
            r#"""%USERPROFILE%\.local\bin\hgs.exe" "ls" "--local" "--json"""#
        );
    }
}
