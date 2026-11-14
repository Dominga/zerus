// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke, isTauri: () => false, Channel: class {} }));
import { actionDelayBegin, runAction, terminationCleanupOffer } from "./bridge";

const target = { computer: "", name: "codex/example/interrupted", run_id: "run", conversation_id: "conversation" };
const message = "zerus-action-busy-v1:" + JSON.stringify({ message: {
  source: "This session is busy with “%1” (%2 elapsed, since %3 UTC). Wait for it to finish.",
  args: [
    { kind: "text", value: { source: "Working folder review" } },
    { kind: "text", value: { source: "%1 s", args: [{ kind: "number", value: 2 }] } },
    { kind: "verbatim", value: "2026-11-13 12:00:00" },
  ],
} });
beforeEach(() => {
  ipc.invoke.mockReset();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { value: {}, configurable: true });
});
afterEach(() => { delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__; });

it("renders busy errors from both End admission and ordinary actions before they reach notices", async () => {
  const expected = "This session is busy with “Working folder review” (2 s elapsed, since 2026-11-13 12:00:00 UTC). Wait for it to finish.";
  for (const request of [
    () => actionDelayBegin("original", [{ target, state: "running", action: { kind: "terminate" } }]),
    () => runAction(target, { kind: "pause" }),
    () => terminationCleanupOffer(target),
  ]) {
    ipc.invoke.mockRejectedValueOnce(message);
    await expect(request()).rejects.toBe(expected);
  }
});
it("preserves unrelated native refusals", async () => {
  ipc.invoke.mockRejectedValueOnce("The session state changed. Open its actions again.");
  await expect(runAction(target, { kind: "pause" })).rejects.toBe("The session state changed. Open its actions again.");
});
