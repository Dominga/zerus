import { describe, expect, it, vi } from "vitest";
import { createRoot, createSignal } from "solid-js";
import { createForks } from "./forks";
import { detailsFromInspection, snapshotOf } from "../ui/sessions/actions-ui";
import { forkAvailable, forkProblem, forkTag } from "../ui/sessions/fork-ui";
import type { Session } from "../ui/sessions/types";
import type { Outcome } from "../types";
const row = (fields: Partial<Session> = {}) => ({ key: "studio\nrun:r", name: "codex/example/review", computer: "studio", machine: "Studio", state: "running", runId: "r", conversationId: "c", created: 1, ...fields } as Session);
const details = () => detailsFromInspection({ run_id: "r", conversation_id: "c", fork_supported: true, fork_reason: "Ready" })!;
const done = { outcome: "done", receipt: null, cleanup: null } as Outcome;
function setup() {
  const source = row(), [rows, setRows] = createSignal([source]), [online, setOnline] = createSignal(true), [busy, setBusy] = createSignal(false);
  const inspect = vi.fn(async () => details()), perform = vi.fn(async (): Promise<Outcome> => done), warning = vi.fn(async (): Promise<string | null> => null);
  const place = vi.fn(), select = vi.fn(), notify = vi.fn(), confirm = vi.fn(async () => true), dispatched = vi.fn();
  const deps = { rows, current: (key: string) => rows().find((r) => r.key === key), online, busy, inspect, perform, warning,
    groupOf: () => "project", defaultGroup: () => "default", hasGroup: () => true, next: () => undefined, place, select, notify };
  const forks = createForks(deps), controller = new AbortController();
  const submit = (tag = "review-fork") => forks.submit(snapshotOf(source), details(), tag, controller.signal, confirm, dispatched);
  return { source, rows, setRows, setOnline, setBusy, inspect, perform, warning, place, select, notify, confirm, dispatched, forks, controller, submit, deps };
}

describe("Fork session", () => {
  it("projects only an affirmative capability and checks its run and conversation", () => {
    const s = row(), d = details();
    expect(d).toMatchObject({ fork_supported: true, fork_reason: "Ready" });
    expect(forkAvailable(s, d, true, false)).toBe(true);
    for (const [data, online, busy] of [[undefined, true, false], [{ ...d, fork_supported: false }, true, false], [d, false, false], [d, true, true], [{ ...d, run_id: "other" }, true, false], [{ ...d, conversation_id: "other" }, true, false]] as const)
      expect(forkAvailable(s, data, online, busy)).toBe(false);
  });
  it("suggests the first unused tag on this host and validates the complete name", () => {
    const s = row(), rows = [s, row({ key: "a", name: "codex/example/review-fork" }), row({ key: "b", name: "codex/example/review-fork-2" }), row({ computer: "other", name: "codex/example/review-fork-3" })];
    expect(forkTag(s, rows)).toBe("review-fork-3");
    expect(forkProblem(s, "review-fork", rows)).toBe("This session name is already in use.");
    expect(forkProblem(s, "bad/name", rows)).toBeTruthy();
    expect(forkProblem(s, "new fork", rows)).toBeUndefined();
  });
  it("submits once with the pinned source, then places and opens only the new conversation", async () => {
    await createRoot(async (dispose) => {
      const t = setup();
      expect(t.perform).not.toHaveBeenCalled();
      await t.submit();
      expect(t.perform).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ runId: "r", conversationId: "c", computer: "studio", name: "codex/example/review" }), { kind: "fork", tag: "review-fork", archive_id: null });
      expect(t.select).not.toHaveBeenCalled();
      t.setRows([t.source, row({ key: "new", name: "codex/example/review-fork", runId: "new", conversationId: "c" })]);
      await Promise.resolve(); expect(t.select).not.toHaveBeenCalled();
      const fork = row({ key: "new", name: "codex/example/review-fork", runId: "new", conversationId: "new-conversation" });
      t.setRows([t.source, fork]); await Promise.resolve();
      expect(t.place).toHaveBeenCalledExactlyOnceWith(fork, "project", undefined);
      expect(t.select).toHaveBeenCalledExactlyOnceWith(fork);
      expect(t.forks.busy()).toBe(false);
      dispose();
    });
  });
  it.each(["run", "conversation", "offline", "busy", "collision", "closed"])("refuses %s changes while its inspection is pending", async (change) => {
    await createRoot(async (dispose) => {
      const t = setup(); let resolve!: (d: ReturnType<typeof details>) => void;
      t.inspect.mockReturnValue(new Promise((done) => { resolve = done; }));
      const first = t.submit(); await t.submit();
      if (change === "run") t.setRows([row({ runId: "other" })]);
      if (change === "conversation") t.setRows([row({ conversationId: "other" })]);
      if (change === "offline") t.setOnline(false);
      if (change === "busy") t.setBusy(true);
      if (change === "collision") t.setRows([t.source, row({ key: "other", name: "codex/example/review-fork" })]);
      if (change === "closed") t.controller.abort();
      resolve(details()); await first;
      expect(t.inspect).toHaveBeenCalledTimes(1); expect(t.perform).not.toHaveBeenCalled();
      dispose();
    });
  });
  it("a cancelled Windows host warning keeps the dialog and sends nothing", async () => {
    await createRoot(async (dispose) => {
      const t = setup(); t.warning.mockResolvedValue("Launch anyway?"); t.confirm.mockResolvedValue(false);
      await t.submit(); expect(t.confirm).toHaveBeenCalledExactlyOnceWith("Launch anyway?");
      expect(t.dispatched).not.toHaveBeenCalled(); expect(t.perform).not.toHaveBeenCalled(); dispose();
    });
  });
  it("an uncertain result is reported once and never adopted or replayed", async () => {
    await createRoot(async (dispose) => {
      const t = setup(); t.perform.mockResolvedValue({ outcome: "uncertain", detail: "Check the session" });
      await t.submit(); t.setRows([t.source, row({ key: "new", name: "codex/example/review-fork", conversationId: "new" })]); await Promise.resolve();
      expect(t.perform).toHaveBeenCalledTimes(1); expect(t.select).not.toHaveBeenCalled();
      expect(t.notify).toHaveBeenLastCalledWith("error", "Check the session", true); dispose();
    });
  });
  it("reports a lost bridge result after the dialog closes without replaying", async () => {
    await createRoot(async (dispose) => {
      const t = setup(); t.dispatched.mockImplementation(() => t.controller.abort()); t.perform.mockRejectedValue(new Error("Lost answer"));
      await t.submit(); expect(t.perform).toHaveBeenCalledTimes(1); expect(t.notify).toHaveBeenLastCalledWith("error", "Lost answer", true); dispose();
    });
  });
  it.each([true, false])("keeps a surviving neighbor or falls back when its project disappears (%s)", async (kept) => {
    await createRoot(async (dispose) => {
      const t = setup(), neighbor = row({ key: "after", name: "codex/example/after" });
      t.setRows([t.source, neighbor]);
      const forks = createForks({ ...t.deps, next: () => neighbor, hasGroup: () => kept });
      await forks.submit(snapshotOf(t.source), details(), "review-fork", t.controller.signal, t.confirm, t.dispatched);
      const fork = row({ key: "new", name: "codex/example/review-fork", runId: "new", conversationId: "new" });
      t.setRows([t.source, neighbor, fork]); await Promise.resolve();
      expect(t.place).toHaveBeenCalledExactlyOnceWith(fork, kept ? "project" : "default", kept ? neighbor : undefined); dispose();
    });
  });
  it("rechecks identity after the Windows-session confirmation", async () => {
    await createRoot(async (dispose) => {
      const t = setup(); t.warning.mockResolvedValue("Launch anyway?");
      t.confirm.mockImplementation(async () => { t.setRows([row({ conversationId: "changed" })]); return true; });
      await t.submit(); expect(t.perform).not.toHaveBeenCalled(); dispose();
    });
  });
  it("preserves an archived source ID", async () => {
    await createRoot(async (dispose) => {
      const t = setup(); const source = row({ state: "archived", archiveId: "archive-source" }); t.setRows([source]);
      await t.forks.submit(snapshotOf(source), details(), "next", t.controller.signal, t.confirm, t.dispatched);
      expect(t.perform).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ archiveId: "archive-source" }), { kind: "fork", tag: "next", archive_id: "archive-source" }); dispose();
    });
  });
});
