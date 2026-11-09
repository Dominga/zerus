import { createSignal } from "solid-js";
import type { AccountsView, Profile } from "../types";
import { tr } from "../i18n";

// Task 69: the account of a running session (`AccountSwitch.*`): options are
// this computer's signed-in profiles of the same agent, the current one first.
// Task 352: every installed agent's usable accounts, grouped by agent; an
// account of another agent moves the session there with a handoff.

/** Agents whose accounts a session changes in place (`hgs switch-account`). */
export const switchable = (provider: string) => provider === "claude" || provider === "codex";

/** Agents a session moves between, in the menu's group order. */
export const MOVE_AGENTS = ["claude", "codex", "kimi", "dsh", "opencode"] as const;
export const movable = (provider: string) => (MOVE_AGENTS as readonly string[]).includes(provider);

/** The session's account id as the switch reads it: none is the native one. */
export const accountOf = (provider: string, accountId: string) =>
  accountId || `native-${provider}`;

const signedOut = new Set(["signed_out", "expired", "credentials_locked", "credentials_unavailable", "desktop_session_unavailable"]);

/** The choices in the session's menu, the current one first and marked. */
export function accountOptions(view: AccountsView, host: string, provider: string, current: string): Profile[] {
  if (!switchable(provider)) return [];
  const rows: Profile[] = [];
  for (const account of view.accounts) {
    for (const profile of account.members) {
      if (profile.host !== host || profile.provider !== provider || profile.signed_in !== true) continue;
      const currentRow = profile.id === current;
      if (!currentRow && signedOut.has(profile.state)) continue;
      rows.push(profile);
    }
  }
  return rows.sort((a, b) => Number(b.id === current) - Number(a.id === current));
}

/** One agent's accounts in the chip's menu. */
export interface AccountGroup { provider: string; rows: Profile[] }

/** Whether a profile can take a session: installed and online here, and for
 * Claude, Codex and Kimi signed in. DeepSeek and OpenCode run on their
 * native configuration. */
function usable(profile: Profile): boolean {
  if (!profile.installed || profile.online === false || profile.state === "not-installed" || profile.state === "offline") return false;
  if (profile.provider === "dsh" || profile.provider === "opencode") return true;
  return profile.signed_in === true && !signedOut.has(profile.state);
}

/** Task 352: every usable account of every agent on the session's computer,
 * grouped by agent: the session's own agent first (its current account first
 * and marked), then the others in MOVE_AGENTS order. The own agent's other
 * accounts are offered only where it switches in place. */
export function accountGroups(view: AccountsView, host: string, provider: string, current: string): AccountGroup[] {
  const groups = new Map<string, Profile[]>();
  const seen = new Set<string>();
  for (const account of view.accounts) {
    for (const profile of account.members) {
      if (profile.host !== host || !movable(profile.provider) || seen.has(`${profile.provider}/${profile.id}`)) continue;
      const own = profile.provider === provider;
      const currentRow = own && profile.id === current;
      if (!currentRow && (!usable(profile) || (own && !switchable(provider)))) continue;
      seen.add(`${profile.provider}/${profile.id}`);
      groups.set(profile.provider, [...(groups.get(profile.provider) ?? []), profile]);
    }
  }
  const order = [provider, ...MOVE_AGENTS.filter((agent) => agent !== provider)];
  return order.filter((agent) => groups.has(agent)).map((agent) => ({
    provider: agent,
    rows: (groups.get(agent) ?? []).sort((a, b) => Number(b.id === current) - Number(a.id === current)),
  }));
}

/** The line the chip shows after a move to another agent. */
export function moveOutcome(receipt: Record<string, unknown>, error: string, target: string): string {
  const status = String(receipt.status ?? "");
  const created = String(receipt.new_session ?? "");
  if (status === "moved") return tr("Moved to %1 in %2; the old session is stopped and stays readable.", target, created);
  if (status === "uncertain") return tr("%1 started for %2, but it did not confirm the handoff. Check it; Zerus does not retry.", created || target, target);
  if (status === "failed") {
    const reason = String(receipt.reason ?? "");
    return receipt.relaunched === false
      ? tr("The move to %1 failed (%2). Resume the old session from Activity.", target, reason)
      : tr("The move to %1 failed (%2); the old session resumed.", target, reason);
  }
  const clean = error.replace(/^hgs:\s*/, "").trim();
  return tr("Not moved: %1", clean || tr("the move gave no receipt"));
}

/** The line the chip shows after a switch, as Qt words its receipt. */
export function switchOutcome(receipt: Record<string, unknown>, error: string, target: string): string {
  const status = String(receipt.status ?? "");
  const from = (receipt.from ?? {}) as Record<string, unknown>;
  const to = (receipt.to ?? {}) as Record<string, unknown>;
  const toLabel = String(to.label ?? to.account_id ?? target);
  const fromLabel = String(from.label ?? from.account_id ?? "");
  if (status === "switched") return tr("Moved to %1; the agent resumed the same conversation.", toLabel);
  if (status === "uncertain") return tr("Moved to %1, but the resumed conversation did not confirm. Check the session; Zerus does not retry it.", toLabel);
  if (status === "rolled_back") {
    const reason = String(receipt.reason ?? "");
    return receipt.relaunched === false
      ? tr("The move to %1 failed (%2) and the agent did not restart on %3. Resume it from Activity.", target, reason, fromLabel)
      : tr("Stayed on %1: %2 could not start the agent (%3). It resumed on %1.", fromLabel, target, reason);
  }
  const clean = error.replace(/^hgs:\s*/, "").trim();
  return tr("Not moved: %1", clean || tr("the switch gave no receipt"));
}

/** A canonical request id for one switch. */
export const switchRequest = () =>
  typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : fallbackRequest();

let fallback = 0;
function fallbackRequest(): string {
  fallback += 1;
  const hex = fallback.toString(16).padStart(12, "0").slice(-12);
  return `00000000-0000-4000-8000-${hex}`;
}

// Owner #83: the status-bar usage chip's "Switch this session's account…"
// entry asks the composer chip to open its list for the same session.
const [requested, setRequested] = createSignal("");
export const accountSwitchRequested = requested;
export function requestAccountSwitch(sessionKey: string) { setRequested(sessionKey); }
export function consumeAccountSwitch(sessionKey: string): boolean {
  if (requested() !== sessionKey) return false;
  setRequested("");
  return true;
}


