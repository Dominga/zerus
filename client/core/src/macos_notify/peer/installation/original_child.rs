//! App -> its original CLI Child, distinct from the service -> app host check.
//! The PID is used only to obtain the kernel task-name right for an unreaped
//! owned Child. Security receives that task's audit token, NEVER a PID guest.
use super::{service::birth, *};
use crate::macos_notify::peer::{
    audit_token_to_pid, check_audit, checked_live_code, Attributes, AuditToken,
};
use std::process::Child;

// Public <mach/task_info.h>: TASK_AUDIT_TOKEN, sizeof(audit_token_t)/natural_t.
const TASK_AUDIT_TOKEN: u32 = 15;
const AUDIT_COUNT: u32 = 8;
const _: () = assert!(std::mem::size_of::<AuditToken>() == AUDIT_COUNT as usize * 4);

/// Point-in-time correlation from the SAME checked unreaped Child. No Clone,
/// serde, caller-input constructor, task right or effect permission is exported.
pub struct OriginalChildIdentity {
    pid: u32,
    parent: u32,
    birth: String,
    audit: [u32; 8],
}
impl OriginalChildIdentity {
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

unsafe extern "C" {
    // <mach/mach_init.h>: mach_task_self() expands to this libSystem value.
    static mach_task_self_: u32;
    fn task_name_for_pid(task: u32, pid: i32, name: *mut u32) -> i32;
    fn mach_port_deallocate(task: u32, name: u32) -> i32;
}
fn own_task() -> u32 {
    // SAFETY: libSystem's initialized current-task port, never mutated by us.
    unsafe { mach_task_self_ }
}
#[link(name = "bsm")]
unsafe extern "C" {
    fn audit_token_to_euid(token: AuditToken) -> libc::uid_t;
    fn audit_token_to_ruid(token: AuditToken) -> libc::uid_t;
}

/// Own exactly the acquired task-name send right, including every refusal.
/// No task control/read right, process suspension or inherited descriptor.
struct TaskName(u32);
impl TaskName {
    fn acquire(child: &Child) -> Result<Self, Refusal> {
        let mut name = 0;
        // SAFETY: initialized output; only the actual already-birth-checked
        // Child's ID is used, never caller input or a Security PID attribute.
        let status = unsafe { task_name_for_pid(own_task(), child.id() as i32, &mut name) };
        let owned = (name != 0 && name != u32::MAX).then(|| Self(name));
        if status != 0 {
            return Err(Refusal::Child);
        }
        owned.ok_or(Refusal::Child)
    }

    fn audit(&self, child: &Child) -> Result<AuditToken, Refusal> {
        let mut token = AuditToken { values: [0; 8] };
        let mut count = AUDIT_COUNT;
        // SAFETY: live task-name right and an exact initialized 8-word output.
        // task_info accepts the name right for TASK_AUDIT_TOKEN; no task_for_pid.
        let status = unsafe {
            libc::task_info(
                self.0,
                TASK_AUDIT_TOKEN,
                (&mut token as *mut AuditToken).cast(),
                &mut count,
            )
        };
        if status != 0 || count != AUDIT_COUNT {
            return Err(Refusal::Child);
        }
        let token =
            check_audit(token, std::mem::size_of::<AuditToken>() as _).map_err(Refusal::Peer)?;
        // SAFETY: initialized public token, interpreted only through BSM.
        if unsafe { audit_token_to_pid(token) } != child.id() as i32
            || unsafe { audit_token_to_euid(token) } != unsafe { libc::geteuid() }
            || unsafe { audit_token_to_ruid(token) } != unsafe { libc::getuid() }
        {
            return Err(Refusal::Child);
        }
        Ok(token)
    }
}
impl Drop for TaskName {
    fn drop(&mut self) {
        // SAFETY: release our single send-right reference, never a Child/PID.
        unsafe { mach_port_deallocate(own_task(), self.0) };
    }
}

pub(super) fn observe(
    child: &mut Child,
    expected: &Signature,
    revalidate: impl FnMut() -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    observe_identity(child, expected, revalidate).map(|_| ())
}

pub(super) fn observe_identity(
    child: &mut Child,
    expected: &Signature,
    revalidate: impl FnMut() -> Result<(), Refusal>,
) -> Result<OriginalChildIdentity, Refusal> {
    let result = autoreleasepool(|_| check(child, expected, revalidate));
    if result.is_err() {
        drop(child.stdin.take());
    }
    result
}

fn check(
    child: &mut Child,
    expected: &Signature,
    mut revalidate: impl FnMut() -> Result<(), Refusal>,
) -> Result<OriginalChildIdentity, Refusal> {
    // An already extracted input cannot be closed on refusal; refuse it before
    // doing any native observation. The actual input must remain in Child.
    if child.stdin.is_none() {
        return Err(Refusal::Child);
    }
    revalidate()?;
    #[cfg(test)]
    tests::after_revalidation(child);
    let before = birth(child)?;
    let task = TaskName::acquire(child)?;
    let token = task.audit(child)?;
    if birth(child)? != before {
        return Err(Refusal::Child);
    }
    verify(&token, expected)?;
    #[cfg(test)]
    tests::after_first_check(child);
    if task.audit(child)? != token || birth(child)? != before {
        return Err(Refusal::Child);
    }
    // A fresh audit-keyed guest object, not a cached SecCode across possible exec.
    verify(&token, expected)?;
    revalidate()?;
    if task.audit(child)? != token || birth(child)? != before {
        return Err(Refusal::Child);
    }
    Ok(OriginalChildIdentity {
        pid: child.id(),
        parent: before.parent(),
        birth: before.binding(),
        audit: token.values,
    })
}

fn verify(token: &AuditToken, expected: &Signature) -> Result<(), Refusal> {
    let requirement = super::super::requirement(&expected.requirement).map_err(Refusal::Peer)?;
    let attributes = Attributes::new(token).map_err(Refusal::Peer)?;
    let code = checked_live_code(&attributes, &requirement).map_err(Refusal::Peer)?;
    // Read signing data from that SAME checked live object. The genuine retained
    // Selection already pins identifier/team/requirement and native CDHash.
    let info = information(code.as_ptr())?;
    if info_path(&info)? != expected.executable || info_unique(&info)? != expected.unique {
        return Err(Refusal::Child);
    }
    // Revalidate after reading signing metadata, with no static/PID fallback.
    if unsafe { super::super::SecCodeCheckValidity(code.as_ptr(), 0, requirement.as_ptr()) } != 0 {
        return Err(Refusal::Peer(super::super::Refusal::LiveCodeRejected));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
