import { describe, expect, it } from "vitest";
import {
  localChange, coverageText, defaultRange, describeCounters, detailText, measure, overviewText, pageText, timeText, voiceText,
  SPEECH_COUNTERS, type ComputerReport, type Report, type Row,
} from "./analytics-text";

// The texts AnalyticsSettings.cpp writes, from Qt's own format strings
// filled in as QString::arg fills them; numbers in the en-US locale.

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const qtSource = fs.readFileSync(new URL("../../../../../tray/src/AnalyticsSettings.cpp", import.meta.url), "utf8");
/** A format Qt's source holds, with its \n escapes as newlines. */
const qt = (format: string) => {
  expect(qtSource).toContain(`"${format}"`);
  return format.replaceAll("\\n", "\n");
};
/** QString::arg, highest place first so %1 never eats %10. */
const fill = (format: string, args: (string | number)[]): string =>
  args.reduceRight<string>((text, arg, index) => text.replaceAll(`%${index + 1}`, String(arg)), format);
const EN = "en-US";

const computer: ComputerReport = {
  route: "", current: true, generated_at: 1_767_312_000.5, revision: 7, start: 1_767_225_600, end: null, partial: true,
  gaps: ["collection_disabled", "unknown"], unsupported: [], collecting: false, error: "Summary is stale or its clock is unconfirmed.",
  waits: { observed: 2, complete: false },
  providers: [{ provider: "codex", partial: true, unsupported: ["tokens"] }, { provider: "claude", partial: false, unsupported: [] },
    { provider: "kimi", partial: null, unsupported: null }],
};

describe("Private analytics texts", () => {
  it("measures as Qt does: whole counts grouped, durations in minutes", () => {
    expect(measure({ turn_completed: 12345 }, "turn_completed", EN)).toBe("12,345");
    expect(measure({ work_ms: 90_000 }, "work_ms", EN)).toBe(fill(qt("%1 min"), ["1.50"]));
    expect(measure({}, "work_ms", EN)).toBe("Unavailable");
    expect(timeText(1_767_312_000.999)).toBe("2026-01-02 00:00:00 UTC");
    expect(timeText(null)).toBe("Unknown");
  });

  it("writes a computer's coverage as Qt's paragraph and lines", () => {
    const paragraph = fill(qt("%1 (%2); generated %3; revision %4\\nCoverage %5 to %6; %7\\nGaps: %8\\nUnsupported: %9\\nCollection: %10"), [
      "This computer", "Current", "2026-01-02 00:00:00 UTC", 7, "2026-01-01 00:00:00 UTC", "Unknown", "partial",
      "collection disabled, unknown", "None reported", "disabled",
    ]);
    const waits = fill(qt("Current unresolved waits observed: %1 (%2)"), [2, "incomplete observation"]);
    const provider = qt("%1 coverage: %2; unsupported: %3");
    expect(coverageText(computer).split("\n")).toEqual([
      ...paragraph.split("\n"), "Summary is stale or its clock is unconfirmed.", waits,
      fill(provider, ["codex", "partial", "tokens"]), fill(provider, ["claude", "reported", "None reported"]),
      fill(provider, ["kimi", "unknown", "unknown"]),
    ]);
  });

  it("writes the overview with the pooled histogram", () => {
    const report: Report = {
      from: "2026-01-01", through: "2026-01-30", routes: 2, present: 1, totals: { turn_completed: 3, work_ms: 60000 },
      work: { counts: [1, 9, 0, 0, 0, 0], count: 10, sum: 9100 }, peer_limit: false, missing: [], computers: [], voice: [], buckets: 1,
    };
    const histogram = fill(qt("Recorded fleet work histogram: %1 samples; weighted mean %2."), ["10", fill("%1 ms", ["910.0"])]);
    const format = qt("%1 through %2 UTC (%3 selected route(s), %4 current unique computer(s)). Recorded current totals: %5 completed turns; %6 work.\\n%7\\nCoverage below applies to every total and recorded sample.");
    expect(overviewText(report, EN)).toBe(fill(format, ["2026-01-01", "2026-01-30", 2, 1, "3", "1.00 min", histogram]));
    expect(overviewText({ ...report, work: null, totals: {} }, EN)).toContain(
      "Unavailable completed turns; Unavailable work.\nRecorded fleet work histogram: Unavailable (missing, incompatible or overflowing samples).");
    expect(overviewText({ ...report, work: { counts: [0, 0, 0, 0, 0, 0], count: 0, sum: 0 } }, EN)).toContain("0 samples; weighted mean Unavailable (no samples).");
  });

  it("writes a voice host's totals with its typed errors", () => {
    const totals = { counters: { submitted: 4, completed: 4, failed: 0, cancelled: 0, uncertain: 0, audio_ms: 120000, queue_ms: 60001.5, processing_ms: 0 }, error_codes: { unknown: 2, http_timeout: 4 } };
    const counters = describeCounters(totals.counters, SPEECH_COUNTERS, EN);
    expect(counters.split("\n")).toEqual(["submitted: 4", "completed: 4", "failed: 0", "cancelled: 0", "uncertain: 0",
      "Accepted audio: 2.00 min", "Total queue time: 1.00 min", "Total processing time: 0.00 min"]);
    const format = qt("%1 (%2; speech collection %3)\\n%4\\nTyped errors: %5");
    const read = { current: false, enabled: true, totals, identified: true };
    expect(voiceText({ route: "mac", error: "", read }, EN)).toBe(fill(format, ["mac", "last known", "enabled", counters, '{"http_timeout":4,"unknown":2}']));
    expect(voiceText({ route: "mac", error: "", read: { ...read, identified: false } }, EN))
      .toMatch(/\n\nComputer identity unavailable; this route is not included in any fleet total\.$/);
    expect(voiceText({ route: "", error: "Computer unavailable. No current analytics read.", read: null }))
      .toBe("This computer: Computer unavailable. No current analytics read.");
  });

  it("details a day with every counter and the sampled histogram means", () => {
    const row: Row = {
      date: "2026-01-01", route: "", provider: "codex", producer: "main", current: true,
      counters: { turn_completed: 3, output_tokens: 1200 },
      histograms: { work_ms: { counts: [0, 0, 0, 1, 0, 0], count: 2, sum: 125 }, input_wait_ms: { counts: [0, 0, 0, 0, 0, 0], count: 0, sum: 0 } },
    };
    const text = detailText(row, EN);
    expect(text).toContain("turn completed: 3\n");
    expect(text).toContain("output tokens: 1,200\n");
    expect(text).toContain("input tokens: Unavailable\n");
    // QString::arg(double, 0, 'f', 1) is not localized.
    expect(text.endsWith(fill(qt("\\n%1 histogram mean: %2 ms (%3 samples)"), ["work_ms", "62.5", 2]))).toBe(true);
    expect(text).not.toContain("input_wait_ms histogram");
  });

  it("names the page of daily buckets and the default range", () => {
    const format = qt("%1–%2 of %3 daily buckets (up to 50 per page)");
    expect(pageText(50, 10, 60)).toBe(fill(format, [51, 60, 60]));
    expect(pageText(0, 0, 0)).toBe(fill(format, [0, 0, 0]));
    expect(defaultRange(Date.UTC(2026, 2, 1, 23, 30))).toEqual({ from: "2026-01-31", through: "2026-03-01" });
  });

  it("asks before a shorter retention and pins the epoch shown at the click", () => {
    const view = { can_change_local: true, local: { retention_days: 90, epoch: "20000000-0000-4000-8000-000000000001" } };
    expect(localChange(view, 45)).toEqual({ epoch: "20000000-0000-4000-8000-000000000001", ask: true });
    expect(localChange(view, 90)).toEqual({ epoch: "20000000-0000-4000-8000-000000000001", ask: false });
    expect(localChange({ ...view, can_change_local: false }, 45)).toBeUndefined();
    expect(localChange({ can_change_local: true, local: null }, 45)).toBeUndefined();
  });
});
