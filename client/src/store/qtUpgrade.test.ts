import { describe, expect, it, vi } from "vitest";
import { createQtUpgradeNotice } from "./qtUpgrade";
import { createNoticeBus } from "./notices";

describe("Qt upgrade scope", () => {
  it("shows one plain notice, acknowledges only its metadata, and stays quiet after restart", async () => {
    let pending = true;
    const backend = { pending: vi.fn(async () => pending), acknowledge: vi.fn(async () => { pending = false; }) };
    const bus = createNoticeBus();
    const show = createQtUpgradeNotice(backend, bus);
    await show(); await show();
    expect(backend.pending).toHaveBeenCalledTimes(1);
    expect(backend.acknowledge).not.toHaveBeenCalled();
    expect(bus.notices()).toHaveLength(1);
    const notice = bus.notices()[0];
    expect(notice.title).toBe("Upgrading from Qt");
    for (const word of ["Projects", "accounts", "drafts", "unsent organization edits", "notification history", "pending operations", "task deliveries", "repository clone", "caches", "Original files", "Existing Next data"]) expect(notice.text).toContain(word);
    expect(notice.sticky).toBe(true);
    await notice.actions![0].run();
    expect(backend.acknowledge).toHaveBeenCalledTimes(1);
    expect(bus.notices()).toEqual([]);
    await createQtUpgradeNotice(backend, bus)();
    expect(bus.notices()).toEqual([]);
  });
  it("keeps the notice available when acknowledgment cannot be persisted", async () => {
    const backend = { pending: vi.fn(async () => true), acknowledge: vi.fn().mockRejectedValueOnce(new Error("private failure")).mockResolvedValue(undefined) };
    const bus = createNoticeBus();
    await createQtUpgradeNotice(backend, bus)();
    const notice = bus.notices()[0];
    await notice.actions![0].run();
    expect(bus.notices()).toHaveLength(1);
    expect(notice.note!()).toBe("The upgrade notice could not be saved. Try again.");
    await notice.actions![0].run();
    expect(bus.notices()).toEqual([]);
  });
  it("does not show a notice to a profile that has already acknowledged", async () => {
    const backend = { pending: vi.fn(async () => false), acknowledge: vi.fn() };
    const bus = createNoticeBus();
    await createQtUpgradeNotice(backend, bus)();
    expect(bus.notices()).toEqual([]);
    expect(backend.acknowledge).not.toHaveBeenCalled();
  });
});
