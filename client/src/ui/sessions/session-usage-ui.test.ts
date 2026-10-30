import { describe, expect, it } from "vitest";
import { footerUsageSummary, sessionUsageView, usageStripVisible } from "./session-usage-ui";
import { toSession } from "../../store/catalog";
import { emptyUsage } from "../../types";

describe("SessionUsage::html projection", () => {
  it("keeps the footer visible for Qt's tracked current sessions only", () => {
    const s = toSession("", "run:r1", { name: "codex/sample-project/a", tracked: true, state: "running", run_id: "r1" });
    expect(usageStripVisible("sessions", s, undefined)).toBe(true);
    expect(usageStripVisible("sessions", { ...s, state: "paused" }, undefined)).toBe(true);
    expect(usageStripVisible("accounts", s, undefined)).toBe(false);
    expect(usageStripVisible("sessions", s, "child")).toBe(false);
    expect(usageStripVisible("sessions", { ...s, archiveId: "archive-1" }, undefined)).toBe(false);
    expect(usageStripVisible("sessions", { ...s, state: "archived" }, undefined)).toBe(false);
    expect(usageStripVisible("sessions", { ...s, tracked: false }, undefined)).toBe(false);
    expect(footerUsageSummary({ ...emptyUsage(), status: "unavailable" })).toBe("limits n/a");
    expect(footerUsageSummary({ ...emptyUsage("opencode"), status: "configured" })).toBe("Configured");
  });
  it("keeps Qt's rows, notes, context and partial explanation", () => {
    const view = sessionUsageView({
      status: "ok", requests: 3, cost_usd: 1.25, cost_stale: true, partial: true,
      totals: { total: 1000, input: 800, uncached_input: 100, cache_read: 500, cache_write: 200, output: 200, reasoning: 20, web_searches: 2, web_fetches: 1 },
      extras: { api_ms: 1250, tool_ms: 500, lines_added: 4, lines_removed: 2 },
      context: { used: 500, limit: 1000, estimated: true },
    });
    expect(view.rows).toEqual([
      { label: "Total tokens", value: "1,000" },
      { label: "Input (including cache)", value: "800" },
      { label: "Uncached input", value: "100" },
      { label: "Cache read", value: "500" },
      { label: "Cache write", value: "200" },
      { label: "Output", value: "200" },
      { label: "Reasoning (in output)", value: "20" },
      { label: "Web searches", value: "2" },
      { label: "Web fetches", value: "1" },
      { label: "Recorded requests", value: "3" },
      { label: "Native cost estimate", value: "USD 1.2500" },
      { label: "API time", value: "1.3 s" },
      { label: "Tool time", value: "0.5 s" },
      { label: "Lines added", value: "4" },
      { label: "Lines removed", value: "2" },
    ]);
    expect(view.costNote).toBe("Last saved native cost snapshot. Newer requests are not included yet.");
    expect(view.context).toBe("Approx. 500 / 1,000 (50.0%)");
    expect(view.footer).toBe("Reading session history. Totals are incomplete.");
  });

  it("uses the unavailable and not-reported texts", () => {
    expect(sessionUsageView({ status: "loading" }).status).toBe("No native usage reported for this conversation yet.");
    const view = sessionUsageView({ status: "ok", totals: {} });
    expect(view.rows).toEqual([{ label: "Total tokens", value: "Not reported" }, { label: "Cost", value: "Not reported" }]);
    expect(view.context).toBe("");
    expect(view.footer).toContain("This conversation only. Subagent usage is separate.");
  });
});
