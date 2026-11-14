//! Claude hook ownership. Markers select a record; native ancestry grants
//! admission. Windows keeps its existing hook path until it is qualified.
use super::*;

const REFUSED: &str = "Claude hook caller cannot be proven";

pub(super) struct Caller {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    authority: Authority,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
enum Authority {
    Unchanged,
    Legacy,
    Native {
        pin: Option<(u32, String)>,
        pid: u32,
        birth: String,
        background: bool,
    },
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn refused(reason: &str) -> String {
    let reason: String = reason.chars().take(768).collect();
    format!("{REFUSED}: {reason}; no hook update or answer was authorized")
}

pub(super) fn report_channel(error: &str) {
    if error.starts_with(REFUSED) {
        eprintln!("hgs: {error}");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn unheld() {
    report_channel(&refused(
        "no live window could be proven to hold this job in its recorded account",
    ));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn pin(record: &Value) -> Option<(u32, String)> {
    let pid = u32::try_from(record["pid"].as_u64()?)
        .ok()
        .filter(|pid| *pid > 0)?;
    let start = string(record, "process_start");
    (!start.is_empty()).then(|| (pid, start.to_owned()))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ancestor(pid: u32) -> Result<String> {
    let mut chain = Vec::new();
    let mut current = std::process::id();
    for _ in 0..64 {
        if chain.iter().any(|(seen, _, _)| *seen == current) {
            return Err(refused("the process ancestry loops"));
        }
        let process = crate::platform::process_identity(current)
            .ok_or_else(|| refused("the native process ancestry is unreadable"))?;
        if process.birth.is_empty() {
            return Err(refused("a native process birth is unavailable"));
        }
        let parent = process.parent;
        chain.push((current, parent, process.birth));
        if current == pid {
            for (node, parent, birth) in &chain {
                let now = crate::platform::process_identity(*node)
                    .ok_or_else(|| refused("the native process ancestry changed"))?;
                if now.parent != *parent || now.birth != *birth {
                    return Err(refused("the native process ancestry changed"));
                }
            }
            return Ok(chain.last().unwrap().2.clone());
        }
        if parent == 0 || parent == current {
            break;
        }
        current = parent;
    }
    Err(refused(
        "this process does not descend from the recorded Claude process",
    ))
}

pub(super) fn admit(record: &Value) -> Result<Caller> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if string(record, "agent") != "claude" {
            return Ok(Caller {
                authority: Authority::Unchanged,
            });
        }
        let pinned = pin(record);
        let old_pid = record.get("pid").is_none_or(|value| {
            value.is_null() || value.as_u64().is_some_and(|pid| pid <= u64::from(u32::MAX))
        });
        let old_start = record
            .get("process_start")
            .is_none_or(|value| value.is_null() || value.is_string());
        let legacy = pinned.is_none()
            && old_pid
            && old_start
            && record.get("run_identity_version").is_none();
        match dsh::host_check() {
            dsh::HostCheck::Under => {
                return Err(refused("a resident DeepSeek host owns this process tree"))
            }
            dsh::HostCheck::Unverified(reason) if !legacy => return Err(refused(&reason)),
            _ => {}
        }
        if legacy {
            eprintln!("hgs: legacy Claude hook record has no PID/start pin; retaining existing hook behavior without caller proof");
            return Ok(Caller {
                authority: Authority::Legacy,
            });
        }
        let pinned =
            pinned.ok_or_else(|| refused("the native launch record has lost its PID/start pin"))?;
        if !process_alive(record) {
            return Err(refused(
                "the recorded Claude PID/start pin is no longer live",
            ));
        }
        // A held background job has its own process, not the window's PID.
        let job = continuations::caller_job(record);
        let pid = job.unwrap_or(pinned.0);
        let birth = ancestor(pid)?;
        if !process_alive(record) {
            return Err(refused("the recorded Claude PID/start pin changed"));
        }
        Ok(Caller {
            authority: Authority::Native {
                pin: Some(pinned),
                pid,
                birth,
                background: job.is_some(),
            },
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = record;
        Ok(Caller {})
    }
}

/// This transport is Claude-only; other providers still use the shared hook.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
pub(super) fn channel(record: &Value) -> Result<Caller> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if string(record, "agent") != "claude" {
        return Err(refused("only a Claude run can own this answer channel"));
    }
    admit(record)
}

/// Before background routing can write a cache link or adopt a conversation.
pub(super) fn background(pid: u32) -> Result<Caller> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        if let Some(reason) = dsh::host_check().sentence() {
            return Err(refused(&reason));
        }
        Ok(Caller {
            authority: Authority::Native {
                pin: None,
                pid,
                birth: ancestor(pid)?,
                background: true,
            },
        })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        Ok(Caller {})
    }
}

impl Caller {
    pub(super) fn bind_owner(&mut self, record: &Value) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Authority::Native {
            pin: bound, pid, ..
        } = &mut self.authority
        {
            let pinned = pin(record)
                .ok_or_else(|| refused("the background job's owning window has no live pin"))?;
            if string(record, "agent") != "claude" || !continuations::holds_job(record, *pid) {
                return Err(refused(
                    "the background job no longer belongs to this Claude window",
                ));
            }
            *bound = Some(pinned);
        }
        let _ = record;
        Ok(())
    }

    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    pub(super) fn recheck(&mut self, record: &Value) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        match &self.authority {
            Authority::Unchanged => {}
            Authority::Legacy => {
                if string(record, "agent") != "claude" {
                    return Err(refused("the admitted Claude provider changed"));
                }
                // Once a pin appears, this worker can never downgrade it.
                if pin(record).is_some() || record.get("run_identity_version").is_some() {
                    *self = admit(record)?;
                }
            }
            Authority::Native {
                pin: bound,
                pid,
                birth,
                background,
            } => {
                if string(record, "agent") != "claude"
                    || pin(record).as_ref() != bound.as_ref()
                    || !process_alive(record)
                    || crate::platform::process_identity(*pid).is_none_or(|now| now.birth != *birth)
                {
                    return Err(refused("the admitted Claude run or process changed"));
                }
                if *background && !continuations::holds_job(record, *pid) {
                    return Err(refused(
                        "the background job is no longer held by this Claude window",
                    ));
                }
                if let Some(reason) = dsh::host_check().sentence() {
                    return Err(refused(&reason));
                }
                if ancestor(*pid)? != *birth {
                    return Err(refused("the admitted process birth changed"));
                }
            }
        }
        let _ = record;
        Ok(())
    }
}
