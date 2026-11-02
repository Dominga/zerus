//! Explicit per-user Task Scheduler registration, never a service start/stop.
use super::registration::trace;
use super::{registration, store::Location};
use crate::config::Config;
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf, time::Duration};
use zerus_client_core::actions::{Exit, Invocation};

const CAP: usize = 64 * 1024;
// Functions only: hgs appends Invoke-RegistrationMain and the exact byte length.
// TASK_CREATE | TASK_IGNORE_REGISTRATION_TRIGGERS never overwrites a task which
// appeared after the ownership check. Removal never calls Run, Stop or End.
// The input is one bounded frame, independent of delayed pipe EOF.
const SCRIPT: &str = include_str!("windows_registration.ps1");

fn powershell() -> Result<PathBuf> {
    let mut path = [0u16; 32_768];
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            path.as_mut_ptr(),
            path.len() as u32,
        )
    } as usize;
    ensure!(
        length > 0 && length < path.len(),
        "Windows system directory is unavailable"
    );
    Ok(PathBuf::from(OsString::from_wide(&path[..length]))
        .join("WindowsPowerShell/v1.0/powershell.exe"))
}

fn invocation(operation: &str, plan: &Value) -> Result<Invocation> {
    ensure!(
        matches!(operation, "check" | "status" | "install" | "remove"),
        "invalid registration operation"
    );
    let request = serde_json::to_vec(&json!({"schema":1,"operation":operation,"plan":plan}))?;
    ensure!(
        request.len() <= CAP,
        "desktop registration request exceeds its bound"
    );
    // No here-strings: trim only source indentation, never request data.
    let source = SCRIPT
        .lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    let script = format!("{source}\nInvoke-RegistrationMain {}\n", request.len());
    let encoded = STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    ensure!(
        encoded.len() < 30_000,
        "desktop registration helper exceeds the Windows command-line bound"
    );
    Ok(Invocation {
        args: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-EncodedCommand".into(),
            encoded,
        ],
        stdin: Some(request),
        timeout: Duration::from_secs(15),
        request_id: None,
    })
}

fn exchange(operation: &str, plan: &Value) -> Result<Value> {
    let invocation = invocation(operation, plan)?;
    let program = powershell()?;
    let trace = trace::Trace::new();
    // Never forward a caller-supplied trace path to the helper.
    let environment = [(
        "HGS_DESKTOP_REGISTRATION_TRACE_FILE",
        trace.path().unwrap_or(""),
    )];
    let result =
        zerus_client_core::run::run_hidden_limited(&program, &invocation, &environment, CAP);
    trace.finish();
    let result = result.map_err(|_| {
        anyhow::anyhow!(
            "desktop registration helper output exceeded its bound; inspect status before retrying"
        )
    })?;
    let value = match result {
        Exit::Code {
            code: 0, stdout, ..
        } => serde_json::from_slice::<Value>(&stdout)
            .context("invalid desktop registration receipt; inspect status before retrying")?,
        Exit::Code { stdout, .. } => {
            let value = serde_json::from_slice::<Value>(&stdout).ok();
            bail!(
                "desktop registration refused: {}",
                value
                    .as_ref()
                    .and_then(|v| v["error"].as_str())
                    .unwrap_or("helper failed; inspect status before retrying")
            );
        }
        _ => bail!("desktop registration outcome is unavailable; inspect status before retrying"),
    };
    ensure!(
        value["schema"] == 1
            && value["operation"] == operation
            && value["configuration"] == plan["configuration"]
            && value["task_name"] == plan["task_name"]
            && value["registered"].is_boolean()
            && value["changed"].is_boolean(),
        "desktop registration receipt mismatch; inspect status before retrying"
    );
    Ok(value)
}

pub(super) fn command(config: &Config, location: &Location, operation: &str) -> Result<Value> {
    ensure!(
        matches!(operation, "install" | "remove" | "status"),
        "invalid desktop registration operation"
    );
    let mut plan = registration::windows_plan(config, location)?;
    // Complete the finite request preflight and reject foreign registrations
    // before changing the stable CLI. Registration itself never starts a task.
    let checked = exchange(
        if operation == "install" {
            "check"
        } else {
            "status"
        },
        &plan,
    )?;
    if operation == "status" {
        return Ok(checked);
    }
    let explicit = std::env::var_os("HGS_EXECUTABLE").is_some_and(|value| !value.is_empty());
    let mut staged = None;
    if operation == "install" && !explicit {
        let layout = crate::host::staging::Layout::new(&config.home, &config.dir);
        let cli = crate::host::staging::install_cli(&layout, &std::env::current_exe()?)?;
        staged = Some(json!({"path":cli.path,"sha256":cli.sha256,"changed":cli.changed}));
        plan = registration::windows_plan(config, location)?;
    }
    let mut result = exchange(operation, &plan)?;
    result["started"] = false.into();
    result["stopped"] = false.into();
    if let Some(cli) = staged {
        result["cli"] = cli;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_helper_has_bounded_input_and_never_passes_shell_arguments() {
        assert!(!SCRIPT.contains("@'"));
        assert!(!SCRIPT.contains("@\""));
        for cmdlet in ["ConvertFrom-Json", "ConvertTo-Json", "New-Object"] {
            assert!(
                !SCRIPT.contains(cmdlet),
                "helper must not auto-load a module"
            );
        }
        let plan =
            json!({"argv":["C:/Example/CLI & tools/hgs.exe"],"configuration":"a".repeat(64)});
        let call = invocation("status", &plan).unwrap();
        assert_eq!(
            &call.args[..4],
            [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand"
            ]
        );
        assert!(!call.args.iter().any(|arg| arg.contains("CLI & tools")));
        let body: Value = serde_json::from_slice(call.stdin.as_ref().unwrap()).unwrap();
        assert_eq!(body["plan"], plan);
        let script = String::from_utf16(
            &STANDARD
                .decode(&call.args[4])
                .unwrap()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(script.ends_with(&format!(
            "Invoke-RegistrationMain {}\n",
            call.stdin.as_ref().unwrap().len()
        )));
        assert_eq!(call.timeout, Duration::from_secs(15));
        assert!(invocation("start", &plan).is_err());
        assert!(invocation("install", &json!({"oversize":"x".repeat(CAP)})).is_err());
    }
}
