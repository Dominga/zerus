// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { createComponent, createRoot, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { readFileSync } from "node:fs";
import { createSettingsCheck, settingsCheckText } from "../../store/settingsCheck";
import type { Outcome, SettingsCheckView, Target } from "../../types";
import type { Session } from "./types";
import { SettingsCheckBar, settingsTarget } from "./SettingsCheckBar";

// Task #7: the owner's lead could not be messaged from the desktop after a
// model change the agent did not visibly take. Every desktop composer now
// shows this bar for it, with Check again and Continue anyway.

vi.mock("../../i18n", () => ({ trNoop: (s: string) => s, tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])) }));
vi.mock("../../bridge", () => ({ runAction: vi.fn() }));
vi.mock("../../store/actions", () => ({ notify: vi.fn() }));

const session = { computer: "", name: "claude/example/lead", runId: "run-1", conversationId: "conv-1" } as Session;
const unknown = (at_prompt = true): SettingsCheckView => ({ requested_model: "claude-sonnet-5-5", requested_effort: "high", shown_model: "claude-opus-5-5",
  shown_effort: "medium", at_prompt, reason: "The agent shows claude-opus-5-5 (medium effort), not the requested claude-sonnet-5-5 (high effort)." });

function fakeStore(states: string[] = ["unknown"]) {
  const runs: [Target, boolean][] = [], notices: [string, string][] = [];
  const store = createRoot(() => createSettingsCheck({
    run: async (target, accept): Promise<Outcome> => { runs.push([target, accept]); return { outcome: "done", receipt: { state: states.shift() ?? "unknown" }, cleanup: null }; },
    notice: (tone, text) => notices.push([tone, text]),
  }));
  return { store, runs, notices };
}

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); vi.useRealTimers(); });

function show(bar: SettingsCheckView | null, store: ReturnType<typeof fakeStore>["store"], recheckMs = 60_000) {
  const [current, setCurrent] = createSignal<SettingsCheckView | null>(bar);
  const host = document.createElement("div"); document.body.append(host);
  dispose = render(() => createComponent(SettingsCheckBar, { get bar() { return current(); }, session, store, recheckMs }), host);
  const button = (name: string) => host.querySelector<HTMLButtonElement>(`[data-settings-${name}]`);
  return { host, button, setCurrent };
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("the settings check bar (task #7)", () => {
  it("explains what was asked and what the agent shows", () => {
    const text = settingsCheckText(unknown());
    expect(text.title).toBe("The agent has not shown the switch to claude-sonnet-5-5 with high effort yet");
    expect(text.detail).toContain("not the requested claude-sonnet-5-5");
    expect(text.accept).toContain("never into a dialog");
    expect(settingsCheckText(unknown(false)).accept).toContain("answer or close it there first");
    expect(settingsTarget(session)).toEqual({ computer: "", name: "claude/example/lead", run_id: "run-1", conversation_id: "conv-1" });
    expect(settingsTarget({ ...session, runId: undefined })).toBeUndefined();
  });

  it("checks again without typing and continues only after an explicit confirmation", async () => {
    const { store, runs, notices } = fakeStore(["unknown", "accepted"]);
    const { host, button } = show(unknown(), store);
    expect(host.textContent).toContain("has not shown the switch");
    button("recheck")!.click();
    await settle();
    expect(runs).toEqual([[settingsTarget(session), false]]);
    expect(notices.at(-1)?.[1]).toContain("still does not show");
    // Continue anyway asks first; Cancel goes back without a call.
    button("continue")!.click();
    expect(button("accept")).toBeTruthy();
    expect(runs).toHaveLength(1);
    (host.querySelector("[data-settings-confirm] .btn:not([data-settings-accept])") as HTMLButtonElement).click();
    expect(button("accept")).toBeNull();
    button("continue")!.click();
    button("accept")!.click();
    await settle();
    expect(runs.at(-1)).toEqual([settingsTarget(session), true]);
    expect(notices.at(-1)?.[1]).toContain("You can send messages again");
  });

  it("never offers Continue anyway while Terminal shows a dialog", () => {
    const { store, runs } = fakeStore();
    const { host, button } = show(unknown(false), store);
    expect(host.querySelector("[data-settings-check]")?.getAttribute("data-settings-check")).toBe("dialog");
    expect(button("continue")!.disabled).toBe(true);
    button("continue")!.click();
    expect(button("accept")).toBeNull();
    expect(runs).toEqual([]);
    // Check again stays: it reads, never types.
    expect(button("recheck")!.disabled).toBe(false);
  });

  it("checks again by itself while it shows, quietly, and goes when the change is confirmed", async () => {
    vi.useFakeTimers();
    const { store, runs, notices } = fakeStore(["unknown", "confirmed"]);
    const { host, setCurrent } = show(unknown(), store, 1000);
    await vi.advanceTimersByTimeAsync(1000);
    expect(runs).toEqual([[settingsTarget(session), false]]);
    expect(notices).toEqual([]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(notices.at(-1)?.[1]).toContain("shows the new model settings");
    // The next inspection reports nothing unconfirmed: the bar and its timer go.
    setCurrent(null);
    expect(host.querySelector("[data-settings-check]")).toBeNull();
    await vi.advanceTimersByTimeAsync(5000);
    expect(runs).toHaveLength(2);
  });

  it("is part of the one composer every desktop surface uses", () => {
    // Session view and Activity (SessionDetail), the Team tab and New team's
    // lead conversation (TeamTab) all draw <Composer />, which draws the bar.
    const read = (path: string) => readFileSync(new URL(path, import.meta.url), "utf8");
    expect(read("./Composer.tsx")).toContain("<SettingsCheckBar bar={v()?.settings_check} session={selected()} />");
    expect(read("./SessionDetail.tsx")).toContain("<Composer />");
    const team = read("../teams/TeamTab.tsx");
    expect(team).toContain("<LeadConversation computer={props.computer} lead={props.lead}");
    const lead = team.slice(team.indexOf("export function LeadConversation"));
    expect(lead.slice(0, lead.indexOf("\n}\n"))).toContain("<Composer />");
    expect(read("../teams/NewTeam.tsx")).toContain("<LeadConversation ");
  });
});
