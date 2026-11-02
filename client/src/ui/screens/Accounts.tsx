import { For, Index, Show, createEffect, createMemo, createSignal } from "solid-js";
import { Icon } from "../lib/icons";
import { MachineTile } from "../components/MachineTile";
import { MachineLabel } from "../components/MachineLabel";
import { Actions, Spinner, Status } from "../components/Status";
import { AgentBadge } from "../sessions/SessionCard";
import type { Account } from "../../types";
import { agentKind, catalog } from "../../store/catalog";
import { machineCards } from "../../store/machines";
import { accountMachine, accountsPage, loadAccounts, requestedProfile, setAccountMachine, setRequestedProfile } from "../../store/accounts";
import { connectionOf, osOf } from "../../store/overview";
import { useNow } from "../../store/latest";
import { setPage } from "../../store/ui";
import { clock, providerName, stateText } from "../lib/usage";
import { ACCOUNT_MANAGEMENT } from "./reasons";
import { creditsText, loginLabel, memberStateText, memberOf, panelWindow, summaryLine, tone, visibleAccounts, visibleMembers } from "./accounts-ui";

// Agent accounts (AccountsPage.cpp), ported from design/webui/src/screens/Accounts.tsx:
// profiles grouped by provider identity across machines; choosing a machine
// tag selects that machine's profile. Every write (rename, defaults,
// permissions, sign-in, install, remove, add) arrives with account management
// (3c) and keeps its place disabled.

const [selectedAccount, setSelectedAccount] = createSignal<string | undefined>();
const [selectedProfile, setSelectedProfile] = createSignal<string | undefined>();

function AccountRow(props: { a: Account }) {
  const on = () => selectedAccount() === props.a.key;
  const pick = () => {
    setSelectedAccount(props.a.key);
    setSelectedProfile(memberOf(props.a, undefined, accountMachine())?.key);
  };
  return (
    <button class="acct-row" classList={{ on: on() }} aria-current={on()} title={props.a.members.map((m) => `${m.machine}: ${m.permission_label}`).join("\n")} onClick={pick}>
      <span class="row">
        <b class="ell grow">{props.a.label}</b>
        <AgentBadge agent={agentKind(props.a.provider)} />
      </span>
      <span class="row acct-row-tags">
        <span class="row acct-machines">
          <For each={props.a.members.slice(0, 3)}>{(m) => <MachineLabel alias={m.machine} dimmed={!m.online} />}</For>
          <Show when={props.a.members.length > 3}><span class="dim">+{props.a.members.length - 3}</span></Show>
        </span>
        <span class="grow" />
        <span class="acct-perm" classList={{ bypass: props.a.permission === "Bypass" }}>{props.a.permission}</span>
      </span>
    </button>
  );
}

function UsagePanel(props: { a: Account }) {
  // Resets and pace re-render every 15 s while shown; nothing else runs on a timer.
  const now = useNow(15_000);
  const u = () => props.a.usage;
  return (
    <section class="acct-usage box">
      <div class="row">
        <h3>Account usage</h3>
        <span class="grow" />
        <Show when={u().checked_at !== null}><span class="dim">{u().seeded ? "Checked" : "Updated"} {clock(u().checked_at!)}</span></Show>
      </div>
      <p class="muted">Shared by sessions using this account.</p>
      <Show when={u().offline}><Status>Machine is offline. Showing the last available account data.</Status></Show>
      <Show when={u().refreshing}><Status kind="busy">Updating… Previous values remain visible.</Status></Show>
      <Show when={!u().refreshing && u().refresh_error}><Status kind="warn">Update failed. Showing the last available values.</Status></Show>
      <Show when={!u().windows.length && !u().balances.length && !u().offline}><p>{stateText(u())}</p></Show>
      <For each={u().windows}>{(p) => {
        const w = () => panelWindow(p, now());
        return (
          <div class="acct-window">
            <div class="row"><b class="tabular">{w().period}</b><span class="tabular">{w().usedText}</span></div>
            <div class="acct-meter rails">
              <div class="acct-bar" classList={{ warn: w().tone === "warning", danger: w().tone === "critical" }}><i style={{ "--w": `${w().used}%` }} /></div>
              <Show when={w().elapsed !== undefined}>
                <div class="acct-bar elapsed" title={`${Math.round(w().elapsed!)}% of the window elapsed`}><i style={{ "--w": `${w().elapsed}%` }} /></div>
              </Show>
            </div>
            <div class="row dim">
              <Show when={w().resets}><span>{w().resets}</span></Show>
              <Show when={u().seeded && u().checked_at !== null}><span>Checked {clock(u().checked_at!)}</span></Show>
              <span class="grow" />
              <span class="acct-pace">{w().forecast}</span>
            </div>
          </div>
        );
      }}</For>
      <For each={u().balances}>{(b) => <p>{`${b.kind}: ${b.balance} ${b.currency}`}</p>}</For>
      <Show when={creditsText(u().credits)}>{(text) => <p>{text()}</p>}</Show>
    </section>
  );
}

function Detail(props: { a: Account }) {
  const m = () => memberOf(props.a, selectedProfile(), accountMachine())!;
  const [moreOpen, setMoreOpen] = createSignal(false);
  const [moreQuery, setMoreQuery] = createSignal("");
  const shown = () => visibleMembers(props.a, m().key);
  const nativeConfig = () => props.a.provider === "opencode";
  const defaultable = () => props.a.provider !== "dsh" && !nativeConfig();
  const matches = () => props.a.members.filter((x) => x.machine.toLowerCase().includes(moreQuery().trim().toLowerCase()));
  return (
    <div class="acct-detail">
      <div class="acct-title">
        <h2 class="ell">{props.a.label}</h2>
        <span class="grow" />
        <button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="pencil" size={13} />Rename…</button>
      </div>
      <div class="row acct-identity">
        <AgentBadge agent={agentKind(props.a.provider)} />
        <div class="col selectable">
          <For each={props.a.identity_lines}>{(line) => <span>{line}</span>}</For>
          <Show when={props.a.identity_note}><span class="muted">{props.a.identity_note}</span></Show>
        </div>
      </div>

      <section class="acct-card box">
        <h3>Machines</h3>
        <div class="acct-tags" role="radiogroup" aria-label="Machine profile">
          <For each={shown()}>{(x) => (
            <span class="acct-tag-wrap">
              <button class="acct-tag" role="radio" aria-checked={x.key === m().key} classList={{ on: x.key === m().key, offline: !x.online }} onClick={() => setSelectedProfile(x.key)}>
                <MachineLabel alias={x.machine} />{x.online ? "" : " (offline)"}
                <Show when={x.is_default}>
                  <span class="acct-default" title={`Default for new ${providerName(props.a.provider)} sessions on ${x.machine} when no account is chosen`}>Default</span>
                </Show>
              </button>
              <Show when={!x.is_default && x.online && x.state === "ready" && defaultable()}>
                <button class="btn small ghost acct-make-default" disabled title={ACCOUNT_MANAGEMENT}>Make default</button>
              </Show>
            </span>
          )}</For>
          <Show when={props.a.members.length > shown().length}>
            <span class="acct-more-anchor">
              <button class="acct-tag more" aria-haspopup="dialog" aria-expanded={moreOpen()} onClick={() => { setMoreQuery(""); setMoreOpen(!moreOpen()); }}>
                +{props.a.members.length - shown().length} more
              </button>
              <Show when={moreOpen()}>
                <div class="acct-more" role="dialog" aria-label={`Machines with ${props.a.label}`}
                  onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setMoreOpen(false); } }}>
                  <input class="field" type="search" placeholder={`Search ${props.a.members.length} machines`} value={moreQuery()}
                    onInput={(e) => setMoreQuery(e.currentTarget.value)} autofocus aria-label="Search machines" />
                  <div class="acct-more-list">
                    <For each={matches()} fallback={<div class="dim acct-more-empty">No machine matches.</div>}>
                      {(x) => (
                        <button class="acct-more-row" classList={{ on: x.key === m().key }} onClick={() => { setSelectedProfile(x.key); setMoreOpen(false); }}>
                          <MachineTile os={osOf(machineCards[x.host]?.platform)} state={x.online ? connectionOf(catalog.computers[x.host]?.status).tile : "offline"} size={22} />
                          <MachineLabel alias={x.machine} />
                          <span class="grow" />
                          <Show when={x.is_default}><span class="acct-default">Default</span></Show>
                          <span class="dim">{memberStateText(x.state)}</span>
                        </button>
                      )}
                    </For>
                  </div>
                </div>
              </Show>
            </span>
          </Show>
          <button class="btn small acct-add" disabled title={ACCOUNT_MANAGEMENT}><Icon name="plus" size={12} />Add to machine…</button>
        </div>
        <div class="acct-status">
          <Status kind={tone(m().state)}>{m().detail}</Status>
          <Actions>
            <Show when={loginLabel(m())}>{(label) => <button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="key-round" size={13} />{label()}</button>}</Show>
            <Show when={!m().installed}><button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="download" size={13} />Install agent…</button></Show>
            <Show when={m().opencode?.stopped}>
              <button class="btn" disabled title={ACCOUNT_MANAGEMENT}><Icon name="play" size={13} />Start OpenCode on {m().machine}</button>
            </Show>
          </Actions>
        </div>
        <Show when={m().opencode}>{(oc) => (
          <>
            <Show when={oc().providers.length}>
              <div class="acct-oc" aria-label="OpenCode providers">
                <For each={oc().providers}>{(p) => (
                  <div class="acct-oc-row"><span class="grow">{p.label}</span><span class="muted tabular">{p.models}</span><span class="dim">{p.connection}</span></div>
                )}</For>
              </div>
            </Show>
            <Show when={oc().truncated}><p class="dim">Showing the first {oc().providers.length} providers.</p></Show>
            <Show when={oc().stale_catalog_at !== null}>
              <p class="dim">Last known catalog from {new Date(oc().stale_catalog_at! * 1000).toLocaleString(undefined, { dateStyle: "short", timeStyle: "short" })}.</p>
            </Show>
          </>
        )}</Show>
        <div class="acct-setting">
          <span title={m().permission_hint}>
            Permissions on {m().machine}: <b classList={{ danger: m().permission === "bypass" }}>{m().permission_label}</b>
            <span class="dim acct-src">{m().permission_source === "zerus" ? "set in Zerus" : "from provider settings"}</span>
          </span>
          <button class="btn small" disabled title={ACCOUNT_MANAGEMENT}>Change…</button>
        </div>
        <Show when={m().model_supported}>
          <div class="acct-setting">
            <span>{m().model_text}</span>
            <button class="btn small" disabled title={ACCOUNT_MANAGEMENT}>Model and effort…</button>
          </div>
        </Show>
        <Actions>
          <Show when={defaultable()}>
            <button class="btn" disabled title={ACCOUNT_MANAGEMENT}>{m().is_default ? `Default on ${m().machine}` : `Make default on ${m().machine}`}</button>
          </Show>
          <button class="btn danger" disabled title={ACCOUNT_MANAGEMENT}><Icon name="trash" size={13} />Remove from {m().machine}…</button>
        </Actions>
      </section>
      <UsagePanel a={props.a} />
    </div>
  );
}

export function AccountsScreen() {
  const view = accountsPage;
  const visible = createMemo(() => visibleAccounts(view(), accountMachine()));
  // An Overview card asks for a profile: select its account once it is listed.
  createEffect(() => {
    const key = requestedProfile();
    const account = key ? view().accounts.find((a) => a.members.some((m) => m.key === key)) : undefined;
    if (!account) return;
    setSelectedAccount(account.key);
    setSelectedProfile(key);
    setRequestedProfile(undefined);
  });
  const current = () => visible().find((a) => a.key === selectedAccount()) ?? visible()[0];
  const filterName = () => view().hosts.find((h) => h.computer === accountMachine())?.name ?? accountMachine();
  return (
    <div class="mach-page acct-page">
      <div class="mach-shell">
        <header class="mach-head">
          <h1>Agent accounts</h1>
          <button class="icon-btn" aria-label="Refresh accounts" disabled={view().busy} onClick={() => void loadAccounts(true)}
            title={view().busy ? `Updating accounts… ${view().pending} remaining` : "Refresh account usage (automatically every 5 minutes)"}>
            <Show when={!view().busy} fallback={<Spinner />}><Icon name="refresh-cw" size={15} /></Show>
          </button>
          <span class="grow" />
          <button class="btn primary" disabled title={view().no_hosts ?? ACCOUNT_MANAGEMENT}><Icon name="plus" size={14} />Add account</button>
        </header>
        <p class="muted mach-summary">Your accounts across all machines. Choose an account to manage where it is available.</p>
        <For each={view().notices}>{(notice) => <Status kind="warn">{notice}</Status>}</For>
        <Show when={!view().no_hosts} fallback={
          <div class="empty acct-empty"><Icon name="user-round-cog" size={28} /><h3>No machine keeps agent accounts</h3>
            <p>{view().no_hosts}</p>
            <button class="btn" onClick={() => setPage("machines")}>Open Machines</button></div>
        }>
          <div class="row acct-filter">
            <select class="field" aria-label="Machine" value={accountMachine()} onChange={(e) => setAccountMachine(e.currentTarget.value)}>
              <option value="*">All machines</option>
              <For each={view().hosts}>{(h) => <option value={h.computer}>{h.label}</option>}</For>
            </select>
            <Show when={view().snapshot}>
              <Show when={view().pending}><Spinner size={12} /></Show>
              <span class="muted">{summaryLine(view(), visible().length)}</span>
            </Show>
          </div>
          <Show when={view().snapshot} fallback={
            <div class="acct-loading"><div class="row"><Spinner /><span>Loading accounts…</span></div><p class="muted">Checking accounts across your machines.</p></div>
          }>
            <div class="mach-body acct-body">
              <div class="mach-list acct-list" role="list" aria-label="Agent accounts">
                <Show when={visible().length} fallback={<p class="muted acct-none">{accountMachine() === "*" ? "No accounts." : `No accounts on ${filterName()}.`}</p>}>
                  <Index each={visible()}>{(a) => <AccountRow a={a()} />}</Index>
                </Show>
                <Show when={view().truncated > 0}><p class="dim acct-none">{view().truncated} more accounts are not shown.</p></Show>
              </div>
              <Show when={current()} fallback={<div class="acct-detail"><h2 class="muted">Choose an account</h2></div>}>
                {(a) => <Show when={a().key} keyed><Detail a={a()} /></Show>}
              </Show>
            </div>
          </Show>
        </Show>
      </div>
    </div>
  );
}
