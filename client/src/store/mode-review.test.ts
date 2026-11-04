import { describe, expect, it, vi } from "vitest";
import { createDesktopService, createModeReviewActions, serviceStatus } from "./desktopService";
import type { DesktopService, ModeReview, NotificationOwnership } from "../types";

const serviceView = (patch: Partial<DesktopService>): DesktopService => ({ connected: true, epoch: "epoch", generation: 1, background: {}, attention_counts: null, automatic_approvals: null, details_available: false, code: "background_unready", ...patch });
const original = (): ModeReview => ({ request_id: "11111111-1111-4111-8111-111111111111",
  configuration: "a".repeat(64), selection: "22222222-2222-4222-8222-222222222222",
  disposition: "required", passive_available: true });
const ownership = (review = original()): NotificationOwnership => ({ status: null, request_id: null, pending: false, running: false, code: "",
  activation: { pending: true, running: false, needs_resume: false, code: "activation_unconfirmed", review, automatic_blocked: false } });

describe("mode review is an explicit action on one displayed original", () => {
  it("publishing, retry presentation and reboot guidance invoke no action", () => {
    const run = vi.fn(), actions = createModeReviewActions(run), source = createDesktopService();
    const owner = ownership();
    for (const disposition of ["required", "reboot_required", "reviewed_unknown"] as const) {
      owner.activation!.review = { ...original(), disposition };
      owner.activation!.automatic_blocked = disposition !== "required";
      source.apply(serviceView({ connected: false, notification_ownership: owner }));
      const view = serviceStatus(source.state())!;
      expect(view.modeReview).toBe(true);
      expect(view.ownership).not.toBe("rearm");
      expect(actions.busy()).toBe(false);
    }
    expect(run).not.toHaveBeenCalled();
  });
  it("keeps the selected original during a late native confirmation and admits one call", async () => {
    let finish!: () => void;
    const run = vi.fn((_display: ModeReview, _action: string) => new Promise<void>(resolve => { finish = resolve; }));
    const actions = createModeReviewActions(run), displayed = original();
    const pending = actions.submit(displayed, "review");
    displayed.request_id = "changed while the native confirmation is open";
    await actions.submit(original(), "background");
    expect(run).toHaveBeenCalledTimes(1);
    expect(run.mock.calls[0][0]).toEqual(original());
    expect(run.mock.calls[0][1]).toBe("review");
    finish(); await pending;
    expect(actions.busy()).toBe(false);
    const choice = actions.submit({ ...original(), disposition: "reviewed_unknown" }, "background");
    expect(run).toHaveBeenCalledTimes(2);
    // A second command is a separate native confirmation, not a continuation
    // attached to Review. Its promise is deliberately finished independently.
    finish(); await choice;
  });
  it("retains the interrupted notice with a failed original check and keeps manual refresh independent", () => {
    const owner = ownership({ ...original(), disposition: "reboot_required" });
    const state = serviceView({ connected: false, epoch: null, notification_ownership: owner });
    expect(serviceStatus(state)?.modeReview).toBe(true);
    state.executable_upgrade = { state: "manual_refresh_required" };
    const view = serviceStatus(state)!;
    expect(view.modeReview).toBe(true);
    expect(view.manualRefresh).toBe(true);
    expect(view.ownership).not.toBe("rearm");
  });
  it("keeps review guidance until the native business gate is current", () => {
    const owner = ownership({ ...original(), disposition: "reviewed_unknown" });
    owner.status = { abi: "desktop-ownership-v1", configuration: "a".repeat(64), journal: original().request_id,
      revision: 1, state: "service", identity_current: true, rearm_required: false,
      request_id: original().request_id, owner_epoch: original().request_id };
    owner.activation = { ...owner.activation!, pending: false, active: true, automatic_blocked: true };
    const state = serviceView({ notification_ownership: owner });
    expect(serviceStatus(state)?.modeReview).toBe(true);
    owner.activation.business_ready = true;
    expect(serviceStatus(state)?.modeReview).toBe(false);
    owner.activation.business_ready = false;
    expect(serviceStatus(state)?.modeReview).toBe(true);
  });

});
