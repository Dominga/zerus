import { afterEach, describe, expect, it, vi } from "vitest";
import * as bridge from "../bridge";
import { receive, subagentActivity } from "./session";
import { setPage, setSelectedKey, setSubagentOpen } from "./ui";
import { rowOut } from "./testing";
import { emptyBatch, type RowOut } from "../types";
import contract from "../../core/tests/fixtures/subagent-activity.json";

// zerus-l7n54: the real wiring in store/session.ts, as the app really starts.
// The main window is created hidden until the page has applied its skin
// (window_theme.rs), so the document is still hidden when this store is made
// and turns visible later with the Sessions page already current.
const { page } = vi.hoisted(() => {
  vi.stubGlobal("location", new URL("http://localhost/"));
  const page = Object.assign(new EventTarget(), { visibilityState: "hidden" as "hidden" | "visible", hasFocus: () => true });
  vi.stubGlobal("document", page);
  return { page };
});
vi.mock("../bridge", async (real) => ({ ...await real<typeof bridge>(), inspectSubagentActivity: vi.fn() }));

const read = vi.mocked(bridge.inspectSubagentActivity);
const answer = contract.answer as unknown as NonNullable<Awaited<ReturnType<typeof bridge.inspectSubagentActivity>>>;
const { target } = contract;
const turn = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
const setVisibility = (state: "hidden" | "visible") => { page.visibilityState = state; page.dispatchEvent(new Event("visibilitychange")); };
const row = (key: string, extra: Partial<RowOut["row"]>): RowOut => ({ ...rowOut("", 1, { name: target.name, ...extra }), key });
const open = async (out: RowOut) => {
  receive({ ...emptyBatch(), upserts: [out] });
  setSelectedKey(`\n${out.key}`);
  setSubagentOpen(target.agentId);
  await turn();
};

describe("the child Activity's wiring", () => {
  afterEach(() => { setSubagentOpen(undefined); setSelectedKey(undefined); setPage("sessions"); read.mockReset(); });

  it("reads once the window shows, when the store was made while it was hidden", async () => {
    read.mockResolvedValue(answer);
    await open(row(target.id.slice(1), { run_id: target.runId, conversation_id: target.conversationId }));
    expect(read).not.toHaveBeenCalled();
    setVisibility("visible");
    await turn();
    expect(read).toHaveBeenCalledExactlyOnceWith(
      { computer: "", name: target.name, run_id: target.runId, conversation_id: target.conversationId }, null, target.agentId);
    expect(subagentActivity.loaded()).toBe(true);
    expect(subagentActivity.error()).toBeUndefined();
    expect(subagentActivity.summary()).toEqual(answer);
  });

  it("stops while the window is hidden and reads again when it returns", async () => {
    setVisibility("visible");
    read.mockResolvedValue(answer);
    await open(row(target.id.slice(1), { run_id: target.runId, conversation_id: target.conversationId }));
    expect(read).toHaveBeenCalledTimes(1);
    setVisibility("hidden");
    await turn();
    expect(subagentActivity.loaded()).toBe(false);
    setVisibility("visible");
    await turn();
    expect(read).toHaveBeenCalledTimes(2);
    expect(subagentActivity.loaded()).toBe(true);
  });

  it("reports a parent with no identity to read under instead of reading forever", async () => {
    setVisibility("visible");
    // The bridge's own answer: the core refuses an empty run or conversation.
    read.mockResolvedValue(null);
    await open(row("run:no-identity", { run_id: target.runId, conversation_id: null }));
    expect(subagentActivity.loaded()).toBe(true);
    expect(subagentActivity.error()).toContain("unavailable");
  });
});
