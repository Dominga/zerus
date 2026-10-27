import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { Icon } from "../lib/icons";
import { accounts, machines } from "../mock/data";
import { accountIdentity, extraAccounts } from "../mock/setup-extra";
import type { Account, Machine, Session } from "../mock/types";
import {
  catalog, counts, demoState, isActive, matchesFilter, setMachineFilter, setPage, setSelectedKey, setStateFilter, setDetailTab,
  setProjectFilter,
} from "../state";
import { AgentBadge, MachineBadge, SessionCard } from "./sessions/SessionCard";
import { MachineTile } from "../components/MachineTile";
import { UsageMeters, type UsageIndicator } from "../components/UsageMeter";
import { accountWindows } from "../lib/usage";
import { prefs } from "./settings/store";
import "../styles/setup.css";

// Overview (DashboardPage.cpp, FleetState.*): summary tiles, machine cards,
// account identities and the sessions that are working, need attention or
// have drafts. A sleeping peer is a pause, not a death: its last known
// sessions keep counting and staleness shows on its card.

const CARD_W = 340;
const CARD_H = 104;
const GAP = 8;

type Freshness = "fresh" | "stale" | "offline";
const freshness = (m: Machine): Freshness => {
  if (demoState() === "error" && m.alias === "win-lab") return "offline";
  return m.connection === "online" ? "fresh" : m.connection === "reconnecting" ? "stale" : "offline";
};

/** Peers whose hgs predates metrics report none (build-02 runs 0.47.2). */
const hasMetrics = (m: Machine) => !m.hgsVersion.startsWith("0.47");

function sampleAge(m: Machine, refreshedAt: number): string {
  if (demoState() === "loading") return "No sample yet";
  const f = freshness(m);
  if (f === "offline") return m.alias === "win-lab" ? "Sampled 4m ago" : "Last sample 27 Oct 11:24";
  if (f === "stale") return "Sampled 40s ago";
  const s = Math.floor((Date.now() - refreshedAt) / 1000);
  return s < 10 ? "Sampled just now" : s < 60 ? `Sampled ${s}s ago` : `Sampled ${Math.floor(s / 60)}m ago`;
}

const level = (pct: number, fresh: boolean) => (!fresh ? "unknown" : pct >= 90 ? "critical" : pct >= 70 ? "warning" : "normal");

function LoadBar(props: { pct?: number; fresh: boolean }) {
  const available = () => props.pct !== undefined;
  return (
    <div class="ov-load" data-level={available() ? level(props.pct!, props.fresh) : "unknown"}
      title={!available() ? "No sample available" : !props.fresh ? "Last known value; waiting for a fresh sample." : "Orange from 70%; red from 90%."}>
      <i style={{ width: `${available() ? Math.min(100, props.pct!) : 0}%` }} />
    </div>
  );
}

function MachineCard(props: { m: Machine; refreshedAt: number }) {
  const f = () => freshness(props.m);
  const fresh = () => f() === "fresh";
  const waiting = () => demoState() === "loading";
  const metrics = () => hasMetrics(props.m) && !waiting();
  const memPct = () => (props.m.memUsedGiB / props.m.memTotalGiB) * 100;
  const rows = () => (demoState() === "empty" ? [] : catalog.rows.filter((s) => s.machine === props.m.alias));
  const needs = () => rows().filter((s) => !s.muted && s.status === "attention").length;
  const label = () => `${rows().length === 1 ? "1 session" : `${rows().length} sessions`}${needs() ? `, ${needs()} need attention` : ""}`;
  return (
    <article class="ov-mcard box" classList={{ stale: !fresh() }} aria-label={props.m.thisComputer ? "This machine" : props.m.alias}>
      <header class="ov-mtop">
        <MachineTile os={props.m.platform} state={fresh() ? "online" : f() === "stale" ? "connecting" : "offline"} size={28} />
        <MachineBadge alias={props.m.alias} />
        <span class="ov-mtitle ell" title={props.m.alias}>{props.m.thisComputer ? "This machine" : ""}</span>
        <span class="ov-mstatus" classList={{ online: fresh() }}>{fresh() ? "Connected" : f() === "stale" ? "Stale" : "Offline"}</span>
        <button class="icon-btn small" data-tip="Machine settings" aria-label="Machine settings" onClick={() => setPage("machines")}><Icon name="settings" size={14} /></button>
      </header>
      <div class="ov-mgrid2">
        <span class="ov-mlabel">CPU</span>
        <span class="ov-mlabel">Memory</span>
        <span class="ov-mmetric tabular" title={metrics() ? "Average CPU use over the last 2.0 seconds, across all logical cores." : "CPU use needs two samples. Refresh again after a moment."}>
          {metrics() ? `${props.m.cpu}%` : "—"}
        </span>
        <span class="ov-mmetric tabular" title={props.m.platform === "macos" ? "App, wired and compressed memory. Reclaimable file cache is excluded." : "Total memory minus available memory, including reclaimable cache."}>
          {metrics() ? `${props.m.memUsedGiB.toFixed(1)} GiB` : "—"}
        </span>
        <LoadBar pct={metrics() ? props.m.cpu : undefined} fresh={fresh()} />
        <LoadBar pct={metrics() ? memPct() : undefined} fresh={fresh()} />
        <span class="ov-mdetail">{waiting() ? "Waiting for second sample" : metrics() ? `${props.m.cores} cores, load ${props.m.load.toFixed(2)}` : "Update hgs for metrics"}</span>
        <span class="ov-mdetail">{metrics() ? `of ${props.m.memTotalGiB.toFixed(1)} GiB (${Math.round(memPct())}%)` : "Memory unavailable"}</span>
      </div>
      <footer class="ov-mfoot">
        <button class="su-link" title={fresh() ? `View sessions on ${props.m.alias}` : `View last known sessions on ${props.m.alias}`}
          onClick={() => { setMachineFilter([props.m.alias]); setProjectFilter([]); setStateFilter("all"); setPage("sessions"); }}>
          {label()}
        </button>
        <span class="grow" />
        <span class="ov-mdetail" title={fresh() ? "Metrics update with the normal machine poll. Use Refresh for a new sample." : "Last known values. This machine is unavailable or has not replied recently."}>
          {sampleAge(props.m, props.refreshedAt)}
        </span>
      </footer>
    </article>
  );
}

// Accounts are grouped by verified identity, never added up across machines.
interface AccountGroup { key: string; label: string; agent: Account["agent"]; machines: string[]; identity: string; installed: boolean; fiveHour: number; week: number; plan: string; account: Account }

function accountGroups(): AccountGroup[] {
  const all = [...accounts, ...extraAccounts] as (Account & { installed?: boolean })[];
  const groups = new Map<string, AccountGroup>();
  for (const a of all) {
    const id = accountIdentity[a.id];
    const installed = a.installed !== false;
    if (!id && installed) continue; // unidentified native profiles are hidden
    const who = id?.email ?? id?.name ?? "";
    const key = `${a.agent}:${who || a.id}`;
    const g = groups.get(key);
    if (g) { if (!g.machines.includes(a.machine)) g.machines.push(a.machine); continue; }
    groups.set(key, {
      key, label: a.label === "Native account" ? AGENT_NAME[a.agent] : a.label, agent: a.agent, machines: [a.machine], installed,
      identity: !installed ? "Agent not installed" : a.plan ? `${who} / ${a.plan}` : who, fiveHour: a.fiveHour, week: a.week, plan: a.plan, account: a,
    });
  }
  return [...groups.values()];
}
const AGENT_NAME: Record<string, string> = { claude: "Claude", codex: "Codex", kimi: "Kimi", deepseek: "DeepSeek", opencode: "OpenCode" };

function AccountCard(props: { g: AccountGroup }) {
  const limited = () => props.g.fiveHour > 0 || props.g.week > 0;
  const peak = () => Math.max(props.g.fiveHour, props.g.week);
  const tone = () => (!props.g.installed || !limited() ? "none" : peak() >= 90 ? "critical" : peak() >= 70 ? "warning" : "normal");
  return (
    <button class="ov-acard box" data-tone={tone()} onClick={() => setPage("accounts")} title="Open this profile in Accounts">
      <span class="ov-al1">
        <b class="ell">{props.g.label}</b>
        <span class="grow" />
        <AgentBadge agent={props.g.agent} />
        <For each={props.g.machines}>{(m) => <MachineBadge alias={m} />}</For>
      </span>
      <span class="ov-al2">
        <span class="ell muted">{props.g.identity}</span>
        <span class="grow" />
        <Show when={props.g.installed}>
          <span class="ov-ausage tabular">
            <Show when={limited()} fallback={<span class="dim">{props.g.plan === "API" ? "Balance: 18.40 USD" : "limits n/a"}</span>}>
              <UsageMeters windows={accountWindows(props.g.account)} compact indicator={prefs.usageIndicator as UsageIndicator} />
            </Show>
          </span>
        </Show>
      </span>
    </button>
  );
}

const attentionFirst = (s: Session) => (s.status === "attention" ? 0 : s.unread && s.attention === "reply" ? 1 : s.status === "working" ? 2 : s.status === "background" ? 3 : 4);

function LiveSessions(props: { scroller: () => HTMLElement | undefined }) {
  let grid!: HTMLDivElement;
  const [width, setWidth] = createSignal(1000);
  const [top, setTop] = createSignal(0);
  const rows = createMemo(() => catalog.rows.filter((s) => isActive(s) || matchesFilter(s, "attention")).sort((a, b) => attentionFirst(a) - attentionFirst(b)));
  const cols = () => Math.max(1, Math.floor((width() + GAP) / (CARD_W + GAP)));
  const lines = () => Math.ceil(rows().length / cols());
  // The page scroller is attached only after this grid mounts; reading it
  // earlier would measure a detached element as 0 px high.
  let attached = false;
  const virtualizer = createVirtualizer({
    get count() { return lines(); },
    getScrollElement: () => (attached ? props.scroller() ?? null : null),
    estimateSize: () => CARD_H + GAP,
    overscan: 4,
    get scrollMargin() { return top(); },
  });
  attached = true;
  onMount(() => {
    const measure = () => { setWidth(grid.clientWidth); setTop(grid.offsetTop); };
    const ro = new ResizeObserver(measure);
    ro.observe(grid);
    measure();
    onCleanup(() => ro.disconnect());
  });
  const open = (s: Session) => { setSelectedKey(s.key); setDetailTab("activity"); setPage("sessions"); };
  return (
    <div class="ov-live" ref={grid} style={{ height: `${virtualizer.getTotalSize()}px` }}>
      <For each={virtualizer.getVirtualItems()}>
        {(line) => (
          <div class="ov-line" style={{ transform: `translateY(${line.start - top()}px)`, "grid-template-columns": `repeat(${cols()}, minmax(0, 1fr))` }}>
            <For each={rows().slice(line.index * cols(), line.index * cols() + cols())}>
              {(s) => (
                <div class="ov-cell rowhit" role="button" tabindex="0" aria-label={`Open ${s.name}`} onClick={() => open(s)}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); open(s); } }}>
                  <SessionCard session={s} selected={false} expandable={false} expanded={false} onToggleChildren={() => open(s)} />
                </div>
              )}
            </For>
          </div>
        )}
      </For>
    </div>
  );
}

export function DashboardScreen() {
  let scroller: HTMLDivElement | undefined;
  const [refreshedAt, setRefreshedAt] = createSignal(Date.now());
  const [spinning, setSpinning] = createSignal(false);
  const [, setTick] = createSignal(0);
  // Redraws every 15 s while visible; only changed cards repaint.
  const timer = setInterval(() => setTick((t) => t + 1), 15000);
  onCleanup(() => clearInterval(timer));

  const empty = () => demoState() === "empty";
  const loading = () => demoState() === "loading";
  const fleet = () => (empty() ? machines.filter((m) => m.thisComputer) : machines);
  const online = () => fleet().filter((m) => freshness(m) === "fresh").length;
  const attention = () => (empty() ? 0 : catalog.rows.filter((s) => !s.muted && (s.status === "attention" || (s.unread && s.attention === "reply"))).length);
  const offlineSessions = () => catalog.rows.filter((s) => { const m = machines.find((x) => x.alias === s.machine); return m && freshness(m) !== "fresh"; }).length;
  const groups = createMemo(accountGroups);
  const refresh = () => {
    setSpinning(true);
    setTimeout(() => { setSpinning(false); setRefreshedAt(Date.now()); }, 700);
  };
  const go = (f: "attention" | "active" | "all") => { setMachineFilter([]); setProjectFilter([]); setStateFilter(f); setPage("sessions"); };

  return (
    <div class="su-page ov-page" ref={scroller}>
      <div class="ov-content">
        <header class="su-head">
          <h1>Overview</h1>
          <span class="grow" />
          <button class="icon-btn" data-tip="Refresh overview" aria-label="Refresh overview" onClick={refresh}>
            <Icon name="refresh-cw" size={16} class={spinning() ? "su-spin" : undefined} />
          </button>
        </header>

        <div class="ov-tiles">
          <button class="ov-tile" classList={{ hot: !loading() && attention() > 0 }} onClick={() => go("attention")}>
            <b class="tabular">{loading() ? "—" : attention()}</b><span>Needs attention</span>
          </button>
          <button class="ov-tile" onClick={() => go("active")}><b class="tabular">{loading() ? "—" : empty() ? 0 : counts().active}</b><span>Active</span></button>
          <button class="ov-tile" onClick={() => setPage("machines")}><b class="tabular">{loading() ? "—" : `${online()} / ${fleet().length}`}</b><span>Machines online</span></button>
          <button class="ov-tile" onClick={() => go("all")}><b class="tabular">{loading() ? "—" : empty() ? 0 : counts().all}</b><span>Sessions</span></button>
        </div>

        <h2 class="section-title ov-h">Machines</h2>
        <div class="ov-mgrid">
          <For each={fleet()}>{(m) => <MachineCard m={m} refreshedAt={refreshedAt()} />}</For>
        </div>

        <div class="ov-h row">
          <h2 class="section-title">Accounts</h2>
          <span class="grow" />
          <button class="su-link" onClick={() => setPage("accounts")}>Manage accounts <Icon name="arrow-right" size={12} /></button>
        </div>
        <Show when={!loading() && !empty()} fallback={<p class="ov-empty">{loading() ? "Loading accounts…" : "Open Manage accounts to view or set up accounts."}</p>}>
          <div class="ov-agrid"><For each={groups()}>{(g) => <AccountCard g={g} />}</For></div>
        </Show>

        <div class="ov-h row">
          <h2 class="section-title">Attention &amp; in progress</h2>
          <span class="grow" />
          <button class="su-link" onClick={() => go("all")}>All sessions <Icon name="arrow-right" size={12} /></button>
        </div>
        <Show when={offlineSessions() > 0 && !empty() && !loading()}>
          <div class="notice warn ov-note"><Icon name="wifi-off" size={14} />
            <span>{offlineSessions()} sessions are on offline or stale machines. Their last known status is shown on the machine cards.</span>
          </div>
        </Show>
        <Show when={!loading()} fallback={<div class="ov-skel"><For each={[0, 1, 2]}>{() => <div class="skel-card"><div class="skeleton" style={{ height: "14px", width: "50%" }} /><div class="skeleton" style={{ height: "10px", width: "70%" }} /><div class="skeleton" style={{ height: "10px", width: "40%" }} /></div>}</For></div>}>
          <Show when={!empty()} fallback={<p class="ov-empty">No sessions are working, need attention or have drafts.</p>}>
            <LiveSessions scroller={() => scroller} />
          </Show>
        </Show>
      </div>
    </div>
  );
}

