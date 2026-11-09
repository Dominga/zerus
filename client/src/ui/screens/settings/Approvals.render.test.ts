// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { DesktopService } from "../../../types";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("../../../bridge", () => ({ inTauri: () => false }));
import { desktopService } from "../../../store/desktopService";
import { flag } from "../../../store/settings";
import { ApprovalsSection } from "./Approvals";
import { APPROVAL_CATEGORIES } from "./approval-categories";

let dispose: (() => void) | undefined;
beforeEach(() => { try { localStorage.clear(); } catch { /* none */ } });
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
function mount() {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(ApprovalsSection, {}), host); return host;
}
const switches = (host: HTMLElement) => APPROVAL_CATEGORIES.map((c) =>
  [...host.querySelectorAll<HTMLButtonElement>("button[role=switch]")].find((b) => b.getAttribute("aria-label") === c.label)!);

it("lists every category as a keyboard-reachable switch with the core's defaults and saves a change per computer", async () => {
  const host = mount();
  const rows = switches(host);
  expect(rows.every(Boolean)).toBe(true);
  expect(rows.map((b) => b.getAttribute("aria-checked"))).toEqual(APPROVAL_CATEGORIES.map((c) => String(c.on)));
  expect(rows.every((b) => !b.disabled && b.tabIndex >= 0)).toBe(true);
  const paid = rows[APPROVAL_CATEGORIES.findIndex((c) => c.id === "paid")];
  paid.click();
  await vi.waitFor(() => expect(flag("automaticApprovals/category/paid")).toBe(true));
  expect(paid.getAttribute("aria-checked")).toBe("true");
});

const serviceState = (patch: Partial<DesktopService> = {}) => ({ connected: true, epoch: "fixture", generation: 1, background: {},
  attention_counts: null, automatic_approvals: true, details_available: true, code: "", ...patch } as DesktopService);

it("waits for the background service to report its categories before they can change", () => {
  desktopService.apply(serviceState());
  const host = mount();
  const rows = switches(host);
  expect(rows.every((b) => b.disabled)).toBe(true);
  expect(host.textContent).toContain("The background service has not reported its approval categories");
});

it("shows the service's committed categories and sends a complete set with the committed switch", async () => {
  native.invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => command === "desktop_service_automatic_approvals"
    ? { request_id: args.request_id, op: "automatic_approvals", accepted: true, durable: true, enabled: args.enabled, categories: args.categories }
    : undefined);
  desktopService.apply(serviceState({ automatic_approval_categories: ["read", "shell", "other"] }));
  const host = mount();
  const rows = switches(host);
  expect(rows.map((b) => b.getAttribute("aria-checked"))).toEqual(APPROVAL_CATEGORIES.map((c) => String(["read", "shell", "other"].includes(c.id))));
  expect(rows.every((b) => !b.disabled)).toBe(true);
  const local = flag("automaticApprovals/category/paid");
  rows[APPROVAL_CATEGORIES.findIndex((c) => c.id === "paid")].click();
  await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("desktop_service_automatic_approvals",
    expect.objectContaining({ enabled: true, categories: ["read", "shell", "other", "paid"] })));
  // The service owns the policy: no per-computer setting changes.
  expect(flag("automaticApprovals/category/paid")).toBe(local);
});
