import { describe, expect, it } from "vitest";
import { servicePaused, serviceStatus } from "../../store/desktopService";
import type { DesktopService, NotificationOwnership } from "../../types";

const healthy = (patch: Partial<DesktopService> = {}): DesktopService => ({ connected: true, epoch: "fixture", generation: 1,
  background: Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((key) => [key, { ready: true, code: "ready" }])),
  attention_counts: null, automatic_approvals: false, details_available: true, code: "", ...patch });
const rearm: NotificationOwnership = { pending: false, running: false, code: "", request_id: null,
  status: { abi: "fixture", configuration: "fixture", journal: null, revision: 1, state: "revoked", rearm_required: true, request_id: null, owner_epoch: null, identity_current: true } };

const notStarted = Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((key) => [key, { ready: false, code: "not_started" }]));
const activation = { pending: false, running: false, needs_resume: true, code: "activation_required" };
const passive = (patch: Partial<DesktopService> = {}, ownership: Partial<NotificationOwnership> = {}): DesktopService => healthy({
  background: notStarted, code: "background_unready", details_available: false,
  notification_ownership: { ...rearm, status: { ...rearm.status!, state: "service", rearm_required: false }, activation, ...ownership }, ...patch });

describe("passive service", () => {
  it("is paused only while connected, with no background work started and Resume offered", () => {
    expect(servicePaused(undefined)).toBe(false);
    expect(servicePaused(passive())).toBe(true);
    expect(servicePaused(passive({ code: "explicit_handoff_required" }))).toBe(true);
  });
  it.each<[string, DesktopService]>([
    ["disconnected", passive({ connected: false })],
    ["reconnecting", passive({ connected: false, reconnecting: true })],
    ["before the first status", { ...passive(), connected: false, background: {}, code: "unavailable" }],
    ["one component started", passive({ background: { ...notStarted, fleet: { ready: false, code: "warming" } } })],
    ["ready", healthy({ notification_ownership: passive().notification_ownership })],
    ["a missing component", passive({ background: { fleet: { ready: false, code: "not_started" } } })],
    ["manual refresh", passive({ executable_upgrade: { state: "manual_refresh_required" } })],
    ["an unconfirmed ownership change", passive({}, { pending: true })],
    ["an unconfirmed activation", passive({}, { activation: { ...activation, pending: true, code: "activation_unconfirmed" } })],
    ["an unavailable helper", passive({}, { activation: { ...activation, code: "activation_helper_unavailable" } })],
    ["no activation offer", passive({}, { activation: undefined })],
    ["a settled activation", passive({}, { activation: { ...activation, needs_resume: false, code: "" } })],
  ])("is not paused when %s", (_, state) => {
    expect(servicePaused(state)).toBe(false);
  });
  it("replaces the skeletons, Connecting and the empty detail only while paused without computers", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const read = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8");
    const component = read("./DesktopServiceStatus.tsx");
    expect(component).toContain("servicePaused(desktopService.state()) && !Object.keys(catalog.computers).length");
    const detail = component.match(/export function PausedServiceDetail[\s\S]*?\n}\n/)?.[0] ?? "";
    expect(detail).toContain('tr("Background work is paused")');
    expect(detail).toContain('tr("Zerus runs your sessions through a background service, which stays paused until you resume it.")');
    expect(detail).toMatch(/class="btn primary" disabled=\{resumeBlocked\(\)\} onClick=\{resume\}>\{tr\("Resume background work"\)\}/);
    expect(component).toContain("const resume = () => void ownershipActions.rearm()");
    expect(component).toContain("ownershipActions.busy() || !!desktopService.state()?.notification_ownership?.running");
    expect(read("../sessions/SessionDetail.tsx")).toMatch(/function EmptyDetail\(\) \{\s*return \(\s*<Show when=\{!servicePausedEmpty\(\)\} fallback=\{<PausedServiceDetail \/>\}>/);
    const list = read("../sessions/SessionList.tsx").match(/function ListPlaceholder\(\)[\s\S]*?\n}\n/)?.[0] ?? "";
    expect(list.indexOf("<Show when={!servicePausedEmpty()}")).toBeGreaterThan(-1);
    expect(list.indexOf("<Show when={!servicePausedEmpty()}")).toBeLessThan(list.indexOf("skel-card"));
    expect(read("../../App.tsx")).toContain('fallback={<Show when={!servicePaused(desktopService.state())}><span>{props.mode() === "starting" ? tr("Starting…") : tr("Connecting…")}</span></Show>}');
  });
});

describe("service status in the footer", () => {
  it("keeps the healthy indicator compact and offers no Retry", () => {
    expect(serviceStatus(undefined)).toBeUndefined();
    expect(serviceStatus(healthy())).toMatchObject({ healthy: true, retry: false, review: false, ownership: undefined, label: "Background service", details: "Background service connected" });
  });
  it.each([
    { connected: false },
    { connected: false, reconnecting: true },
    { code: "explicit_handoff_required" },
    { code: "background_unready" },
    { background: { fleet: { ready: false, code: "warming" } } },
    { details_available: false },
  ])("offers Retry for a connection or readiness problem: %j", (patch) => {
    expect(serviceStatus(healthy(patch))).toMatchObject({ healthy: false, retry: true });
  });
  it("keeps Resume and uncertain-result Check separate from Retry", () => {
    expect(serviceStatus(healthy({ notification_ownership: rearm }))).toMatchObject({ healthy: false, retry: false, ownership: "rearm" });
    expect(serviceStatus(healthy({ notification_ownership: { ...rearm, pending: true } }))).toMatchObject({ healthy: false, retry: false, ownership: "check" });
    expect(serviceStatus(healthy({ code: "background_unready", notification_ownership: rearm }))).toMatchObject({ retry: true, ownership: "rearm" });
  });
  it.each(["manual_refresh_required", "service_unavailable", "original_service_changed"])("keeps Windows manual refresh visible after revoke and %s without offering Resume or Retry", (code) => {
    const manual = healthy({ connected: false, code,
      executable_upgrade: { state: "manual_refresh_required" }, notification_ownership: rearm });
    expect(serviceStatus(manual)).toMatchObject({ healthy: false, manualRefresh: true,
      label: "Service refresh required", retry: false, ownership: undefined });
    expect(serviceStatus(manual)?.details).toContain("Settings > About");
    expect(serviceStatus(manual)?.details).not.toContain("Resume");
    expect(serviceStatus({ ...manual, notification_ownership: { ...rearm, pending: true } })).toMatchObject({ ownership: "check", retry: false });
    expect(serviceStatus(healthy({ executable_upgrade: { state: "draining" } }))?.manualRefresh).toBe(false);
    expect(serviceStatus(healthy())?.manualRefresh).toBe(false);
  });
  it("offers explicit recovery even when a failed native start has no revocation marker", () => {
    const recovery = { ...rearm, native_recovery: true, status: { ...rearm.status!, state: "passive" as const, rearm_required: false } };
    expect(serviceStatus(healthy({ notification_ownership: recovery }))).toMatchObject({ healthy: false, ownership: "rearm" });
    expect(serviceStatus(healthy({ notification_ownership: { ...recovery, pending: true } }))).toMatchObject({ ownership: "check" });
  });
  it("keeps report review and saved-read recovery visible together", () => {
    const status = serviceStatus(healthy({ retained_source_import: { required: true, complete: false, running: false, code: "retained_import_qt_unavailable", phase: null },
      pending_read_import: { required: true, complete: false, running: false, through: 0, superseded: 0, delivered: false, code: "pending" } }));
    expect(status).toMatchObject({ healthy: false, retry: true, review: true });
    expect(status?.details).toContain("Saved read marks");
    expect(status?.details).toContain("migration report needs review");
  });
  it("presents a passive service as paused work with Resume, not as a fault to Retry", () => {
    const status = serviceStatus(passive());
    expect(status).toMatchObject({ healthy: false, retry: false, ownership: "rearm", label: "Background access paused" });
    expect(status?.details).toBe("Background work is paused. Resume to activate the service explicitly.");
    // A durable handoff block keeps its own label and Retry.
    expect(serviceStatus(passive({ code: "explicit_handoff_required" }))).toMatchObject({ label: "Background handoff required", retry: true, ownership: "rearm" });
  });
  it("inserts status only in the fixed-height footer, never above the page", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const app = fs.readFileSync(new URL("../../App.tsx", import.meta.url), "utf8");
    expect(app.match(/<DesktopServiceStatus\b/g)).toHaveLength(1);
    expect(app.match(/<footer\b[\s\S]*?<\/footer>/)?.[0]).toContain("<DesktopServiceStatus");
    expect(app.match(/<main\b[\s\S]*?<\/main>/)?.[0]).not.toContain("DesktopServiceStatus");
    expect(app).not.toContain("with-service-status");
    const css = fs.readFileSync(new URL("../styles/app.css", import.meta.url), "utf8");
    expect(css.match(/\.statusbar\s*\{([^}]+)\}/)?.[1]).toContain("height: 28px");
    expect(css).not.toContain(".page.with-service-status");
  });
});
