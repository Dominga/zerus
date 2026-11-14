// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TeamSettingsDialog } from "./TeamSettingsDialog";
import { TeamWorkspace } from "./TeamTab";
import { teamView } from "./fixtures";
import { CAPS, DELEGATIONS, TIMERS, completeSettings, wholeNumber } from "./team-settings";
import type { TeamCall, TeamOutcome, TeamView } from "../../store/teams";
// Product writer: the retained hgs team status fixture, not a hand-written settings row.
import status from "../../../core/src/teams/fixtures/cli-status.json";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
const written = () => { const value = structuredClone(status.team.settings); if (!completeSettings(value)) throw new Error("Incomplete product fixture"); return value; };
const view = (): TeamView => ({ ...structuredClone(teamView), settings: written() });
function setup(outcome: TeamOutcome = { outcome: "done", receipt: {}, request_id: "result" }, initial = view()) {
  const host = document.createElement("div"); document.body.append(host);
  const calls: TeamCall[] = [];
  const close = vi.fn();
  const [current, setCurrent] = createSignal(initial);
  const save = vi.fn(async (call: TeamCall) => { calls.push(call); return outcome; });
  dispose = render(() => createComponent(TeamSettingsDialog, { get view() { return current(); }, busy: false, onSave: save, onClose: close }), host);
  const input = (key: string) => host.querySelector<HTMLInputElement>(`input[data-setting="${key}"]`)!;
  const change = (key: string, value: string) => { const field = host.querySelector<HTMLInputElement | HTMLSelectElement>(`[data-setting="${key}"]`)!; field.value = value; field.dispatchEvent(new Event(field.tagName === "SELECT" ? "change" : "input", { bubbles: true })); };
  const submit = async () => { host.querySelector<HTMLButtonElement>("[data-settings-save]")!.click(); await tick(); };
  return { host, calls, close, save, setCurrent, input, change, submit };
}

it("opens the settings from the Team bar and closes them without sending on Cancel", () => {
  const host = document.createElement("div"); document.body.append(host); const save = vi.fn();
  dispose = render(() => createComponent(TeamWorkspace, { view: view(), busy: false, failure: null, onAction: () => {}, onPage: () => {}, onAddTask: () => {}, onSettingsSave: save }), host);
  host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!.click();
  expect(host.querySelector("[data-team-settings]")).not.toBeNull();
  host.querySelector<HTMLButtonElement>("[data-initial]")!.click();
  expect(host.querySelector("[data-team-settings]")).toBeNull(); expect(save).not.toHaveBeenCalled();
});

it("shows every setting and preserves the complete product-written object on a one-field edit", async () => {
  const f = setup();
  expect(f.host.querySelectorAll("[data-setting]")).toHaveLength(22);
  expect(f.host.querySelector<HTMLButtonElement>("[data-settings-save]")!.disabled).toBe(true);
  f.change("routing", "lead"); await f.submit();
  expect(f.calls).toHaveLength(1);
  expect(f.calls[0]).toMatchObject({ team: teamView.bar.id, expected_revision: 16, action: { kind: "settings", settings: { ...written(), routing: "lead" } } });
  expect(f.calls[0].request_id).toMatch(/^[0-9a-f-]{36}$/);
  expect(f.close).toHaveBeenCalledOnce();
});

it.each(DELEGATIONS)("edits the %s delegation without changing another setting", async (key) => {
  const f = setup(); f.input(key).click(); await f.submit();
  expect(f.calls[0].action.settings).toEqual({ ...written(), delegations: { ...written().delegations, [key]: !written().delegations[key] } });
});

it("edits all numeric groups and workflow choices together", async () => {
  const f = setup();
  for (const key of CAPS) f.change(key, "7");
  for (const key of TIMERS) f.change(key, "11");
  for (const [key, value] of [["routing", "lead"], ["plan_approval", "auto"], ["phase_acceptance", "person"], ["integration", "none"]]) f.change(key, value);
  await f.submit();
  expect(f.calls[0].action.settings).toEqual({ ...written(), caps: Object.fromEntries(CAPS.map((key) => [key, 7])), timers: Object.fromEntries(TIMERS.map((key) => [key, 11])), routing: "lead", plan_approval: "auto", phase_acceptance: "person", integration: "none" });
});

it.each(["", "-1", "1.5", "501", "4294967296"])("refuses invalid open-task cap %s without sending", async (text) => {
  const f = setup(); f.change("max_open_tasks", text); await f.submit(); expect(f.calls).toHaveLength(0);
  expect(f.host.textContent).toContain("Enter whole numbers within the shown limits.");
});

it("keeps zero timers as zero and never turns an empty timer into zero", async () => {
  const f = setup(); f.change("report_timeout_seconds", ""); await f.submit(); expect(f.calls).toHaveLength(0);
  f.change("report_timeout_seconds", "0"); await f.submit(); expect((f.calls[0].action.settings as ReturnType<typeof written>).timers.report_timeout_seconds).toBe(0);
  expect(wholeNumber("1e3", 0, 0xffffffff)).toBeUndefined();
});

it("asks before widening landing, and Cancel does not save", async () => {
  const f = setup(); f.input("land_allowed").click(); await f.submit();
  expect(f.calls).toHaveLength(0); expect(f.host.querySelector("[role=alertdialog]")).not.toBeNull();
  f.host.querySelector<HTMLButtonElement>("[role=alertdialog] .btn")!.click(); expect(f.calls).toHaveLength(0);
  await f.submit(); f.host.querySelector<HTMLButtonElement>("[data-allow-landing]")!.click(); await tick();
  expect(f.calls).toHaveLength(1); expect((f.calls[0].action.settings as ReturnType<typeof written>).land_allowed).toBe(true);
});

it("keeps the revision and unedited values that were shown when a refreshed view arrives", async () => {
  const f = setup(); f.change("routing", "lead");
  const newer = view(); newer.revision = 99; newer.settings!.caps.max_open_tasks = 99; f.setCurrent(newer);
  await f.submit(); expect(f.calls[0].expected_revision).toBe(16); expect(f.calls[0].action.settings).toEqual({ ...written(), routing: "lead" });
});

it("shows a stale refusal verbatim and disables another write until the dialog is reopened", async () => {
  const f = setup({ outcome: "failed", code: "stale_revision", detail: "The team changed. Read the team before sending it again.", request_id: "stale" });
  f.change("routing", "lead"); await f.submit(); await f.submit();
  expect(f.calls).toHaveLength(1); expect(f.close).not.toHaveBeenCalled();
  expect(f.host.querySelector("[data-settings-error]")?.textContent).toBe("The team changed. Read the team before sending it again.");
});

it.each(["uncertain", "pending"] as const)("hands %s back to the existing store controls without an automatic replay", async (outcome) => {
  const f = setup(outcome === "uncertain" ? { outcome, detail: "Check it or dismiss it.", request_id: "unknown" } : { outcome, request_id: "pending" });
  f.change("routing", "lead"); await f.submit(); await tick(); expect(f.calls).toHaveLength(1); expect(f.close).toHaveBeenCalledOnce();
});

it("keeps the server refusal in the dialog and permits a corrected fresh request", async () => {
  const f = setup({ outcome: "failed", code: "over_cap", detail: "Caps are below the admitted live set.", request_id: "refused" });
  f.change("max_members", "1"); await f.submit(); expect(f.host.textContent).toContain("Caps are below the admitted live set.");
  f.change("max_members", "6"); f.change("routing", "lead"); await f.submit();
  expect(f.calls).toHaveLength(2); expect(f.calls[0].request_id).not.toBe(f.calls[1].request_id);
});

it("does not offer a second settings intent while the first is unresolved, or on a terminal team", () => {
  const host = document.createElement("div"); document.body.append(host); const [blocked, setBlocked] = createSignal(true); const [shown, setShown] = createSignal(view());
  dispose = render(() => createComponent(TeamWorkspace, { get view() { return shown(); }, busy: false, failure: null, onAction: () => {}, onPage: () => {}, onAddTask: () => {}, onSettingsSave: vi.fn(), get settingsBlocked() { return blocked(); } }), host);
  const button = host.querySelector<HTMLButtonElement>("[data-team-settings-open]")!;
  expect(button.disabled).toBe(true); button.click(); expect(host.querySelector("[data-team-settings]")).toBeNull();
  setBlocked(false); expect(button.disabled).toBe(false); setShown({ ...view(), bar: { ...view().bar, status: "stopped" } }); expect(button.disabled).toBe(true);
});

it("never fills unknown or incomplete settings with defaults", async () => {
  for (const group of ["delegations", "caps", "timers"] as const) {
    const missing = written(); delete (missing[group] as Record<string, unknown>)[Object.keys(missing[group])[0]];
    expect(completeSettings(missing)).toBe(false);
  }
  const future = { ...written(), future_power: true }; expect(completeSettings(future)).toBe(false);
  const f = setup(undefined, { ...view(), settings: undefined }); await f.submit(); expect(f.calls).toHaveLength(0);
  expect(f.host.textContent).toContain("This version cannot show all team settings.");
});
