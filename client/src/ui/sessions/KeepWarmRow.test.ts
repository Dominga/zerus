// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { createStore } from "solid-js/store";
import { render } from "solid-js/web";
const native = vi.hoisted(() => ({ setKeepWarm: vi.fn() }));
vi.mock("../../bridge", () => ({ inTauri: () => true, setKeepWarm: native.setKeepWarm }));
vi.mock("../../store/actions", () => ({ errorText: (error: unknown) => (error instanceof Error ? error.message : String(error)) }));
vi.mock("../lib/icons", () => ({ Icon: () => null }));
import { KeepWarmRow } from "./KeepWarmRow";
import { confirmKeepWarm } from "./keep-warm-state";
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
// The one confirmed-change slot is shared with the header; move it to a target no test uses.
afterEach(() => { dispose?.(); dispose = undefined; host.remove(); confirmKeepWarm("unused", warm()); });

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
  // zerus-ua4bi: an infinity sign, named Always for assistive technology.
  const value = host.querySelector(".kw-value")!;
  expect(value.textContent).toBe("\u221e");
  expect(value.getAttribute("aria-hidden")).toBe("true");
  expect(value.getAttribute("title")).toBe("Always");
  expect(range.getAttribute("aria-valuetext")).toBe("Always");
  expect(row.hints()).toContain("Always");
});

it("a one hour choice stays at one hour when the reply's end is a little later than this window's clock (zerus-ua4bi)", async () => {
  // The host set until from its own clock; this window's clock read is up to 30 s old.
  native.setKeepWarm.mockResolvedValue(warm({ enabled: true, until: Date.now() / 1000 + 3600 + 25, revision: 5, state: "waiting" }));
  const row = mount(session(warm({ enabled: true, until: NOW + 3 * 3600, revision: 4, state: "waiting" })));
  const range = row.range()!;
  range.value = "1";
  range.dispatchEvent(new Event("input"));
  range.dispatchEvent(new Event("change"));
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledWith(expect.anything(), { kind: "hours", hours: 1 }, 4);
  expect(row.range()!.value).toBe("1");
  expect(host.querySelector(".kw-value")!.textContent).toBe("1 h");
});

it("the slider offers exactly the host's 1 to 7 hours and Always (zerus-ua4bi)", () => {
  const range = mount(session(warm({ enabled: true, until: NOW + 3 * 3600, revision: 4, state: "waiting" }))).range()!;
  expect([range.min, range.max, range.step]).toEqual(["1", "8", "1"]);
});

it("a change keeps the switch, slider and value nodes and never disables them while it is saved (zerus-ua4bi)", async () => {
  let resolve!: (value: unknown) => void;
  native.setKeepWarm.mockImplementation(() => new Promise((done) => { resolve = done; }));
  const row = mount(session(warm({ enabled: true, until: NOW + 3 * 3600, revision: 4, state: "waiting" })));
  const nodes = [row.toggle(), row.range()!, host.querySelector(".kw-value")!];
  const removed: Node[] = [];
  const observer = new MutationObserver((records) => { for (const r of records) removed.push(...r.removedNodes); });
  observer.observe(host, { childList: true, subtree: true });
  const range = row.range()!;
  range.value = "5";
  range.dispatchEvent(new Event("input"));
  range.dispatchEvent(new Event("change"));
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledWith(expect.anything(), { kind: "hours", hours: 5 }, 4);
  expect([row.toggle().disabled, row.range()!.disabled]).toEqual([false, false]);
  expect(host.querySelector(".kw-row")!.getAttribute("aria-busy")).toBe("true");
  expect(host.querySelector(".kw-value")!.textContent).toBe("5 h");
  resolve(warm({ enabled: true, until: Date.now() / 1000 + 5 * 3600, revision: 5, state: "waiting" }));
  await flush();
  expect(host.querySelector(".kw-row")!.getAttribute("aria-busy")).toBeNull();
  expect([row.range()!.value, host.querySelector(".kw-value")!.textContent]).toEqual(["5", "5 h"]);
  expect([row.toggle(), row.range(), host.querySelector(".kw-value")]).toEqual(nodes);
  observer.disconnect();
  expect(removed.filter((node) => nodes.some((kept) => node === kept || node.contains(kept)))).toEqual([]);
});

it("a choice made while another is saved goes next with the confirmed revision, and the last one wins (zerus-ua4bi)", async () => {
  const replies: ((value: unknown) => void)[] = [];
  native.setKeepWarm.mockImplementation(() => new Promise((done) => { replies.push(done); }));
  const row = mount(session(warm({ enabled: true, until: NOW + 3 * 3600, revision: 4, state: "waiting" })));
  const choose = (value: string) => { const range = row.range()!; range.value = value; range.dispatchEvent(new Event("input")); range.dispatchEvent(new Event("change")); };
  choose("5");
  await flush();
  choose("6");
  choose("2");
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledTimes(1);
  expect(host.querySelector(".kw-value")!.textContent).toBe("2 h");
  replies[0](warm({ enabled: true, until: Date.now() / 1000 + 5 * 3600, revision: 5, state: "waiting" }));
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledTimes(2);
  expect(native.setKeepWarm).toHaveBeenLastCalledWith(expect.anything(), { kind: "hours", hours: 2 }, 5);
  expect(host.querySelector(".kw-value")!.textContent).toBe("2 h");
  replies[1](warm({ enabled: true, until: Date.now() / 1000 + 2 * 3600, revision: 6, state: "waiting" }));
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledTimes(2);
  expect([row.range()!.value, host.querySelector(".kw-value")!.textContent]).toEqual(["2", "2 h"]);
});

it("the switch shows a change at once and returns to the confirmed state when the host refuses it (zerus-ua4bi)", async () => {
  let reject!: (error: Error) => void;
  native.setKeepWarm.mockImplementation(() => new Promise((_, fail) => { reject = fail; }));
  const row = mount(session(warm()), { keep_warm: { ...warm(), supported: true } });
  row.toggle().click();
  await flush();
  expect(row.toggle().getAttribute("aria-checked")).toBe("true");
  expect(row.range()!.value).toBe("2");
  reject(new Error("At most 64 sessions on one computer can keep their cache warm."));
  await flush();
  expect(row.toggle().getAttribute("aria-checked")).toBe("false");
  expect(row.range()).toBeNull();
  expect(host.querySelector("[role=alert]")!.textContent).toBe("At most 64 sessions on one computer can keep their cache warm.");
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
  expect(toggle().getAttribute("aria-checked")).toBe("true");
  select(beta);
  await flush();
  expect(toggle().getAttribute("aria-checked")).toBe("false");
  expect(host.querySelector(".kw-row")!.getAttribute("aria-busy")).toBeNull();
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

// review-win-d's counterexamples for #1841 (zerus-zzuy0, zerus-trjfi).
const chooseAt = (value: string) => {
  const range = host.querySelector<HTMLInputElement>("input[type=range]")!;
  range.value = value;
  range.dispatchEvent(new Event("input"));
  range.dispatchEvent(new Event("change"));
};

it("a row removed with a change pending never sends its queued choice to the session selected next (zerus-zzuy0)", async () => {
  const replies: ((value: unknown) => void)[] = [];
  native.setKeepWarm.mockImplementation(() => new Promise((resolve) => { replies.push(resolve); }));
  const alpha = { ...session(warm({ enabled: true, until: Date.now() / 1000 + 10800, revision: 4 })), key: "alpha", name: "codex/p/alpha" } as Session;
  const beta = { ...session(warm({ revision: 4 })), key: "beta", name: "codex/p/beta", runId: "run-b", conversationId: "conversation-b" } as Session;
  const [selected, select] = createSignal(alpha);
  dispose = render(() => createComponent(KeepWarmRow, { get session() { return selected(); }, inspected: {}, online: true }), host);
  chooseAt("5"); await flush(); chooseAt("8"); await flush();
  expect(native.setKeepWarm).toHaveBeenCalledTimes(1);
  dispose(); dispose = undefined;
  select(beta);
  replies[0](warm({ enabled: true, until: Date.now() / 1000 + 5 * 3600, revision: 5 }));
  await flush();
  expect(native.setKeepWarm.mock.calls.map(([target]) => target.name)).toEqual([alpha.name]);
});

it("a queued choice goes with the revision its predecessor confirmed, so a change from another device is refused, not overwritten (zerus-trjfi)", async () => {
  const replies: ((value: unknown) => void)[] = [];
  native.setKeepWarm.mockImplementation(() => new Promise((resolve) => { replies.push(resolve); }));
  const [live, setLive] = createStore({ ...session(warm({ enabled: true, until: Date.now() / 1000 + 10800, revision: 4 })), key: "alpha" } as Session);
  dispose = render(() => createComponent(KeepWarmRow, { get session() { return live; }, inspected: {}, online: true }), host);
  chooseAt("5"); await flush(); chooseAt("2"); await flush();
  // Another device turned it off after the first write committed, before its revision 5 reply arrived here.
  setLive("keepWarm", warm({ enabled: false, revision: 8 })); await flush();
  replies[0](warm({ enabled: true, until: Date.now() / 1000 + 5 * 3600, revision: 5 })); await flush();
  expect(native.setKeepWarm).toHaveBeenCalledTimes(2);
  expect(native.setKeepWarm).toHaveBeenLastCalledWith(expect.objectContaining({ name: "codex/p/warm", run_id: "run-1" }), { kind: "hours", hours: 2 }, 5);
});

it("a queued choice is dropped when its predecessor's reply carries no state to continue from", async () => {
  const replies: ((value: unknown) => void)[] = [];
  native.setKeepWarm.mockImplementation(() => new Promise((resolve) => { replies.push(resolve); }));
  mount(session(warm({ enabled: true, until: Date.now() / 1000 + 10800, revision: 4 })));
  chooseAt("5"); await flush(); chooseAt("2"); await flush();
  replies[0]({ ok: true });
  await flush();
  expect(native.setKeepWarm).toHaveBeenCalledTimes(1);
  expect(host.querySelector(".kw-row")!.getAttribute("aria-busy")).toBeNull();
});
