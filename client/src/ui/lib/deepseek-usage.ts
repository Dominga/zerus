// The DeepSeek API-key account's spend (from its balance readings) and the
// requests and tokens of this computer's DeepSeek sessions, as the account's
// usage panel shows them.
import { tr, uiLanguage } from "../../i18n";
import type { DayUsage, LocalUsage, ModelUsage, Spend, SpendAmount } from "../../types";
import { dayMonth, dayTimeOf } from "./dates";
import { balanceAmount } from "./usage";

const at = (seconds: number) => dayTimeOf(new Date(seconds * 1000));
const money = (amounts: SpendAmount[], field: "spent" | "granted") =>
  amounts.map((a) => balanceAmount({ kind: "", balance: a[field], currency: a.currency })).join("  ") || "—";
const period = (spend: Spend, name: string) => spend.periods.find((p) => p.period === name);
const positive = (amount: string) => Number(amount) > 0;

/** "Spent today" and "Spent this month" with their amounts ("$0.14"); a missing period shows a dash. */
export function spendRows(spend: Spend) {
  return ([["today", tr("Spent today")], ["month", tr("Spent this month")]] as const).map(([name, label]) => {
    const amounts = period(spend, name)?.amounts ?? [];
    return { label, value: money(amounts, "spent"), title: amounts.map((a) => `${a.spent} ${a.currency}`).join("\n") };
  });
}

/** What the spend covers and leaves out. */
export function spendNotes(spend: Spend) {
  const notes = [tr("Account-wide, from balance changes since %1. Top-ups excluded.", at(spend.since))];
  const today = period(spend, "today"), month = period(spend, "month");
  const granted = (month ?? today)?.amounts.filter((a) => positive(a.granted)) ?? [];
  if (granted.length) notes.push(tr("Includes %1 of granted balance, which can also be expired grants.", money(granted, "granted")));
  if (today?.top_up || month?.top_up) notes.push(tr("A check interval with a top-up is left out."));
  if (today?.gap_from) notes.push(tr("Today includes spend since %1, the previous balance check.", at(today.gap_from)));
  if (month?.gap_from) notes.push(tr("This month includes spend since %1, the previous balance check.", at(month.gap_from)));
  return notes;
}

function merged(models: ModelUsage[]): ModelUsage[] {
  const byModel = new Map<string, ModelUsage>();
  for (const m of models) {
    const sum = byModel.get(m.model) ?? { model: m.model, requests: 0, cache_hit: 0, cache_miss: 0, output: 0 };
    byModel.set(m.model, { model: m.model, requests: sum.requests + m.requests, cache_hit: sum.cache_hit + m.cache_hit, cache_miss: sum.cache_miss + m.cache_miss, output: sum.output + m.output });
  }
  return [...byModel.values()].sort((a, b) => b.requests - a.requests || a.model.localeCompare(b.model));
}

const dateOf = (day: string) => {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, (m || 1) - 1, d || 1);
};

/** Why the session totals may be incomplete: logs still to read, or logs that could not be read. */
export function localUsageNotes(usage: LocalUsage) {
  const notes: string[] = [];
  if (usage.reading) notes.push(tr("Still reading session logs; totals may grow."));
  if (usage.unreadable > 0) notes.push(tr("%n session log(s) could not be read and are not counted.", usage.unreadable));
  return notes;
}

/** Today's and this month's per-model totals, and each recorded day's totals, newest first. */
export function localUsageGroups(usage: LocalUsage) {
  const month = usage.today.slice(0, 7);
  const total = (day: DayUsage): ModelUsage => merged(day.models.map((m) => ({ ...m, model: "" })))[0] ?? { model: "", requests: 0, cache_hit: 0, cache_miss: 0, output: 0 };
  return {
    today: merged(usage.days.find((d) => d.day === usage.today)?.models ?? []),
    month: merged(usage.days.filter((d) => d.day.startsWith(month)).flatMap((d) => d.models)),
    days: usage.days.map((d) => ({ ...total(d), model: dayMonth(dateOf(d.day)) })),
  };
}

let compact: Intl.NumberFormat | undefined, compactLanguage = "";
/** "16.5M" ("16,5 млн"); the exact count belongs in its title. */
export function compactCount(n: number) {
  const language = uiLanguage() === "ru" ? "ru" : "en";
  if (!compact || compactLanguage !== language) {
    compact = new Intl.NumberFormat(language, { notation: "compact", maximumFractionDigits: 1 });
    compactLanguage = language;
  }
  return compact.format(n);
}
export const exactCount = (n: number) => n.toLocaleString(uiLanguage() === "ru" ? "ru" : "en");
