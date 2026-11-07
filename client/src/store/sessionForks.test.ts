import { afterEach, describe, expect, it, vi } from "vitest";
import { createComputed, createRoot } from "solid-js";
import * as bridge from "../bridge";
import { applyBatch, catalog } from "./catalog";
import { composer } from "./composer";
import { sessionForks } from "./sessionForks";
import "./session";
import * as organization from "./organization";
import { terminal } from "./terminal";
import { detailTab, selectedKey, setDetailTab, setSelectedKey } from "./ui";
import { detailsFromInspection, snapshotOf } from "../ui/sessions/actions-ui";
import { emptyBatch, type ViewRow } from "../types";

vi.hoisted(() => { vi.stubGlobal("location", new URL("http://localhost/")); });
const computer = "fork-fixture";
const sourceKey = `${computer}\nrun:source`, forkKey = `${computer}\nrun:fork`;
const row = (fork: boolean): ViewRow => ({ name: `codex/example/source${fork ? "-fork" : ""}`, agent: "codex", state: "running", tracked: true,
  status: "ready", run_id: fork ? "fork" : "source", conversation_id: fork ? "fork-conversation" : "source-conversation", created: fork ? 2 : 1 });
const ticks = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };
afterEach(async () => {
  setSelectedKey(undefined); setDetailTab("activity");
  applyBatch({ ...emptyBatch(), removes: ["run:source", "run:fork"].map((key) => ({ computer, key })) });
  await ticks(); vi.restoreAllMocks();
});

describe("fork destination view", () => {
  it.each(["terminal", "taskview"] as const)("opens the verified fork in Activity after %s", async (previous) => {
    const open = vi.spyOn(terminal, "opened").mockImplementation(() => {});
    vi.spyOn(composer, "open").mockResolvedValue();
    vi.spyOn(bridge.activityPort, "follow").mockResolvedValue();
    vi.spyOn(bridge.activityPort, "unfollow").mockResolvedValue();
    vi.spyOn(organization, "moveToProject").mockResolvedValue({ outcome: "done", conflicts: 0 });
    vi.spyOn(bridge, "runAction").mockResolvedValue({ outcome: "done", receipt: null, cleanup: null });
    const details = detailsFromInspection({ run_id: "source", conversation_id: "source-conversation", fork_supported: true })!;
    vi.spyOn(bridge, "inspectSession").mockImplementation(async (target) => target.name.endsWith("-fork")
      ? detailsFromInspection({ run_id: "fork", conversation_id: "fork-conversation", fork_parent_id: "source-conversation" })!
      : details);
    applyBatch({ ...emptyBatch(), computers: [{ computer, status: "live", attempt: 0 }], upserts: [{ computer, key: "run:source", row: row(false) }] });
    setSelectedKey(sourceKey); setDetailTab(previous);
    const seen: string[] = [];
    const dispose = createRoot((stop) => { createComputed(() => { if (selectedKey() === forkKey) seen.push(detailTab()); }); return stop; });
    try {
      await sessionForks.submit(snapshotOf(catalog.rows[sourceKey]), details, "source-fork", new AbortController().signal, async () => true, () => {});
      applyBatch({ ...emptyBatch(), upserts: [{ computer, key: "run:fork", row: row(true) }] });
      await ticks();
      expect(selectedKey()).toBe(forkKey);
      expect(detailTab()).toBe("activity");
      expect(seen.length).toBeGreaterThan(0);
      expect(seen.every((tab) => tab === "activity")).toBe(true);
      expect(open).not.toHaveBeenCalled();
      expect(sessionForks.busy()).toBe(false);
    } finally { dispose(); }
  });
});
