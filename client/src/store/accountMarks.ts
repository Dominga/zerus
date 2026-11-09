import { createMemo } from "solid-js";
import type { Account, AccountsView, Profile } from "../types";
import { accountsPage } from "./accounts";
import { settings, setSetting, type Settings } from "./settings";
import type { accountEditApply, accountEditOpen } from "../bridge";
import { accountChoices, rowBinding, type BoundRow } from "../ui/sessions/scope-filters";

// Account marks on session cards (zerus-sa9qn.1, spec
// docs/superpowers/specs/2026-11-07-account-labels-child-sessions-design.md
// section 1.6). The label is the host catalog's. The color and emoji are kept
// with the account on each of its computers (zerus-sa9qn.6), so every desktop
// and phone shows the same mark; a computer with an older hgs keeps them as
// this desktop's preferences for its member profile, as before. Hiding one
// account's mark stays this desktop's choice.

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

/** A member profile with what its computer keeps of the account's mark. */
export type MarkMember = Pick<Profile, "host" | "id"> & Partial<Pick<Profile, "key" | "online" | "mark_color" | "mark_emoji" | "marks_supported">>;

/**
 * An account's appearance: the first member profile's choice wins. A
 * member's computer value wins over this desktop's, which stands in until it
 * moves there (migrateMarks) or for a computer with an older hgs.
 */
export function accountAppearance(values: Readonly<Settings>, members: readonly MarkMember[]): { color: string; emoji: string; hidden: boolean } {
  let color = "", emoji = "", hidden = false;
  for (const m of members) {
    const token = profileToken(m.host, m.id);
    const hosted = !!m.marks_supported;
    const c = hosted && validColor(m.mark_color) ? m.mark_color : values[ACCOUNT_COLOR + token];
    const e = hosted && m.mark_emoji && validEmoji(m.mark_emoji) ? m.mark_emoji : values[ACCOUNT_MARK + token];
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

/** This desktop could not keep its part of the appearance; the dialog translates it. */
export const MARK_NOT_SAVED = "The account appearance could not be saved. Try again.";
/** Where an account mark is saved: a mark edit of the account's computers. */
export interface MarkPort { open: typeof accountEditOpen; apply: typeof accountEditApply }

/**
 * Save one account's color and emoji with the account on each of its
 * computers (zerus-sa9qn.6); null clears them. A computer with an older hgs
 * keeps them as this desktop's preference; a computer that saved them makes
 * this desktop forget its own. Hiding stays this desktop's. Returns what
 * went wrong, nothing when every computer has the mark.
 */
export async function saveAccountAppearance(account: Pick<Account, "key" | "members">, color: string | null, emoji: string | null, hidden: boolean,
  port: MarkPort): Promise<string[]> {
  if (color !== null && !validColor(color)) throw new Error("Invalid color");
  if (emoji !== null && !validEmoji(emoji)) throw new Error("Invalid emoji");
  const errors: string[] = [];
  let local = account.members.filter((m) => !m.marks_supported).map((m) => m.key);
  let marked: string[] = [];
  const hosted = account.members.filter((m) => m.marks_supported);
  if (hosted.length) {
    const selected = hosted.find((m) => m.online) ?? hosted[0];
    try {
      const view = await port.open(account.key, selected.key, "mark");
      const result = await port.apply(view.handle, { color: color?.toLowerCase() ?? "", emoji: emoji?.trim() ?? "" });
      local = [...local, ...(result.local ?? [])];
      marked = result.marked ?? [];
      errors.push(...result.errors);
    } catch (error) { errors.push(String(error)); }
  }
  let stored = true;
  for (const m of account.members) {
    const token = profileToken(m.host, m.id);
    const own = local.includes(m.key);
    if (own || marked.includes(m.key)) {
      stored = (await setSetting(ACCOUNT_COLOR + token, own && color !== null ? color.toLowerCase() : null)) && stored;
      stored = (await setSetting(ACCOUNT_MARK + token, own && emoji !== null ? emoji.trim() : null)) && stored;
    }
    stored = (await setSetting(ACCOUNT_HIDE + token, hidden ? true : null)) && stored;
  }
  if (!stored) errors.push(MARK_NOT_SAVED);
  return errors;
}

/** One member profile's mark this desktop still keeps for a computer that keeps marks. */
export interface PendingMark { key: string; token: string; color: string; emoji: string; move: boolean }

/**
 * This desktop's marks to move to the computers that keep them, once
 * (zerus-sa9qn.6, pure): every reachable member whose computer keeps marks
 * and that this desktop still has a color or emoji for. `move` is false when
 * nothing would change there: the computer already has those values, so they
 * win and the desktop's are only forgotten.
 */
export function pendingMarks(values: Readonly<Settings>, view: Pick<AccountsView, "accounts">): PendingMark[] {
  const pending = new Map<string, PendingMark>();
  for (const account of view.accounts) for (const m of account.members) {
    if (!m.marks_supported || !m.online || pending.has(m.key)) continue;
    const token = profileToken(m.host, m.id);
    const c = values[ACCOUNT_COLOR + token], e = values[ACCOUNT_MARK + token];
    if (c === undefined && e === undefined) continue;
    const color = validColor(c) ? c.toLowerCase() : "";
    const emoji = typeof e === "string" && validEmoji(e) ? e.trim() : "";
    pending.set(m.key, { key: m.key, token, color, emoji, move: (!!color && !m.mark_color) || (!!emoji && !m.mark_emoji) });
  }
  return [...pending.values()];
}
