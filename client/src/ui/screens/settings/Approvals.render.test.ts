// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { DesktopService } from "../../../types";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
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

it("shows the background service's default categories read-only", () => {
  desktopService.apply({ connected: true, epoch: "fixture", generation: 1, background: {}, attention_counts: null,
    automatic_approvals: true, details_available: true, code: "" } as DesktopService);
  const host = mount();
  const rows = switches(host);
  expect(rows.every((b) => b.disabled)).toBe(true);
  expect(rows.map((b) => b.getAttribute("aria-checked"))).toEqual(APPROVAL_CATEGORIES.map((c) => String(c.on)));
  expect(host.textContent).toContain("The background service approves the default categories");
});
