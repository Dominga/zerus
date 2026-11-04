import { describe, expect, it, vi } from "vitest";
import { readCleanupOffer, unavailableServiceRemoval, type CleanupOfferState } from "./termination-offer";

const target = { computer: "example", name: "codex/example/session", run_id: "run", conversation_id: "conversation" };
describe("Terminate dialog removal availability", () => {
  it.each([
    ["Linux", "termination_native_relay_unavailable"],
    ["macOS", "termination_native_image_unavailable"],
  ])("visibly explains %s Service refusal without an automatic fallback", async (_platform, error) => {
    let state: CleanupOfferState = {};
    const calls: string[] = [];
    const offer = vi.fn(async () => { calls.push("offer"); throw error; });
    const cancel = vi.fn(async () => { calls.push("cancel"); });
    await readCleanupOffer(target, { offer, cancel }, () => true, (next) => { state = next; });
    expect(state).toEqual({ error });
    expect(state.offer).toBeUndefined();
    expect(unavailableServiceRemoval(state.error!)).toBe(true);
    expect(calls).toEqual(["offer"]); // No confirm/ordinary probe/fallback port exists.
    expect(cancel).not.toHaveBeenCalled();
  });
  it("keeps other offer errors visible and offers only the separate review", async () => {
    let state: CleanupOfferState = {};
    await readCleanupOffer(target, { offer: async () => { throw Error("The captured session changed."); }, cancel: async () => {} }, () => true, (next) => { state = next; });
    expect(state).toEqual({ error: "The captured session changed." });
    expect(unavailableServiceRemoval(state.error!)).toBe(false);
  });
  it("preserves real offers and cancels an offer arriving after close", async () => {
    let state: CleanupOfferState = {};
    const offered = { id: "original-offer", checked: true };
    const ports = { offer: async () => offered, cancel: vi.fn(async () => {}) };
    await readCleanupOffer(target, ports, () => true, (next) => { state = next; });
    expect(state).toEqual({ offer: offered });
    const publish = vi.fn();
    await readCleanupOffer(target, ports, () => false, publish);
    expect(ports.cancel).toHaveBeenCalledExactlyOnceWith("original-offer");
    expect(publish).not.toHaveBeenCalled();
  });
});
