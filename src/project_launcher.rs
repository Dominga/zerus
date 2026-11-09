//! Project launchers replace the native program while retaining its arguments.
//! Unix keeps executable run_<command>.sh; Windows selects run_<command>.ps1.
//! The saved Windows recipe uses a fixed -File prefix. At spawn, its data is
//! passed to a fixed bootstrap: PowerShell's -File parser otherwise splits
//! literal agent arguments such as -name:value before the script receives them.
use std::path::Path;

pub fn apply(
    directory: &str,
    command: &str,
    bare: bool,
    program: usize,
    base: &mut Vec<String>,
    resume: &mut Vec<String>,
) -> Result<(), String> {
    if bare || directory.is_empty() {
        return Ok(());
    }
    let selected = words(
        Path::new(directory),
        command,
        cfg!(windows),
        |path| crate::platform::executable(path),
        powershell,
    )?;
    if let Some(selected) = selected {
        if !resume.is_empty() {
            resume.splice(0..1, selected.iter().cloned());
        }
        base.splice(0..program.min(base.len()), selected);
    }
    Ok(())
}

fn powershell() -> Result<String, String> {
    #[cfg(windows)]
    {
        crate::platform::powershell().map(|words| words[0].clone())
    }
    #[cfg(unix)]
    {
        Err("PowerShell project launchers are selected only on Windows".into())
    }
}

fn words(
    directory: &Path,
    command: &str,
    windows: bool,
    executable: impl FnOnce(&Path) -> bool,
    powershell: impl FnOnce() -> Result<String, String>,
) -> Result<Option<Vec<String>>, String> {
    if !windows {
        let script = directory.join(format!("run_{command}.sh"));
        return Ok(executable(&script).then(|| vec![script.to_string_lossy().into_owned()]));
    }
    let script = directory.join(format!("run_{command}.ps1"));
    let inspected = std::fs::symlink_metadata(&script);
    match inspected {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "cannot inspect project launcher {}: {error}",
                script.display()
            ))
        }
        Ok(_) => {}
    }
    if !std::fs::metadata(&script).is_ok_and(|meta| meta.is_file()) {
        return Err(format!(
            "project launcher {} is not a readable regular file",
            script.display()
        ));
    }
    let shell = powershell().map_err(|why| {
        format!(
            "project launcher {} needs PowerShell: {why}; install PowerShell or use --bare",
            script.display()
        )
    })?;
    Ok(Some(vec![
        shell,
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        script.to_string_lossy().into_owned(),
    ]))
}

/// Only our complete fixed prefix counts as a multiword program. A malformed
/// or foreign recipe retains ordinary option validation, rather than hiding
/// unsupported interpreter flags inside a guessed program boundary.
pub fn program_words(argv: &[String]) -> usize {
    let Some(first) = argv.first() else {
        return 0;
    };
    let name = first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(first)
        .to_ascii_lowercase();
    let known = matches!(
        name.strip_suffix(".exe").unwrap_or(&name),
        "pwsh" | "powershell"
    );
    if known
        && argv.len() >= 7
        && argv[1..6]
            == [
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]
    {
        let name = argv[6].rsplit(['/', '\\']).next().unwrap_or("");
        if name.starts_with("run_") && name.ends_with(".ps1") {
            return 7;
        }
    }
    1
}

/// A project script is not the native agent executable. Version/hook setup
/// must keep its identity unknown, never adopt the interpreter's version.
pub fn script(argv: &[String]) -> Option<&str> {
    (program_words(argv) == 7).then(|| argv[6].as_str())
}

#[cfg(any(windows, test))]
pub const DATA_ENV: &str = "HGS_PROJECT_LAUNCH_DATA";

// All script/argument data stays out of PowerShell source and is removed from
// the environment before the user's script starts. -NoProfile and the policy
// apply only to this new process; no machine policy or user profile is changed.
// PowerShell's $null binds to ""; .NET 9 retains empty environment entries.
// https://learn.microsoft.com/en-us/dotnet/core/compatibility/core-libraries/9.0/empty-env-variable
// Fixed Utility loading avoids cold 5.1 command discovery under env_clear.
// Restore autoload presence/value before running the person's project script.
#[cfg(any(windows, test))]
pub const BOOTSTRAP: &str = r#"try {
    $autoload = $ExecutionContext.SessionState.PSVariable.Get('PSModuleAutoLoadingPreference')
    $autoloadPresent = $null -ne $autoload
    $autoloadValue = $null
    if ($autoloadPresent) { $autoloadValue = $autoload.Value }
    $PSModuleAutoLoadingPreference = 'None'
    Import-Module -Name "$PSHOME\Modules\Microsoft.PowerShell.Utility" -ErrorAction Stop
    $launch = [Environment]::GetEnvironmentVariable('HGS_PROJECT_LAUNCH_DATA', 'Process') | ConvertFrom-Json
    [Environment]::SetEnvironmentVariable('HGS_PROJECT_LAUNCH_DATA', [NullString]::Value, 'Process')
    [string]$script = $launch.script
    [string[]]$arguments = $launch.arguments
    if ($autoloadPresent) {
        $ExecutionContext.SessionState.PSVariable.Set('PSModuleAutoLoadingPreference', $autoloadValue)
    } else {
        $ExecutionContext.SessionState.PSVariable.Remove('PSModuleAutoLoadingPreference')
    }
    & $script @arguments
    exit $LASTEXITCODE
} catch {
    [Console]::Error.WriteLine($_.ToString())
    exit 1
}"#;

#[cfg(any(windows, test))]
pub fn windows_command(argv: &[String]) -> std::io::Result<Option<(Vec<String>, String)>> {
    if program_words(argv) != 7 {
        return Ok(None);
    }
    if argv.iter().any(|word| word.contains('\0')) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "NUL in project launcher argument",
        ));
    }
    let data = serde_json::json!({"script": argv[6], "arguments": argv[7..]}).to_string();
    // Windows limits an individual environment entry. Refuse before spawn,
    // never truncate arguments or fall through to the native agent.
    if DATA_ENV.len() + 1 + data.encode_utf16().count() >= 32_767 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "project launcher arguments exceed the Windows environment limit",
        ));
    }
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(
        BOOTSTRAP
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    Ok(Some((
        vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-EncodedCommand".into(),
            encoded,
        ],
        data,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strings(words: &[&str]) -> Vec<String> {
        words.iter().map(|v| (*v).to_owned()).collect()
    }
    fn prefix() -> Vec<String> {
        strings(&[
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            r"C:\project\run_codex.ps1",
        ])
    }

    #[test]
    fn windows_selects_ps1_and_unix_keeps_only_executable_sh() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("run_codex.ps1"), "").unwrap();
        let selected = words(
            root.path(),
            "codex",
            true,
            |_| panic!("Unix executable rule"),
            || Ok("powershell.exe".into()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            selected[1..6],
            [
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File"
            ]
        );
        assert!(selected[6].ends_with("run_codex.ps1"));
        assert!(words(
            root.path(),
            "codex",
            false,
            |_| false,
            || panic!("no Unix pwsh lookup")
        )
        .unwrap()
        .is_none());
        let selected = words(
            root.path(),
            "codex",
            false,
            |_| true,
            || panic!("no Unix pwsh lookup"),
        )
        .unwrap()
        .unwrap();
        assert!(selected[0].ends_with("run_codex.sh"));
        assert_eq!(selected.len(), 1);
        assert!(words(
            root.path(),
            "claude",
            true,
            |_| true,
            || panic!("no other-agent lookup")
        )
        .unwrap()
        .is_none());
        let error = words(
            root.path(),
            "codex",
            true,
            |_| false,
            || Err("missing".into()),
        )
        .unwrap_err();
        assert!(error.contains("run_codex.ps1") && error.contains("--bare"));
        std::fs::remove_file(root.path().join("run_codex.ps1")).unwrap();
        std::fs::create_dir(root.path().join("run_codex.ps1")).unwrap();
        assert!(words(
            root.path(),
            "codex",
            true,
            |_| true,
            || panic!("no invalid-wrapper lookup")
        )
        .is_err());
    }

    #[test]
    fn bare_and_folderless_keep_both_native_variants() {
        let mut base = strings(&["codex"]);
        let mut resume = strings(&["codex", "resume", "--last"]);
        apply("project", "codex", true, 1, &mut base, &mut resume).unwrap();
        apply("", "codex", false, 1, &mut base, &mut resume).unwrap();
        assert_eq!(base, ["codex"]);
        assert_eq!(resume, ["codex", "resume", "--last"]);
    }

    #[test]
    fn only_the_exact_recipe_prefix_hides_interpreter_words() {
        let argv = prefix();
        assert_eq!(program_words(&argv), 7);
        assert_eq!(script(&argv), Some(argv[6].as_str()));
        assert_eq!(script(&strings(&["codex", "resume", "id"])), None);
        for index in 0..7 {
            let mut bad = argv.clone();
            bad[index] = "foreign".into();
            assert_eq!(program_words(&bad), 1, "{index}");
        }
        for size in 1..7 {
            assert_eq!(program_words(&argv[..size]), 1);
        }
        assert_eq!(program_words(&[]), 0);
        assert_eq!(program_words(&strings(&["codex", "resume", "id"])), 1);
    }

    #[test]
    fn windows_bootstrap_keeps_every_argument_as_data() {
        let mut argv = prefix();
        argv.extend(strings(&[
            "resume",
            "id",
            "",
            "a\"b",
            "-name:value",
            "semi;literal",
            "日本語",
            "back\\",
        ]));
        let (command, data) = windows_command(&argv).unwrap().unwrap();
        assert_eq!(
            &command[..5],
            [
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-EncodedCommand"
            ]
        );
        let value: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["arguments"], serde_json::json!(argv[7..]));
        assert_eq!(value["script"], argv[6]);
        assert!(!BOOTSTRAP.contains("日本語"));
        assert!(
            BOOTSTRAP.find("SetEnvironmentVariable").unwrap()
                < BOOTSTRAP.find("& $script").unwrap()
        );
        let mut invalid = prefix();
        invalid.push("nul\0value".into());
        assert!(windows_command(&invalid).is_err());
        let mut huge = prefix();
        huge.push("x".repeat(32_768));
        assert!(windows_command(&huge).is_err());
        assert!(windows_command(&strings(&["codex", "resume", "id"]))
            .unwrap()
            .is_none());
    }
    #[cfg(windows)]
    #[test]
    fn actual_powershell_script_receives_data_and_preserves_failure_status() {
        use std::process::Stdio;
        use std::time::Duration;
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("run_codex.ps1");
        let result = root.path().join("arguments.json");
        let mut shells = vec![crate::platform::powershell().unwrap()[0].clone()];
        let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if !shells
            .iter()
            .any(|shell| shell.eq_ignore_ascii_case(&system.to_string_lossy()))
        {
            shells.push(system.to_string_lossy().into_owned());
        }
        for shell in shells {
            for (code, expected) in [("$value=@{args=@($args);cleared=($null -eq $env:HGS_PROJECT_LAUNCH_DATA)}|ConvertTo-Json -Depth 5 -Compress; [IO.File]::WriteAllText($env:PROBE_RESULT,$value,[Text.UTF8Encoding]::new($false)); exit 7",7), ("throw 'synthetic wrapper refusal'",1)] {
                std::fs::write(&script,code).unwrap();
                let mut argv=prefix();argv[0]=shell.clone();argv[6]=script.to_string_lossy().into_owned();
                let values=strings(&["", "a\"b", "-name:value", "Пример", "C:\\tail\\", "$(literal);&only-data"]);argv.extend(values.clone());
                let mut command=crate::platform::agent_command(&argv).unwrap();
                let data=command.get_envs().find_map(|(key,value)|(key==DATA_ENV).then(||value.unwrap().to_owned())).unwrap();
                command.env_clear().env(DATA_ENV,data).env("SystemRoot",std::env::var_os("SystemRoot").unwrap()).env("PROBE_RESULT",&result).stdin(Stdio::null());
                let modules=std::path::Path::new(&shell).parent().unwrap().join("Modules");
                let system_modules=std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join(r"System32\WindowsPowerShell\v1.0\Modules");
                command.env("PSModulePath",std::env::join_paths([modules,system_modules]).unwrap());
                for name in ["HOME","USERPROFILE","APPDATA","LOCALAPPDATA","TEMP","TMP","HGS_CONFIG_DIR","HGS_STATE_DIR"] {
                    let path=root.path().join(name);std::fs::create_dir_all(&path).unwrap();command.env(name,path);
                }
                let output=crate::platform::output_until(&mut command,Duration::from_secs(20)).unwrap();
                assert_eq!(output.status.code(),Some(expected),"{shell}: {}",String::from_utf8_lossy(&output.stderr));
                if expected==7 {let value:serde_json::Value=serde_json::from_slice(&std::fs::read(&result).unwrap()).unwrap();assert_eq!(value["args"],serde_json::json!(values));assert_eq!(value["cleared"],true);}
            }
        }
    }
}
