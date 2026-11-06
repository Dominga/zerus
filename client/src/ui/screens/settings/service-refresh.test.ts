import { describe, expect, it, vi } from "vitest";
import { createServiceRefresh } from "./service-refresh";
import type { ServiceRefreshView } from "../../../types";
const offer: ServiceRefreshView = { code: "", running: false, settled: false, offer: { configuration: "a".repeat(64), epoch: "original", selection: "native-nonce" } };
const pending: ServiceRefreshView = { code: "refresh_unconfirmed", running: false, settled: false, offer: null, request_id: "original-id" };
describe("Settings service refresh callsite", () => {
  it("observes availability without submitting consent, and a click uses only the native display nonce", async () => {
    let started = false;
    const send = vi.fn(async (command: string) => { if (command === "desktop_service_refresh") started = true; return started ? pending : offer; });
    const action = createServiceRefresh(() => true, send, vi.fn());
    await action.check(); await action.check(); expect(send.mock.calls.map(c => c[0])).toEqual(["desktop_service_refresh_status", "desktop_service_refresh_status"]);
    await action.submit(); expect(send).toHaveBeenLastCalledWith("desktop_service_refresh", { display: offer.offer });
    await action.submit(); await action.check(); expect(send.mock.calls.filter(c => c[0] === "desktop_service_refresh")).toHaveLength(1);
    expect(action.view()?.request_id).toBe("original-id");
  });
  it("lost replies close the offer and every later click reads the original without replay", async () => {
    let effect = false;
    const send = vi.fn(async (command: string) => {
      if (command === "desktop_service_refresh") { effect = true; throw Error("lost IPC"); }
      return effect ? pending : offer;
    });
    const action = createServiceRefresh(() => true, send, vi.fn());
    await action.check(); await action.submit(); expect(action.view()?.offer).toBeNull();
    await action.submit(); await action.check(); await action.submit(); await action.check();
    expect(send.mock.calls.filter(c => c[0] === "desktop_service_refresh")).toHaveLength(1);
    expect(action.view()?.request_id).toBe("original-id");
  });
  it("coalesces duplicate clicks and never submits an operation when the manual binding is absent", async () => {
    let enabled = true, finish!: (value: ServiceRefreshView) => void;
    const send = vi.fn((command: string) => command === "desktop_service_refresh_status" ? Promise.resolve(offer) : new Promise<ServiceRefreshView>(resolve => { finish = resolve; }));
    const action = createServiceRefresh(() => enabled, send, vi.fn());
    await action.check(); const first = action.submit(); await action.submit(); expect(send).toHaveBeenCalledTimes(2);
    finish({ ...pending, code: "", settled: true }); await first;
    await action.submit(); expect(send).toHaveBeenCalledTimes(2);
    enabled = false; await action.check(); await action.submit(); expect(send).toHaveBeenCalledTimes(2);
  });
});
