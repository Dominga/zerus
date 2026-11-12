// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { Project } from "../../types";
import type { Session } from "../sessions/types";

const state = vi.hoisted(() => ({ row: undefined as Session | undefined, projects: [] as Project[] }));

vi.mock("../../bridge", () => ({ teamAction: vi.fn(), teamRoleModels: vi.fn(async () => []) }));
vi.mock("../../store/catalog", () => ({ orderedRows: () => [] }));
vi.mock("../../store/organization", () => ({
  groupOf: () => "orbit",
  headerOf: () => ({ name: "Orbit" }),
  defaultProject: () => "",
  INBOX: "ungrouped",
}));
vi.mock("../../store/projects", () => ({ loadProjects: vi.fn(async () => {}), projectsPage: () => ({ projects: state.projects }) }));
vi.mock("../../store/machines", () => ({ machineCards: { "": { host: "workstation" } } }));
vi.mock("../../store/session", () => ({ activity: {}, designerActivity: { follow: vi.fn() } }));
vi.mock("../../store/ui", () => ({ selected: () => state.row, setDetailTab: vi.fn(), setSelectedKey: vi.fn() }));
vi.mock("../../store/teams", () => ({ showCatalog: () => vi.fn(), teams: { catalog: () => undefined } }));
vi.mock("../../store/teamsCatalog", () => ({ catalogPort: {} }));
vi.mock("../../store/teamConfirm", () => ({ teamConfirm: { ask: vi.fn() } }));
vi.mock("../../store/catalogEditor", () => ({ createCatalogEditor: () => ({}) }));
vi.mock("./catalog", () => ({ parseCatalog: vi.fn(), proposalCounts: vi.fn() }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
vi.mock("../sessions/Activity", () => ({ ActivityView: () => null }));
vi.mock("./TeamCatalog", () => ({ TeamCatalog: () => null }));
vi.mock("./TeamCatalogEditor", () => ({ TeamCatalogEditor: () => null }));

import { TeamDesigner } from "./TeamDesigner";
import { teamRoleModels } from "../../bridge";

const project = (id: string, name: string): Project => ({
  id,
  name,
  folders: [{ computer: "workstation", machine: "workstation", path: `/work/${id}`, name: id }],
}) as Project;

let host: HTMLDivElement;
let dispose: (() => void) | undefined;
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };

beforeEach(() => {
  state.projects = [project("orbit", "Orbit"), project("payments", "Payments")];
  state.row = {
    key: "orbit-row",
    computer: "",
    name: "claude/orbit/main",
    state: "running",
    cwdPath: "/work/orbit",
    cwdCanonical: "/work/orbit",
  } as Session;
  host = document.createElement("div");
  document.body.append(host);
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  host.remove();
  vi.clearAllMocks();
});

describe("Team designer project selection", () => {
  it("starts with no project when no session is selected and lists every project", async () => {
    state.row = undefined;
    dispose = render(() => createComponent(TeamDesigner, {}), host);
    await settle();
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Project for the Team designer"]')!;

    expect(select.value).toBe("-1");
    expect([...select.options].map((option) => option.textContent)).toEqual(["", "Orbit (orbit)", "Payments (payments)"]);
  });

  it("keeps the project chosen by the owner instead of restoring the selected session's project", async () => {
    dispose = render(() => createComponent(TeamDesigner, {}), host);
    await settle();
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Project for the Team designer"]')!;
    expect(select.value).toBe("0");

    select.value = "1";
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await settle();

    expect(select.value).toBe("1");
    expect(select.selectedOptions[0].textContent).toContain("Payments");
    expect(host.querySelector(".designer-head")!.textContent).toContain("/work/payments");
    const start = [...host.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Start")!;
    expect(start.disabled).toBe(true);

    const goal = host.querySelector<HTMLTextAreaElement>("textarea")!;
    goal.value = "Design the release team";
    goal.dispatchEvent(new InputEvent("input", { bubbles: true }));
    await settle();

    expect(start.disabled).toBe(false);
  });

  it("offers only the selected agent's account models and that model's efforts", async () => {
    vi.mocked(teamRoleModels).mockResolvedValue([{ id: "account-gpt", label: "Account GPT", effort_options: ["low", "max"] }]);
    dispose = render(() => createComponent(TeamDesigner, {}), host);
    await settle();
    const agent = host.querySelector<HTMLSelectElement>(".designer-agent select")!;
    agent.value = "codex";
    agent.dispatchEvent(new Event("change", { bubbles: true }));
    host.querySelector<HTMLButtonElement>('[data-action="designer-model"]')!.click();
    await settle();

    expect(teamRoleModels).toHaveBeenCalledWith("", "codex", "");
    const models = [...host.querySelectorAll<HTMLElement>("[data-designer-model]")];
    expect(models.map((item) => item.dataset.designerModel)).toEqual(["account-gpt"]);
    expect(models[0].textContent).toBe("Account GPT");
    models[0].click();
    host.querySelector<HTMLButtonElement>('[data-action="designer-model"]')!.click();
    await settle();

    host.querySelector<HTMLButtonElement>('[data-action="designer-effort"]')!.click();
    await settle();
    expect([...host.querySelectorAll<HTMLElement>("[data-designer-effort]")].map((item) => item.dataset.designerEffort)).toEqual(["", "low", "max"]);
  });
});
