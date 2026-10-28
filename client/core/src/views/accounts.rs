//! The Accounts screen and the Overview account cards: the `account ls`
//! profiles of every host, each with its usage, grouped by the provider's
//! identity (tray/src/AccountCatalog.h), with Qt's member states and texts
//! (tray/src/AccountsPage.cpp) and the Overview's hidden-card rule
//! (tray/src/DashboardPage.cpp:348-350). When the reads run is decided in
//! [`super::schedule`].
use super::usage::{self, Identity, Usage};
use super::{flag, items, number, text, FleetEntry};
use crate::ComputerId;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

/// The most profiles a host's catalog shows.
const PROFILES_PER_HOST: usize = 256;
/// The most machines the account screens list.
const HOSTS: usize = 64;
/// The most accounts one projection carries; the rest is counted in `truncated`.
const ACCOUNTS: usize = 1024;
/// The most notices one projection carries.
const NOTICES: usize = 16;
/// The most OpenCode providers a row lists (OpenCodeModels::providers).
const OPENCODE_PROVIDERS: usize = 64;

/// A profile's usage is kept per host, profile ID and home (AccountUsageRef::key).
pub type UsageKey = (ComputerId, String, String);

/// The profile an inspect reads (AccountUsageRef::profile).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageRef {
    pub host: ComputerId,
    pub id: String,
    pub home: String,
    pub provider: String,
    pub auth_revision: String,
}

impl UsageRef {
    pub fn key(&self) -> UsageKey {
        (self.host.clone(), self.id.clone(), self.home.clone())
    }
}

/// One host's `account ls`.
#[derive(Clone, Debug, Default)]
pub struct CatalogSlot {
    /// When the last `account ls` started, on the hub's millisecond clock.
    pub requested: Option<u64>,
    pub pending: bool,
    /// A forced refresh arrived while one ran: one follow-up runs after it.
    pub queued: bool,
    /// The last good reply; a failure keeps it.
    pub data: Option<Value>,
    /// The last failure, cleared by a good reply.
    pub error: Option<String>,
}

/// One profile's inspect (an AccountUsageStore entry).
#[derive(Clone, Debug, Default)]
pub struct UsageSlot {
    /// The credential revision the data belongs to.
    pub auth_revision: String,
    pub attempted: Option<u64>,
    pub pending: bool,
    pub failed: bool,
    pub data: Option<Value>,
}

/// Everything the account reads produced.
#[derive(Clone, Debug, Default)]
pub struct AccountsState {
    pub catalogs: BTreeMap<ComputerId, CatalogSlot>,
    pub usage: BTreeMap<UsageKey, UsageSlot>,
    pub(crate) queue: VecDeque<UsageRef>,
    pub(crate) running: usize,
    pub(crate) warmed: BTreeSet<ComputerId>,
}

/// A profile's state on its machine, as the mockup's machine tags show it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemberState {
    Ready,
    Offline,
    NotInstalled,
    Apikey,
    SignedOut,
    Keychain,
    Desktop,
    Unavailable,
    Expired,
    Checking,
    Unknown,
}

/// A machine that keeps agent accounts.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Host {
    pub computer: ComputerId,
    pub name: String,
    /// `"<host> (this machine)"` for this computer (machineChoice, AccountsPage.cpp:178).
    pub label: String,
    pub online: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenCodeProvider {
    pub label: String,
    pub connection: String,
    pub models: String,
}

/// OpenCode's native configuration on one machine (AccountsPage.cpp:657-704).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenCode {
    pub providers: Vec<OpenCodeProvider>,
    /// "Showing the first N providers."
    pub truncated: bool,
    /// The last known catalog's time when the catalog is not ready ("Last known catalog from ...").
    pub stale_catalog_at: Option<f64>,
    /// Installed but not running ("Start OpenCode on ...").
    pub stopped: bool,
}

/// One profile on one machine.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Profile {
    /// `host + U+001F + id` (AccountCatalog::profileKey).
    pub key: String,
    pub host: ComputerId,
    pub machine: String,
    pub id: String,
    pub label: String,
    pub provider: String,
    pub native: bool,
    pub installed: bool,
    pub is_default: bool,
    pub home: String,
    pub auth_revision: String,
    pub online: bool,
    pub state: MemberState,
    /// The status line under the machine tags.
    pub detail: String,
    pub signed_in: bool,
    /// `bypass`, `auto`, `plan`, `default` or `unknown`.
    pub permission: String,
    pub permission_label: String,
    pub permission_hint: String,
    pub model_supported: bool,
    pub model_text: String,
    pub usage: Usage,
    pub opencode: Option<OpenCode>,
}

/// One account: the profiles of one identity across machines.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Account {
    pub key: String,
    pub label: String,
    pub provider: String,
    pub installed: bool,
    pub machines: Vec<String>,
    pub members: Vec<Profile>,
    /// The better snapshot of the members (betterUsage).
    pub usage: Usage,
    /// Bypass, Auto, Plan, Default, or Varies when the members differ.
    pub permission: String,
    pub identity_lines: Vec<String>,
    /// Shown without identity lines: "Reading account details…" or "Account identity is unavailable.".
    pub identity_note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AccountsView {
    pub hosts: Vec<Host>,
    pub accounts: Vec<Account>,
    /// The Overview's cards: the same grouping without the hidden native profiles.
    pub overview: Vec<Account>,
    /// A first snapshot exists ("Loading accounts…" until then).
    pub snapshot: bool,
    /// A shown profile's usage is still loading ("Loading account details…").
    pub loading: bool,
    /// Hosts and profiles still being read ("Updating… N remaining").
    pub pending: usize,
    pub loaded_machines: usize,
    /// A read runs; Refresh is disabled.
    pub busy: bool,
    pub notices: Vec<String>,
    /// Accounts left out beyond the most one projection carries.
    pub truncated: usize,
    /// Why there is no machine to list, when there is none.
    pub no_hosts: Option<String>,
}

/// What the previous projection showed: the rows held back while they
/// resolve, and whether a first snapshot exists.
#[derive(Clone, Debug, Default)]
pub struct AccountsDisplay {
    held: BTreeMap<String, Profile>,
    snapshot: bool,
}

/// AccountsPage::online: this computer unless it is a control console; a
/// peer until its stream failed (Qt counts a peer offline only after a failed poll).
pub fn online(entry: &FleetEntry) -> bool {
    if entry.computer.is_local() {
        return entry.local_sessions;
    }
    !(entry.status == "ended" || (entry.status == "reconnecting" && entry.attempt > 0))
}

/// AccountUsageStore::online: this computer always, a peer as [`online`].
pub fn reachable(entry: &FleetEntry) -> bool {
    entry.computer.is_local() || online(entry)
}

/// This computer keeps accounts only once its card says it runs sessions: before
/// that `local_sessions` is a default, and a control console must not be read.
fn keeps_accounts(entry: &FleetEntry) -> bool {
    !entry.computer.is_local() || (entry.host.is_some() && entry.local_sessions)
}

/// The machines that keep accounts, in fleet order, at most [`HOSTS`].
fn account_fleet(fleet: &[FleetEntry]) -> impl Iterator<Item = &FleetEntry> {
    fleet
        .iter()
        .filter(|entry| keeps_accounts(entry))
        .take(HOSTS)
}

/// The machines that keep agent accounts (accountHosts, AccountsPage.cpp:165-170).
pub fn hosts(fleet: &[FleetEntry]) -> Vec<Host> {
    account_fleet(fleet)
        .map(|entry| Host {
            computer: entry.computer.clone(),
            name: entry.name(),
            label: if entry.computer.is_local() {
                format!("{} (this machine)", entry.name())
            } else {
                entry.name()
            },
            online: online(entry),
        })
        .collect()
}

/// The local Windows computer runs neither DeepSeek nor OpenCode (AccountCatalog::launchable).
fn launchable(entry: &FleetEntry, provider: &str) -> bool {
    !(entry.computer.is_local()
        && entry.platform.as_deref() == Some("windows")
        && matches!(provider, "dsh" | "opencode"))
}

fn usage_ref(host: &ComputerId, raw: &Value) -> UsageRef {
    UsageRef {
        host: host.clone(),
        id: text(raw, "id"),
        home: text(raw, "home"),
        provider: text(raw, "provider"),
        auth_revision: text(raw, "auth_revision"),
    }
}

/// The installed profiles an inspect reads, with whether their host is reachable.
pub fn refs(fleet: &[FleetEntry], state: &AccountsState) -> Vec<(UsageRef, bool)> {
    let mut out = Vec::new();
    for entry in account_fleet(fleet) {
        let Some(data) = state
            .catalogs
            .get(&entry.computer)
            .and_then(|slot| slot.data.as_ref())
        else {
            continue;
        };
        for raw in items(data, "profiles").iter().take(PROFILES_PER_HOST) {
            let r = usage_ref(&entry.computer, raw);
            if flag(raw, "installed") && !r.id.is_empty() && launchable(entry, &r.provider) {
                out.push((r, reachable(entry)));
            }
        }
    }
    out
}

pub(crate) fn provider_name(provider: &str) -> String {
    match provider {
        "claude" => "Claude",
        "codex" => "Codex",
        "kimi" => "Kimi",
        "opencode" => "OpenCode",
        "dsh" => "DeepSeek",
        "sh" => "Shell",
        other => other,
    }
    .to_owned()
}

/// AccountUsageStore::data plus the seeding of AccountsPage::profiles.
fn profile_usage(entry: &FleetEntry, raw: &Value, r: &UsageRef, state: &AccountsState) -> Usage {
    let slot = state.usage.get(&r.key());
    // A snapshot of another credential revision is not this profile's any more.
    let data = slot
        .filter(|slot| r.auth_revision.is_empty() || slot.auth_revision == r.auth_revision)
        // Only a real reply counts as a successful inspect, never a stub.
        .and_then(|slot| slot.data.as_ref())
        .filter(|data| data.is_object());
    let reachable = reachable(entry);
    let mut u = match data {
        Some(data) => usage::usage(data),
        None => Usage {
            status: if !reachable {
                "offline"
            } else if slot.is_some_and(|slot| slot.failed) {
                // The first inspect failed: settled, shown from the cache.
                "unavailable"
            } else {
                "loading"
            }
            .to_owned(),
            ..Usage::default()
        },
    };
    if u.provider.is_empty() {
        u.provider = r.provider.clone();
    }
    u.label = text(raw, "label");
    u.refreshing = slot.is_some_and(|slot| slot.pending);
    u.refresh_error = slot.is_some_and(|slot| slot.failed);
    u.offline = !reachable;
    if let Some(cached) = raw
        .get("account_status")
        .filter(|cached| cached.is_object())
    {
        let cached = usage::usage(cached);
        if data.is_none() {
            // Until the first inspect answers, the cached inspection stands in
            // (owner ruling; Qt seeds identity and checked_at only).
            u.identity = cached.identity;
            u.checked_at = cached.checked_at;
            u.windows = cached.windows;
            u.balances = cached.balances;
            u.credits = cached.credits;
            u.seeded = true;
        } else if matches!(u.status.as_str(), "unavailable" | "offline")
            && u.identity == Identity::default()
        {
            u.identity = cached.identity;
            u.checked_at = cached.checked_at;
        }
    }
    u
}

/// The state and status line of a profile (AccountsPage.cpp:637-654).
fn member(
    online: bool,
    installed: bool,
    provider: &str,
    u: &Usage,
    machine: &str,
) -> (MemberState, String, bool) {
    use MemberState::*;
    let status = u.status.as_str();
    let signed_in = status == "ok" || status == "expired" || u.auth_status == "signed_in";
    let (state, detail) = if !online {
        (Offline, format!("{machine} is offline."))
    } else if !installed {
        (
            NotInstalled,
            format!("{} is not installed on {machine}.", provider_name(provider)),
        )
    } else {
        match status {
            "configured" => (Apikey, format!("API key configured on {machine}.")),
            "signed_out" if provider == "dsh" => (SignedOut, format!("Set a DeepSeek API key on {machine} to use API models.")),
            "signed_out" => (SignedOut, format!("Sign in on {machine} to use this account.")),
            "credentials_locked" => (Keychain, format!("The login Keychain on {machine} is locked. Unlock it in Keychain Access, then refresh.")),
            "desktop_session_unavailable" => (Desktop, format!("Desktop sign-in on {machine} is unavailable. Log in to macOS and check the HGS installation.")),
            "credentials_unavailable" => (Unavailable, format!("The saved sign-in on {machine} could not be used. Check Keychain access or sign in again.")),
            "expired" => (Expired, format!("Sign-in expired on {machine}.")),
            "loading" => (Checking, format!("Checking sign-in on {machine}…")),
            _ if signed_in => (Ready, format!("Ready on {machine}.")),
            _ => (Unknown, format!("Account status is unavailable on {machine}.")),
        }
    };
    (state, detail, signed_in)
}

/// permissionMode (AccountsPage.cpp:72-74).
fn permission_mode(raw: &Value) -> String {
    if text(raw, "permission_mode") == "bypass" {
        return "bypass".into();
    }
    Some(text(raw, "effective_permission_mode"))
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// permissionLabel (AccountsPage.cpp:75-78).
pub fn permission_label(mode: &str) -> &'static str {
    match mode {
        "bypass" => "Bypass",
        "auto" => "Auto",
        "plan" => "Plan",
        "mixed" => "Varies",
        _ => "Default",
    }
}

/// permissionHint (AccountsPage.cpp:89-92).
fn permission_hint(raw: &Value) -> String {
    let source = if text(raw, "permission_mode") == "bypass" {
        "Set in Zerus"
    } else {
        "From provider settings"
    };
    let detail = Some(text(raw, "permission_detail"))
        .filter(|detail| !detail.is_empty())
        .unwrap_or_else(|| "Provider defaults have not been verified.".into());
    format!("{source}\n{detail}\nApplies to new and resumed sessions; a running session may use a different mode.")
}

/// The model line (AccountsPage.cpp:610-612), with the effort in parentheses (the dots rule).
fn model_text(raw: &Value) -> String {
    let defaults = raw.get("model_defaults").unwrap_or(&Value::Null);
    let (model, effort) = (text(defaults, "model"), text(defaults, "effort"));
    match (model.is_empty(), effort.is_empty()) {
        (true, _) => "New sessions: Provider settings".into(),
        (false, true) => format!("New sessions: {model}"),
        (false, false) => format!("New sessions: {model} ({effort})"),
    }
}

/// OpenCodeModels::kindText (OpenCodeModels.cpp:115-130).
fn kind_text(kind: &str) -> &'static str {
    match kind {
        "free" => "Free, no key",
        "key" => "API key",
        "oauth" => "Signed in",
        "env" => "Environment variable",
        "config" => "Configuration",
        "command" => "Command",
        "external" => "External credentials",
        "local" => "Detected locally",
        _ => "Connected",
    }
}

fn short(value: &Value, key: &str, limit: usize) -> String {
    text(value, key)
        .chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

/// OpenCode's providers (OpenCodeModels::providers, OpenCodeModels.cpp:64-80):
/// distinct valid IDs, at most 64; a provider ID here is 1 to 128 bytes
/// without spaces or control characters.
fn opencode(raw: &Value) -> Option<OpenCode> {
    if text(raw, "provider") != "opencode" {
        return None;
    }
    let mut seen = BTreeSet::new();
    let mut providers = Vec::new();
    for row in items(raw, "providers") {
        if providers.len() >= OPENCODE_PROVIDERS {
            break;
        }
        let id = text(row, "id");
        if id.is_empty()
            || id.len() > 128
            || id.chars().any(|c| c.is_control() || c.is_whitespace())
            || !seen.insert(id.clone())
        {
            continue;
        }
        let label = Some(short(row, "label", 160))
            .filter(|label| !label.is_empty())
            .unwrap_or(id);
        let active = short(row, "active_label", 160);
        let credentials = row
            .get("credential_count")
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let mut connection = kind_text(&short(row, "kind", 32)).to_owned();
        if !active.is_empty() {
            connection.push_str(&format!(", {active}"));
        } else if credentials > 1 {
            connection.push_str(&format!(", {credentials} accounts"));
        }
        let models = match row.get("model_count").and_then(Value::as_i64).unwrap_or(-1) {
            count if count < 0 => String::new(),
            1 => "1 model".into(),
            count => format!("{count} models"),
        };
        providers.push(OpenCodeProvider {
            label,
            connection,
            models,
        });
    }
    let state = text(raw, "catalog_state");
    let at = number(raw, "catalog_at").filter(|at| *at > 0.0 && *at < 253_402_300_800.0);
    Some(OpenCode {
        providers,
        truncated: flag(raw, "providers_truncated"),
        stale_catalog_at: if state != "ready" { at } else { None },
        stopped: flag(raw, "installed") && state == "not_running",
    })
}

fn profile(entry: &FleetEntry, raw: &Value, state: &AccountsState) -> Profile {
    let r = usage_ref(&entry.computer, raw);
    let machine = entry.name();
    let installed = flag(raw, "installed");
    let online = online(entry);
    let usage = profile_usage(entry, raw, &r, state);
    let (member_state, mut detail, signed_in) =
        member(online, installed, &r.provider, &usage, &machine);
    if r.provider == "opencode" {
        // OpenCode keeps its own configuration (AccountsPage.cpp:657-663).
        let catalog = text(raw, "catalog_state");
        let reason = if !installed {
            text(raw, "install_reason")
        } else if !catalog.is_empty() && catalog != "ready" {
            text(raw, "catalog_reason")
        } else {
            String::new()
        };
        detail = if !online {
            format!("{machine} is offline.")
        } else if !reason.is_empty() {
            reason
        } else if !installed {
            format!("OpenCode is not installed on {machine}.")
        } else {
            format!("Configure providers, models and permissions with OpenCode on {machine}. Zerus uses that native configuration.")
        };
    }
    let permission = permission_mode(raw);
    Profile {
        key: format!("{}\u{1f}{}", entry.computer.0, r.id),
        host: entry.computer.clone(),
        machine,
        label: text(raw, "label"),
        native: flag(raw, "native"),
        installed,
        is_default: flag(raw, "is_default"),
        online,
        state: member_state,
        detail,
        signed_in,
        permission_label: permission_label(&permission).into(),
        permission_hint: permission_hint(raw),
        permission,
        model_supported: flag(raw, "model_defaults_supported"),
        model_text: model_text(raw),
        usage,
        opencode: opencode(raw),
        id: r.id,
        home: r.home,
        provider: r.provider,
        auth_revision: r.auth_revision,
    }
}

/// AccountCatalog::named.
fn named(label: &str) -> bool {
    !label.is_empty() && label != "Default account" && label != "Native DeepSeek account"
}

/// AccountCatalog::key, with or without the account ID.
fn identity_key(p: &Profile, with_account: bool) -> String {
    let identity = &p.usage.identity;
    let email = identity.email.trim().to_lowercase();
    let account = if with_account {
        identity.account_id.as_str()
    } else {
        ""
    };
    if p.usage.status == "signed_out" || (email.is_empty() && account.is_empty()) {
        return format!("profile:{}", p.key);
    }
    let who = if account.is_empty() {
        format!("email:{email}")
    } else {
        format!("id:{account}")
    };
    format!(
        "account:{}\n{who}\n{}",
        p.provider,
        identity.organization.trim().to_lowercase()
    )
}

fn quality(u: &Usage) -> u8 {
    match u.status.as_str() {
        "ok" | "configured" if u.offline => 2,
        "ok" | "configured" => 3,
        _ => 1,
    }
}

/// AccountCatalog::betterUsage: a quota belongs to the account; pick a snapshot, never add them up.
fn better(usage: &Usage, previous: &Usage) -> bool {
    quality(usage) > quality(previous)
        || (quality(usage) == quality(previous)
            && usage.checked_at.unwrap_or(0.0) > previous.checked_at.unwrap_or(0.0))
}

/// accountPermission (AccountsPage.cpp:80-88).
fn account_permission(members: &[Profile]) -> String {
    let mut mode = String::new();
    for member in members {
        if !mode.is_empty() && mode != member.permission {
            return "mixed".into();
        }
        mode = member.permission.clone();
    }
    mode
}

/// The identity block (AccountsPage.cpp:625-636).
fn identity_lines(title: &str, u: &Usage) -> (Vec<String>, String) {
    let identity = &u.identity;
    let mut lines = Vec::new();
    let mut seen = vec![title.trim().to_lowercase()];
    for field in [&identity.name, &identity.email, &identity.organization] {
        let text = field.trim();
        if !text.is_empty() && !seen.contains(&text.to_lowercase()) {
            lines.push(text.to_owned());
            seen.push(text.to_lowercase());
        }
    }
    if !identity.plan.is_empty() {
        lines.push(format!("Plan: {}", identity.plan));
    }
    if !identity.auth_method.is_empty() && identity.auth_method != "none" {
        lines.push(format!("Sign-in: {}", identity.auth_method));
    }
    let note = if !lines.is_empty() || *identity != Identity::default() {
        String::new()
    } else if u.status == "loading" {
        "Reading account details…".into()
    } else {
        "Account identity is unavailable.".into()
    };
    (lines, note)
}

/// AccountCatalog::group (AccountCatalog.h:37-79).
pub fn group(profiles: &[Profile]) -> Vec<Account> {
    // An email-only profile can join a known ID only when that email and
    // scope resolve to one account.
    let mut known: HashMap<String, BTreeSet<String>> = HashMap::new();
    for p in profiles {
        let identity = &p.usage.identity;
        if identity.account_id.is_empty()
            || identity.email.is_empty()
            || p.usage.status == "signed_out"
        {
            continue;
        }
        known
            .entry(identity_key(p, false))
            .or_default()
            .insert(identity_key(p, true));
    }
    let mut groups: Vec<Account> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for p in profiles {
        let mut key = identity_key(p, true);
        if let Some(only) = known
            .get(&key)
            .filter(|ids| ids.len() == 1)
            .and_then(|ids| ids.iter().next())
        {
            key = only.clone();
        }
        let at = *positions.entry(key.clone()).or_insert_with(|| {
            groups.push(Account {
                key: key.clone(),
                label: p.label.clone(),
                provider: p.provider.clone(),
                installed: false,
                machines: Vec::new(),
                members: Vec::new(),
                usage: p.usage.clone(),
                permission: String::new(),
                identity_lines: Vec::new(),
                identity_note: String::new(),
            });
            groups.len() - 1
        });
        let group = &mut groups[at];
        if !group.machines.contains(&p.machine) {
            group.machines.push(p.machine.clone());
        }
        if better(&p.usage, &group.usage) {
            group.usage = p.usage.clone();
        }
        if !named(&group.label) && named(&p.label) {
            group.label = p.label.clone();
        }
        group.installed |= p.installed;
        group.members.push(p.clone());
    }
    for group in &mut groups {
        if !named(&group.label) && !group.usage.identity.email.is_empty() {
            group.label = group.usage.identity.email.clone();
        }
        group.permission = permission_label(&account_permission(&group.members)).into();
        (group.identity_lines, group.identity_note) = identity_lines(&group.label, &group.usage);
    }
    groups
}

/// A native profile awaiting its identity is not another account on the
/// Overview (DashboardPage.cpp:348-350).
fn hidden_on_overview(p: &Profile) -> bool {
    p.native
        && !named(&p.label)
        && identity_key(p, true).starts_with("profile:")
        && matches!(
            p.usage.status.as_str(),
            "loading" | "unavailable" | "offline"
        )
}

fn resolving(p: &Profile) -> bool {
    p.installed && p.online && (p.usage.refreshing || p.usage.status == "loading")
}

/// The Accounts screen and the Overview cards from the fleet and the reads so far.
pub fn project(
    fleet: &[FleetEntry],
    state: &AccountsState,
    display: &mut AccountsDisplay,
) -> AccountsView {
    let hosts = hosts(fleet);
    let mut all = Vec::new();
    for entry in account_fleet(fleet) {
        let Some(data) = state
            .catalogs
            .get(&entry.computer)
            .and_then(|slot| slot.data.as_ref())
        else {
            continue;
        };
        for raw in items(data, "profiles").iter().take(PROFILES_PER_HOST) {
            if launchable(entry, &text(raw, "provider")) {
                all.push(profile(entry, raw, state));
            }
        }
    }
    // pendingAccounts (AccountsPage.cpp:419-433).
    let mut pending = hosts
        .iter()
        .filter(|host| {
            state
                .catalogs
                .get(&host.computer)
                .is_none_or(|slot| slot.requested.is_none() || slot.pending)
        })
        .count();
    pending += all.iter().filter(|p| resolving(p)).count();
    // This computer's card has not arrived: it is a host still to come, so the
    // first snapshot waits and the screen does not claim there are no hosts.
    let awaiting_card = fleet
        .iter()
        .any(|entry| entry.computer.is_local() && entry.host.is_none());
    pending += usize::from(awaiting_card);
    // Identities appear as they become known, without flashing unresolved
    // duplicates while another machine is still being inspected.
    let shown: Vec<Profile> = all
        .into_iter()
        .filter_map(|p| {
            let identified = !p.usage.identity.account_id.is_empty()
                || !p.usage.identity.email.trim().is_empty();
            if !resolving(&p) || identified {
                Some(p)
            } else {
                display.held.get(&p.key).cloned()
            }
        })
        .collect();
    if !awaiting_card && (!shown.is_empty() || pending == 0) {
        display.snapshot = true;
    }
    display.held = shown.iter().map(|p| (p.key.clone(), p.clone())).collect();
    let loaded: BTreeSet<&ComputerId> = shown.iter().map(|p| &p.host).collect();
    let visible: Vec<Profile> = shown
        .iter()
        .filter(|p| !hidden_on_overview(p))
        .cloned()
        .collect();
    let notices = hosts
        .iter()
        .filter_map(|host| {
            let error = state.catalogs.get(&host.computer)?.error.as_ref()?;
            let name = if host.computer.is_local() {
                "This machine".to_owned()
            } else {
                host.name.clone()
            };
            Some(format!("{name}: {error}"))
        })
        .take(NOTICES)
        .collect();
    let console = fleet.iter().find(|entry| entry.computer.is_local());
    let can_enable = console.is_some_and(|l| {
        !l.local_sessions && l.local_host && l.platform.as_deref() == Some("windows")
    });
    let no_hosts = (hosts.is_empty() && !awaiting_card).then(|| {
        if can_enable {
            "To manage agent accounts, enable local sessions for this computer in Machines or add a remote machine."
        } else {
            "Add a remote machine in Machines to manage agent accounts; this computer runs no agents."
        }
        .to_owned()
    });
    let mut accounts = group(&shown);
    let truncated = accounts.len().saturating_sub(ACCOUNTS);
    accounts.truncate(ACCOUNTS);
    let mut overview = group(&visible);
    overview.truncate(ACCOUNTS);
    AccountsView {
        loading: shown.iter().any(|p| p.usage.status == "loading"),
        busy: pending > 0 || state.catalogs.values().any(|slot| slot.pending),
        accounts,
        overview,
        truncated,
        snapshot: display.snapshot,
        pending,
        loaded_machines: loaded.len(),
        notices,
        no_hosts,
        hosts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::fixture;
    use serde_json::json;

    fn entry(computer: &str, status: &str, attempt: u32) -> FleetEntry {
        FleetEntry {
            computer: ComputerId::from(computer),
            status: status.into(),
            attempt,
            local_sessions: true,
            local_host: false,
            host: computer.is_empty().then(|| "workstation".to_owned()),
            platform: Some("linux".into()),
        }
    }

    fn loaded() -> AccountsState {
        let mut state = AccountsState::default();
        for (computer, file) in [
            ("", "accounts-local.json"),
            ("studio", "accounts-studio.json"),
        ] {
            state.catalogs.insert(
                ComputerId::from(computer),
                CatalogSlot {
                    requested: Some(0),
                    data: Some(fixture(file)),
                    ..CatalogSlot::default()
                },
            );
        }
        state
    }

    fn inspected(
        state: &mut AccountsState,
        host: &str,
        id: &str,
        home: &str,
        revision: &str,
        reply: Value,
    ) {
        state.usage.insert(
            (ComputerId::from(host), id.into(), home.into()),
            UsageSlot {
                auth_revision: revision.into(),
                data: Some(reply),
                ..UsageSlot::default()
            },
        );
    }

    fn signed_out() -> Value {
        json!({"id": "native-claude", "home": "/home/example/.claude", "provider": "claude", "status": "signed_out",
           "identity": {"email": "person@example.com", "account_id": "acct-1"}, "windows": [], "balances": []})
    }

    /// Every installed profile has answered except claude-studio, which shows its cached status.
    fn with_inspects() -> AccountsState {
        let mut state = loaded();
        inspected(
            &mut state,
            "",
            "claude-work",
            "/home/example/.config/hgs/accounts/claude-work",
            "a1",
            fixture("inspect-claude-work.json"),
        );
        inspected(
            &mut state,
            "",
            "native-claude",
            "/home/example/.claude",
            "b1",
            signed_out(),
        );
        inspected(
            &mut state,
            "",
            "native-codex",
            "/home/example/.codex",
            "c1",
            fixture("inspect-native-codex.json"),
        );
        inspected(
            &mut state,
            "studio",
            "kimi-studio",
            "/home/example/.config/hgs/accounts/kimi-studio",
            "k1",
            json!({"id": "kimi-studio", "home": "/home/example/.config/hgs/accounts/kimi-studio", "provider": "kimi", "status": "ok",
               "identity": {"plan": "Pro"}, "windows": [{"id": "limit_month_total", "label": "Monthly", "used_percent": 50,
               "window_minutes": null, "resets_at": null}], "balances": []}),
        );
        state
    }

    // Review Focus 2.
    #[test]
    fn one_identity_on_two_hosts_is_one_account_and_signed_out_or_unidentified_profiles_stay_apart()
    {
        let fleet = [entry("", "live", 0), entry("studio", "live", 0)];
        let view = project(&fleet, &with_inspects(), &mut AccountsDisplay::default());
        let groups: Vec<(&str, Vec<&str>)> = view
            .accounts
            .iter()
            .map(|a| {
                (
                    a.label.as_str(),
                    a.members.iter().map(|m| m.id.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            groups,
            vec![
                ("Work", vec!["claude-work", "claude-studio"]),
                ("person@example.com", vec!["native-claude"]),
                ("Default account", vec!["native-codex"]),
                ("Native DeepSeek account", vec!["native-dsh"]),
                ("Moonshot", vec!["kimi-studio"]),
            ]
        );
        let work = &view.accounts[0];
        assert_eq!(work.machines, ["workstation", "studio"]);
        assert_eq!(work.permission, "Varies");
        assert_eq!(
            work.usage.checked_at,
            Some(1793000300.0),
            "the answered inspect is the better snapshot"
        );
        assert_eq!(
            work.identity_lines,
            [
                "Example Person",
                "person@example.com",
                "Plan: max",
                "Sign-in: oauth"
            ]
        );
        assert_eq!(
            view.accounts[2].identity_note,
            "Account identity is unavailable."
        );
        assert_eq!(view.accounts[3].identity_note, "Reading account details…");
        // The Overview leaves out a native, unnamed profile while it loads (native-dsh).
        let overview: Vec<&str> = view.overview.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(
            overview,
            ["Work", "person@example.com", "Default account", "Moonshot"]
        );
    }

    #[test]
    fn member_states_and_texts_follow_qt() {
        use MemberState::*;
        let member = |view: &AccountsView, id: &str| -> Profile {
            view.accounts
                .iter()
                .flat_map(|a| &a.members)
                .find(|m| m.id == id)
                .unwrap()
                .clone()
        };
        let state = |view: &AccountsView, id: &str| {
            let m = member(view, id);
            (m.state, m.detail)
        };
        let fleet = [entry("", "live", 0), entry("studio", "live", 0)];
        let view = project(&fleet, &with_inspects(), &mut AccountsDisplay::default());
        assert_eq!(
            state(&view, "claude-work"),
            (Ready, "Ready on workstation.".into())
        );
        assert_eq!(
            state(&view, "native-claude"),
            (
                SignedOut,
                "Sign in on workstation to use this account.".into()
            )
        );
        assert_eq!(
            state(&view, "native-codex"),
            (
                SignedOut,
                "Sign in on workstation to use this account.".into()
            )
        );
        assert_eq!(
            state(&view, "native-dsh"),
            (
                NotInstalled,
                "DeepSeek is not installed on workstation.".into()
            )
        );
        assert_eq!(
            state(&view, "claude-studio"),
            (Checking, "Checking sign-in on studio…".into())
        );
        assert_eq!(member(&view, "kimi-studio").state, Ready);
        assert_eq!(member(&view, "native-codex").permission_label, "Bypass");
        assert_eq!(
            member(&view, "claude-work").model_text,
            "New sessions: opus (high)"
        );
        assert_eq!(
            member(&view, "claude-studio").model_text,
            "New sessions: Provider settings"
        );
        assert_eq!(member(&view, "claude-work").permission_hint,
        "From provider settings\nFrom ~/.claude/settings.json\nApplies to new and resumed sessions; a running session may use a different mode.");
        // studio's stream failed: its profiles are offline, and the cached inspection
        // stands in until the first inspect answers (ruling).
        let away = [entry("", "live", 0), entry("studio", "reconnecting", 1)];
        let view = project(&away, &with_inspects(), &mut AccountsDisplay::default());
        let studio = member(&view, "claude-studio");
        assert_eq!(
            (studio.state, studio.detail.as_str(), studio.online),
            (Offline, "studio is offline.", false)
        );
        assert!(studio.usage.offline);
        assert_eq!(studio.usage.status, "offline");
        assert_eq!(
            studio
                .usage
                .windows
                .iter()
                .map(|w| w.resets_at)
                .collect::<Vec<_>>(),
            [Some(1793005200.0), Some(1793400000.0)]
        );
        assert_eq!(studio.usage.checked_at, Some(1793000100.0));
        assert!(
            studio.usage.seeded,
            "the cached inspection is marked until inspect answers"
        );
        assert!(
            !member(&view, "claude-work").usage.seeded,
            "an answered inspect is not seeded"
        );
        assert!(
            member(&view, "kimi-studio").usage.offline
                && member(&view, "kimi-studio").state == Offline
        );
        // Qt's lenient rule: a peer is offline only after its stream failed.
        for (status, attempt, up) in [
            ("reconnecting", 0, true),
            ("probing", 0, true),
            ("unsupported", 0, true),
            ("polled", 0, true),
            ("live", 0, true),
            ("reconnecting", 2, false),
            ("ended", 0, false),
        ] {
            assert_eq!(
                online(&entry("studio", status, attempt)),
                up,
                "{status} {attempt}"
            );
        }
        let mut console = entry("", "live", 0);
        console.local_sessions = false;
        assert!(!online(&console) && reachable(&console));
    }

    #[test]
    fn a_control_console_has_no_account_hosts_of_its_own() {
        let mut console = entry("", "live", 0);
        console.local_sessions = false;
        assert!(hosts(std::slice::from_ref(&console)).is_empty());
        let view = project(
            std::slice::from_ref(&console),
            &AccountsState::default(),
            &mut AccountsDisplay::default(),
        );
        assert_eq!(view.no_hosts.as_deref(), Some("Add a remote machine in Machines to manage agent accounts; this computer runs no agents."));
        console.local_host = true;
        console.platform = Some("windows".into());
        let view = project(
            &[console],
            &AccountsState::default(),
            &mut AccountsDisplay::default(),
        );
        assert_eq!(view.no_hosts.as_deref(),
        Some("To manage agent accounts, enable local sessions for this computer in Machines or add a remote machine."));
        let labels: Vec<String> = hosts(&[entry("", "live", 0), entry("studio", "live", 0)])
            .into_iter()
            .map(|h| h.label)
            .collect();
        assert_eq!(labels, ["workstation (this machine)", "studio"]);
        // The local Windows computer lists neither DeepSeek nor OpenCode.
        let mut windows = entry("", "live", 0);
        windows.platform = Some("windows".into());
        let view = project(&[windows], &loaded(), &mut AccountsDisplay::default());
        assert!(view.accounts.iter().all(|a| a.provider != "dsh"));
    }

    #[test]
    fn pending_counts_unread_hosts_and_resolving_profiles_and_the_display_holds_them_back() {
        let fleet = [entry("", "live", 0), entry("studio", "live", 0)];
        let mut state = loaded();
        state.catalogs.remove(&ComputerId::from("studio"));
        state.catalogs.get_mut(&ComputerId::local()).unwrap().error =
            Some("hgs did not answer in time".into());
        let mut display = AccountsDisplay::default();
        let ids = |view: &AccountsView| -> Vec<String> {
            view.accounts
                .iter()
                .flat_map(|a| a.members.iter().map(|m| m.id.clone()))
                .collect()
        };
        let view = project(&fleet, &state, &mut display);
        // studio never answered; claude-work, native-claude and native-codex are loading.
        assert_eq!(view.pending, 4);
        assert!(view.busy && view.loading);
        assert_eq!(
            ids(&view),
            ["claude-work", "native-dsh"],
            "unidentified profiles wait while they resolve"
        );
        assert!(view.snapshot);
        assert_eq!(view.loaded_machines, 1);
        assert_eq!(view.notices, ["This machine: hgs did not answer in time"]);
        inspected(
            &mut state,
            "",
            "native-claude",
            "/home/example/.claude",
            "b1",
            signed_out(),
        );
        assert_eq!(
            ids(&project(&fleet, &state, &mut display)),
            ["claude-work", "native-claude", "native-dsh"]
        );
        // A refresh without data yet: the previous row stays.
        let slot = state
            .usage
            .get_mut(&(
                ComputerId::local(),
                "native-claude".into(),
                "/home/example/.claude".into(),
            ))
            .unwrap();
        slot.data = None;
        slot.pending = true;
        let view = project(&fleet, &state, &mut display);
        assert_eq!(ids(&view), ["claude-work", "native-claude", "native-dsh"]);
        assert_eq!(
            view.accounts[1].members[0].usage.status, "signed_out",
            "the held row"
        );
    }

    #[test]
    fn opencode_rows_show_its_native_configuration() {
        let raw = json!({"id": "native-opencode", "label": "OpenCode", "provider": "opencode", "native": true, "installed": true,
        "home": "/home/example/.local/share/opencode", "catalog_state": "not_running",
        "catalog_reason": "OpenCode is not running on studio.", "catalog_at": 1793000000.5, "providers_truncated": true,
        "providers": [{"id": "anthropic", "label": "Anthropic", "kind": "oauth", "credential_count": 1, "model_count": 12},
                      {"id": "openai", "label": "", "kind": "key", "credential_count": 3, "model_count": 1},
                      {"id": "anthropic", "label": "Again"}, {"id": "bad id"}]});
        let studio = ComputerId::from("studio");
        let mut state = AccountsState::default();
        state.catalogs.insert(
            studio.clone(),
            CatalogSlot {
                requested: Some(0),
                data: Some(json!({"profiles": [raw], "revision": "r"})),
                ..CatalogSlot::default()
            },
        );
        state.usage.insert((studio, "native-opencode".into(), "/home/example/.local/share/opencode".into()),
        UsageSlot { data: Some(json!({"id": "native-opencode", "provider": "opencode", "status": "configured"})), ..UsageSlot::default() });
        let view = project(
            &[entry("studio", "live", 0)],
            &state,
            &mut AccountsDisplay::default(),
        );
        let p = &view.accounts[0].members[0];
        assert_eq!(p.detail, "OpenCode is not running on studio.");
        let oc = p.opencode.as_ref().unwrap();
        assert_eq!(
            oc.providers,
            vec![
                OpenCodeProvider {
                    label: "Anthropic".into(),
                    connection: "Signed in".into(),
                    models: "12 models".into()
                },
                OpenCodeProvider {
                    label: "openai".into(),
                    connection: "API key, 3 accounts".into(),
                    models: "1 model".into()
                },
            ]
        );
        assert!(oc.truncated && oc.stopped);
        assert_eq!(oc.stale_catalog_at, Some(1793000000.5));
    }

    #[test]
    fn a_failed_first_inspect_keeps_the_cached_windows_seeded_with_a_refresh_error() {
        let mut state = loaded();
        state.usage.insert(
            (
                ComputerId::from("studio"),
                "claude-studio".into(),
                "/home/example/.claude".into(),
            ),
            UsageSlot {
                auth_revision: "s1".into(),
                attempted: Some(1),
                failed: true,
                ..UsageSlot::default()
            },
        );
        let view = project(
            &[entry("", "live", 0), entry("studio", "live", 0)],
            &state,
            &mut AccountsDisplay::default(),
        );
        let studio = view
            .accounts
            .iter()
            .flat_map(|a| &a.members)
            .find(|m| m.id == "claude-studio")
            .unwrap();
        assert_eq!(studio.usage.windows.len(), 2);
        assert!(studio.usage.seeded && studio.usage.refresh_error);
        assert_eq!(studio.usage.checked_at, Some(1793000100.0));
        assert_eq!(studio.usage.status, "unavailable", "settled, not loading");
    }

    #[test]
    fn a_failed_first_inspect_settles_the_pending_count() {
        let fleet = [entry("", "live", 0), entry("studio", "live", 0)];
        let key = (
            ComputerId::from("studio"),
            "claude-studio".to_owned(),
            "/home/example/.claude".to_owned(),
        );
        let mut state = loaded();
        let before = project(&fleet, &state, &mut AccountsDisplay::default()).pending;
        state.usage.insert(
            key.clone(),
            UsageSlot {
                auth_revision: "s1".into(),
                attempted: Some(1),
                failed: true,
                ..UsageSlot::default()
            },
        );
        let after = project(&fleet, &state, &mut AccountsDisplay::default()).pending;
        assert_eq!(after + 1, before, "the failed profile no longer counts");
        // A retry in flight counts again.
        state.usage.get_mut(&key).unwrap().pending = true;
        let retry = project(&fleet, &state, &mut AccountsDisplay::default()).pending;
        assert_eq!(retry, before);
    }

    #[test]
    fn this_computer_is_not_an_account_host_until_its_card_arrived() {
        let mut local = entry("", "live", 0);
        local.host = None;
        assert!(hosts(std::slice::from_ref(&local)).is_empty());
        assert!(refs(std::slice::from_ref(&local), &loaded()).is_empty());
        local.host = Some("workstation".into());
        assert_eq!(hosts(std::slice::from_ref(&local)).len(), 1);
    }

    #[test]
    fn no_snapshot_before_this_computers_card() {
        let mut local = entry("", "live", 0);
        local.host = None;
        let mut display = AccountsDisplay::default();
        let early = project(
            std::slice::from_ref(&local),
            &AccountsState::default(),
            &mut display,
        );
        assert!(!early.snapshot && early.pending >= 1 && early.no_hosts.is_none());
        local.host = Some("workstation".into());
        let mut state = AccountsState::default();
        state.catalogs.insert(
            ComputerId::local(),
            CatalogSlot {
                requested: Some(0),
                data: Some(fixture("accounts-local.json")),
                ..CatalogSlot::default()
            },
        );
        let late = project(std::slice::from_ref(&local), &state, &mut display);
        assert!(late.snapshot && late.no_hosts.is_none() && !late.accounts.is_empty());
    }

    #[test]
    fn hosts_accounts_and_notices_are_bounded() {
        let fleet: Vec<FleetEntry> = (0..70)
            .map(|n| entry(&format!("h{n}"), "live", 0))
            .collect();
        assert_eq!(hosts(&fleet).len(), 64);
        let mut state = AccountsState::default();
        for n in 0..6 {
            let profiles: Vec<Value> = (0..256)
                .map(|p| {
                    json!({"id": format!("p{p}"), "label": format!("L{n}-{p}"), "provider": "kimi", "installed": true,
                        "account_status": {"status": "ok", "identity": {"account_id": format!("a{n}-{p}")}}})
                })
                .collect();
            state.catalogs.insert(
                ComputerId::from(format!("h{n}").as_str()),
                CatalogSlot {
                    requested: Some(0),
                    data: Some(json!({"profiles": profiles})),
                    error: Some("e".into()),
                    ..CatalogSlot::default()
                },
            );
        }
        for n in 6..30 {
            state.catalogs.insert(
                ComputerId::from(format!("h{n}").as_str()),
                CatalogSlot {
                    requested: Some(0),
                    error: Some("e".into()),
                    ..CatalogSlot::default()
                },
            );
        }
        let view = project(&fleet, &state, &mut AccountsDisplay::default());
        assert_eq!(view.accounts.len(), 1024);
        assert_eq!(view.truncated, 6 * 256 - 1024);
        assert_eq!(view.notices.len(), 16);
    }
}
