import { tr } from "../../i18n";
import type { WaitingItem } from "../../store/teams";

// Waiting for you's vocabulary: a card's kind in words and with an icon, its
// age, and the one-click decisions it offers. A decision is a word the store
// sends with one request id; the core answers done, failed, uncertain, confirm
// or pending, and the view never decides by itself.

export interface WaitingDecision {
  /** The decision word the store sends (`decide {card, decision}`). */
  decision: string;
  text: string;
  danger?: boolean;
}

/** Every card kind the store and the team service open on integrate/teams
 * and the branches that add kinds (packages 7, 8, 10 and 17): the store opens
 * integration and pipeline_acceptance itself; the launch worker
 * launch_blocked, launch_uncertain and launch_capacity; standing
 * confirmations standing_confirmation; the team actor team_actor_blocked;
 * the timers report_waiting, silent_lead and late_session; a person, a lead
 * or the service may open a note. */
export const KNOWN_KINDS = [
  "note", "integration", "pipeline_acceptance", "launch_blocked", "launch_uncertain", "launch_capacity",
  "standing_confirmation", "team_actor_blocked", "report_waiting", "silent_lead", "late_session",
  "rejected_merge",
] as const;

/** The card's kind in words, keyed by the exact kind the store sends. A kind
 * not in the table shows a neutral label with its plain kind, never another
 * kind's text. */
export function waitingKindLabel(item: Pick<WaitingItem, "kind">): string {
  switch (item.kind) {
    case "note": return tr("Note");
    case "integration": return tr("Integration");
    case "pipeline_acceptance": return tr("Work acceptance");
    case "launch_blocked": return tr("Member not started");
    case "launch_uncertain": return tr("Start not confirmed");
    case "launch_capacity": return tr("No room for members");
    case "standing_confirmation": return tr("Member needs you");
    case "team_actor_blocked": return tr("Team service blocked");
    case "report_waiting": return tr("Report waiting");
    case "silent_lead": return tr("Lead not working");
    case "late_session": return tr("Late session");
    case "rejected_merge": return tr("Rejected result is merged");
    default: return tr("Card: %1", item.kind);
  }
}

/** The card's parameters as the service's writer bounds them: at most eight
 * pairs, each key lower-case ASCII letters and underscores, each value
 * nonempty, at most 256 characters and free of control characters, line or
 * paragraph separators and bidirectional marks. Anything else, or a duplicate
 * key, means the stored sentence stands and no value is interpolated. */
function cardParameters(raw: WaitingItem["parameters"]): Map<string, string> | null {
  if (raw === undefined) return new Map();
  if (!Array.isArray(raw) || raw.length > 8) return null;
  const values = new Map<string, string>();
  for (const pair of raw) {
    if (!Array.isArray(pair) || pair.length !== 2) return null;
    const [key, value] = pair;
    if (typeof key !== "string" || typeof value !== "string") return null;
    if (!/^[a-z_]+$/.test(key) || values.has(key)) return null;
    if (value.trim().length === 0) return null;
    if ([...value].length > 256) return null;
    if (/[\p{Cc}\u2028\u2029\u202a-\u202e\u2066-\u2069]/u.test(value)) return null;
    values.set(key, value);
  }
  return values;
}

/** One sentence that says what a card of this kind is about. The kinds whose
 * writer sends bounded parameters substitute them (zerus-63rru.34); an
 * invalid parameter, or a kind this version does not describe, keeps the
 * stored sentence itself, never a generic description. The capacity notice is
 * team-wide: the budget marker is not a member (F5). */
export function waitingKindSentence(item: Pick<WaitingItem, "kind" | "parameters" | "text">): string {
  const parameters = cardParameters(item.parameters);
  if (parameters === null) return item.text;
  // The ledger's internal markers (@budget, @branch) are not member names:
  // a card that still carries one reads as a team-wide sentence.
  const written = parameters.get("member");
  const member = written !== undefined && !written.startsWith("@") ? written : undefined;
  switch (item.kind) {
    case "note": return tr("A note that the lead, the service or you left for the person.");
    case "integration": return tr("The lead accepted a result; merging it into the integration branch waits for your approval.");
    case "pipeline_acceptance": return tr("A task's pipeline finished and asks you to accept its work.");
    case "launch_blocked": return member
      ? tr("Member %1 could not be started; the card says why and whether Zerus tries again.", member)
      : tr("A team member could not be started; the card says why and whether Zerus tries again.");
    case "launch_uncertain": return member
      ? tr("Whether member %1 started is not known; Zerus checks before it starts anything again.", member)
      : tr("Whether a team member started is not known; Zerus checks before it starts anything again.");
    case "launch_capacity": return member
      ? tr("Member %1 waits until there is room for another session or account.", member)
      : tr("New team members wait until there is room for another session or account.");
    case "standing_confirmation": return member
      ? tr("Member %1 needs you: the card says what it asks.", member)
      : tr("A member's session asks for something Zerus does not confirm on your behalf.");
    case "team_actor_blocked": return tr("The team's service could not finish a step and keeps trying.");
    case "report_waiting": return tr("A member's report has waited for the lead's decision longer than the team allows.");
    case "silent_lead": return tr("The lead has not worked for a while although items wait in its inbox.");
    case "late_session": return tr("A member's session started after the team stopped or the member was removed; Zerus does not stop it.");
    case "rejected_merge": return tr("The rejection did not undo the merge; only a revert on the integration branch removes it.");
    default: return item.text;
  }
}

/** The card's icon (ui/lib/icons names), keyed like the label; a kind not in
 * the table gets a neutral icon of its own. */
export function waitingKindIcon(kind: string): string {
  switch (kind) {
    case "note": return "message-square";
    case "integration": return "git-merge";
    case "pipeline_acceptance": return "check";
    case "launch_blocked": return "circle-alert";
    case "launch_uncertain": return "circle-question-mark";
    case "launch_capacity": return "hourglass";
    case "standing_confirmation": return "hand";
    case "team_actor_blocked": return "server";
    case "report_waiting": return "clock";
    case "silent_lead": return "bell";
    case "late_session": return "timer";
    case "rejected_merge": return "git-branch";
    default: return "info";
  }
}

/** The words of the decisions this version can offer, by the card list's
 * token: the store's approve and reject, the services' retry and the
 * acknowledgement every other card takes. */
const DECISION_WORDS: Record<string, { text: () => string; danger?: boolean }> = {
  approve: { text: () => tr("Approve") },
  reject: { text: () => tr("Reject"), danger: true },
  retry: { text: () => tr("Retry") },
  acknowledged: { text: () => tr("Acknowledge") },
};

/** The decisions a card takes: its row's list, in the list's order, and
 * nothing else (zerus-63rru.29: every opener states the decisions something
 * reads, and the store refuses one the card does not list). No decision comes
 * from the card's kind; a token this version has no words for is not offered;
 * a row without a list (an older hgs) takes acknowledged alone. */
export function waitingDecisions(item: Pick<WaitingItem, "kind" | "decisions">): WaitingDecision[] {
  return (item.decisions ?? ["acknowledged"]).flatMap((token) => {
    const words = Object.hasOwn(DECISION_WORDS, token) ? DECISION_WORDS[token] : undefined;
    return words ? [{ decision: token, text: words.text(), ...(words.danger ? { danger: true } : {}) }] : [];
  });
}

/** The card's age in words, from its creation time. */
export function waitingAge(created_ms: number, now_ms: number): string {
  if (!created_ms) return "";
  const seconds = Math.max(0, Math.floor((now_ms - created_ms) / 1000));
  const minutes = Math.floor(seconds / 60);
  if (minutes < 1) return tr("now");
  if (minutes < 60) return tr("%1 m", minutes);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tr("%1 h", hours);
  return tr("%1 d", Math.floor(hours / 24));
}
