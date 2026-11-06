import type { AccountsView, Profile } from "../types";
import { tr } from "../i18n";

// Task 69: the account of a running session (`AccountSwitch.*`): options are
// this computer's signed-in profiles of the same agent, the current one first.

export const switchable = (provider: string) => provider === "claude" || provider === "codex";

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
