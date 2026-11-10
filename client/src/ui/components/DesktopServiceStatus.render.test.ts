// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { DesktopService } from "../../types";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("../../store/catalog", () => ({ catalog: { computers: {} } }));
vi.mock("../../store/ui", () => ({ toast: vi.fn() }));
vi.mock("../../i18n", () => ({ tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])) }));
vi.mock("./TaskMessageInputsDialog", () => ({ TaskMessageInputsDialog: () => "Message review" }));
import { desktopService } from "../../store/desktopService";
import { DesktopServiceStatus } from "./DesktopServiceStatus";
const healthy = (patch: Partial<DesktopService> = {}): DesktopService => ({ connected: true, epoch: "fixture", generation: 1,
  background: Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((key) => [key, { ready: true, code: "ready" }])),
  attention_counts: null, automatic_approvals: false, details_available: true, code: "", ...patch });
let dispose: (() => void) | undefined;
beforeEach(() => { vi.clearAllMocks(); native.invoke.mockResolvedValue(undefined); desktopService.apply(healthy()); });
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
function mount(counts = false) {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(DesktopServiceStatus, { counts }), host); return host;
}

it("hides healthy online status in the footer and restores the original Retry only on a fault", () => {
  const host = mount(); expect(host.textContent).toBe(""); expect(host.querySelector("[role=status]")).toBeNull();
  desktopService.apply(healthy({ connected: false }));
  expect(host.textContent).toContain("Background service unavailable");
  [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Retry now")!.click();
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith("desktop_service_retry");
  desktopService.apply(healthy()); expect(host.textContent).toBe("");
  expect(native.invoke).toHaveBeenCalledTimes(1);
});

it("keeps healthy service details visible in Settings About", () => {
  const host = mount(true);
  expect(host.textContent).toBe("Background service");
  expect(host.querySelector(".service-summary")?.getAttribute("aria-label")).toContain("Shared host attention");
  expect(host.querySelector("button")).toBeNull(); expect(native.invoke).not.toHaveBeenCalled();
});

it("keeps paused and unconfirmed ownership actions rather than hiding them as online", () => {
  const ownership = { pending: false, running: false, code: "", request_id: null,
    status: { abi: "fixture", configuration: "fixture", journal: null, revision: 1, state: "revoked" as const, rearm_required: true, request_id: null, owner_epoch: null, identity_current: true } };
  desktopService.apply(healthy({ notification_ownership: ownership })); const host = mount();
  expect(host.textContent).toContain("Resume background work");
  desktopService.apply(healthy({ notification_ownership: { ...ownership, pending: true } }));
  expect(host.textContent).toContain("Check result"); expect(host.textContent).not.toContain("Retry now");
  expect(native.invoke).not.toHaveBeenCalled();
});
