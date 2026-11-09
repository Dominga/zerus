// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import type { ComputerState } from "../../store/catalog";
import type { DesktopService } from "../../types";
const state = vi.hoisted(() => ({ read: null as unknown as () => Record<string, ComputerState>, write: null as unknown as (value: Record<string, ComputerState>) => void }));
const calls = vi.hoisted(() => ({ page: vi.fn(), retry: vi.fn(), toast: vi.fn() }));
vi.mock("../../store/catalog", () => ({ catalog: { get computers() { return state.read(); } } }));
vi.mock("../../bridge", () => ({ fleetRefresh: calls.retry }));
vi.mock("../../store/ui", () => ({ setPage: calls.page, toast: calls.toast }));
vi.mock("../../i18n", () => ({ tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])) }));
import { desktopService } from "../../store/desktopService";
import { MachinesStatus, PeerConfigurationNotice } from "./MachinesStatus";
const healthy = (): DesktopService => ({ connected: true, epoch: "fixture", generation: 1, code: "", details_available: true,
  automatic_approvals: false, attention_counts: null, background: Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((name) => [name, { ready: true, code: "ready" }])) });
let dispose: (() => void) | undefined;
beforeEach(() => {
  vi.clearAllMocks(); calls.retry.mockResolvedValue(undefined);
  const [read, write] = createSignal<Record<string, ComputerState>>({}); state.read = read; state.write = write;
  desktopService.apply(healthy());
});
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
function mount() {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(MachinesStatus, { mode: () => "tauri" }), host);
  return host;
}
const machines = (values: [string, string, number?][]) => state.write(Object.fromEntries(values.map(([id, status, attempt]) => [id, { id, status, attempt: attempt ?? 0 }])));

it("renders only a machine icon and compact counter, with full accessible details and navigation", () => {
  machines([["", "live"], ["peer.example", "polled"]]);
  const host = mount(), button = host.querySelector<HTMLButtonElement>(".machines-status")!;
  expect(button.textContent).toBe("2/2"); expect(button.querySelector("svg")).not.toBeNull();
  expect(button.getAttribute("aria-label")).toBe("Open Machines\n2 of 2 machines connected");
  expect(button.title).toBe(button.getAttribute("aria-label"));
  expect(button.classList.contains("warn")).toBe(false); expect(button.type).toBe("button");
  button.click(); expect(calls.page).toHaveBeenCalledExactlyOnceWith("machines");
  expect(calls.retry).not.toHaveBeenCalled();
});

it("warns with every offline machine name in its accessible tooltip, without expanding the bar", () => {
  machines([["", "live"], ["build.example", "reconnecting", 2], ["other.example", "ended"]]);
  const host = mount(), button = host.querySelector<HTMLButtonElement>(".machines-status")!;
  expect(button.textContent).toBe("1/3"); expect(button.classList.contains("warn")).toBe(true);
  expect(button.getAttribute("aria-label")).toContain("build.example, other.example offline");
  expect(button.title).toContain("build.example: reconnecting (attempt 2)");
  expect(button.title).toContain("other.example: stream ended");
  expect(host.textContent).not.toContain("build.example");
  button.click(); expect(calls.page).toHaveBeenCalledExactlyOnceWith("machines");
  expect(calls.retry).not.toHaveBeenCalled();
});

it("keeps the existing explicit Retry, suppresses its duplicate service action, and follows reconnection", () => {
  machines([["", "ended"]]); const host = mount();
  const retry = [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === "Retry now")!;
  retry.click(); expect(calls.retry).toHaveBeenCalledTimes(1); expect(calls.page).not.toHaveBeenCalled();
  desktopService.apply({ ...healthy(), connected: false });
  expect([...host.querySelectorAll("button")].some((b) => b.textContent === "Retry now")).toBe(false);
  machines([["", "live"]]);
  expect(host.querySelector(".machines-status")?.classList.contains("warn")).toBe(false);
  expect(host.querySelector(".machines-status")?.textContent).toBe("1/1");
});

it("uses the same local machine name while checking and never offers a probing Retry", () => {
  machines([["", "probing"]]); const host = mount();
  expect(host.querySelector<HTMLButtonElement>(".machines-status")?.title).toContain("this computer: checking");
  expect(host.querySelector(".machines-status")?.classList.contains("warn")).toBe(false);
  expect(host.querySelector<HTMLButtonElement>(".machines-status")?.title).not.toContain("offline");
  expect(host.textContent).toBe("0/1"); expect(calls.retry).not.toHaveBeenCalled();
  expect(host.querySelectorAll("button")).toHaveLength(1);
});

it("counts a configured peer with no catalog row as offline, without blocking healthy computers", () => {
  machines([["", "live"], ["healthy.example", "live"]]);
  desktopService.apply({ ...healthy(), fleet_computers: [
    { id: "", status: "live", attempt: 0 }, { id: "healthy.example", status: "live", attempt: 0 },
    { id: "broken.example", status: "reconnecting", attempt: 4 },
  ] });
  const host = mount(), button = host.querySelector<HTMLButtonElement>(".machines-status")!;
  expect(button.textContent).toBe("2/3");
  expect(button.title).toContain("broken.example offline");
  expect(button.title).toContain("broken.example: reconnecting (attempt 4)");
  expect(button.classList.contains("warn")).toBe(true);
  expect(calls.retry).not.toHaveBeenCalled();
  button.click(); expect(calls.page).toHaveBeenCalledExactlyOnceWith("machines");
});

it("keeps configured initial probing calm and follows the current checked roster", () => {
  machines([["", "live"]]);
  desktopService.apply({ ...healthy(), fleet_computers: [
    { id: "", status: "live", attempt: 0 }, { id: "warming.example", status: "probing", attempt: 0 },
  ] });
  const host = mount(), button = host.querySelector<HTMLButtonElement>(".machines-status")!;
  expect(button.textContent).toBe("1/2");
  expect(button.title).toContain("warming.example: checking");
  expect(button.title).not.toContain("offline"); expect(button.classList.contains("warn")).toBe(false);
  desktopService.apply({ ...healthy(), fleet_computers: [{ id: "", status: "live", attempt: 0 }] });
  expect(button.textContent).toBe("1/1"); expect(button.title).not.toContain("warming.example");
});

it("explains that HGS_PEERS changes need a service restart, without restarting it", () => {
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(PeerConfigurationNotice, {}), host);
  expect(host.textContent).toBe("Restart the background service after changing HGS_PEERS. Existing sessions keep running.");
  expect(host.querySelector("button")).toBeNull(); expect(calls.retry).not.toHaveBeenCalled();
});
