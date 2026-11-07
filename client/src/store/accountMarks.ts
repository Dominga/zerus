import { createMemo } from "solid-js";
import type { Account, AccountsView, Profile } from "../types";
import { accountsPage } from "./accounts";
import { settings, setSetting, type Settings } from "./settings";
import { accountChoices, rowBinding, type BoundRow } from "../ui/sessions/scope-filters";

// Account marks on session cards (zerus-sa9qn.1, spec
// docs/superpowers/specs/2026-11-07-account-labels-child-sessions-design.md
// section 1.6). The label is the host catalog's; the color and emoji are this
// desktop's preferences for each member profile, like machine colors.

export const ACCOUNT_COLOR = "accounts/color/";
export const ACCOUNT_MARK = "accounts/mark/";
export const MARK_STYLE = "sessions/accountMarks";
/** Session cards show account marks at all (zerus-sa9qn.13; default on). */
export const SHOW_MARKS = "sessions/showAccountMarks";
/** One account's mark is not shown on session cards (presentation only). */
export const ACCOUNT_HIDE = "accounts/hide/";
const SEPARATOR = String.fromCharCode(31);
const HEX = /^#[\da-f]{6}$/iu;

export interface AccountMark {
  /** The grouped account key (Accounts screen). */
  key: string;
  label: string;
  /** The label's first letter or digit, uppercased. */
  letter: string;
  /** The chosen emoji, "" when none. */
  emoji: string;
  /** What the circle shows: the emoji when the option is on and one is set. */
  glyph: string;
  isEmoji: boolean;
  /** A CSS color: the chosen hex, else the label's palette slot. */
  color: string;
  /** Dark or light ink for the letter on `color`. */
  ink: string;
  provider: string;
  /** The profile's machine name, for the tooltip. */
  machine: string;
}

/** The settings token of one member profile: URI-encoded computer and profile ID. */
export const profileToken = (host: string, id: string) => encodeURIComponent(`${host}${SEPARATOR}${id}`);

/** A stable palette slot (0-7) for a label. */
export function labelSlot(label: string): number {
  let hash = 0;
  for (let i = 0; i < label.length; i++) hash = (hash * 31 + label.charCodeAt(i)) | 0;
  return Math.abs(hash) % 8;
}

function graphemes(text: string): string[] {
  const Segmenter = (Intl as unknown as { Segmenter?: new (locale?: string, options?: { granularity: string }) => { segment(text: string): Iterable<{ segment: string }> } }).Segmenter;
  return Segmenter ? Array.from(new Segmenter(undefined, { granularity: "grapheme" }).segment(text), (s) => s.segment) : Array.from(text);
}

/** The first letter or digit of the label, uppercased; the provider's initial when there is none. */
export function markLetter(label: string, provider = ""): string {
  const first = graphemes(label.trim()).find((g) => /[\p{L}\p{N}]/u.test(g));
  return (first ?? provider.slice(0, 1) ?? "").toLocaleUpperCase() || "?";
}

/** Exactly one emoji grapheme (a pictograph or a flag). */
export function validEmoji(text: string): boolean {
  const value = text.trim();
  if (!value || value.length > 32) return false;
  const parts = graphemes(value);
  return parts.length === 1 && /\p{Extended_Pictographic}|\p{Regional_Indicator}/u.test(parts[0]);
}

export const validColor = (value: unknown): value is string => typeof value === "string" && HEX.test(value);

/** WCAG relative luminance of a #rrggbb color. */
function luminance(hex: string): number {
  const channel = (at: number) => {
    const c = parseInt(hex.slice(at, at + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
}
const DARK_INK = "#10151b", LIGHT_INK = "#ffffff";
/** The ink with the stronger contrast on the color (WCAG contrast ratio). */
export function inkFor(hex: string): string {
  const color = luminance(hex);
  const ratio = (ink: string) => { const l = luminance(ink); return (Math.max(l, color) + 0.05) / (Math.min(l, color) + 0.05); };
  return ratio(DARK_INK) >= ratio(LIGHT_INK) ? DARK_INK : LIGHT_INK;
}

/** The appearance a desktop keeps for an account: the first member profile's choice wins. */
export function accountAppearance(values: Readonly<Settings>, members: readonly Pick<Profile, "host" | "id">[]): { color: string; emoji: string; hidden: boolean } {
  let color = "", emoji = "", hidden = false;
  for (const m of members) {
    const token = profileToken(m.host, m.id);
    const c = values[ACCOUNT_COLOR + token], e = values[ACCOUNT_MARK + token];
    if (!color && validColor(c)) color = c.toLowerCase();
    if (!emoji && typeof e === "string" && validEmoji(e)) emoji = e.trim();
    // The account's choice is stored on every member; any of them hides it.
    if (values[ACCOUNT_HIDE + token] === true) hidden = true;
  }
  return { color, emoji, hidden };
}

/** Whether session cards show account marks at all (Settings > Sessions; default on). */
export const marksShown = (values: Readonly<Settings>) => values[SHOW_MARKS] !== false;

/** The mark of one grouped account. */
export function accountMark(account: Account, values: Readonly<Settings>, machine = ""): AccountMark {
  const label = account.label || account.usage.identity.email || account.provider;
  const { color, emoji } = accountAppearance(values, account.members);
  const isEmoji = !!emoji && values[MARK_STYLE] === "emoji";
  return {
    key: account.key, label, letter: markLetter(label, account.provider), emoji,
    glyph: isEmoji ? emoji : markLetter(label, account.provider), isEmoji,
    color: color || `var(--z-entity-palette-${labelSlot(label)})`,
    ink: color ? inkFor(color) : "#000",
    provider: account.provider, machine,
  };
}

/** Each listed binding (computer, provider, profile ID or unambiguous home) to its account and profile. */
export function markIndex(view: Pick<AccountsView, "accounts" | "binding_evidence" | "binding_evidence_complete">): Map<string, { account: Account; profile?: Profile }> {
  const index = new Map<string, { account: Account; profile?: Profile }>();
  const accounts = new Map(view.accounts.map((a) => [a.key, a] as const));
  for (const choice of accountChoices(view.accounts, view.binding_evidence_complete ? view.binding_evidence ?? [] : [], [])) {
    const account = accounts.get(choice.id);
    if (!account) continue;
    for (const binding of choice.bindings) if (!index.has(binding)) index.set(binding, { account });
  }
  return index;
}

/** A row's mark, or undefined when its binding is not reported or not in the loaded catalog. */
export function rowMark(row: BoundRow, index: ReadonlyMap<string, { account: Account }>, values: Readonly<Settings>): AccountMark | undefined {
  // Hiding is presentation only: the label and the binding stay as they are.
  if (!marksShown(values)) return undefined;
  if (row.accountReported === false || (!row.accountId && !row.accountHome)) return undefined;
  const found = index.get(rowBinding(row));
  if (!found) return undefined;
  const profile = found.account.members.find((p) => p.host === row.computer && (row.accountId ? p.id === row.accountId : p.home === row.accountHome));
  if (accountAppearance(values, found.account.members).hidden) return undefined;
  return accountMark(found.account, values, profile?.machine ?? "");
}

const index = createMemo(() => markIndex(accountsPage()));
/** The card's account mark (reactive on the Accounts catalog and the desktop's appearance settings). */
export const sessionMark = (row: BoundRow) => rowMark(row, index(), settings);

/** Store one account's color and emoji on every member profile; null clears the choice. */
export async function saveAccountAppearance(account: Pick<Account, "members">, color: string | null, emoji: string | null, hidden = false) {
  if (color !== null && !validColor(color)) throw new Error("Invalid color");
  if (emoji !== null && !validEmoji(emoji)) throw new Error("Invalid emoji");
  let stored = true;
  for (const m of account.members) {
    const token = profileToken(m.host, m.id);
    stored = (await setSetting(ACCOUNT_COLOR + token, color === null ? null : color.toLowerCase())) && stored;
    stored = (await setSetting(ACCOUNT_MARK + token, emoji === null ? null : emoji.trim())) && stored;
    stored = (await setSetting(ACCOUNT_HIDE + token, hidden ? true : null)) && stored;
  }
  return stored;
}
