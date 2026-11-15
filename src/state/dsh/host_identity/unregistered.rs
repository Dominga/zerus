//! Negative caller evidence for resident DeepSeek hosts that have no matching
//! native record. Nothing here adopts a host or reconstructs call authority.
use super::{argv, recorded};
use recorded::{Ancestor, Recorded, Refusal};
use std::{io::Read, path::Path};

const MANIFEST_BYTES: u64 = 64 << 10;

/// Existing records retain their ordinary proof. Only a native ancestor with
/// a DeepSeek CLI entry and server arguments adds the new refusal. The same
/// 64-step birth/owner walk and bounded argv reader serve both checks.
/// An error AFTER observing a host is uncertainty, never an outside answer.
pub(crate) fn ancestor() -> Result<bool, Refusal> {
    let records = recorded::observable_records();
    let Ok(chain) = recorded::walk(std::process::id(), recorded::inspect) else {
        return Ok(false); // No new evidence about an ordinary sandboxed person.
    };
    detect(&records, &chain, argv::read, native_host, || {
        recorded::unchanged(&chain, recorded::inspect)
    })
}

fn detect(
    records: &[Recorded],
    chain: &[Ancestor],
    mut read: impl FnMut(u32) -> Option<Vec<Vec<u8>>>,
    mut native: impl FnMut(&[Vec<u8>]) -> bool,
    recheck: impl FnOnce() -> Result<(), Refusal>,
) -> Result<bool, Refusal> {
    for process in chain {
        let Some(birth) = process.identity.birth.as_deref() else {
            continue;
        };
        if records.iter().any(|record| {
            record.named.pid == process.pid && record.named.birth.as_deref() == Some(birth)
        }) {
            // The nearest recorded host is the custodian boundary. Its own
            // hosted proof, not an older launcher above it, decides the call.
            return Ok(false);
        }
        let Some(tokens) = read(process.pid) else {
            continue;
        };
        if native(&tokens) {
            // The healthy global scan may be only a bounded prefix. Absence
            // there cannot override this exact native PID/birth registration.
            // This withholds only the extra refusal; ordinary proof still runs.
            if recorded::for_patch(&tokens)
                .iter()
                .any(|record| record.pid == process.pid && record.birth.as_deref() == Some(birth))
            {
                return Ok(false);
            }
            recheck()?;
            if read(process.pid).as_ref() != Some(&tokens) {
                return Err(Refusal::UnreadableAncestry);
            }
            return Ok(true);
        }
    }
    Ok(false)
}

/// A basename such as `dsh` or `node` never establishes this evidence. The
/// native argv must name the DeepSeek CLI module AND only server options.
/// npm's known module route remains recognizable after an installed package
/// is replaced while its old host stays resident. Other layouts require the
/// bounded package manifest to bind the exact entry. These are refusal-only
/// correlations, not an authentication of arbitrary same-user instrumentation.
fn native_host(tokens: &[Vec<u8>]) -> bool {
    let node = tokens.first().and_then(|v| std::str::from_utf8(v).ok());
    if !node.is_some_and(|v| {
        Path::new(v)
            .file_name()
            .is_some_and(|n| n == "node" || n == "nodejs")
    }) {
        return false;
    }
    let Some(at) = node_entry(tokens) else {
        return false;
    };
    let Some(entry) = tokens.get(at).and_then(|v| std::str::from_utf8(v).ok()) else {
        return false;
    };
    if !server_options(&tokens[at + 1..]) {
        return false;
    }
    let path = Path::new(entry);
    if entry.len() > 4096 || !path.is_absolute() {
        return false;
    }
    let suffix = Path::new("@deepseek-ai/dsh/lib/bin.js");
    if path.ends_with(suffix) {
        return true;
    }
    manifest_entry(path)
}

/// Locate Node's first script operand without treating eval/print/test input
/// or an option value as the DeepSeek entry. Runtime options are bounded;
/// unknown long flags can only be boolean here, while value options require
/// their separate operand (or Node's unambiguous --name=value spelling).
fn node_entry(tokens: &[Vec<u8>]) -> Option<usize> {
    let mut at = 1;
    while at <= 32 {
        let token = tokens.get(at)?.as_slice();
        if token == b"--" {
            return tokens.get(at + 1).map(|_| at + 1);
        }
        if !token.starts_with(b"-") {
            return Some(at);
        }
        let flag = token.split(|b| *b == b'=').next()?;
        if matches!(
            flag,
            b"--eval"
                | b"--print"
                | b"--check"
                | b"--test"
                | b"--run"
                | b"--help"
                | b"--version"
                | b"--prof-process"
        ) {
            return None;
        }
        let value = matches!(
            flag,
            b"-r"
                | b"--require"
                | b"--import"
                | b"--loader"
                | b"--experimental-loader"
                | b"-C"
                | b"--conditions"
                | b"--title"
                | b"--icu-data-dir"
                | b"--openssl-config"
                | b"--redirect-warnings"
                | b"--diagnostic-dir"
                | b"--heapsnapshot-signal"
                | b"--unhandled-rejections"
                | b"--stack-trace-limit"
                | b"--max-old-space-size"
                | b"--max-semi-space-size"
                | b"--cpu-prof-dir"
                | b"--cpu-prof-name"
                | b"--heap-prof-dir"
                | b"--heap-prof-name"
                | b"--disable-warning"
                | b"--snapshot-blob"
                | b"--env-file"
                | b"--env-file-if-exists"
        );
        if value && flag == token {
            let operand = tokens.get(at + 1)?;
            if operand.is_empty() || operand.starts_with(b"-") {
                return None;
            }
            at += 2;
        } else if token.starts_with(b"--") && token.len() > 2 && token.len() <= 4096
            || (token.starts_with(b"-r") || token.starts_with(b"-C")) && token.len() > 2
        {
            at += 1;
        } else {
            return None; // In particular -e, -p and their attached forms.
        }
    }
    None
}

fn server_options(tokens: &[Vec<u8>]) -> bool {
    if tokens.len() > 32 {
        return false;
    }
    let mut at = 0;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(flag) = tokens.get(at) {
        if !seen.insert(flag.as_slice()) {
            return false;
        }
        match flag.as_slice() {
            b"--no-open" => at += 1,
            b"--profile" | b"--from-default-profile" | b"--patch" | b"--host" | b"--port" => {
                let Some(value) = tokens.get(at + 1) else {
                    return false;
                };
                if value.is_empty() || value.len() > 4096 || value.starts_with(b"-") {
                    return false;
                }
                if flag == b"--port"
                    && !std::str::from_utf8(value).is_ok_and(|v| v.parse::<u16>().is_ok())
                {
                    return false;
                }
                at += 2;
            }
            _ => return false, // help/version, a client command, or another program.
        }
    }
    true
}

fn manifest_entry(entry: &Path) -> bool {
    let Ok(entry) = std::fs::canonicalize(entry) else {
        return false;
    };
    let Some(package) = entry.parent().and_then(Path::parent) else {
        return false;
    };
    let path = package.join("package.json");
    let Ok(file) = crate::platform::no_follow(std::fs::OpenOptions::new().read(true)).open(path)
    else {
        return false;
    };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= MANIFEST_BYTES)
    {
        return false;
    }
    let mut bytes = Vec::new();
    if file
        .take(MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MANIFEST_BYTES
    {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    if value["name"] != "@deepseek-ai/dsh" {
        return false;
    }
    let Some(bin) = value["bin"]["dsh"]
        .as_str()
        .or_else(|| value["bin"].as_str())
    else {
        return false;
    };
    let relative = Path::new(bin);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return false;
    }
    std::fs::canonicalize(package.join(relative)).is_ok_and(|declared| declared == entry)
}

#[cfg(test)]
mod tests;
