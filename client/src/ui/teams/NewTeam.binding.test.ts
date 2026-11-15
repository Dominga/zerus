// @vitest-environment happy-dom
// The create binding (zerus-63rru.30, the review's S1): what the person
// confirmed is bound at the create. The rendered New team dialog, the real
// Teams store and its transport run here; only the native invoke is a fake,
// with its own catalog that another window can change after the read. Its
// create resolves the template by id as the command line does and refuses it
// with template_changed when the digest the form sent is not the template's
// any more; otherwise the team gets the permissions the create carries.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import type { TeamCall, TeamCatalog } from "../../store/teams";
import type { NewTeamInput } from "./NewTeam";
import unknownPermission from "../../../core/src/teams/fixtures/form-catalog-unknown-permission.json";
import planningSource from "./fixtures/planning-start.json";

const FIVE = ["accept_results", "answer_members", "confirm_trust", "resume_exited", "merge_accepted"] as const;
type Five = Record<(typeof FIVE)[number], boolean>;
const f = vi.hoisted(() => ({ catalog: null as unknown as TeamCatalog, acts: [] as TeamCall[], grants: [] as unknown[], reads: 0 }));
vi.mock("../../bridge", async (original) => ({ ...(await original() as object), inTauri: () => true }));
vi.mock("../../store/ui", async (original) => ({ ...(await original() as object), selected: () => ({ project: "example-project", gitRoot: "/workspace/example" }) }));
vi.mock("../../store/machines", async (original) => ({ ...(await original() as object), machineCards: { "": { machine_id: "00000000-0000-4000-8000-0000000000aa" } } }));
vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => false,
  invoke: async (cmd: string, args: { call: TeamCall }) => {
    if (cmd === "teams_catalog") { f.reads += 1; return { catalog: structuredClone(f.catalog) }; }
    if (cmd === "teams_project") return { state: "ok", head: "0123456789abcdef0123456789abcdef01234567", branch: "main" };
    if (cmd === "team_action") {
      const call = structuredClone(args.call);
      f.acts.push(call);
      const action = call.action as Record<string, unknown>;
      const template = f.catalog.templates.find((candidate) => candidate.id === action.template);
      if (!template || action.expected_template !== template.digest) {
        return { outcome: "failed", detail: "The template changed since it was shown. Review it again.", code: "template_changed", request_id: call.request_id };
      }
      f.grants.push(action.delegations);
      return { outcome: "done", receipt: { ok: true }, request_id: call.request_id };
    }
    if (["teams_follow", "teams_hidden", "teams_tasks_changed"].includes(cmd)) return;
    throw new Error(`Unexpected invoke ${cmd}`);
  },
}));

const D1 = "6d15fdbdd92ac4d5c5bc53f0bd6da7871c0dbf1842f4d4f25bf94aebc95bbb9d";
const D2 = "1111111111111111111111111111111111111111111111111111111111111111";
const none: Five = { accept_results: false, answer_members: false, confirm_trust: false, resume_exited: false, merge_accepted: false };
let store: typeof import("../../store/teams").teams;
let Dialog: typeof import("./NewTeam").NewTeamDialog;
let Form: typeof import("./NewTeam").NewTeam;
let fixture: TeamCatalog;
let dispose: (() => void) | undefined;
beforeEach(async () => {
  vi.resetModules();
  ({ teams: store } = await import("../../store/teams"));
  ({ NewTeamDialog: Dialog, NewTeam: Form } = await import("./NewTeam"));
  fixture = structuredClone((await import("./fixtures")).catalog);
  f.catalog = structuredClone(fixture);
  f.catalog.templates[0].settings.delegations = { ...none };
  f.catalog.templates[0].digest = D1;
  f.acts = []; f.grants = []; f.reads = 0;
});
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.clearAllMocks(); });

const button = (host: HTMLElement, label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.textContent === label)!;
const shown = (host: HTMLElement) => Object.fromEntries([...host.querySelectorAll<HTMLLabelElement>("[data-delegation]")].map((line) => [line.dataset.delegation, line.querySelector<HTMLInputElement>("input")!.checked]));
const toggle = (host: HTMLElement, key: string, on: boolean) => {
  const input = host.querySelector<HTMLInputElement>(`[data-delegation="${key}"] input`)!;
  input.checked = on; input.dispatchEvent(new Event("change", { bubbles: true }));
};
const settle = async () => { for (let n = 0; n < 20; n++) await Promise.resolve(); };
/** The other window: the same template id with other content and digest. */
function changeElsewhere(delegations: Record<string, boolean>, digest = D2) {
  f.catalog.templates[0].settings.delegations = delegations;
  f.catalog.templates[0].digest = digest;
}
async function open() {
  await store.openNewTeam();
  const host = document.createElement("div");
  document.body.append(host);
  dispose = render(() => createComponent(Dialog, {}), host);
  const name = host.querySelector<HTMLInputElement>('input[placeholder="Fixes"]')!;
  name.value = "Fixes"; name.dispatchEvent(new Event("input", { bubbles: true }));
  button(host, "Check folder").click();
  await vi.waitFor(() => expect(host.querySelector("[data-folder-ok]")).not.toBeNull());
  return host;
}

it("a second window cannot grant permissions different from those shown (the review's race, taken over)", async () => {
  const host = await open();
  const visible = shown(host);
  expect(visible.merge_accepted).toBe(false);
  changeElsewhere({ ...none, merge_accepted: true });
  button(host, "Create team").click();
  await vi.waitFor(() => expect(f.acts).toHaveLength(1));
  const first = f.acts[0].action as Record<string, unknown>;
  expect(first.expected_template, "the digest of the template that was shown").toBe(D1);
  expect(first.delegations).toEqual(visible);
  expect(f.grants, "the changed template was refused: no team").toEqual([]);
  // Shown again as it is now; nothing is sent again by itself.
  await vi.waitFor(() => expect(host.querySelector("[data-template-changed]")).not.toBeNull());
  await vi.waitFor(() => expect(shown(host).merge_accepted).toBe(true));
  expect(f.reads).toBe(2);
  expect(host.querySelector("[data-new-team-error]"), "said in the form's words, not as the refusal's").toBeNull();
  await settle();
  expect(f.acts).toHaveLength(1);
  expect(store.createIntent(), "the refused create is settled, not left uncertain").toBeNull();
  expect(store.unresolved()).toEqual([]);
  // The person confirms again: the team gets exactly what is on screen now.
  const now = shown(host);
  button(host, "Create team").click();
  await vi.waitFor(() => expect(f.grants).toHaveLength(1));
  const second = f.acts[1];
  expect((second.action as Record<string, unknown>).expected_template).toBe(D2);
  expect(second.request_id).not.toBe(f.acts[0].request_id);
  expect(f.grants[0]).toEqual(now);
});

it("starts only a planning intent with the writer-derived catalog, required goal and shown permissions", async () => {
  f.catalog = structuredClone(planningSource.catalog) as unknown as TeamCatalog;
  const host = await open();
  host.querySelector<HTMLInputElement>("[data-start-planning]")!.click();
  expect(host.textContent).toContain("Only the lead starts");
  expect(button(host, "Start planning").disabled).toBe(true);
  const goal = host.querySelector<HTMLInputElement>('input[placeholder="What should the team work on?"]')!;
  goal.value = planningSource.view.planning.goal;
  goal.dispatchEvent(new Event("input", { bubbles: true }));
  const permissions = shown(host);
  expect(button(host, "Start planning").disabled).toBe(false);
  button(host, "Start planning").click();
  await vi.waitFor(() => expect(f.acts).toHaveLength(1));
  expect(f.acts[0].action).toMatchObject({kind:"start_planning",goal:planningSource.view.planning.goal,delegations:permissions});
  expect(f.acts[0].request_id).toMatch(/^[0-9a-f-]{36}$/);
  await vi.waitFor(() => expect(store.createIntent()).toBeNull());
});

it("sends the checkboxes as they are for every combination that differs from the template", async () => {
  const given: Five = { accept_results: true, answer_members: true, confirm_trust: true, resume_exited: true, merge_accepted: true };
  const catalog = structuredClone(fixture);
  catalog.templates[0].settings.delegations = { ...given };
  let checked = 0;
  for (let bits = 0; bits < 32; bits++) {
    const wanted = Object.fromEntries(FIVE.map((key, index) => [key, !!(bits & (1 << index))])) as Five;
    if (FIVE.every((key) => wanted[key] === given[key])) continue;
    const created: NewTeamInput[] = [];
    const host = document.createElement("div");
    document.body.append(host);
    const stop = render(() => createComponent(Form, {
      catalog, busy: false, machineId: "00000000-0000-4000-8000-0000000000aa", defaultProject: "example-project", defaultFolder: "/workspace/example",
      onCheckFolder: async () => ({ git: true, head: "0123456789abcdef0123456789abcdef01234567", branch: "main", reason: "" }),
      onCreate: async (input) => { created.push(input); return { outcome: "failed", detail: "fixture", request_id: input.request_id }; },
      onSaveTemplate: async () => {}, onClose: () => {},
    }), host);
    const name = host.querySelector<HTMLInputElement>('input[placeholder="Fixes"]')!;
    name.value = "Fixes"; name.dispatchEvent(new Event("input", { bubbles: true }));
    button(host, "Check folder").click();
    await vi.waitFor(() => expect(host.querySelector("[data-folder-ok]")).not.toBeNull());
    for (const key of FIVE) if (wanted[key] !== given[key]) toggle(host, key, wanted[key]);
    expect(shown(host)).toEqual(wanted);
    button(host, "Create team").click();
    await vi.waitFor(() => expect(created).toHaveLength(1));
    expect(created[0].delegations, `combination ${bits}`).toEqual(wanted);
    expect(Object.keys(created[0].delegations)).toEqual([...FIVE]);
    expect(created[0].expected_template).toBe(catalog.templates[0].digest);
    stop(); host.remove(); checked++;
  }
  expect(checked).toBe(31);
});

it("sends the digest of the template on screen, never one read later, with the person's choices kept", async () => {
  const host = await open();
  toggle(host, "accept_results", true);
  // The command line's template moves on; nothing has read it yet.
  changeElsewhere({ ...none, resume_exited: true });
  button(host, "Create team").click();
  await vi.waitFor(() => expect(f.acts).toHaveLength(1));
  expect(f.reads, "the create reads no catalog before it is sent").toBe(1);
  expect((f.acts[0].action as Record<string, unknown>).expected_template).toBe(D1);
  expect((f.acts[0].action as Record<string, unknown>).delegations).toEqual({ ...none, accept_results: true });
  // Read again: the screen is the new template with the person's own choice
  // over it, and the next create binds that pair.
  await vi.waitFor(() => expect(shown(host)).toEqual({ ...none, accept_results: true, resume_exited: true }));
  button(host, "Create team").click();
  await vi.waitFor(() => expect(f.grants).toHaveLength(1));
  expect((f.acts[1].action as Record<string, unknown>).expected_template).toBe(D2);
  expect(f.grants[0]).toEqual({ ...none, accept_results: true, resume_exited: true });
});

it("a changed template is shown as it is now: choices stay only where their permission still exists, and nothing is resent", async () => {
  const host = await open();
  toggle(host, "accept_results", true);
  toggle(host, "resume_exited", true);
  const { resume_exited: _gone, ...four } = none;
  changeElsewhere({ ...four, merge_accepted: true });
  button(host, "Create team").click();
  await vi.waitFor(() => expect(host.querySelector("[data-template-changed]")).not.toBeNull());
  expect(host.querySelector("[data-template-changed]")!.textContent).toBe("The template changed since you saw it, and nothing was created: review it as it is now, then create the team again.");
  await vi.waitFor(() => expect(shown(host)).toEqual({ ...four, accept_results: true, merge_accepted: true }));
  expect(host.querySelector('[data-delegation="resume_exited"]'), "a permission the template no longer has is not shown").toBeNull();
  // Four permissions cannot be bound: Create says why, and nothing was sent again.
  expect(button(host, "Create team").disabled).toBe(true);
  expect(host.querySelector("[data-create-blocker]")!.textContent).toContain("This version does not know every permission of the template. Update Zerus to create a team.");
  button(host, "Create team").click();
  await settle();
  expect(f.acts).toHaveLength(1);
  expect(f.grants).toEqual([]);
  expect(host.querySelector("[data-new-team-error]"), "the refusal's own words are not shown as an error").toBeNull();
});

it("a permission this version does not know keeps Create blocked and sends nothing (the review's B1, taken over)", async () => {
  // The core's form catalog for the store's catalog with future_power under
  // bugfix's delegations (client/core teams::catalog::tests pins this file).
  f.catalog = structuredClone(unknownPermission.catalog) as unknown as TeamCatalog;
  const host = await open();
  host.querySelector<HTMLInputElement>('[data-template="bugfix"] input')!.click();
  await settle();
  expect(Object.keys(shown(host)).sort(), "the five known permissions are shown").toEqual([...FIVE].sort());
  expect(button(host, "Create team").disabled).toBe(true);
  expect(host.querySelector("[data-create-blocker]")!.textContent).toContain("This version does not know every permission of the template. Update Zerus to create a team.");
  button(host, "Create team").click();
  await settle();
  expect(f.acts).toEqual([]);
  // Another template of the same catalog, without unknown permissions, can be created.
  host.querySelector<HTMLInputElement>('[data-template="small"] input')!.click();
  await settle();
  expect(button(host, "Create team").disabled).toBe(false);
});

it("an unknown field in what a template shows keeps Create blocked with its own reason", async () => {
  f.catalog.templates[0].unknown_fields = ["settings/auto_land"];
  const host = await open();
  expect(button(host, "Create team").disabled).toBe(true);
  expect(host.querySelector("[data-create-blocker]")!.textContent).toContain("This version cannot show everything the template sets. Update Zerus to create a team.");
  button(host, "Create team").click();
  await settle();
  expect(f.acts).toEqual([]);
});

it("a catalog without digests, from an older command line, disables Create and sends nothing", async () => {
  delete f.catalog.templates[0].digest;
  const host = await open();
  expect(button(host, "Create team").disabled).toBe(true);
  expect(host.querySelector("[data-create-blocker]")!.textContent).toContain("This hgs cannot confirm the template you see. Update hgs to create a team.");
  button(host, "Create team").click();
  await settle();
  expect(f.acts).toEqual([]);
});
