import { createEffect, createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { AgentBadge } from "./SessionCard";
import { Icon } from "../lib/icons";
import { ChipPopover } from "./ChipPopover";
import { UsageMeters, type UsageIndicator } from "../components/UsageMeter";
import { useNow } from "../../store/latest";
import { followSessionUsage, refreshSessionUsage, sessionUsage, startSessionUsage, stopSessionUsage } from "../../store/sessionUsage";
import { setAccountMachine, setRequestedProfile, loadAccounts } from "../../store/accounts";
import { setPage } from "../../store/ui";
import { text } from "../../store/settings";
import { composer } from "../../store/composer";
import { computerOffline } from "../../store/catalog";
import { followedFor } from "../../store/actions";
import { accountTooltip, exhausted, exhaustedSummary, usageLoading, windowsAt } from "../lib/usage";
import { recoveryInspection, recoveryUsageNotice } from "./recovery-ui";
import { footerUsageSummary, sessionUsageView } from "./session-usage-ui";
import type { Session } from "./types";

const toneClass = (tone: string) => tone === "danger" ? "danger" : tone === "warning" ? "warn" : tone === "success" ? "ok" : "";

/** AccountUsage::Button and RefreshButton in the bottom status bar. */
export function AccountUsageChip(props: { session: Session }) {
  const now = useNow(30_000);
  onMount(startSessionUsage);
  createEffect(() => {
    const s = props.session;
    followSessionUsage(s.tracked && !s.archiveId ? { host: s.computer, name: s.name, run_id: s.runId ?? "" } : null, s.cmd);
  });
  onCleanup(() => { followSessionUsage(null); stopSessionUsage(); });
  const u = sessionUsage;
  const online = () => !computerOffline(props.session.computer);
  const openAccount = () => {
    const s = props.session;
    setAccountMachine(s.computer);
    setRequestedProfile(u().id ? `${s.computer}\u001f${u().id}` : undefined);
    setPage("accounts");
    void loadAccounts();
  };
  return (
    <Show when={props.session.tracked && !props.session.archiveId}>
      <span class="session-usage-strip">
        <button type="button" class="sb-usage account-usage" title={accountTooltip(u(), now())} aria-label={`Account usage\n${exhausted(u(), now())}\n${accountTooltip(u(), now())}`} onClick={openAccount}>
          <Show when={u().provider}><AgentBadge agent={props.session.agent} /></Show>
          <UsageMeters windows={windowsAt(u().windows, now())} loading={usageLoading(u())} summary={footerUsageSummary(u())} indicator={text("workspace/usageIndicator") as UsageIndicator} />
        </button>
        <button type="button" class="icon-btn usage-refresh" disabled={!online() || u().refreshing} aria-label="Refresh account usage"
          title={u().refreshing ? "Updating account usage…" : "Refresh account usage (automatically every 5 minutes)"} onClick={() => void refreshSessionUsage(true)}>
          <Show when={!u().refreshing} fallback={<span class="su-spin"><Icon name="refresh-cw" size={14} /></span>}><Icon name="refresh-cw" size={14} /></Show>
        </button>
      </span>
    </Show>
  );
}

/** The composer's Usage limit chip and its cached/offline warning popover. */
export function UsageLimitChip(props: { session: Session; compact: boolean }) {
  const now = useNow(30_000);
  const [open, setOpen] = createSignal(false);
  const [keyboard, setKeyboard] = createSignal(false);
  let chip!: HTMLButtonElement;
  const u = sessionUsage;
  const details = () => recoveryInspection(props.session, followedFor(props.session.key));
  const exhaustedText = () => exhausted(u(), now());
  const notice = () => recoveryUsageNotice(props.session, details(), u(), !computerOffline(props.session.computer), now());
  const warning = () => exhaustedText() || notice();
  const summary = () => exhaustedSummary(u(), now());
  createEffect(() => { if (!warning()) setOpen(false); });
  return (
    <Show when={warning()}>
      <span class="att-anchor">
        <button ref={chip} type="button" class={`tchip ${exhaustedText() ? "danger" : ""}`} classList={{ on: open() }} aria-haspopup="dialog" aria-expanded={open()}
          aria-label={summary() ? `${summary()!.label}${summary()!.remaining ? ` ${summary()!.remaining}` : ""}` : "Usage available"} title={warning()}
          onClick={(event) => { setKeyboard(event.detail === 0); setOpen(!open()); }}>
          <Icon name="attention" size={13} />
          <Show when={props.compact} fallback={<>{(summary()?.label ?? "Usage available")}<Show when={summary()?.remaining}><span class="usage-reset">{summary()!.remaining}</span></Show></>}>
            {summary()?.shortLabel ?? "Usage"}
          </Show>
        </button>
        <ChipPopover open={open()} anchor={chip} label="Account usage limit" keyboard={keyboard()} class="usage-limit-pop" onClose={(focus) => { setOpen(false); if (focus) chip.focus(); }}>
          <p class="usage-warning">{warning()}</p>
          <div class="cache-actions"><button type="button" class="btn small" disabled={computerOffline(props.session.computer) || u().refreshing} onClick={() => void refreshSessionUsage(true)}>Refresh usage</button></div>
        </ChipPopover>
      </span>
    </Show>
  );
}

/** SessionUsage::ContextButton and the Details side panel's usage section. */
export function SessionUsageChip(props: { session: Session }) {
  const [open, setOpen] = createSignal(false);
  const [keyboard, setKeyboard] = createSignal(false);
  let chip!: HTMLButtonElement;
  const context = () => composer.view()?.context;
  const raw = () => followedFor(props.session.key).value?.session_usage;
  const panel = () => sessionUsageView(raw());
  createEffect(() => { if (!context()?.visible) setOpen(false); });
  return (
    <Show when={context()?.visible}>
      <span class="att-anchor">
        <button ref={chip} type="button" class={`tchip ${toneClass(context()!.tone)}`} classList={{ on: open() }} aria-haspopup="dialog" aria-expanded={open()}
          title={context()!.tooltip} aria-label={context()!.accessible_name}
          onClick={(event) => { setKeyboard(event.detail === 0); setOpen(!open()); }}>
          <Show when={!context()!.warning} fallback={<Icon name="context-warning" size={13} />}>
            <span class="ctx-ring" style={{ "--p": `${Math.max(0, Math.min(100, context()!.percent ?? 0))}%` }} />
          </Show>
          {context()!.text}
        </button>
        <ChipPopover open={open()} anchor={chip} label="Session usage" keyboard={keyboard()} class="session-usage-pop" width={560} onClose={(focus) => { setOpen(false); if (focus) chip.focus(); }}>
          <h3>Session usage</h3>
          <Show when={!panel().status} fallback={<p class="muted">{panel().status}</p>}>
            <table class="usage"><For each={panel().rows}>{(row) => <tr><td>{row.label}</td><td>{row.value}</td></tr>}</For></table>
            <Show when={panel().costNote}><p class="session-usage-note">{panel().costNote}</p></Show>
            <Show when={panel().context}><p><b>Latest context</b><br />{panel().context}</p></Show>
            <p class="session-usage-note">{panel().footer}</p>
          </Show>
        </ChipPopover>
      </span>
    </Show>
  );
}
