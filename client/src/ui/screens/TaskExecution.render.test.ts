// @vitest-environment happy-dom
import { afterEach, expect, it } from "vitest";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TaskExecution } from "./TaskExecution";
import { savedExecution, type ExecutionPhase, type SavedExecution } from "./task-execution";
import { translateWithCatalog } from "../../i18n";
import russian from "../../i18n.next-ru.json";

function projection(phase: ExecutionPhase = "submitted", current = {}) {
  return savedExecution({ requirement: 7, attempts: 3, conflict: false, has_unknown: false, has_unexplained: false,
    current: { id: "11111111-1111-4111-8111-111111111111", generation: 2, origin: "send", requirement: 3,
      request_id: "example-original", phase, at: 1234, began_at: phase === "running" ? 1200 : null, progress_at: null, legacy: false,
      executor: { machine: "example-machine", session: "codex/orchard/worker", session_id: "run_id:original", host: "studio", resolved: false }, ...current } })!;
}
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); document.body.replaceChildren(); });
function mount(execution?: SavedExecution, stale = false) {
  const host = document.createElement("div");document.body.append(host);
  dispose = render(() => createComponent(TaskExecution, { execution, stale }), host);
  return host;
}
it("renders submission separately from recorded begin and exposes exact saved identities without task controls", () => {
  const host = mount(projection());
  expect(host.querySelector(".tv-execution-phase")?.textContent).toBe("Submitted — begin unconfirmed");
  for (const value of ["11111111-1111-4111-8111-111111111111", "run_id:original", "codex/orchard/worker", "example-machine", "Executor identity unresolved", "example-original"]) expect(host.textContent).toContain(value);
  expect([...host.querySelectorAll("dd")].map((v) => v.textContent)).toEqual(expect.arrayContaining(["2", "3", "7"]));
  expect(host.querySelectorAll("time")).toHaveLength(1);
  expect(host.textContent).toContain("Live execution state and executor queue are unavailable");
  expect(host.querySelectorAll("button,a,input,textarea,select")).toHaveLength(0);
});
it("shows each saved phase as evidence even when native runtime and queue are unavailable", () => {
  for (const [phase, label] of Object.entries({ running: "Begin recorded", paused: "Pause recorded", ready: "Ready for review recorded", cancelled: "Cancellation recorded", not_started: "Not started", unknown: "Delivery unknown", unconfirmed: "Begin unconfirmed" })) {
    const host = mount(projection(phase as ExecutionPhase));
    expect(host.querySelector(".tv-execution-phase")?.textContent).toBe(label);
    expect(host.textContent).toContain("Saved evidence only");
    expect(host.textContent).not.toMatch(/\bRunning\b/);
    dispose?.(); host.remove();
  }
});
it("retains old unknown/conflict/unexplained evidence even when the saved current attempt is ready", () => {
  const execution = { ...projection("ready"), has_unknown: true, has_unexplained: true, conflict: true };
  const host = mount(execution, true);
  for (const text of ["Execution records conflict.", "Some attempt delivery is unknown.", "Some execution history is unexplained.", "could not be refreshed"]) expect(host.textContent).toContain(text);
  expect(host.querySelector(".tv-execution-phase")?.textContent).toBe("Ready for review recorded");
});
it("distinguishes unavailable, no recorded current attempt and explicit legacy evidence", () => {
  let host = mount();expect(host.textContent).toContain("Execution details unavailable.");expect(host.textContent).not.toContain("Legacy");dispose?.();host.remove();
  host = mount({ ...projection(), current: null });expect(host.textContent).toContain("No current attempt recorded.");expect(host.textContent).not.toContain("Legacy");dispose?.();host.remove();
  host = mount(projection("unconfirmed", { id: "", generation: 0, legacy: true, executor: null, at: 0 }));
  expect(host.textContent).toContain("Legacy execution record");expect(host.textContent).toContain("Begin unconfirmed");expect(host.querySelectorAll("time")).toHaveLength(0);
});
it("reactively replaces one saved attempt and marks a failed refresh without promoting it to live state", () => {
  const [execution, setExecution] = createSignal<SavedExecution | undefined>(projection());const [stale, setStale] = createSignal(false);
  const host = document.createElement("div");document.body.append(host);
  dispose = render(() => createComponent(TaskExecution, { get execution() { return execution(); }, get stale() { return stale(); } }), host);
  setExecution(projection("paused", { generation: 4, id: "22222222-2222-4222-8222-222222222222" }));setStale(true);
  expect(host.textContent).not.toContain("11111111-1111-4111-8111-111111111111");expect(host.textContent).toContain("22222222-2222-4222-8222-222222222222");
  expect(host.querySelectorAll(".tv-execution-phase")).toHaveLength(1);expect(host.textContent).toContain("Pause recorded");expect(host.textContent).toContain("could not be refreshed");
});
it("translates the saved-evidence and unavailability boundary into Russian", () => {
  const source = "Saved evidence only. Live execution state and executor queue are unavailable.";
  expect(translateWithCatalog(russian, "ru", source)).toContain("недоступны");
  expect(translateWithCatalog(russian, "ru", "Submitted — begin unconfirmed")).toContain("не подтверждено");
});
