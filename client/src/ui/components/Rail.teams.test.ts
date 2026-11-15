// @vitest-environment happy-dom
// The Teams entry points: no rail button and no command dialog while Teams are
// off or hgs cannot run; the button appears once the core says Teams exist.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
const f = vi.hoisted(() => ({ setAvailable: (_: boolean | undefined) => {}, page: vi.fn() }));
vi.mock("../../store/ui", () => ({ page: () => "sessions", setPage: f.page, counts: () => ({ attention: 0 }), setMenu: vi.fn(), logoMotion: () => "none" }));
vi.mock("../../skin/runtime", () => ({ skin: () => ({ layout: { rail: "icons" } }) }));
vi.mock("../../store/windowLayer", () => ({ windowLayer: () => ({ on: false, supported: true, hint: "" }), setWindowLayer: vi.fn() }));
vi.mock("../sessions/mark-entries", () => ({ markAllEntry: () => ({ label: "Mark all read" }) }));
vi.mock("../sessions/background-menu", () => ({ expandAllCollapsedEntry: () => ({ label: "Expand all" }) }));
vi.mock("../sessions/newsession-view", () => ({ newSessionFromRail: vi.fn() }));
vi.mock("../../store/desk", () => ({ openAbout: vi.fn() }));
vi.mock("../../store/buildInfo", () => ({ appBuildInfo: () => undefined, refreshAppBuildInfo: vi.fn(async () => {}) }));
vi.mock("../../store/shortcuts", () => ({ shortcutTooltip: (text: string) => text }));
vi.mock("../../store/teams", async () => {
  const { createSignal: signal } = await import("solid-js");
  const [available, setAvailable] = signal<boolean | undefined>(undefined);
  const openNewTeam = vi.fn(async () => {});
  f.setAvailable = setAvailable;
  return { teams: { available, waitingCount: () => 0, openNewTeam } };
});
import { Rail } from "./Rail";

let dispose: (() => void) | undefined;
beforeEach(() => { f.setAvailable(undefined); f.page.mockClear(); });
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
const setup = () => {
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => createComponent(Rail, {}), host);
  return host;
};
const entry = (host: HTMLElement, label: string) =>
  [...host.querySelectorAll("button")].find((button) => button.getAttribute("aria-label") === label);

it("hides Teams and Waiting while the preview is off or unavailable", () => {
  const host = setup();
  expect(entry(host, "Teams")).toBeUndefined();
  expect(entry(host, "Waiting for you")).toBeUndefined();
  f.setAvailable(false);
  expect(entry(host, "Teams")).toBeUndefined();
  expect(entry(host, "Waiting for you")).toBeUndefined();
});

it("puts Teams and Waiting after the main screens, with bottom controls last", () => {
  const host = setup();
  f.setAvailable(true);
  expect([...host.querySelectorAll("button")].map((button) => button.getAttribute("aria-label") ?? button.textContent)).toEqual([
    "Overview", "New session", "Sessions", "Projects", "Machines", "Accounts", "Teams", "Waiting for you",
    "Keep Zerus above other windows", "Settings", "About",
  ]);
  expect(entry(host, "New team")).toBeUndefined();
  entry(host, "Teams")!.click();
  expect(f.page).toHaveBeenCalledWith("teams");
});
