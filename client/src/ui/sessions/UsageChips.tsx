import { createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { steadyTitle } from "../lib/steady-title";
import { AgentBadge } from "./SessionCard";
import { Icon } from "../lib/icons";
import { ChipPopover } from "./ChipPopover";
import { UsageMeters, type UsageIndicator } from "../components/UsageMeter";
import { useNow } from "../../store/latest";
import { followSessionUsage, refreshSessionUsage, sessionUsage, startSessionUsage, stopSessionUsage } from "../../store/sessionUsage";
import { accountsPage, setAccountMachine, setRequestedProfile, loadAccounts } from "../../store/accounts";
import { requestAccountSwitch, switchable } from "../../store/accountSwitch";
import { accountNameForId } from "../provider-selection";
import { setPage } from "../../store/ui";
import { openInspector } from "../../store/inspector";
import { text } from "../../store/settings";
import { composer } from "../../store/composer";
import { computerOffline } from "../../store/catalog";
import { followedFor } from "../../store/actions";
import { accountTooltip, exhausted, rateLimitNotice, usageRetryWaiting, usageChipView, usageLoading, windowsAt } from "../lib/usage";
import { recoveryInspection, recoveryUsageNotice } from "./recovery-ui";
import { footerUsageSummary, footerUsageTitle } from "./session-usage-ui";
import type { Session } from "./types";
import { tr } from "../../i18n";

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
  const [open, setOpen] = createSignal(false);
  let chip!: HTMLButtonElement;
  const online = () => !computerOffline(props.session.computer);
  const canSwitch = () => switchable(props.session.accountProvider || props.session.agent);
  const accountName = () => {
    const provider = props.session.accountProvider || props.session.agent;
    const id = props.session.accountId || `native-${provider}`;
    return accountNameForId(accountsPage(), props.session.computer, provider, id)
      || u().id || props.session.accountId || tr("the native account");
  };
  const openAccount = () => {
    setOpen(false);
    const s = props.session;
    setAccountMachine(s.computer);
    setRequestedProfile(u().id ? `${s.computer}\u001f${u().id}` : undefined);
    setPage("accounts");
    void loadAccounts();
  };
  const openSwitch = () => {
    setOpen(false);
    requestAccountSwitch(props.session.key);
  };
  // The account tooltip holds still under the pointer (zerus-5ryrx).
  const accountTip = steadyTitle(() => accountTooltip(u(), now()));
  const refreshTip = steadyTitle(() => (usageRetryWaiting(u(), now()) ? rateLimitNotice(u(), now(), true)
    : u().refreshing ? tr("Updating account usage…") : tr("Refresh usage")));
  return (
    <Show when={props.session.tracked && !props.session.archiveId}>
      <span class="session-usage-strip">
        <button ref={chip} type="button" class="sb-usage account-usage" title={accountTip.title()} onPointerEnter={accountTip.enter} onPointerLeave={accountTip.leave} aria-label={`${tr("Account usage")}\n${exhausted(u(), now())}\n${accountTooltip(u(), now())}`} aria-haspopup="menu" aria-expanded={open()} onClick={() => setOpen(!open())}>
          <Show when={u().provider}><AgentBadge agent={props.session.agent} /></Show>
          <UsageMeters windows={windowsAt(u().windows, now())} loading={usageLoading(u())} summary={footerUsageSummary(u())} summaryTitle={footerUsageTitle(u())} indicator={text("workspace/usageIndicator") as UsageIndicator} />
          <Show when={rateLimitNotice(u(), now())}><span class="dim">{rateLimitNotice(u(), now())}</span></Show>
        </button>
        <button type="button" class="icon-btn small usage-refresh" disabled={!online() || u().refreshing || usageRetryWaiting(u(), now())} aria-label={tr("Refresh usage")}
          title={refreshTip.title()} onPointerEnter={refreshTip.enter} onPointerLeave={refreshTip.leave} onClick={() => void refreshSessionUsage(true)}>
          <Show when={!u().refreshing} fallback={<span class="su-spin"><Icon name="refresh-cw" size={12} /></span>}><Icon name="refresh-cw" size={12} /></Show>
        </button>
        <ChipPopover open={open()} anchor={chip} label={tr("Account usage")} keyboard={false} class="usage-limit-pop" onClose={(focus) => { setOpen(false); if (focus) chip.focus(); }}>
          <p class="muted">{tr("This session runs on %1.", accountName())}</p>
          <div class="cache-actions">
            <Show when={canSwitch()}>
              <button type="button" class="btn small" onClick={openSwitch}>{tr("Switch this session's account…")}</button>
            </Show>
            <button type="button" class="btn small ghost" onClick={openAccount}>{tr("Manage accounts…")}</button>
          </div>
        </ChipPopover>
      </span>
    </Show>
  );
}

/**
 * Task #351: the composer's usage chip. It carries the two bars the status bar
 * used to show: the account's windows with the weekly percent and the days to
 * its reset (the 5 h window stays in the tooltip), or a DeepSeek (dsh)
 * session's account balance, or why that balance is unknown.
 */
export function UsageLimitChip(props: { session: Session; compact: boolean }) {
  const now = useNow(30_000);
  // Task #351: the chip is now the only usage reader in the window; the status
  // bar's strip that followed the selected session is gone.
  onMount(startSessionUsage);
  createEffect(() => {
    const s = props.session;
    followSessionUsage(s.tracked && !s.archiveId ? { host: s.computer, name: s.name, run_id: s.runId ?? "" } : null, s.cmd);
  });
  onCleanup(() => { followSessionUsage(null); stopSessionUsage(); });
  const [open, setOpen] = createSignal(false);
  const [keyboard, setKeyboard] = createSignal(false);
  let chip!: HTMLButtonElement;
  const u = sessionUsage;
  const details = () => recoveryInspection(props.session, followedFor(props.session.key));
  const exhaustedText = () => exhausted(u(), now());
  const notice = () => recoveryUsageNotice(props.session, details(), u(), !computerOffline(props.session.computer), now());
  const warning = () => exhaustedText() || notice();
  const view = () => usageChipView(u(), now(), props.session.agent);
  const label = () => warning() || view().label;
  const show = () => !!warning() || (view().kind === "balance"
    ? u().status !== "" && u().status !== "loading"
    : view().windows.length > 0);
  const tooltip = () => warning() || (view().kind === "balance"
    ? [view().balance, view().reason, accountTooltip(u(), now())].filter(Boolean).join("\n")
    : accountTooltip(u(), now()));
  createEffect(() => { if (!show()) setOpen(false); });
  return (
    <Show when={show()}>
      <span class="att-anchor">
        <button ref={chip} type="button" class={`tchip usage-chip ${warning() ? exhaustedText() ? "danger" : "warn" : ""}`} classList={{ on: open() }} aria-haspopup="dialog" aria-expanded={open()}
          aria-label={label()} title={tooltip()}
          onClick={(event) => { setKeyboard(event.detail === 0); setOpen(!open()); }}>
          <Icon name="gauge" size={13} />
          <Show when={view().kind === "balance"} fallback={
            <UsageMeters windows={view().windows} compact={props.compact} indicator={text("workspace/usageIndicator") as UsageIndicator}
              summary={footerUsageSummary(u())} resetFrom={0} />
          }>
            <span class="usage-balance">{view().balance ?? tr("Balance unknown")}</span>
          </Show>
        </button>
        <ChipPopover open={open()} anchor={chip} label={tr("Account usage limit")} keyboard={keyboard()} class="usage-limit-pop" onClose={(focus) => { setOpen(false); if (focus) chip.focus(); }}>
          <p class="usage-warning">{warning() || label()}</p>
          <Show when={view().reason}><p class="muted">{view().reason}</p></Show>
          <div class="cache-actions"><button type="button" class="btn small" disabled={computerOffline(props.session.computer) || u().refreshing} onClick={() => void refreshSessionUsage(true)}>{tr("Refresh usage")}</button></div>
        </ChipPopover>
      </span>
    </Show>
  );
}

/** Context opens the mockup's Details tab and its existing Session usage card. */
export function SessionUsageChip() {
  const context = () => composer.view()?.context;
  return (
    <Show when={context()?.visible}>
      <button type="button" class={`tchip ${toneClass(context()!.tone)}`} title={context()!.tooltip} aria-label={context()!.accessible_name}
        onClick={() => openInspector("details")}>
        <Show when={!context()!.warning} fallback={<Icon name="context-warning" size={13} />}>
          <span class="ctx-ring" style={{ "--p": `${Math.max(0, Math.min(100, context()!.percent ?? 0))}%` }} />
        </Show>
        {context()!.text}
      </button>
    </Show>
  );
}
