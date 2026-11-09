import { describe, expect, it, vi } from "vitest";
import { readForceOffer } from "./force-offer";

const original = "original-cleanup", confirmation = "fresh-force-handle";
describe("separate Force confirmation", () => {
  it("only reads the fresh native list, retaining the exact opaque handle", async () => {
    const value = { confirmation, remaining_millis: 60_000, revision: 1, programs: [{ pid: 42, label: "editor" }] };
    const offer = vi.fn(async () => value), cancel = vi.fn();
    expect(await readForceOffer(original, { offer, cancel }, () => true)).toBe(value);
    expect(offer).toHaveBeenCalledExactlyOnceWith(original);
    expect(cancel).not.toHaveBeenCalled();
  });
  it("cancels a late offer when the dialog closed, without confirming it", async () => {
    const cancel = vi.fn();
    expect(await readForceOffer(original, { offer: async () => ({ confirmation, remaining_millis: 60_000, revision: 1, programs: [{ pid: 42, label: "editor" }] }), cancel }, () => false)).toBeUndefined();
    expect(cancel).toHaveBeenCalledExactlyOnceWith(original, confirmation);
  });
  it.each([
    { programs: [] }, { programs: [{ pid: 42, label: "editor" }, { pid: 42, label: "terminal" }] }, { programs: [{ pid: 42, label: "editor\u0000" }] },
  ])("refuses an empty, partial or changed native list before confirmation", async ({ programs }) => {
    const cancel = vi.fn();
    await expect(readForceOffer(original, { offer: async () => ({ confirmation, remaining_millis: 60_000, revision: 1, programs }), cancel }, () => true)).rejects.toThrow("program list changed");
    expect(cancel).toHaveBeenCalledExactlyOnceWith(original, confirmation);
  });
  it.each([undefined, null, -1, 300_001, NaN, Infinity, 0.5])("refuses malformed or unbounded remaining time %s", async (remaining) => {
    const cancel = vi.fn();
    const value = { confirmation, remaining_millis: remaining, revision: 1, programs: [{ pid: 42, label: "editor" }] };
    await expect(readForceOffer(original, { offer: async () => value as import("../../bridge").ProgramForceOffer, cancel }, () => true)).rejects.toThrow("program list changed");
    expect(cancel).toHaveBeenCalledExactlyOnceWith(original, confirmation);
  });
  it("preserves an already-expired native budget instead of renewing it", async () => {
    const value = { confirmation, remaining_millis: 0, revision: 1, programs: [{ pid: 42, label: "editor" }] };
    expect(await readForceOffer(original, { offer: async () => value, cancel: vi.fn() }, () => true)).toBe(value);
  });
  it("propagates an unavailable or unknown original without any fallback", async () => {
    const cancel = vi.fn();
    await expect(readForceOffer(original, { offer: async () => { throw Error("original unknown"); }, cancel }, () => true)).rejects.toThrow("original unknown");
    expect(cancel).not.toHaveBeenCalled();
  });
});
