//! Native snapshots for explicit Accounts edits. Uses the existing screens
//! catalog slot, so a background read cannot overwrite a mutation's reply.
use super::*;
use serde::Deserialize;
use std::collections::VecDeque;
use zerus_client_core::account_actions::{
    self,
    models::Catalog as ModelCatalog,
    permissions::{
        Catalog as PermissionCatalog, Check as PermissionCheck, Editor as PermissionEditor,
        Selection,
    },
    Binding, Kind, Values,
};
use zerus_client_core::text::{Arg, Text};
pub mod distribution;
pub mod setup;

#[derive(Default)]
pub(super) struct Edits(Mutex<EditState>);
#[derive(Default)]
struct EditState {
    plans: VecDeque<(String, Plan)>,
    setups: VecDeque<(String, setup::Plan)>,
    distributions: VecDeque<(String, distribution::Plan)>,
    busy: bool,
}
struct Plan {
    kind: Kind,
    targets: Vec<Target>,
    models: Option<ModelCatalog>,
    /// Task 68: the profile's native permission catalog, captured at open.
    permissions: Option<PermissionCatalog>,
    /// A permission change opened as shared: every member must still share
    /// it when it is saved (zerus-f0uz8).
    shared: bool,
    /// The account the plan was opened for.
    account: String,
}
struct Target {
    machine: String,
    binding: Result<Binding, String>,
    /// The computer's hgs keeps a permission scope (zerus-f0uz8).
    scope_supported: bool,
    /// The computer's hgs keeps account marks (zerus-sa9qn.6).
    marks_supported: bool,
    /// The member's profile key (`host + U+001F + id`).
    key: String,
}
#[derive(Clone, Serialize)]
pub struct EditView {
    handle: String,
    kind: Kind,
    label: String,
    machine: String,
    provider: String,
    permission_mode: String,
    permission_detail: String,
    models: Option<ModelCatalog>,
    /// Task 68: the permission selector and native editor (Qt's
    /// accountChoices and PermissionCatalogDialog), for Permissions only.
    permissions: Option<PermissionEditor>,
    /// Every computer this edit saves on, the selected one first: a model,
    /// a shared permission mode or the scope reach all of the account's
    /// computers (zerus-f0uz8).
    machines: Vec<String>,
    /// Those computers' current values differ, so saving the shown value
    /// is a change even when it equals the selected computer's.
    differs: bool,
    /// The account keeps its permission mode per computer.
    per_computer: bool,
}
#[derive(Serialize)]
pub struct EditResult {
    /// Translated by the webview (zerus-0y2yx); errors carry hgs's own text.
    notices: Vec<Text>,
    errors: Vec<String>,
    /// Members whose computer keeps no marks (an older hgs): the webview
    /// keeps their mark as this desktop's preference (zerus-sa9qn.6).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    local: Vec<String>,
    /// Members whose computer saved the mark: the desktop forgets its own.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    marked: Vec<String>,
}
#[derive(Deserialize, Default)]
pub struct EditRequest {
    handle: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    effort: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    emoji: String,
}

struct Busy<'a>(&'a Edits);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0
             .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .busy = false;
    }
}
impl Screens {
    fn edit_open(
        &self,
        pump: &crate::pump::Pump,
        account: &str,
        profile: &str,
        kind: Kind,
    ) -> Result<EditView, String> {
        if !self.0.allowed {
            return Err(REPLAY_NEEDS_HGS.into());
        }
        let _opening = {
            let mut state = self
                .0
                .edits
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.busy {
                return Err(account_actions::UPDATING.into());
            }
            state.busy = true;
            Busy(&self.0.edits)
        };
        let view = self.accounts(false);
        let group = view
            .accounts
            .iter()
            .find(|a| a.key == account)
            .ok_or(account_actions::CHANGED)?;
        let selected = group
            .members
            .iter()
            .find(|m| m.key == profile)
            .ok_or(account_actions::CHANGED)?;
        if !matches!(kind, Kind::Rename | Kind::Mark) && !selected.online {
            return Err(format!("{} is offline.", selected.machine));
        }
        if kind == Kind::Default
            && (selected.is_default || matches!(selected.provider.as_str(), "dsh" | "opencode"))
        {
            return Err(account_actions::CHANGED.into());
        }
        if kind == Kind::Permissions && selected.provider == "opencode" {
            return Err("OpenCode permissions are managed in its native configuration".into());
        }
        if kind == Kind::Model && !selected.model_supported {
            return Err("This agent manages model defaults in its native settings.".into());
        }
        if kind == Kind::Scope && selected.provider == "opencode" {
            return Err("OpenCode permissions are managed in its native configuration".into());
        }
        // One model and effort per account, and one permission mode unless
        // the account keeps it per computer (owner, 2026-11-07). The selected
        // computer comes first: its model catalog validates the choice.
        let fan_out = |keep: &dyn Fn(&&accounts::Profile) -> bool| {
            std::iter::once(selected)
                .chain(group.members.iter().filter(|m| m.key != selected.key))
                .filter(keep)
                .collect::<Vec<_>>()
        };
        let members: Vec<_> = match kind {
            Kind::Rename => group.members.iter().collect(),
            Kind::Model => fan_out(&|m| m.model_supported),
            Kind::Permissions if !group.permissions_per_computer => {
                fan_out(&|m| m.provider != "opencode")
            }
            Kind::Scope => fan_out(&|m| m.provider != "opencode"),
            // One mark per account, kept on each of its computers.
            Kind::Mark => fan_out(&|_| true),
            _ => vec![selected],
        };
        let differs = match kind {
            Kind::Model => group.model_differs,
            Kind::Permissions => group.permission_differs && !group.permissions_per_computer,
            _ => false,
        };
        if members.len() > account_actions::MAX_MEMBERS {
            return Err(account_actions::CHANGED.into());
        }
        let machines: BTreeMap<_, _> = members
            .iter()
            .map(|member| {
                (
                    member.host.clone(),
                    pump.machine(&member.host).and_then(|m| m.machine_id),
                )
            })
            .collect();
        let hub = self.lock();
        let raw = hub
            .accounts
            .catalogs
            .get(&selected.host)
            .and_then(|s| s.data.as_ref())
            .and_then(|v| v["profiles"].as_array())
            .and_then(|rows| rows.iter().find(|r| r["id"] == selected.id))
            .ok_or(account_actions::CHANGED)?;
        let permissions = (kind == Kind::Permissions).then(|| PermissionCatalog::parse(raw));
        let editor = permissions
            .as_ref()
            .map(|catalog| catalog.editor(&selected.machine));
        // The saved selector the dialog opens on; saving it again changes nothing.
        let permission_mode = editor
            .as_ref()
            .map(|e| e.initial.clone())
            .unwrap_or_else(|| {
                if raw["permission_mode"] == "bypass" {
                    "bypass"
                } else {
                    "provider"
                }
                .to_owned()
            });
        let permission_detail = raw["permission_detail"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(2000)
            .collect();
        let machines_shown: Vec<String> = members.iter().map(|m| m.machine.clone()).collect();
        let targets: Vec<Target> = members
            .into_iter()
            .map(|member| {
                let scope_supported = hub
                    .accounts
                    .catalogs
                    .get(&member.host)
                    .and_then(|s| s.data.as_ref())
                    .is_some_and(|catalog| catalog["permission_scope_supported"] == true);
                let binding = (|| {
                    if !member.online {
                        return Err(format!("{} is offline.", member.machine));
                    }
                    let machine = machines
                        .get(&member.host)
                        .cloned()
                        .flatten()
                        .ok_or(account_actions::CHANGED)?;
                    let catalog = hub
                        .accounts
                        .catalogs
                        .get(&member.host)
                        .and_then(|s| s.data.as_ref())
                        .ok_or(account_actions::CHANGED)?;
                    let binding =
                        Binding::capture(member.host.clone(), machine, catalog, &member.id)?;
                    if binding.provider != member.provider
                        || binding.home != member.home
                        || binding.auth_revision != member.auth_revision
                    {
                        return Err(account_actions::CHANGED.into());
                    }
                    Ok(binding)
                })();
                Target {
                    machine: member.machine.clone(),
                    binding,
                    scope_supported,
                    marks_supported: member.marks_supported,
                    key: member.key.clone(),
                }
            })
            .collect();
        drop(hub);
        let models = if kind == Kind::Model {
            let binding = targets[0].binding.as_ref().map_err(Clone::clone)?;
            Some(binding.model_settings(&self.0.hgs)?)
        } else {
            None
        };
        let handle = uuid::Uuid::new_v4().to_string();
        let shown = EditView {
            handle: handle.clone(),
            kind,
            label: group.label.clone(),
            machine: selected.machine.clone(),
            provider: group.provider.clone(),
            permission_mode,
            permission_detail,
            models: models.clone(),
            permissions: editor,
            machines: machines_shown,
            differs,
            per_computer: group.permissions_per_computer,
        };
        let mut edits = self
            .0
            .edits
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while edits.plans.len() >= 8 {
            edits.plans.pop_front();
        }
        edits.plans.push_back((
            handle,
            Plan {
                kind,
                shared: kind == Kind::Permissions && !group.permissions_per_computer,
                account: group.key.clone(),
                targets,
                models,
                permissions,
            },
        ));
        Ok(shown)
    }
    /// Task 68: the native editor's answer about one selection, from the
    /// catalog captured when the dialog opened. Reads nothing; sends nothing.
    fn edit_permission_check(
        &self,
        handle: &str,
        selection: Selection,
    ) -> Result<PermissionCheck, String> {
        if selection.len() > 64 {
            return Err(account_actions::CHANGED.into());
        }
        let edits = self
            .0
            .edits
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (_, plan) = edits
            .plans
            .iter()
            .find(|(key, _)| key == handle)
            .ok_or(account_actions::CHANGED)?;
        let catalog = plan.permissions.as_ref().ok_or(account_actions::CHANGED)?;
        Ok(catalog.check(&selection))
    }
    fn edit_begin(&self, binding: &Binding) -> Result<(), String> {
        self.catalog_begin(&binding.computer, |catalog| binding.verify(catalog))
    }
    fn catalog_begin(
        &self,
        computer: &ComputerId,
        verify: impl FnOnce(&Value) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut hub = self.current();
        if !hub
            .fleet
            .iter()
            .any(|e| e.computer == *computer && accounts::reachable(e))
        {
            return Err("Machine is offline.".into());
        }
        let slot = hub
            .accounts
            .catalogs
            .get_mut(computer)
            .ok_or(account_actions::CHANGED)?;
        if slot.pending {
            return Err(account_actions::UPDATING.into());
        }
        verify(slot.data.as_ref().ok_or(account_actions::CHANGED)?)?;
        // Borrow the same slot as catalog reads. Explicit refresh may queue one
        // read behind this edit, but cannot race an older reply into the store.
        slot.pending = true;
        slot.requested = Some(self.now());
        hub.dirty.extend(ACCOUNT_SCREENS);
        drop(hub);
        self.publish();
        Ok(())
    }
    fn edit_finish(&self, host: &ComputerId, result: Result<Value, String>) {
        let (runs, heard) = self.catalog_reply(host.clone(), result, self.now(), true);
        self.run(runs);
        self.publish();
        self.publish_session_usage();
        if let (Some(host), Some(notify)) = (heard, self.0.launch_notify.get()) {
            notify(&host);
        }
    }

    fn edit_apply(
        &self,
        pump: &crate::pump::Pump,
        request: EditRequest,
    ) -> Result<EditResult, String> {
        let plan = {
            let mut edits = self
                .0
                .edits
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if edits.busy {
                return Err(account_actions::UPDATING.into());
            }
            let index = edits
                .plans
                .iter()
                .position(|(key, _)| key == &request.handle)
                .ok_or(account_actions::CHANGED)?;
            let (_, plan) = edits.plans.remove(index).ok_or(account_actions::CHANGED)?;
            edits.busy = true;
            plan
        };
        // The handle is consumed before any effect. No exit, retry or reopened
        // webview can dispatch this captured plan for a second time.
        let _busy = Busy(&self.0.edits);
        let mut values = Values {
            label: request.label,
            mode: request.mode,
            model: request.model,
            effort: request.effort,
            color: request.color,
            emoji: request.emoji,
            ..Default::default()
        };
        if plan.kind == Kind::Permissions {
            // Only a selector or native map the captured catalog offers.
            values.mode = plan
                .permissions
                .as_ref()
                .ok_or(account_actions::CHANGED)?
                .mode_to_save(&values.mode)?;
        }
        if plan.kind == Kind::Model {
            plan.models
                .as_ref()
                .ok_or(account_actions::CHANGED)?
                .validate(&values)?;
        }
        let mut reply = EditResult {
            notices: Vec::new(),
            errors: Vec::new(),
            local: Vec::new(),
            marked: Vec::new(),
        };
        if plan.shared {
            // The scope is outside the catalog revision: before any write,
            // every member must still share its permissions. A member that
            // keeps them per computer since the dialog opened makes the
            // account per computer, so nothing is saved (zerus-f0uz8): the
            // hub's current view of every member (offline and newly added
            // ones too), then a fresh read of each computer written.
            let still_shared = self
                .accounts(false)
                .accounts
                .iter()
                .find(|account| account.key == plan.account)
                .is_some_and(|account| !account.permissions_per_computer);
            if !still_shared {
                return Ok(EditResult {
                    notices: Vec::new(),
                    errors: vec![PER_COMPUTER_NOW.into()],
                    local: Vec::new(),
                    marked: Vec::new(),
                });
            }
            for target in &plan.targets {
                let Ok(binding) = target.binding.as_ref() else {
                    continue;
                };
                let current = binding.fresh(&self.0.hgs).ok();
                let per_computer = current.as_ref().is_some_and(|catalog| {
                    catalog["profiles"].as_array().is_some_and(|rows| {
                        rows.iter().any(|row| {
                            row["id"] == binding.id.as_str()
                                && row["permission_scope"] == "computer"
                        })
                    })
                });
                if per_computer {
                    return Ok(EditResult {
                        notices: Vec::new(),
                        errors: vec![format!("{}: {PER_COMPUTER_NOW}", target.machine)],
                        local: Vec::new(),
                        marked: Vec::new(),
                    });
                }
            }
        }
        let mut own_revisions: BTreeMap<ComputerId, (String, String)> = BTreeMap::new();
        // Computers saved by a model, permission or scope edit: one notice
        // names them all; each failure keeps its own computer.
        let mut saved: Vec<String> = Vec::new();
        for target in plan.targets {
            if plan.kind == Kind::Scope && !target.scope_supported {
                // An older computer keeps no scope: sharing needs nothing
                // there, but per-computer permissions need a newer hgs.
                if values.mode != "account" {
                    reply.errors.push(format!(
                        "{}: Update hgs there to keep its permissions per computer.",
                        target.machine
                    ));
                }
                continue;
            }
            if plan.kind == Kind::Mark && !target.marks_supported {
                // An older hgs keeps no marks: this desktop keeps the
                // member's mark until that computer updates.
                reply.local.push(target.key.clone());
                continue;
            }
            let error_machine = target
                .binding
                .as_ref()
                .ok()
                .filter(|binding| binding.computer.is_local())
                .map(|_| "This machine")
                .unwrap_or(&target.machine)
                .to_owned();
            // Each shared write also names the scope, checked under that
            // computer's catalog lock; an older hgs keeps no scope.
            values.expect_scope = if plan.shared && target.scope_supported {
                "account".into()
            } else {
                String::new()
            };
            let result = (|| {
                let mut binding = target.binding?;
                let original_revision = binding.revision.clone();
                if let Some((original, current)) = own_revisions.get(&binding.computer) {
                    if original != &original_revision {
                        return Err(account_actions::CHANGED.into());
                    }
                    // A logical account may have two profiles on one machine.
                    // Advance only through this plan's confirmed replies; all
                    // profile/auth and fresh-read checks remain unchanged.
                    binding.revision = current.clone();
                }
                if !pump.online(&binding.computer.0)
                    || pump
                        .machine(&binding.computer)
                        .and_then(|m| m.machine_id)
                        .as_deref()
                        != Some(&binding.machine_id)
                {
                    return Err(account_actions::CHANGED.into());
                }
                // Reject invalid input before borrowing the catalog slot.
                binding.edit_values(plan.kind, &values)?;
                self.edit_begin(&binding)?;
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    binding.execute_values(&self.0.hgs, plan.kind, &values)
                }))
                .unwrap_or_else(|_| Err(READ_FAILED.into()));
                self.edit_finish(&binding.computer, outcome.clone());
                outcome.map(|catalog| {
                    (
                        binding.computer,
                        original_revision,
                        catalog["revision"].as_str().unwrap_or_default().to_owned(),
                    )
                })
            })();
            match result {
                Ok((computer, original, current)) => {
                    own_revisions.insert(computer, (original, current));
                    if plan.kind == Kind::Mark {
                        reply.marked.push(target.key.clone());
                    }
                    if matches!(
                        plan.kind,
                        Kind::Model | Kind::Permissions | Kind::Scope | Kind::Mark
                    ) {
                        saved.push(target.machine.clone());
                        continue;
                    }
                    reply.notices.push(match plan.kind {
                        Kind::Rename
                        | Kind::Model
                        | Kind::Permissions
                        | Kind::Scope
                        | Kind::Mark => continue,
                        Kind::Default => zerus_client_core::text!(
                            "Default account saved for new sessions on %1.",
                            target.machine.clone()
                        ),
                        Kind::Remove => zerus_client_core::text!(
                            "Account removed from %1. Add it again whenever you need it.",
                            target.machine.clone()
                        ),
                    });
                }
                Err(error) => {
                    reply.errors.push(format!("{error_machine}: {error}"));
                    // AccountsPage clears its remaining rename queue on the
                    // first failed member; a partial rename is never replayed.
                    if plan.kind == Kind::Rename {
                        break;
                    }
                }
            }
        }
        if plan.kind == Kind::Rename && reply.errors.is_empty() {
            reply
                .notices
                .push(zerus_client_core::text!("Account name updated."));
        }
        if !saved.is_empty() {
            let on = machine_list(&saved);
            reply.notices.push(match plan.kind {
                Kind::Model => {
                    zerus_client_core::text!("Model defaults saved for new sessions on %1.", on)
                }
                Kind::Permissions => zerus_client_core::text!(
                    "Permission mode saved for new and resumed sessions on %1.",
                    on
                ),
                Kind::Mark => zerus_client_core::text!("Color and mark saved on %1.", on),
                _ if values.mode == "computer" => {
                    zerus_client_core::text!("Permissions are now kept per computer on %1.", on)
                }
                _ => zerus_client_core::text!("Permissions are now shared by %1.", on),
            });
        }
        reply.notices.dedup();
        Ok(reply)
    }

    /// Move this desktop's mark of one member profile to its computer, once
    /// (zerus-sa9qn.6): only the fields that computer has no value for, so a
    /// value another desktop moved there first stays. Ok(false) when that
    /// computer is offline or keeps no marks: the desktop keeps its own.
    fn mark_migrate(
        &self,
        pump: &crate::pump::Pump,
        profile: &str,
        color: &str,
        emoji: &str,
    ) -> Result<bool, String> {
        if !self.0.allowed {
            return Err(REPLAY_NEEDS_HGS.into());
        }
        let _migrating = {
            let mut state = self
                .0
                .edits
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.busy {
                return Err(account_actions::UPDATING.into());
            }
            state.busy = true;
            Busy(&self.0.edits)
        };
        let view = self.accounts(false);
        let Some(member) = view
            .accounts
            .iter()
            .flat_map(|account| account.members.iter())
            .find(|member| member.key == profile)
        else {
            return Ok(false);
        };
        if !member.online || !member.marks_supported {
            return Ok(false);
        }
        let values = Values {
            color: color.into(),
            emoji: emoji.into(),
            if_unset: true,
            ..Default::default()
        };
        let machine = pump
            .machine(&member.host)
            .and_then(|m| m.machine_id)
            .ok_or(account_actions::CHANGED)?;
        let binding = {
            let hub = self.lock();
            let catalog = hub
                .accounts
                .catalogs
                .get(&member.host)
                .and_then(|s| s.data.as_ref())
                .ok_or(account_actions::CHANGED)?;
            Binding::capture(member.host.clone(), machine, catalog, &member.id)?
        };
        if binding.provider != member.provider
            || binding.home != member.home
            || binding.auth_revision != member.auth_revision
            || !pump.online(&binding.computer.0)
        {
            return Err(account_actions::CHANGED.into());
        }
        binding.edit_values(Kind::Mark, &values)?;
        self.edit_begin(&binding)?;
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            binding.execute_values(&self.0.hgs, Kind::Mark, &values)
        }))
        .unwrap_or_else(|_| Err(READ_FAILED.into()));
        self.edit_finish(&binding.computer, outcome.clone());
        outcome.map(|_| true)
    }
}

const PER_COMPUTER_NOW: &str =
    "Permissions on this account are now kept per computer. Nothing was saved; open Change… again.";

/// `arch`, `arch and mac`, `arch, mac and va-kids`, translated by the
/// webview: names verbatim, the joins as nested prose.
pub(crate) fn machine_list(machines: &[String]) -> Arg {
    match machines {
        [] => Arg::Verbatim(String::new()),
        [one] => Arg::Verbatim(one.clone()),
        [one, last] => Arg::Text(zerus_client_core::text!(
            "%1 and %2",
            one.clone(),
            last.clone()
        )),
        [first, rest @ ..] => Arg::Text(zerus_client_core::text!(
            "%1, %2",
            first.clone(),
            machine_list(rest)
        )),
    }
}

#[cfg(feature = "live")]
#[tauri::command(async)]
pub fn account_edit_open(
    state: State<'_, Screens>,
    source: State<'_, crate::commands::AppState>,
    account: String,
    profile: String,
    kind: Kind,
) -> Result<EditView, String> {
    state.edit_open(&source.pump(), &account, &profile, kind)
}
#[cfg(feature = "live")]
#[tauri::command(async)]
pub fn account_edit_apply(
    state: State<'_, Screens>,
    source: State<'_, crate::commands::AppState>,
    request: EditRequest,
) -> Result<EditResult, String> {
    state.edit_apply(&source.pump(), request)
}
/// Task 68: checks one native permission selection of an open dialog.
#[tauri::command]
pub fn account_permission_check(
    state: State<'_, Screens>,
    handle: String,
    selection: Selection,
) -> Result<PermissionCheck, String> {
    state.edit_permission_check(&handle, selection)
}
#[cfg(feature = "live")]
#[tauri::command(async)]
pub fn account_mark_migrate(
    state: State<'_, Screens>,
    source: State<'_, crate::commands::AppState>,
    profile: String,
    color: String,
    emoji: String,
) -> Result<bool, String> {
    state.mark_migrate(&source.pump(), &profile, &color, &emoji)
}
#[cfg(not(feature = "live"))]
#[tauri::command]
pub fn account_mark_migrate() -> Result<bool, String> {
    Err(REPLAY_NEEDS_HGS.into())
}
#[tauri::command]
pub fn account_edit_close(state: State<'_, Screens>, handle: String) {
    state
        .0
        .edits
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .plans
        .retain(|(key, _)| key != &handle);
}
#[cfg(not(feature = "live"))]
#[tauri::command]
pub fn account_edit_open() -> Result<EditView, String> {
    Err(REPLAY_NEEDS_HGS.into())
}
#[cfg(not(feature = "live"))]
#[tauri::command]
pub fn account_edit_apply() -> Result<EditResult, String> {
    Err(REPLAY_NEEDS_HGS.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use zerus_client_core::{Change, ComputerStatus, MachineMeta};
    pub(super) fn fixture() -> (Screens, crate::pump::Pump, Binding) {
        fixture_at(PathBuf::from("unused-hgs"))
    }
    pub(super) fn fixture_at(hgs: PathBuf) -> (Screens, crate::pump::Pump, Binding) {
        let local = ComputerId::local();
        let fleet = vec![FleetEntry {
            computer: local.clone(),
            status: "live".into(),
            attempt: 0,
            local_sessions: true,
            local_host: false,
            host: Some("workstation".into()),
            platform: Some("linux".into()),
        }];
        let screens = Screens::new(
            hgs,
            true,
            Arc::new(|_| {}),
            Arc::new(|_| {}),
            Arc::new(move || fleet.clone()),
            Arc::new(|| 0),
        );
        let catalog = json!({"revision":"r1","setup_revision":true,"profiles":[{"id":"work","provider":"codex","label":"Example","home":"/accounts/example","auth_revision":"a1","installed":false}]});
        screens.lock().accounts.catalogs.insert(
            local.clone(),
            accounts::CatalogSlot {
                requested: Some(0),
                data: Some(catalog.clone()),
                ..Default::default()
            },
        );
        let pump = crate::pump::Pump::default();
        pump.push(Change::Computer {
            computer: local.clone(),
            status: ComputerStatus::Live,
        });
        let machine = "11111111-1111-4111-8111-111111111111";
        pump.push(Change::Machine {
            computer: local.clone(),
            machine: MachineMeta {
                machine_id: Some(machine.into()),
                ..Default::default()
            },
        });
        let binding = Binding::capture(local, machine.into(), &catalog, "work").unwrap();
        (screens, pump, binding)
    }
    #[test]
    fn captures_the_native_group_and_consumes_a_stale_handle_once() {
        let (screens, pump, _) = fixture();
        let view = screens.accounts(false);
        let group = &view.accounts[0];
        let opened = screens
            .edit_open(&pump, &group.key, &group.members[0].key, Kind::Rename)
            .unwrap();
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&ComputerId::local())
            .unwrap()
            .data
            .as_mut()
            .unwrap()["revision"] = json!("r2");
        let result = screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: opened.handle.clone(),
                    label: "Updated".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains(account_actions::CHANGED));
        assert!(screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: opened.handle,
                    label: "Updated".into(),
                    ..Default::default()
                }
            )
            .is_err());
        assert!(!screens.0.edits.0.lock().unwrap().busy);
    }
    #[test]
    fn a_changed_physical_machine_refuses_the_captured_profile() {
        let (screens, pump, _) = fixture();
        let view = screens.accounts(false);
        let group = &view.accounts[0];
        let opened = screens
            .edit_open(&pump, &group.key, &group.members[0].key, Kind::Remove)
            .unwrap();
        pump.push(Change::Machine {
            computer: ComputerId::local(),
            machine: MachineMeta {
                machine_id: Some("22222222-2222-4222-8222-222222222222".into()),
                ..Default::default()
            },
        });
        let result = screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: opened.handle,
                    label: String::new(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(result.notices.is_empty());
        assert!(result.errors[0].contains(account_actions::CHANGED));
    }
    #[test]
    fn uses_the_existing_catalog_slot_and_releases_it_after_a_failed_effect() {
        let (screens, _, binding) = fixture();
        screens.edit_begin(&binding).unwrap();
        assert_eq!(
            screens.edit_begin(&binding).unwrap_err(),
            account_actions::UPDATING
        );
        screens.edit_finish(&binding.computer, Err("Uncertain result".into()));
        let hub = screens.lock();
        let slot = &hub.accounts.catalogs[&binding.computer];
        assert!(!slot.pending);
        assert_eq!(slot.data.as_ref().unwrap()["revision"], "r1");
        assert_eq!(
            slot.error, None,
            "mutation errors use the notice bus, not a catalog-read banner"
        );
    }
    #[test]
    fn native_pending_handles_have_a_fixed_bound() {
        let (screens, pump, _) = fixture();
        let view = screens.accounts(false);
        let group = &view.accounts[0];
        for _ in 0..20 {
            screens
                .edit_open(&pump, &group.key, &group.members[0].key, Kind::Remove)
                .unwrap();
        }
        assert_eq!(screens.0.edits.0.lock().unwrap().plans.len(), 8);
    }
    #[test]
    fn a_model_open_shares_single_admission_and_cannot_clear_another_operation() {
        let (screens, pump, _) = fixture();
        let view = screens.accounts(false);
        let account = &view.accounts[0];
        screens.0.edits.0.lock().unwrap().busy = true;
        assert!(screens
            .edit_open(&pump, &account.key, &account.members[0].key, Kind::Model)
            .is_err());
        assert!(screens.0.edits.0.lock().unwrap().busy);
    }
    #[test]
    fn permissions_capture_the_raw_override_and_models_reject_forged_choices() {
        let (screens, pump, binding) = fixture();
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&binding.computer)
            .unwrap()
            .data
            .as_mut()
            .unwrap()["profiles"][0]["permission_mode"] = json!("bypass");
        let view = screens.accounts(false);
        let account = &view.accounts[0];
        let opened = screens
            .edit_open(
                &pump,
                &account.key,
                &account.members[0].key,
                Kind::Permissions,
            )
            .unwrap();
        assert_eq!(opened.permission_mode, "bypass");
        assert!(opened.models.is_none());
        assert_eq!(
            opened.permissions.as_ref().unwrap().choices[1].value,
            "bypass"
        );
        screens.0.edits.0.lock().unwrap().plans.push_back((
            "model".into(),
            Plan {
                shared: false,
                account: String::new(),
                kind: Kind::Model,
                targets: vec![Target {
                    machine: "workstation".into(),
                    binding: Ok(binding),
                    scope_supported: true,
                    marks_supported: true,
                    key: String::new(),
                }],
                models: Some(ModelCatalog {
                    models: Vec::new(),
                    defaults: Default::default(),
                    provider_defaults: Default::default(),
                    effort_supported: true,
                }),
                permissions: None,
            },
        ));
        let request = EditRequest {
            handle: "model".into(),
            model: "forged".into(),
            ..Default::default()
        };
        assert!(screens.edit_apply(&pump, request).is_err());
        assert!(!screens.0.edits.0.lock().unwrap().busy);
        assert!(!screens
            .0
            .edits
            .0
            .lock()
            .unwrap()
            .plans
            .iter()
            .any(|(id, _)| id == "model"));
    }
    #[test]
    fn native_permissions_are_checked_against_the_captured_catalog() {
        let (screens, pump, binding) = fixture();
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&binding.computer)
            .unwrap()
            .data
            .as_mut()
            .unwrap()["profiles"][0]["permission_catalog"] = json!({"status":"available","source":"codex","controls":[
                {"id":"approval","label":"Approval","options":[{"value":"never","label":"Never"}],"conflicts_with":["sandbox"]},
                {"id":"sandbox","label":"Sandbox","options":[{"value":"read-only"}]}]});
        let view = screens.accounts(false);
        let account = &view.accounts[0];
        let opened = screens
            .edit_open(
                &pump,
                &account.key,
                &account.members[0].key,
                Kind::Permissions,
            )
            .unwrap();
        assert_eq!(opened.permission_mode, "provider");
        assert!(opened
            .permissions
            .as_ref()
            .unwrap()
            .choices
            .iter()
            .any(|c| c.editor));
        let check = screens
            .edit_permission_check(
                &opened.handle,
                Selection::from([("approval".into(), "never".into())]),
            )
            .unwrap();
        assert!(check.problem.is_empty());
        assert_eq!(check.mode, r#"native:{"approval":"never"}"#);
        let both = Selection::from([
            ("approval".into(), "never".into()),
            ("sandbox".into(), "read-only".into()),
        ]);
        assert!(!screens
            .edit_permission_check(&opened.handle, both)
            .unwrap()
            .problem
            .is_empty());
        assert!(screens
            .edit_permission_check("unknown", Selection::new())
            .is_err());
        // A conflicting or withdrawn map is refused before anything runs.
        let request = EditRequest {
            handle: opened.handle.clone(),
            mode: r#"native:{"approval":"never","sandbox":"read-only"}"#.into(),
            ..Default::default()
        };
        assert!(screens.edit_apply(&pump, request).is_err());
        assert!(!screens.0.edits.0.lock().unwrap().busy);
    }
    #[test]
    fn a_new_session_opened_during_a_write_receives_its_catalog_reply() {
        let (screens, _, binding) = fixture();
        screens.edit_begin(&binding).unwrap();
        assert!(screens
            .launch_start(&binding.computer, false, Some("codex"))
            .is_empty());
        assert!(!screens.launch_rows(&binding.computer).loaded);
        let catalog = screens.lock().accounts.catalogs[&binding.computer]
            .data
            .clone()
            .unwrap();
        screens.edit_finish(&binding.computer, Ok(catalog));
        assert!(screens.launch_rows(&binding.computer).loaded);
    }
    #[test]
    fn rename_stops_at_the_first_failed_member_as_qt_does() {
        let (screens, pump, _) = fixture();
        screens.0.edits.0.lock().unwrap().plans.push_back((
            "captured".into(),
            Plan {
                shared: false,
                account: String::new(),
                models: None,
                permissions: None,
                kind: Kind::Rename,
                targets: vec![
                    Target {
                        machine: "studio".into(),
                        binding: Err("offline".into()),
                        scope_supported: true,
                        marks_supported: true,
                        key: String::new(),
                    },
                    Target {
                        machine: "other".into(),
                        binding: Err("must not run".into()),
                        scope_supported: true,
                        marks_supported: true,
                        key: String::new(),
                    },
                ],
            },
        ));
        let result = screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: "captured".into(),
                    label: "Updated".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(result.notices.is_empty());
        assert_eq!(result.errors, ["studio: offline"]);
    }
    #[cfg(unix)]
    #[test]
    fn rename_advances_only_its_own_revision_for_two_profiles_on_one_machine() {
        use std::os::unix::fs::PermissionsExt;
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "zerus-account-rename-{}-{stamp}",
            std::process::id()
        )));
        std::fs::create_dir(&scratch.0).unwrap();
        let hgs = scratch.0.join("hgs");
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../core/tests/support/fake_hgs_account_edits.py"),
            &hgs,
        )
        .unwrap();
        std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (screens, pump, first) = fixture_at(hgs);
        let mut catalog = screens.lock().accounts.catalogs[&first.computer]
            .data
            .clone()
            .unwrap();
        let mut second = catalog["profiles"][0].clone();
        second["id"] = json!("second");
        second["home"] = json!("/accounts/second");
        catalog["profiles"].as_array_mut().unwrap().push(second);
        std::fs::write(scratch.0.join("catalog.json"), catalog.to_string()).unwrap();
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&first.computer)
            .unwrap()
            .data = Some(catalog.clone());
        let second = Binding::capture(
            first.computer.clone(),
            first.machine_id.clone(),
            &catalog,
            "second",
        )
        .unwrap();
        screens.0.edits.0.lock().unwrap().plans.push_back((
            "same-machine".into(),
            Plan {
                shared: false,
                account: String::new(),
                models: None,
                permissions: None,
                kind: Kind::Rename,
                targets: vec![
                    Target {
                        machine: "workstation".into(),
                        binding: Ok(first),
                        scope_supported: true,
                        marks_supported: true,
                        key: String::new(),
                    },
                    Target {
                        machine: "workstation".into(),
                        binding: Ok(second),
                        scope_supported: true,
                        marks_supported: true,
                        key: String::new(),
                    },
                ],
            },
        ));
        let result = screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: "same-machine".into(),
                    label: "Updated".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(english(&result.notices), ["Account name updated."]);
        let saved: Value =
            serde_json::from_slice(&std::fs::read(scratch.0.join("catalog.json")).unwrap())
                .unwrap();
        assert_eq!(saved["revision"], "r3");
        assert!(saved["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["label"] == "Updated"));
        assert_eq!(
            std::fs::read_to_string(scratch.0.join("calls.jsonl"))
                .unwrap()
                .lines()
                .count(),
            4
        );
    }
    #[cfg(unix)]
    #[test]
    fn a_mark_reaches_each_computer_that_keeps_marks_and_migration_fills_only_unset_fields() {
        // zerus-sa9qn.6: the account's mark is kept on its computers; a
        // computer with an older hgs keeps it as this desktop's preference.
        use std::os::unix::fs::PermissionsExt;
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "zerus-account-marks-{}-{stamp}",
            std::process::id()
        )));
        std::fs::create_dir(&scratch.0).unwrap();
        let hgs = scratch.0.join("hgs");
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../core/tests/support/fake_hgs_account_edits.py"),
            &hgs,
        )
        .unwrap();
        std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (screens, pump, first) = fixture_at(hgs);
        let profile = |id: &str, home: &str| {
            json!({"id":id,"provider":"codex","label":"Example","home":home,"auth_revision":"a1","installed":true,
                "account_status":{"status":"ok","identity":{"account_id":"example-account"}}})
        };
        let mut work = profile("work", "/accounts/example");
        work["mark"] = json!({"color":null,"emoji":null});
        // `second` stands for an older hgs: its row has no `mark`.
        let catalog = json!({"revision":"r1","setup_revision":true,"marks_supported":true,
            "profiles":[work, profile("second", "/accounts/second")]});
        std::fs::write(scratch.0.join("catalog.json"), catalog.to_string()).unwrap();
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&first.computer)
            .unwrap()
            .data = Some(catalog);
        let group = || {
            screens
                .accounts(false)
                .accounts
                .iter()
                .find(|a| a.members.len() == 2)
                .expect("one account on two profiles")
                .clone()
        };
        let member = |id: &str| group().members.into_iter().find(|m| m.id == id).unwrap();
        assert!(member("work").marks_supported && !member("second").marks_supported);
        let opened = screens
            .edit_open(&pump, &group().key, &member("work").key, Kind::Mark)
            .unwrap();
        assert_eq!(opened.machines.len(), 2);
        let result = screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: opened.handle,
                    color: "#2B97FF".into(),
                    emoji: "🦊".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            english(&result.notices),
            ["Color and mark saved on workstation."]
        );
        assert_eq!(result.local, [member("second").key]);
        assert_eq!(result.marked, [member("work").key]);
        assert_eq!(
            (
                member("work").mark_color.as_str(),
                member("work").mark_emoji.as_str()
            ),
            ("#2b97ff", "🦊")
        );
        // Migration fills only what the computer has no value for, and skips
        // a computer that keeps no marks.
        let work_key = member("work").key;
        let cleared = screens
            .edit_open(&pump, &group().key, &work_key, Kind::Mark)
            .unwrap();
        screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: cleared.handle,
                    color: "#2b97ff".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(screens
            .mark_migrate(&pump, &work_key, "#00aa7f", "🐙")
            .unwrap());
        assert_eq!(
            (
                member("work").mark_color.as_str(),
                member("work").mark_emoji.as_str()
            ),
            ("#2b97ff", "🐙")
        );
        assert!(!screens
            .mark_migrate(&pump, &member("second").key, "#00aa7f", "🐙")
            .unwrap());
        assert!(!screens
            .mark_migrate(&pump, "unknown", "#00aa7f", "")
            .unwrap());
        assert!(screens.mark_migrate(&pump, &work_key, "red", "").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn model_and_shared_permissions_reach_every_computer_of_the_account() {
        // zerus-f0uz8: one model and effort per account; one permission mode
        // unless the account keeps it per computer.
        use std::os::unix::fs::PermissionsExt;
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "zerus-account-shared-{}-{stamp}",
            std::process::id()
        )));
        std::fs::create_dir(&scratch.0).unwrap();
        let hgs = scratch.0.join("hgs");
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../core/tests/support/fake_hgs_account_edits.py"),
            &hgs,
        )
        .unwrap();
        std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (screens, pump, first) = fixture_at(hgs);
        let shared = |id: &str, home: &str, scope: &str| {
            json!({"id":id,"provider":"codex","label":"Example","home":home,"auth_revision":"a1","installed":true,
                "model_defaults_supported":true,"permission_scope":scope,
                "account_status":{"status":"ok","identity":{"account_id":"example-account"}}})
        };
        let mut work = shared("work", "/accounts/example", "account");
        work["model_defaults"] = json!({"model":"model-a","effort":"low"});
        work["permission_mode"] = json!("bypass");
        let catalog = json!({"revision":"r1","setup_revision":true,"permission_scope_supported":true,
            "profiles":[work, shared("second", "/accounts/second", "account")]});
        std::fs::write(scratch.0.join("catalog.json"), catalog.to_string()).unwrap();
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&first.computer)
            .unwrap()
            .data = Some(catalog);
        let group = || {
            let view = screens.accounts(false);
            let group = view
                .accounts
                .iter()
                .find(|a| a.members.len() == 2)
                .expect("one account on two profiles")
                .clone();
            group
        };
        let account = group();
        assert!(account.model_differs && account.permission_differs);
        assert!(!account.permissions_per_computer);
        let selected = account
            .members
            .iter()
            .find(|m| m.id == "work")
            .unwrap()
            .key
            .clone();
        let apply = |kind: Kind, request: EditRequest| {
            let opened = screens
                .edit_open(&pump, &group().key, &selected, kind)
                .unwrap();
            let result = screens
                .edit_apply(
                    &pump,
                    EditRequest {
                        handle: opened.handle.clone(),
                        ..request
                    },
                )
                .unwrap();
            (opened, result)
        };
        let (opened, result) = apply(
            Kind::Model,
            EditRequest {
                model: "model-a".into(),
                effort: "high".into(),
                ..Default::default()
            },
        );
        assert_eq!(opened.machines, ["workstation", "workstation"]);
        assert!(opened.differs);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            english(&result.notices),
            ["Model defaults saved for new sessions on workstation and workstation."]
        );
        let saved = || -> Value {
            serde_json::from_slice(&std::fs::read(scratch.0.join("catalog.json")).unwrap()).unwrap()
        };
        assert!(saved()["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["model_defaults"] == json!({"model":"model-a","effort":"high"})));
        assert!(!group().model_differs);
        // A shared permission mode reaches both profiles.
        let (opened, result) = apply(
            Kind::Permissions,
            EditRequest {
                mode: "bypass".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            (opened.machines.len(), opened.differs, opened.per_computer),
            (2, true, false)
        );
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(!group().permission_differs);
        // Per computer: the scope reaches both, then a permission edit stays
        // on the selected computer only.
        let (opened, result) = apply(
            Kind::Scope,
            EditRequest {
                mode: "computer".into(),
                ..Default::default()
            },
        );
        assert_eq!(opened.machines.len(), 2);
        assert_eq!(
            english(&result.notices),
            ["Permissions are now kept per computer on workstation and workstation."]
        );
        assert!(group().permissions_per_computer);
        let (opened, result) = apply(
            Kind::Permissions,
            EditRequest {
                mode: "provider".into(),
                ..Default::default()
            },
        );
        assert_eq!((opened.machines.len(), opened.per_computer), (1, true));
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let modes: Vec<Value> = saved()["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["permission_mode"].clone())
            .collect();
        assert_eq!(modes, [json!("provider"), json!("bypass")]);
    }
    #[cfg(unix)]
    #[test]
    fn a_shared_permission_change_stops_when_a_computer_kept_its_own_since() {
        // zerus-f0uz8 review: the scope is outside the catalog revision, so a
        // dialog opened as shared must not spread a mode after another
        // member switched to per computer.
        use std::os::unix::fs::PermissionsExt;
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "zerus-account-scope-{}-{stamp}",
            std::process::id()
        )));
        std::fs::create_dir(&scratch.0).unwrap();
        let hgs = scratch.0.join("hgs");
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../core/tests/support/fake_hgs_account_edits.py"),
            &hgs,
        )
        .unwrap();
        std::fs::set_permissions(&hgs, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (screens, pump, first) = fixture_at(hgs);
        let member = |id: &str, home: &str| {
            json!({"id":id,"provider":"codex","label":"Example","home":home,"auth_revision":"a1","installed":true,
                "permission_scope":"account","account_status":{"status":"ok","identity":{"account_id":"example-account"}}})
        };
        let catalog = json!({"revision":"r1","setup_revision":true,"permission_scope_supported":true,
            "profiles":[member("work", "/accounts/example"), member("second", "/accounts/second")]});
        std::fs::write(scratch.0.join("catalog.json"), catalog.to_string()).unwrap();
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&first.computer)
            .unwrap()
            .data = Some(catalog.clone());
        let view = screens.accounts(false);
        let account = view.accounts.iter().find(|a| a.members.len() == 2).unwrap();
        let selected = account.members.iter().find(|m| m.id == "work").unwrap();
        let opened = screens
            .edit_open(&pump, &account.key, &selected.key, Kind::Permissions)
            .unwrap();
        assert_eq!((opened.machines.len(), opened.per_computer), (2, false));
        // Meanwhile the second profile keeps its permissions per computer;
        // the revision does not change, and the hub has not heard yet.
        let mut changed = catalog.clone();
        changed["profiles"][1]["permission_scope"] = json!("computer");
        std::fs::write(scratch.0.join("catalog.json"), changed.to_string()).unwrap();
        let result = screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: opened.handle.clone(),
                    mode: "bypass".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(result.notices.is_empty(), "{:?}", result.notices);
        assert_eq!(result.errors.len(), 1);
        assert!(
            result.errors[0].contains("now kept per computer"),
            "{:?}",
            result.errors
        );
        let calls = std::fs::read_to_string(scratch.0.join("calls.jsonl")).unwrap();
        assert!(!calls.contains("\"permissions\""), "{calls}");
        // The review's case: the hub has heard of the switch already.
        std::fs::write(scratch.0.join("catalog.json"), catalog.to_string()).unwrap();
        let reopened = screens
            .edit_open(&pump, &account.key, &selected.key, Kind::Permissions)
            .unwrap();
        assert!(!reopened.per_computer);
        screens
            .lock()
            .accounts
            .catalogs
            .get_mut(&first.computer)
            .unwrap()
            .data = Some(changed.clone());
        assert!(screens
            .accounts(false)
            .accounts
            .iter()
            .any(|a| a.permissions_per_computer));
        let result = screens
            .edit_apply(
                &pump,
                EditRequest {
                    handle: reopened.handle.clone(),
                    mode: "bypass".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(result.errors, [PER_COMPUTER_NOW]);
        let calls = std::fs::read_to_string(scratch.0.join("calls.jsonl")).unwrap();
        assert!(!calls.contains("\"permissions\""), "{calls}");
        // Each shared write also names the scope its computer must still have.
        let binding = Binding::capture(
            first.computer.clone(),
            first.machine_id.clone(),
            &catalog,
            "work",
        )
        .unwrap();
        let shared = binding
            .edit_values(
                Kind::Permissions,
                &Values {
                    mode: "bypass".into(),
                    expect_scope: "account".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(shared
            .args
            .windows(2)
            .any(|pair| pair == ["--expect-scope", "account"]));
    }
    #[cfg(unix)]
    fn english(notices: &[Text]) -> Vec<String> {
        notices.iter().map(Text::english).collect()
    }
    #[test]
    fn computer_lists_read_as_text() {
        let list = |names: &[&str]| {
            let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
            Text::with("%1", vec![machine_list(&names)]).english()
        };
        assert_eq!(list(&["arch"]), "arch");
        assert_eq!(list(&["arch", "mac"]), "arch and mac");
        assert_eq!(list(&["arch", "mac", "va-kids"]), "arch, mac and va-kids");
        // The joins are prose the webview translates; the names never are.
        let names = vec!["arch".to_owned(), "mac".to_owned()];
        assert!(matches!(machine_list(&names), Arg::Text(ref text) if text.source == "%1 and %2"));
    }
}
