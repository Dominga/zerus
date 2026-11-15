// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import rows from "../../../core/src/teams/fixtures/settings-refusals.json";
import status from "../../../core/src/teams/fixtures/cli-status.json";
import { createTeams, type TeamOutcome, type TeamsTransport } from "../../store/teams";
import { TeamSettingsDialog } from "./TeamSettingsDialog";
import { teamView } from "./fixtures";
import { completeSettings } from "./team-settings";
vi.mock("../../store/teams", async (original) => ({ ...(await original<object>()), newRequestId: () => "00000000-0000-4000-8000-000000000001" }));
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

// Refusals and their core outcomes are generated together by real operations.
// The surrounding view is the existing synthetic wrapper of cli-status.json.
it.each([["live_members", "max_members"], ["live_tasks", "max_open_tasks"]] as const)("places product-written %s through the real UI store with a synthetic view wrapper", async (name, key) => {
  const outcome = rows.outcomes[name] as TeamOutcome;
  const act = vi.fn(async () => outcome);
  const transport: TeamsTransport = {
    follow: vi.fn(async () => {}), hidden: vi.fn(async () => {}), tasksChanged: vi.fn(async () => {}), page: vi.fn(async () => {}),
    act, actAgain: act, dismiss: vi.fn(async () => {}), catalog: vi.fn(async () => ({})), saveTemplate: vi.fn(async () => {}), project: vi.fn(async () => ({})), preview: vi.fn(async () => ({})),
  };
  const store = createTeams(transport);
  const settings = structuredClone(status.team.settings);
  if (!completeSettings(settings)) throw new Error("Incomplete writer fixture");
  const host = document.createElement("div"); document.body.append(host); const close = vi.fn();
  dispose = render(() => createComponent(TeamSettingsDialog, {
    view: { ...structuredClone(teamView), settings }, busy: false,
    onSave: async (call) => { const returned = await store.run(call); expect(returned).toEqual(outcome); return returned; }, onClose: close,
  }), host);
  const input = host.querySelector<HTMLInputElement>(`input[data-setting="${key}"]`)!;
  input.value = "1"; input.dispatchEvent(new Event("input", { bubbles: true }));
  host.querySelector<HTMLButtonElement>("[data-settings-save]")!.click();
  await vi.waitFor(() => expect(input.closest("label")?.querySelector('[role="alert"]')?.textContent).toBe(rows.native[name].error.message));
  expect(input.getAttribute("aria-invalid")).toBe("true");
  expect(store.unresolved()).toHaveLength(0); expect(close).not.toHaveBeenCalled(); expect(act).toHaveBeenCalledOnce();
});
