// SessionUsage::html, as labelled rows rather than HTML. Values stay separate
// through the table layout; no field separator is added.
export type UsageObject = Record<string, unknown>;
const object = (value: unknown): UsageObject => value && typeof value === "object" && !Array.isArray(value) ? value as UsageObject : {};
const valid = (value: unknown) => typeof value === "number" && Number.isFinite(value) && value >= 0;
const integer = (value: unknown) => valid(value) ? Math.trunc(value as number).toLocaleString() : "Not reported";
const fixed = (value: number, digits: number) => value.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits });

export interface SessionUsageView {
  status: string;
  rows: { label: string; value: string }[];
  costNote: string;
  context: string;
  footer: string;
}

export function sessionUsageView(raw: unknown): SessionUsageView {
  const usage = object(raw);
  if (usage.status !== "ok") return { status: "No native usage reported for this conversation yet.", rows: [], costNote: "", context: "", footer: "" };
  const totals = object(usage.totals);
  const rows: { label: string; value: string }[] = [{ label: "Total tokens", value: integer(totals.total) }];
  const fields: [string, string][] = [
    ["input", "Input (including cache)"], ["uncached_input", "Uncached input"], ["cache_read", "Cache read"], ["cache_write", "Cache write"],
    ["output", "Output"], ["reasoning", "Reasoning (in output)"], ["web_searches", "Web searches"], ["web_fetches", "Web fetches"],
  ];
  for (const [key, label] of fields) if (valid(totals[key])) rows.push({ label, value: integer(totals[key]) });
  if (valid(usage.requests)) rows.push({ label: "Recorded requests", value: integer(usage.requests) });
  const hasCost = valid(usage.cost_usd);
  rows.push({ label: hasCost ? "Native cost estimate" : "Cost", value: hasCost ? `USD ${fixed(usage.cost_usd as number, 4)}` : "Not reported" });
  const extras = object(usage.extras);
  for (const [key, label] of [["api_ms", "API time"], ["tool_ms", "Tool time"]] as const) {
    if (valid(extras[key])) rows.push({ label, value: `${fixed((extras[key] as number) / 1000, 1)} s` });
  }
  for (const [key, label] of [["lines_added", "Lines added"], ["lines_removed", "Lines removed"]] as const) {
    if (valid(extras[key])) rows.push({ label, value: integer(extras[key]) });
  }
  const costNote = !hasCost ? "" : usage.cost_incomplete === true ? "The provider reports incomplete cost data."
    : usage.cost_stale === true ? "Last saved native cost snapshot. Newer requests are not included yet."
    : "Provider estimate from its latest saved snapshot; may differ from the bill.";
  const context = object(usage.context);
  let contextText = "";
  if (valid(context.used)) {
    contextText = integer(context.used);
    if (valid(context.limit) && (context.limit as number) > 0) contextText += ` / ${integer(context.limit)} (${fixed(100 * (context.used as number) / (context.limit as number), 1)}%)`;
    if (context.estimated === true) contextText = `Approx. ${contextText}`;
  }
  return {
    status: "", rows, costNote, context: contextText,
    footer: usage.partial === true ? "Reading session history. Totals are incomplete."
      : "This conversation only. Subagent usage is separate. Cache is included in input; reasoning is included in output.",
  };
}
