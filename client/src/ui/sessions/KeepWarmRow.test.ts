// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
const native = vi.hoisted(() => ({ setKeepWarm: vi.fn() }));
vi.mock("../../bridge", () => ({ inTauri: () => true, setKeepWarm: native.setKeepWarm }));
vi.mock("../../store/actions", () => ({ errorText: (error: unknown) => (error instanceof Error ? error.message : String(error)) }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
import { KeepWarmRow } from "./KeepWarmRow";
import type { Session } from "./types";
import type { KeepWarmState } from "../../types";

const NOW = Date.now() / 1000;
const warm = (extra: Partial<KeepWarmState> = {}): KeepWarmState => ({ enabled: false, until: null, revision: 3, state: "off", next_at: null, reason: "", turn_active: false, ...extra });
const session = (keepWarm?: KeepWarmState) => ({ key: "k", computer: "", name: "codex/p/warm", runId: "run-1", conversationId: "conversation-1", keepWarm }) as unknown as Session;
const flush = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
let host: HTMLDivElement, dispose: (() => void) | undefined;
const mount = (s: Session, inspected: unknown = {}, online = true) => {
  dispose = render(() => createComponent(KeepWarmRow, { session: s, inspected, online }), host);
  return {
    toggle: () => host.querySelector<HTMLButtonElement>("button[role=switch]")!,
    range: () => host.querySelector<HTMLInputElement>("input[type=range]"),
    hints: () => [...host.querySelectorAll(".kw-hint")].map((node) => node.textContent),
  };
};
beforeEach(() => { vi.clearAllMocks(); host = document.createElement("div"); document.body.append(host); });
afterEach(() => { dispose?.(); dispose = undefined; host.remove(); });

it("turns keep-warm on for two hours with the switch's accessible name and the host's revision", async () => {
  native.setKeepWarm.mockResolvedValue({ ...warm({ enabled: true, until: NOW + 7200, revision: 4, state: "waiting" }), supported: true, ttl_seconds: 3600, interval_seconds: 2880 });
  const row = mount(session(warm()), { keep_warm: { ...warm(), supported: true, ttl_seconds: 3600, interval_seconds: 2880 } });
  expect(row.toggle().getAttribute("aria-label")).toBe("Keep cache warm");
  expect(row.toggle().getAttribute("aria-checked")).toBe("false");
  expect(row.range()).toBeNull();
  row.toggle().click();
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledWith({ computer: "", name: "codex/p/warm", run_id: "run-1", conversation_id: "conversation-1" }, { kind: "hours", hours: 2 }, 3);
  expect(row.toggle().getAttribute("aria-checked")).toBe("true");
  expect(row.range()!.value).toBe("2");
  expect(row.hints()[0]).toMatch(/^(1 h 59 m|2 h) left$/);
});

it("offers Always at the slider's last position", async () => {
  native.setKeepWarm.mockResolvedValue(warm({ enabled: true, until: null, revision: 5, state: "waiting" }));
  const row = mount(session(warm({ enabled: true, until: NOW + 3 * 3600, revision: 4, state: "waiting" })));
  const range = row.range()!;
  range.value = "8";
  range.dispatchEvent(new Event("input"));
  range.dispatchEvent(new Event("change"));
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledWith(expect.anything(), { kind: "always" }, 4);
  expect(host.querySelector(".kw-value")!.textContent).toBe("Always");
  expect(row.hints()).toContain("Always");
});

it("shows why an ineligible agent cannot keep its cache warm, with the switch disabled", () => {
  const reason = "OpenAI keeps a 30-minute cache only for GPT-5.6 and later. This model's cache lasts about 5 to 10 minutes.";
  const row = mount(session(warm()), { keep_warm: { ...warm(), supported: false, reason_code: "codex_model", reason } });
  expect(row.toggle().disabled).toBe(true);
  expect(row.hints()).toEqual([reason]);
});

it("keeps the host's refusal visible and the switch usable again", async () => {
  native.setKeepWarm.mockRejectedValue(new Error("At most 64 sessions on one computer can keep their cache warm."));
  const row = mount(session(warm()), { keep_warm: { ...warm(), supported: true } });
  row.toggle().click();
  await flush();
  expect(host.querySelector("[role=alert]")!.textContent).toBe("At most 64 sessions on one computer can keep their cache warm.");
  expect(row.toggle().disabled).toBe(false);
  expect(row.toggle().getAttribute("aria-checked")).toBe("false");
});

it("an older computer without keep-warm shows an update hint and an offline one cannot change it", () => {
  const old = mount(session(undefined));
  expect(old.toggle().disabled).toBe(true);
  expect(old.hints()).toEqual(["Update Zerus on this computer to keep the prompt cache warm."]);
  dispose!();
  const offline = mount(session(warm()), {}, false);
  expect(offline.toggle().disabled).toBe(true);
});

it("a reply for the previously selected session never changes the newly selected one", async () => {
  let resolve!: (value: Record<string, unknown>) => void;
  native.setKeepWarm.mockImplementation(() => new Promise((done) => { resolve = done; }));
  const alpha = { ...session(warm()), key: "alpha", name: "codex/p/alpha" } as Session;
  const beta = { ...session(warm({ revision: 7 })), key: "beta", name: "codex/p/beta", runId: "run-b", conversationId: "conversation-b" } as Session;
  const [selected, select] = createSignal<Session>(alpha);
  dispose = render(() => createComponent(KeepWarmRow, { get session() { return selected(); }, inspected: { keep_warm: { ...warm(), supported: true } }, online: true }), host);
  const toggle = () => host.querySelector<HTMLButtonElement>("button[role=switch]")!;
  toggle().click();
  await flush();
  expect(toggle().disabled).toBe(true);
  select(beta);
  await flush();
  expect(toggle().disabled).toBe(false);
  resolve({ ...warm({ enabled: true, until: null, revision: 4, state: "waiting" }), supported: true });
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledTimes(1);
  expect(toggle().getAttribute("aria-checked")).toBe("false");
  expect(host.querySelector(".kw-value")).toBeNull();
  expect(host.querySelector("[role=alert]")).toBeNull();
});

it("a late failure for the previously selected session shows nothing on the new one", async () => {
  let reject!: (error: Error) => void;
  native.setKeepWarm.mockImplementation(() => new Promise((_, fail) => { reject = fail; }));
  const alpha = { ...session(warm()), key: "alpha" } as Session;
  const beta = { ...session(warm()), key: "beta", conversationId: "conversation-b" } as Session;
  const [selected, select] = createSignal<Session>(alpha);
  dispose = render(() => createComponent(KeepWarmRow, { get session() { return selected(); }, inspected: { keep_warm: { ...warm(), supported: true } }, online: true }), host);
  host.querySelector<HTMLButtonElement>("button[role=switch]")!.click();
  await flush();
  select(beta);
  await flush();
  reject(new Error("At most 64 sessions on one computer can keep their cache warm."));
  await flush();
  expect(host.querySelector("[role=alert]")).toBeNull();
  expect(host.querySelector<HTMLButtonElement>("button[role=switch]")!.disabled).toBe(false);
});
