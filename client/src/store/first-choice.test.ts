import { describe, expect, it, vi } from "vitest";
// Only the first-choice command answers; nothing here reaches a native side.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) =>
    command === "desktop_service_first_choice" ? { request_id: "22222222-2222-4222-8222-222222222222" } : null),
}));
import { firstChoiceActions, firstChoicePending, serviceStatus } from "./desktopService";
import type { DesktopService } from "../types";
import windowsPassive from "../../fixtures/desktop-service-windows-passive.json";

const REQUEST = "22222222-2222-4222-8222-222222222222";
const OFFER = { configuration: "a".repeat(64), selection: "11111111-1111-4111-8111-111111111111" };
const HINT = "The new mode choice is unconfirmed. Check its original result; do not make another choice.";
function inFlight(patch: (state: DesktopService) => void = () => {}): DesktopService {
  const state = structuredClone(windowsPassive) as unknown as DesktopService;
  Object.assign(state.notification_ownership!.activation!, { first: null, pending: true, request_id: REQUEST, code: "activation_unconfirmed" });
  state.connected = false;
  patch(state);
  return state;
}

describe("a first background choice in flight keeps independent reasons", async () => {
  await firstChoiceActions.submit(OFFER);
  it("records its own original ID", () => {
    expect(firstChoiceActions.request()).toBe(REQUEST);
    expect(firstChoicePending(inFlight())).toBe(true);
  });
  it("keeps a distinct unconfirmed access request", () => {
    const status = serviceStatus(inFlight((state) => { state.notification_ownership!.pending = true; }));
    expect(status).toMatchObject({ label: "Turning on background mode", ownership: "check", retry: false });
    expect(status?.details).toContain("Background access is unconfirmed");
  });
  it("keeps the held message-copy notice, with its Review only while the service is ready", () => {
    const notReady = serviceStatus(inFlight((state) => {
      state.connected = true;
      state.task_message_inputs = { review: 2, full: true } as DesktopService["task_message_inputs"];
    }));
    expect(notReady).toMatchObject({ label: "Turning on background mode", ownership: "check", retry: false, inputsReview: false });
    expect(notReady?.details).toContain("Large task messages are paused: some held message copies need review.");
    const ready = serviceStatus(inFlight((state) => {
      state.connected = true;
      state.code = "";
      for (const name of Object.keys(state.background)) state.background[name] = { ready: true, code: "running" };
      state.task_message_inputs = { review: 2, full: true } as DesktopService["task_message_inputs"];
    }));
    expect(ready).toMatchObject({ label: "Turning on background mode", ownership: "check", retry: false, inputsReview: true });
  });
  it("lets manual-refresh guidance lead while the original lookup stays available", () => {
    const state = inFlight((s) => { s.executable_upgrade = { state: "manual_refresh_required" }; });
    expect(firstChoicePending(state)).toBe(false);
    expect(serviceStatus(state)).toMatchObject({ label: "Service refresh required", manualRefresh: true, ownership: "check" });
  });
});
