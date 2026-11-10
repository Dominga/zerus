import { describe, expect, it, vi } from "vitest";
import { createDesktopService, createServiceApprovalIntent, pendingReadNotice, ownershipNotice, takeoverRefusal, createOwnershipActions, serviceStatus } from "./desktopService";
import { createApprovalOptIn } from "../ui/screens/settings/approval-opt-in";
import { emptyBatch, isEmptyBatch, type DesktopService, type NotificationOwnership } from "../types";
import { normalize } from "../bridge";

export const serviceView = (patch: Partial<DesktopService> = {}): DesktopService => ({
  connected: true, epoch: "epoch", generation: 1, background: {}, attention_counts: null,
  automatic_approvals: null, details_available: false, code: "background_unready", ...patch,
});
describe("desktop service presentation", () => {
  it("shows blocked read transfer and its existing Retry path until durable completion", () => {
    expect(pendingReadNotice(undefined)).toBe("");
    const pending = { required: true, complete: false, running: true, code: "pending_read_import_waiting", through: 0, superseded: 0, delivered: false as const };
    expect(pendingReadNotice(serviceView({ pending_read_import: pending }))).toContain("Transferring saved read marks");
    expect(pendingReadNotice(serviceView({ pending_read_import: { ...pending, running: false } }))).toContain("Retry to continue");
    expect(pendingReadNotice(serviceView({ pending_read_import: { ...pending, running: false, complete: true, superseded: 1 } }))).toBe("");
  });
  it("is off until the native startup mode publishes a projection; IPC alone enables no component", () => {
    const service = createDesktopService();
    expect(service.enabled()).toBe(false);
    expect(service.ready("reads")).toBe(false);
    service.apply(serviceView());
    expect(service.enabled()).toBe(true);
    expect(service.ready("reads")).toBe(false);
    service.apply(serviceView({ background: { reads: { ready: true, code: "ready" } } }));
    expect(service.ready("reads")).toBe(true);
    expect(service.ready("approvals")).toBe(false);
    service.apply(serviceView({ connected: false }));
    expect(service.enabled()).toBe(true);
    expect(service.ready("reads")).toBe(false);
  });
  it("delivers service-only batches and preserves incomplete/null count provenance", () => {
    const projection = serviceView({ attention_counts: { epoch: "epoch", scope: "shared_host_state", display_only: true, authority: false, local_marks_included: false,
      aggregate: { complete: false, attention: null, total: null, known_attention: 0, known_total: 3 }, computers: [] } });
    const batch = normalize({ ...emptyBatch(), desktop_service: projection });
    expect(isEmptyBatch(batch)).toBe(false);
    expect(batch.desktop_service).toEqual(projection);
    expect(batch.desktop_service?.attention_counts?.aggregate.attention).toBeNull();
  });
});
describe("service approval intents", () => {
  const origin = {} as HTMLElement;
  it("uses the existing enable confirmation before its native seam and never writes a setting", async () => {
    const send = vi.fn(async (request_id: string, enabled: boolean) => ({ request_id, enabled, op: "automatic_approvals", accepted: true, durable: true }));
    const intent = createServiceApprovalIntent(send, () => "request-1");
    const confirmation = createApprovalOptIn((_key, enabled) => intent.submit(enabled), vi.fn());
    confirmation.request(true, origin);
    expect(send).not.toHaveBeenCalled();
    confirmation.cancel(); await confirmation.confirm();
    expect(send).not.toHaveBeenCalled();
    confirmation.request(true, origin); await confirmation.confirm();
    expect(send).toHaveBeenCalledExactlyOnceWith("request-1", true);
  });
  it("retains an uncertain UUID and refuses a replay, including another confirm", async () => {
    const send = vi.fn(async () => { throw Error("transport closed"); });
    const intent = createServiceApprovalIntent(send, () => "request-1");
    await expect(intent.submit(true)).rejects.toThrow("transport closed");
    expect(intent.pending()).toEqual({ id: "request-1", enabled: true });
    await expect(intent.submit(true)).rejects.toThrow("unconfirmed");
    await expect(intent.submit(false)).rejects.toThrow("unconfirmed");
    expect(send).toHaveBeenCalledOnce();
  });
  it("rejects non-durable or mismatched receipts and permits a fresh request after a known refusal", async () => {
    for (const reply of [{ request_id: "request-1", accepted: true, durable: false }, { request_id: "other", accepted: true, durable: true }]) {
      const intent = createServiceApprovalIntent(async () => reply, () => "request-1");
      await expect(intent.submit(true)).rejects.toThrow("not confirmed");
      expect(intent.pending()?.id).toBe("request-1");
    }
    const send = vi.fn(async (request_id: string) => ({ request_id, op: "automatic_approvals", accepted: false, durable: false }));
    const intent = createServiceApprovalIntent(send, () => "request-1");
    await expect(intent.submit(true)).rejects.toThrow("unavailable");
    expect(intent.pending()).toBeUndefined();
  });
  it("checks the original receipt without sending again and restores uncertainty after a page reload", async () => {
    const send = vi.fn(async () => { throw Error("lost response"); });
    const read = vi.fn(async () => ({ request_id: "request-1", op: "automatic_approvals", enabled: true, accepted: true, durable: true, pending: false }));
    const intent = createServiceApprovalIntent(send, () => "request-1", read);
    await expect(intent.submit(true)).rejects.toThrow("lost response");
    await intent.check();
    expect(send).toHaveBeenCalledOnce();
    expect(read).toHaveBeenCalledOnce();
    expect(intent.pending()).toBeUndefined();
    const missing = vi.fn(async () => ({ request_id: "native-original", op: "automatic_approvals", enabled: false, accepted: null, durable: false, pending: true }));
    const reloaded = createServiceApprovalIntent(send, () => "fresh-id", missing);
    await expect(reloaded.check()).rejects.toThrow("not confirmed");
    expect(reloaded.pending()).toEqual({ id: "native-original", enabled: false });
    await expect(reloaded.submit(true)).rejects.toThrow("unconfirmed");
    expect(send).toHaveBeenCalledOnce();
  });
  it("allows turning approvals off after native confirmed-exit receipt lookup proves the first request was not applied", async () => {
    let sequence = 0;
    const send = vi.fn(async (request_id: string, enabled: boolean) => {
      if (request_id === "request-1") throw Error("service restarting");
      return { request_id, enabled, op: "automatic_approvals", accepted: true, durable: true };
    });
    const read = vi.fn(async () => ({ request_id: "request-1", op: "automatic_approvals", accepted: false, durable: false, pending: false, code: "frontend_request_not_applied" }));
    const intent = createServiceApprovalIntent(send, () => `request-${++sequence}`, read);
    await expect(intent.submit(true)).rejects.toThrow("service restarting");
    await expect(intent.submit(false)).rejects.toThrow("unconfirmed");
    await expect(intent.check()).rejects.toThrow("The previous change was not applied.");
    expect(intent.pending()).toBeUndefined();
    await intent.submit(false);
    expect(send.mock.calls).toEqual([["request-1", true], ["request-2", false]]);
    expect(read).toHaveBeenCalledOnce();
  });
  it("sends a complete category set with the committed switch and settles only on its exact receipt", async () => {
    const ids = ["read", "edit", "other"];
    const send = vi.fn(async (request_id: string, enabled: boolean, categories?: string[]) =>
      ({ request_id, enabled, categories, op: "automatic_approvals", accepted: true, durable: true }));
    const intent = createServiceApprovalIntent(send, () => "request-1");
    await intent.submitCategories(true, ids);
    expect(send).toHaveBeenCalledExactlyOnceWith("request-1", true, ids);
    expect(intent.pending()).toBeUndefined();
    const other = createServiceApprovalIntent(async (request_id, enabled) =>
      ({ request_id, enabled, categories: ["read"], op: "automatic_approvals", accepted: true, durable: true }), () => "request-2");
    await expect(other.submitCategories(true, ids)).rejects.toThrow("not confirmed");
    expect(other.pending()).toEqual({ id: "request-2", enabled: true, categories: ids });
    await expect(other.submitCategories(true, ids)).rejects.toThrow("unconfirmed");
  });
  it("settles a category change refused for a switch changed elsewhere, so the next edit may go", async () => {
    const send = vi.fn(async (request_id: string) => ({ request_id, op: "automatic_approvals", accepted: false, durable: false, pending: false, code: "approval_policy_changed" }));
    const intent = createServiceApprovalIntent(send, () => "request-1");
    await expect(intent.submitCategories(true, ["read"])).rejects.toThrow("turned on or off elsewhere");
    expect(intent.pending()).toBeUndefined();
  });
  it("restores an uncertain category change after a reload and confirms it by its exact set", async () => {
    const send = vi.fn();
    const read = vi.fn(async () => ({ request_id: "native-original", op: "automatic_approvals", enabled: false,
      categories: ["read", "paid"], accepted: true, durable: true, pending: false }));
    const reloaded = createServiceApprovalIntent(send, () => "fresh-id", read);
    await reloaded.check();
    expect(reloaded.pending()).toBeUndefined();
    const pending = vi.fn(async () => ({ request_id: "native-original", op: "automatic_approvals", enabled: false,
      categories: ["read", "paid"], accepted: null, durable: false, pending: true }));
    const waiting = createServiceApprovalIntent(send, () => "fresh-id", pending);
    await expect(waiting.check()).rejects.toThrow("not confirmed");
    expect(waiting.pending()).toEqual({ id: "native-original", enabled: false, categories: ["read", "paid"] });
    expect(send).not.toHaveBeenCalled();
  });

});


describe("notification ownership presentation", () => {
  const view = (patch: Partial<NotificationOwnership> = {}): NotificationOwnership => ({
    status: { abi: "desktop-ownership-v1", configuration: "a".repeat(64), journal: "journal", revision: 3,
      state: "revoked", rearm_required: true, request_id: null, owner_epoch: null, identity_current: false },
    request_id: null, pending: false, running: false, code: "", ...patch,
  });
  it("shows explicit rearm after revoke and keeps unknown intents on Check result", () => {
    expect(ownershipNotice(view())).toContain("revoked");
    expect(ownershipNotice(view({ pending: true }))).toContain("unconfirmed");
    expect(ownershipNotice(undefined)).toBe("");
    const updated = view(); updated.status = { ...updated.status!, state: "identity_changed", rearm_required: false };
    expect(ownershipNotice(updated)).toContain("unavailable");
    expect(ownershipNotice(updated)).not.toContain("revoked");
    const current = view(); current.status = { ...current.status!, state: "service", rearm_required: false, identity_current: true };
    expect(ownershipNotice(current)).toBe("");
  });
  // Task 180 (owner, 2026-11-07, local build 152): a refused native takeover
  // names its check and the next step, never a bare "not ready".
  it("names the check that refused the native takeover and the next step", () => {
    const refused = (code: string, check: string) => view({ native_recovery: true, native_recovery_reason: { code, check } });
    const binding = takeoverRefusal(refused("frontend_binding_unavailable", "binding"))!;
    expect(binding.check).toBe("App binding");
    expect(binding.reason).toContain("frontend.json");
    expect(binding.next).toContain("Install Zerus again");
    expect(takeoverRefusal(refused("frontend_folders_not_private", "folder_privacy"))!.check).toBe("Folder privacy");
    expect(takeoverRefusal(refused("agent_descended_frontend_refused", "ancestry"))!.check).toBe("Process ancestry");
    expect(takeoverRefusal(refused("ownership_not_settled", "ownership_journal"))!.check).toBe("Ownership journal");
    expect(takeoverRefusal(refused("timed_out", "other"))!.reason).toContain("timed_out");
    // Task #421: a takeover whose hgs child failed names the desktop service and a working next step.
    const child = takeoverRefusal(refused("child_failed", "other"))!;
    expect(child.check).toBe("Desktop service");
    expect(child.next).toContain("hgs desktop serve");
    expect(ownershipNotice(refused("child_failed", "other"))).toContain("Zerus could not take over its sessions (Desktop service)");
    // Task #440: a planned self-restart is said ahead of the refusal.
    const restarting = view({ native_recovery: true, native_recovery_reason: { code: "child_failed", check: "other" }, native_recovery_restart_at: 1_800_000_000_000 });
    expect(ownershipNotice(restarting)).toBe("Zerus restarts its window to finish starting. Your drafts are saved.");
    // The notice says it instead of a bare "not ready"; without a reason the old notice stays.
    const notice = ownershipNotice(refused("frontend_binding_unavailable", "binding"));
    expect(notice).toContain("Zerus could not take over its sessions (App binding)");
    expect(notice).not.toContain("not ready");
    // Task #53 (owner, 2026-11-10, build 175): a missing binding the install
    // could not record says why and the one step that fixes it.
    const started = takeoverRefusal(refused("frontend_parent_not_beside", "ancestry"))!;
    expect(started.check).toBe("Process ancestry");
    expect(started.reason).toContain("started by another program");
    expect(started.next).toBe("Close Zerus and start it from the Start menu or Explorer.");
    expect(ownershipNotice(refused("frontend_parent_not_beside", "ancestry"))).toContain("Start menu or Explorer");
    expect(takeoverRefusal(refused("frontend_record_timed_out", "binding"))!.reason).toContain("took too long");
    for (const code of ["frontend_parent_gone", "frontend_ancestry_unverified", "frontend_cli_pin_changed", "frontend_cli_image_unreadable", "frontend_app_unreadable", "frontend_record_refused"])
      expect(takeoverRefusal(refused(code, "binding"))!.reason, code).not.toContain(code);
    expect(takeoverRefusal(view({ native_recovery: true }))).toBeUndefined();
    expect(ownershipNotice(view({ native_recovery: true, status: undefined }))).toContain("not ready");
    expect(takeoverRefusal(view({ native_recovery: false, native_recovery_reason: { code: "frontend_binding_unavailable", check: "binding" } }))).toBeUndefined();
  });
  it("offers explicit activation for retained passive mode and keeps unknown activation on Check", () => {
    const current = view(); current.status = { ...current.status!, state: "service", rearm_required: false, identity_current: true };
    current.activation = { pending: false, running: false, needs_resume: true, code: "activation_required" };
    expect(ownershipNotice(current)).toContain("explicitly");
    expect(ownershipNotice({ ...current, pending: true })).toContain("Background access is unconfirmed");
    const status = { connected: true, code: "explicit_handoff_required", background: {}, notification_ownership: current } as DesktopService;
    expect(serviceStatus(status)?.ownership).toBe("rearm");
    current.activation = { pending: true, running: false, needs_resume: true, code: "activation_unconfirmed" };
    expect(ownershipNotice(current)).toContain("original result");
    expect(serviceStatus(status)?.ownership).toBe("check");
  });
  it("mount and result lookups never grant or rearm; only the explicit action calls its native route", async () => {
    const send = vi.fn(async () => view());
    const read = vi.fn(async () => view({ pending: true }));
    const actions = createOwnershipActions(send, read);
    expect(send).not.toHaveBeenCalled();
    expect(read).not.toHaveBeenCalled();
    await actions.check(); await actions.check();
    expect(read).toHaveBeenCalledTimes(2);
    expect(send).not.toHaveBeenCalled();
    await actions.rearm();
    expect(send).toHaveBeenCalledExactlyOnceWith();
  });
  it("serializes explicit clicks while retaining a failed result as native-owned uncertainty", async () => {
    let release!: () => void;
    const send = vi.fn(() => new Promise<NotificationOwnership>(resolve => { release = () => resolve(view({ pending: true })); }));
    const read = vi.fn(async () => { throw Error("disconnected"); });
    const actions = createOwnershipActions(send, read);
    const first = actions.rearm();
    expect(actions.busy()).toBe(true);
    await actions.rearm(); await actions.check();
    expect(send).toHaveBeenCalledOnce(); expect(read).not.toHaveBeenCalled();
    release(); await first;
    await expect(actions.check()).rejects.toThrow("disconnected");
    expect(send).toHaveBeenCalledOnce();
    expect(actions.busy()).toBe(false);
  });
});
