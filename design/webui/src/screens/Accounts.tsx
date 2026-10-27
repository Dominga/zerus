import { For, Show, createMemo, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { Icon } from "../lib/icons";
import { demoState, setDialog, setPage, toast } from "../state";
import { machines as fleet } from "../mock/data";
import { ALL_MACHINES, anyMachine } from "../mock/lab-fleet";
import { MachineTile } from "../components/MachineTile";
import type { OsKind } from "../lib/os-icons";
import { AGENT_ACCOUNTS, type AgentAccount, type AccountMember, type MemberState, type UsageWindow } from "../mock/settings-extra";
import { AgentBadge } from "./sessions/SessionCard";
import { MachineLabel } from "./settings/machine-label";
import { prefs } from "./settings/store";
import { Actions, Status, Spinner } from "./settings/ui";
import "../styles/settings.css";

// Agent accounts (AccountsPage.cpp): profiles grouped by provider identity
// across machines. Choosing a machine tag selects that machine's profile;
// every action names the machine it changes.

const params = new URLSearchParams(location.search);
export const PROVIDER_NAME: Record<string, string> = { claude: "Claude", codex: "Codex", kimi: "Kimi", deepseek: "DeepSeek", opencode: "OpenCode" };
const LOCAL = fleet.find((m) => m.thisComputer)!.alias;

export const [accounts, setAccounts] = createStore<AgentAccount[]>(structuredClone(AGENT_ACCOUNTS));
export const [accountMachine, setAccountMachine] = createSignal<string>(params.get("amachine") ?? "*");
export const [selectedAccount, setSelectedAccount] = createSignal<string>(params.get("account") ?? AGENT_ACCOUNTS[0].key);
export const [selectedHost, setSelectedHost] = createSignal<string>(AGENT_ACCOUNTS[0].members[0].host);
/** Machine an "Add to machine" tag targets ("*" = choose in the dialog). */
export const [addTarget, setAddTarget] = createSignal("*");
export const [accountNotice, setAccountNotice] = createSignal("");

export const currentAccount = () => accounts.find((a) => a.key === selectedAccount());
export const currentMember = () => currentAccount()?.members.find((m) => m.host === selectedHost()) ?? currentAccount()?.members[0];

const offlineHosts = () => (demoState() === "error" ? ["build-02", "macbook"] : ["build-02"]);
export const online = (host: string) => !offlineHosts().includes(host) && anyMachine(host)?.connection !== "offline";
const stateOf = (m: AccountMember): MemberState => (online(m.host) ? m.state : "offline");

/** Machines that can run accounts: this computer (when it has local sessions) and every peer. */
export const accountHosts = () => fleet.map((m) => m.alias);

export const permissionLabel = (mode: string) => (mode === "bypass" ? "Bypass" : mode === "auto" ? "Auto" : mode === "plan" ? "Plan" : "Default");
const accountPermission = (a: AgentAccount) => {
  const modes = new Set(a.members.map((m) => m.permission));
  return modes.size > 1 ? "Varies" : permissionLabel([...modes][0]);
};

export function statusText(a: AgentAccount, m: AccountMember): string {
  const h = m.host;
  const p = PROVIDER_NAME[a.provider];
  switch (stateOf(m)) {
    case "offline": return `${h} is offline.`;
    case "not-installed": return `${p} is not installed on ${h}.`;
    case "apikey": return `API key configured on ${h}.`;
    case "signed-out": return a.provider === "deepseek" ? `Set a DeepSeek API key on ${h} to use API models.` : `Sign in on ${h} to use this account.`;
    case "keychain": return `The login Keychain on ${h} is locked. Unlock it in Keychain Access, then refresh.`;
    case "desktop": return `Desktop sign-in on ${h} is unavailable. Log in to macOS and check the HGS installation.`;
    case "unavailable": return `The saved sign-in on ${h} could not be used. Check Keychain access or sign in again.`;
    case "expired": return `Sign-in expired on ${h}.`;
    case "checking": return `Checking sign-in on ${h}…`;
    default: return a.provider === "opencode" ? `Configure providers, models and permissions with OpenCode on ${h}. Zerus uses that native configuration.` : `Ready on ${h}.`;
  }
}

const tone = (s: MemberState) => (s === "ready" || s === "apikey" ? "ok" : s === "checking" ? "busy" : s === "offline" ? "muted" : "warn");

/** Used share of a limit window drawn per Settings → Usage limit indicator. */
function UsageMeter(props: { w: UsageWindow }) {
  const ahead = () => props.w.used >= props.w.elapsed;
  const kind = () => (props.w.used >= 90 ? "danger" : ahead() ? "warn" : "");
  return (
    <div class="acct-meter" classList={{ rails: prefs.usageIndicator === "rails" }}>
      <div class="acct-bar" classList={{ [kind()]: !!kind() }}>
        <i style={{ "--w": `${props.w.used}%` }} />
        <Show when={prefs.usageIndicator !== "rails"}><b class="acct-marker" style={{ "--x": `${props.w.elapsed}%` }} title={`${props.w.elapsed}% of the window elapsed`} /></Show>
      </div>
      <Show when={prefs.usageIndicator === "rails"}>
        <div class="acct-bar elapsed" title={`${props.w.elapsed}% of the window elapsed`}><i style={{ "--w": `${props.w.elapsed}%` }} /></div>
      </Show>
    </div>
  );
}

function UsagePanel(props: { a: AgentAccount }) {
  const offline = () => props.a.members.every((m) => !online(m.host));
  return (
    <section class="acct-usage box">
      <div class="row">
        <h3>Account usage</h3>
        <span class="grow" />
        <Show when={props.a.usage.updated}><span class="dim">Updated {props.a.usage.updated}</span></Show>
      </div>
      <p class="muted">Shared by sessions using this account.</p>
      <Show when={offline()}><Status>Machine is offline. Showing the last available account data.</Status></Show>
      <Show when={!props.a.usage.windows.length && props.a.usage.state}><p>{props.a.usage.state}</p></Show>
      <For each={props.a.usage.windows}>
        {(w) => {
          // Projected use at reset from the elapsed share of the window (AccountUsage pace).
          const minutes = w.label === "5h" ? 300 : 10080;
          const projected = () => (w.used * 100) / Math.max(1, w.elapsed);
          const runsOut = () => {
            const left = ((100 - w.used) * (w.elapsed / 100) * minutes) / Math.max(1, w.used);
            return left >= 1440 ? `${Math.floor(left / 1440)}d ${Math.round((left % 1440) / 60)}h` : `${Math.floor(left / 60)}h ${Math.round(left % 60)}m`;
          };
          const pace = () => (projected() >= 100 ? `At the current pace, the limit runs out in about ${runsOut()}` : `At the current pace, about ${Math.round(projected())}% by reset`);
          return (
            <div class="acct-window">
              <div class="row"><b class="tabular">{w.label}</b><span class="tabular">{w.used}% used</span><span class="grow" />
                <Show when={prefs.usageIndicator === "marker-countdown"}><span class="dim tabular">resets in {w.resetIn}</span></Show>
              </div>
              <UsageMeter w={w} />
              <div class="row dim"><span>Resets {w.resets}</span><span class="grow" /><span class="acct-pace" classList={{ warn: projected() >= 100 }}>{pace()}</span></div>
            </div>
          );
        }}
      </For>
      <Show when={props.a.usage.balance}><p>{props.a.usage.balance}</p></Show>
      <Show when={props.a.usage.credits}><p>{props.a.usage.credits}</p></Show>
    </section>
  );
}

function AccountRow(props: { a: AgentAccount }) {
  const perm = () => accountPermission(props.a);
  return (
    <button class="acct-row" classList={{ on: selectedAccount() === props.a.key }} aria-current={selectedAccount() === props.a.key}
      title={props.a.members.map((m) => `${m.host}: ${permissionLabel(m.permission)}`).join("\n")}
      onClick={() => { setSelectedAccount(props.a.key); setSelectedHost((accountMachine() !== "*" && props.a.members.some((m) => m.host === accountMachine()) ? accountMachine() : props.a.members[0].host)); setAccountNotice(""); }}>
      <span class="row">
        <b class="ell grow">{props.a.label}</b>
        <AgentBadge agent={props.a.provider} />
      </span>
      <span class="row acct-row-tags">
        <span class="row acct-machines">
          <For each={props.a.members.slice(0, 3)}>{(m) => <MachineLabel alias={m.host} class={online(m.host) ? "" : "dimmed"} />}</For>
          <Show when={props.a.members.length > 3}><span class="dim">+{props.a.members.length - 3}</span></Show>
        </span>
        <span class="grow" />
        <span class="acct-perm" classList={{ bypass: perm() === "Bypass" }}>{perm()}</span>
      </span>
    </button>
  );
}

function Detail() {
  const a = () => currentAccount()!;
  const m = () => currentMember()!;
  const st = () => stateOf(m());
  const present = () => a().members.map((x) => x.host);
  const missing = () => accountHosts().filter((h) => !present().includes(h) && !(a().provider === "opencode" && h !== LOCAL));
  const signIn = () => {
    if (a().provider === "opencode" || st() === "not-installed") return undefined;
    if (a().provider === "deepseek") return st() === "apikey" ? `Change API key on ${m().host}…` : `Set API key on ${m().host}…`;
    return st() === "ready" ? `Sign in again on ${m().host}…` : `Sign in on ${m().host}…`;
  };
  const busy = () => st() === "checking";
  // Machine chips: at most four, keeping the selected machine visible.
  const VISIBLE = 4;
  const visibleMembers = () => {
    const all = a().members;
    if (all.length <= VISIBLE + 1) return all;
    const first = all.slice(0, VISIBLE);
    const sel = all.find((x) => x.host === m().host);
    return sel && !first.includes(sel) ? [...first.slice(0, VISIBLE - 1), sel] : first;
  };
  const [moreOpen, setMoreOpen] = createSignal(false);
  const [moreQuery, setMoreQuery] = createSignal("");
  const addable = () => ALL_MACHINES.filter((x) => !a().members.some((mm) => mm.host === x.alias) && online(x.alias) && x.connection === "online").length;
  // AccountsPage default button: one default account per agent and machine.
  const makeDefault = (host: string) => {
    const ai = accounts.findIndex((x) => x.key === a().key);
    for (const [oi, other] of accounts.entries())
      if (other.provider === a().provider) other.members.forEach((mm, mi) => { if (mm.host === host) setAccounts(oi, "members", mi, "isDefault", oi === ai); });
    setAccountNotice(`Default account saved for new sessions on ${host}.`);
  };
  return (
    <div class="acct-detail">
      <div class="acct-title">
        <h2 class="ell">{a().label}</h2>
        <span class="grow" />
        <button class="btn" onClick={() => setDialog("accounts-rename")}><Icon name="pencil" size={13} />Rename…</button>
      </div>
      <div class="row acct-identity">
        <AgentBadge agent={a().provider} />
        <div class="col selectable">
          <Show when={a().plan}><span>Plan: {a().plan}</span></Show>
          <Show when={a().email} fallback={<span class="muted">{a().provider === "opencode" ? "Native OpenCode configuration" : a().provider === "deepseek" ? "API key account" : "Account identity is unavailable."}</span>}>
            <span>Sign-in: {a().email}</span>
          </Show>
        </div>
      </div>

      <section class="acct-card box">
        <h3>Machines</h3>
        <div class="acct-tags" role="radiogroup" aria-label="Machine profile">
          <For each={visibleMembers()}>
            {(x) => (
              // Default for new sessions is spelled out: a "Default" chip where it is,
              // a "Make default" action beside the other connected machines.
              <span class="acct-tag-wrap">
                <button class="acct-tag" role="radio" aria-checked={x.host === m().host} classList={{ on: x.host === m().host, offline: !online(x.host) }} onClick={() => { setSelectedHost(x.host); setAccountNotice(""); }}>
                  <MachineLabel alias={x.host} />{online(x.host) ? "" : " (offline)"}
                  <Show when={x.isDefault}><span class="acct-default" title={`Default for new ${PROVIDER_NAME[a().provider]} sessions on ${x.host} when no account is chosen`}>Default</span></Show>
                </button>
                <Show when={!x.isDefault && online(x.host) && x.state === "ready" && a().provider !== "deepseek" && a().provider !== "opencode"}>
                  <button class="btn small ghost acct-make-default" title={`Use this account for new ${PROVIDER_NAME[a().provider]} sessions on ${x.host} when no account is specified. Existing sessions keep their account.`}
                    onClick={() => makeDefault(x.host)}>Make default</button>
                </Show>
              </span>
            )}
          </For>
          {/* Large fleets: the first machines (always including the selected one), then the rest in a searchable list. */}
          <Show when={a().members.length > visibleMembers().length}>
            <span class="acct-more-anchor">
              <button class="acct-tag more" aria-haspopup="dialog" aria-expanded={moreOpen()} onClick={() => { setMoreQuery(""); setMoreOpen(!moreOpen()); }}>
                +{a().members.length - visibleMembers().length} more
              </button>
              <Show when={moreOpen()}>
                <div class="acct-more" role="dialog" aria-label={`Machines with ${a().label}`} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setMoreOpen(false); } }}>
                  <input class="field" type="search" placeholder={`Search ${a().members.length} machines`} value={moreQuery()} onInput={(e) => setMoreQuery(e.currentTarget.value)} autofocus aria-label="Search machines" />
                  <div class="acct-more-list">
                    <For each={a().members.filter((x) => x.host.includes(moreQuery().trim().toLowerCase()))} fallback={<div class="dim acct-more-empty">No machine matches.</div>}>
                      {(x) => (
                        <button class="acct-more-row" classList={{ on: x.host === m().host }} onClick={() => { setSelectedHost(x.host); setAccountNotice(""); setMoreOpen(false); }}>
                          <MachineTile os={anyMachine(x.host)?.platform as OsKind | undefined} state={online(x.host) ? (anyMachine(x.host)?.connection === "reconnecting" ? "connecting" : "online") : "offline"} size={22} />
                          <MachineLabel alias={x.host} />
                          <span class="grow" />
                          <Show when={x.isDefault}><span class="acct-default">Default</span></Show>
                          <span class="dim">{x.state === "ready" ? "" : x.state === "signed-out" ? "Sign-in needed" : x.state}</span>
                        </button>
                      )}
                    </For>
                  </div>
                </div>
              </Show>
            </span>
          </Show>
          <Show when={addable() > 0 || a().provider === "deepseek"}>
            <button class="btn small acct-add" title="Choose machines, then how to add this account to them" onClick={() => { setAddTarget("*"); setDialog("accounts-add-to-machine"); }}>
              <Icon name="plus" size={12} />Add to machine…<span class="dim">{addable()} available</span>
            </button>
          </Show>
        </div>
        <div class="acct-status">
          <Status kind={tone(st())}>{statusText(a(), m())}</Status>
          <Actions>
            <Show when={signIn()}>
              <button class="btn" disabled={!online(m().host) || busy()}
                title={a().provider === "deepseek" ? `Save a DeepSeek API key on ${m().host}.` : `Open ${PROVIDER_NAME[a().provider]} sign-in in a terminal on ${m().host} for this profile. Completing sign-in updates the credentials used by this profile on ${m().host}.`}
                onClick={() => a().provider === "deepseek" ? (setAddTarget(m().host), setDialog("accounts-api-key")) : setAccountNotice(`Complete ${PROVIDER_NAME[a().provider]} sign-in in the terminal on ${m().host}, this page updates automatically.`)}>
                <Icon name="key-round" size={13} />{signIn()}
              </button>
            </Show>
            <Show when={st() === "not-installed"}>
              <button class="btn" onClick={() => toast("Agent installer", `Opened the ${PROVIDER_NAME[a().provider]} installer in a terminal on ${m().host}.`)}><Icon name="download" size={13} />Install agent…</button>
            </Show>
            <Show when={a().provider === "opencode"}>
              <button class="btn" disabled={!online(m().host)} title="Start the shared OpenCode service on this machine. Zerus never stops it." onClick={() => setAccountNotice(`OpenCode started on ${m().host}.`)}>
                <Icon name="play" size={13} />Start OpenCode on {m().host}
              </button>
            </Show>
          </Actions>
        </div>
        <Show when={a().openCodeProviders}>
          <div class="acct-oc" aria-label="OpenCode providers">
            <For each={a().openCodeProviders}>
              {(p) => <div class="acct-oc-row"><span class="grow">{p.name}</span><span class="muted tabular">{p.models === 1 ? "1 model" : `${p.models} models`}</span><span class="dim">{p.detail}</span></div>}
            </For>
          </div>
        </Show>
        <div class="acct-setting">
          <span title={m().permission === "bypass" ? "Applies to new and resumed sessions; a running session may use a different mode." : m().permissionDetail ?? "Provider defaults have not been verified."}>
            Permissions on {m().host}: <b classList={{ danger: m().permission === "bypass" }}>{permissionLabel(m().permission)}</b>
            <span class="dim acct-src">{m().permission === "bypass" ? "set in Zerus" : "from provider settings"}</span>
          </span>
          <button class="btn small" disabled={!online(m().host) || a().provider === "opencode"} title={`Change permission defaults on ${m().host}`} onClick={() => setDialog("accounts-permissions")}>Change…</button>
        </div>
        <Show when={a().provider !== "opencode"}>
          <div class="acct-setting">
            <span>{m().model ? `New sessions: ${m().model}${m().effort ? ` (${m().effort})` : ""}` : "New sessions: Provider settings"}</span>
            <button class="btn small" disabled={!online(m().host)} onClick={() => setDialog("accounts-model")}>Model and effort…</button>
          </div>
        </Show>
        <Actions>
          <button class="btn" disabled={m().isDefault || !online(m().host)}
            title={`Use this account for new ${PROVIDER_NAME[a().provider]} sessions on ${m().host} when no account is specified. Existing sessions keep their account.`}
            onClick={() => makeDefault(m().host)}>
            {m().isDefault ? `Default on ${m().host}` : `Make default on ${m().host}`}
          </button>
          <button class="btn danger" disabled={!online(m().host)} title="Remove this profile from the selected machine's Zerus catalog. You will confirm the machine and consequences before anything changes." onClick={() => setDialog("accounts-remove")}>
            <Icon name="trash" size={13} />{a().members.length > 1 ? `Remove from ${m().host}…` : "Remove…"}
          </button>
        </Actions>
        <Show when={accountNotice()}><Status kind="ok">{accountNotice()}</Status></Show>
      </section>
      <UsagePanel a={a()} />
    </div>
  );
}

export function AccountsScreen() {
  const [refreshing, setRefreshing] = createSignal(false);
  const visible = createMemo(() => accounts.filter((a) => accountMachine() === "*" || a.members.some((m) => m.host === accountMachine())));
  const pending = () => (demoState() === "normal" ? 1 : 0) + (refreshing() ? 2 : 0);
  const loading = () => demoState() === "loading";
  const empty = () => demoState() === "empty";
  const summary = () => {
    const n = visible().length;
    const hosts = accountHosts().length - (demoState() === "error" ? 2 : 1);
    return `${n === 1 ? "1 account" : `${n} accounts`}, ${hosts === 1 ? "1 machine loaded" : `${hosts} machines loaded`}${pending() ? ` (Updating… ${pending()} remaining)` : ""}`;
  };
  const refresh = () => { setRefreshing(true); setTimeout(() => setRefreshing(false), 1100); };
  return (
    <div class="mach-page acct-page">
      <div class="mach-shell">
        <header class="mach-head">
          <h1>Agent accounts</h1>
          <button class="icon-btn" title="Refresh account usage (automatically every 5 minutes)" aria-label="Refresh accounts" onClick={refresh}>
            <Show when={!refreshing()} fallback={<Spinner />}><Icon name="refresh-cw" size={15} /></Show>
          </button>
          <span class="grow" />
          <button class="btn primary" disabled={empty()} onClick={() => setDialog("accounts-add")}><Icon name="plus" size={14} />Add account</button>
        </header>
        <p class="muted mach-summary">Your accounts across all machines. Choose an account to manage where it is available.</p>
        <Show when={!empty()} fallback={
          <div class="empty acct-empty"><Icon name="user-round-cog" size={28} /><h3>No machine keeps agent accounts</h3>
            <p>To manage agent accounts, enable local sessions for this computer in Machines or add a remote machine.</p>
            <button class="btn" onClick={() => setPage("machines")}>Open Machines</button></div>
        }>
          <div class="row acct-filter">
            <select class="field" aria-label="Machine" value={accountMachine()} onChange={(e) => setAccountMachine(e.currentTarget.value)}>
              <option value="*">All machines</option>
              <For each={accountHosts()}>{(h) => <option value={h}>{h === LOCAL ? `${h} (this machine)` : h}</option>}</For>
            </select>
            <Show when={!loading()}>
              <Show when={pending()}><Spinner size={12} /></Show>
              <span class="muted">{summary()}</span>
            </Show>
          </div>
          <Show when={!loading()} fallback={
            <div class="acct-loading"><div class="row"><Spinner /><span>Loading accounts…</span></div><p class="muted">Checking accounts across your machines.</p></div>
          }>
            <div class="mach-body acct-body">
              <div class="mach-list acct-list" role="list" aria-label="Agent accounts">
                <For each={visible()} fallback={<p class="muted acct-none">No accounts on {accountMachine()}.</p>}>{(a) => <AccountRow a={a} />}</For>
              </div>
              <Show when={currentAccount()} fallback={<div class="acct-detail"><h2 class="muted">Choose an account</h2></div>}>
                <Detail />
              </Show>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
}
