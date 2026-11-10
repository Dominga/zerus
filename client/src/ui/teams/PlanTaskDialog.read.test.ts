// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createRoot } from "solid-js";
import { render } from "solid-js/web";

// Task #57: the polled plan clips a long description (and keeps a few
// criteria), so the plan's task dialog reads the task's own record when it
// opens: it shows the whole description and criteria, and a save sends them,
// never the polled clip. Until the read answers nothing can be saved.

const planEdit = vi.fn();
vi.mock("../../i18n", () => ({ trNoop: (s: string) => s, tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])) }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
vi.mock("../../store/teams", () => ({ memberRow: () => undefined, teams: () => [] }));
vi.mock("../../store/ui", () => ({ setDetailTab: () => {}, setPage: () => {}, setSelectedKey: () => {} }));
vi.mock("../../store/catalog", () => ({ orderedRows: () => [] }));
vi.mock("../../store/composer", () => ({ composer: {}, draftKey: () => "" }));
vi.mock("../../bridge", () => ({ teamTaskRead: () => Promise.reject(new Error("not in this test")) }));
vi.mock("./ProposalCards", () => ({ useModel: () => ({ model: { planEdit, notice: "" }, tick: () => 0 }), windowPort: { confirm: () => Promise.resolve(false) } }));

const { createPlanEditing } = await import("./TeamActions");

const CLIPPED = `${"word ".repeat(80)}... (clipped; hgs team status --task N has the full record)`;
const FULL = `${"word ".repeat(80)}and the rest of a long description the poll does not carry.`;

function team() {
  return {
    id: "t1", name: "T", status: "planning", plan_mode: "standard", revision: 3, pending_phase: null, waiting_for_lead: false, settings: {},
    members: [{ id: "lead", role: "Lead", lead: true }, { id: "dev", role: "Backend", lead: false }],
    phases: [{ n: 1, title: "One" }],
    tasks: [{ id: 7, title: "API", status: "pending", assignee: "dev", phase: 1, deps: [], attempts: 0, description: CLIPPED, criteria: ["first"] }],
  } as never;
}

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); planEdit.mockReset(); });

function open(read: () => Promise<unknown>) {
  if (!HTMLDialogElement.prototype.showModal) HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  const reads: unknown[] = [];
  let plan!: ReturnType<typeof createPlanEditing>;
  createRoot((done) => {
    plan = createPlanEditing(() => ({ computer: "local" as never, team: team() }), (...args) => { reads.push(args); return read() as never; });
    dispose = done;
  });
  const root = document.createElement("div");
  document.body.append(root);
  const unmount = render(() => plan.dialog(), root);
  const previous = dispose;
  dispose = () => { unmount(); previous?.(); };
  plan.editing().editTask(7);
  return { root, reads };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const field = (root: HTMLElement, name: string) => root.querySelector(`[data-field="${name}"]`) as HTMLTextAreaElement;
const save = (root: HTMLElement) => [...root.querySelectorAll("button")].find((button) => button.textContent === "Save") as HTMLButtonElement;

describe("the plan's task dialog", () => {
  it("shows and saves the task's whole description and criteria from its own read", async () => {
    const { root, reads } = open(() => Promise.resolve({ task: { id: 7, title: "API", description: FULL, criteria: ["first", "second in full"] } }));
    expect(reads).toEqual([["local", "t1", 7]]);
    // While the read runs the polled clip is not editable and nothing saves.
    expect(save(root).disabled).toBe(true);
    expect(field(root, "task-description").disabled).toBe(true);
    await settle();
    expect(field(root, "task-description").value).toBe(FULL);
    expect(field(root, "task-criteria").value).toBe("first\nsecond in full");
    expect(save(root).disabled).toBe(false);
    save(root).click();
    expect(planEdit).toHaveBeenCalledTimes(1);
    const sent = planEdit.mock.calls[0][0].task;
    expect(sent.description).toBe(FULL.trim());
    expect(sent.criteria).toEqual(["first", "second in full"]);
  });

  it("never saves the polled clip when the read fails", async () => {
    const { root } = open(() => Promise.reject(new Error("offline")));
    await settle();
    expect(root.querySelector('[data-field="task-unread"]')).not.toBeNull();
    expect(save(root).disabled).toBe(true);
    save(root).click();
    expect(planEdit).not.toHaveBeenCalled();
  });
});
