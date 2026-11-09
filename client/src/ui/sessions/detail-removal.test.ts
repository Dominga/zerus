import { expect, it, vi } from "vitest";
import { watchDetailRemoval, type DetailFrame } from "./detail-removal";

const sessionId = "00000000-0000-4000-8000-000000000001";
function fixture() {
  const listed = new Set(["original"]);
  const frames = new Map<number, () => void>();
  let id = 0;
  const report = vi.fn();
  const watcher = watchDetailRemoval({
    listed: (key) => listed.has(key),
    schedule: (check) => { const key = ++id; frames.set(key, check); return () => { frames.delete(key); }; },
    report,
  });
  const element = { isConnected: true };
  const original: DetailFrame = { key: "original", catalogBound: true, sessionId,
    route: { tab: "processes", inspector: "plan", child: false }, element };
  const nextFrame = () => { const pending = [...frames.values()]; frames.clear(); pending.forEach((check) => check()); };
  watcher.update(original);
  return { watcher, listed, frames, report, original, element, nextFrame };
}

it("checks on the next frame and logs one fixed line for a still-mounted removed detail", () => {
  const f = fixture(); f.listed.clear(); f.watcher.update(undefined);
  expect(f.report).not.toHaveBeenCalled(); expect(f.frames.size).toBe(1);
  f.nextFrame();
  expect(f.report).toHaveBeenCalledExactlyOnceWith(`Zerus detail invariant: route=sessions/processes/plan/parent session_id=${sessionId} reason=removed_catalog_row_still_mounted`);
  f.watcher.update(f.original); f.watcher.update(undefined); f.nextFrame();
  expect(f.report).toHaveBeenCalledTimes(1); expect(f.frames.size).toBe(0);
});

it("does not warn when ordinary disposal disconnects the frame before the check", () => {
  const f = fixture(); f.listed.clear(); f.watcher.update(undefined);
  f.element.isConnected = false; f.nextFrame();
  expect(f.report).not.toHaveBeenCalled();
});

it("does not warn on metadata updates, another selection, or a restored catalog row", () => {
  const f = fixture(); f.watcher.update({ ...f.original });
  expect(f.frames.size).toBe(0);
  f.listed.clear(); f.listed.add("next");
  f.watcher.update({ ...f.original, key: "next" }); f.nextFrame();
  expect(f.report).not.toHaveBeenCalled();
  f.listed.delete("next"); f.watcher.update(undefined); f.listed.add("next"); f.nextFrame();
  expect(f.report).not.toHaveBeenCalled();
});

it("retains at most one pending check and cancels it on view disposal", () => {
  const f = fixture(); f.listed.clear();
  for (let i = 0; i < 100; i++) f.watcher.update(f.original);
  expect(f.frames.size).toBe(1);
  f.watcher.dispose(); f.nextFrame(); expect(f.frames.size).toBe(0);
  expect(f.report).not.toHaveBeenCalled();
});

it("does not treat a retained archive frame as a removed live session", () => {
  const f = fixture(); f.listed.clear();
  f.watcher.update({ ...f.original, key: "archive:original", catalogBound: false });
  f.nextFrame(); expect(f.report).not.toHaveBeenCalled();
  f.watcher.update(undefined); expect(f.frames.size).toBe(0);
});

it("never logs a label, raw key, malformed identifier or arbitrary route text", () => {
  const f = fixture();
  f.watcher.update({ ...f.original, key: "private/session/name", sessionId: "private\ncontent",
    route: { tab: "private input" as never, inspector: "private task" as never, child: true } });
  f.watcher.update(undefined); f.nextFrame();
  expect(f.report).toHaveBeenCalledExactlyOnceWith("Zerus detail invariant: route=sessions/unknown/unknown/child session_id=unavailable reason=removed_catalog_row_still_mounted");
  expect(f.report.mock.calls[0][0].length).toBeLessThan(256);
});
