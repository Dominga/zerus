import { describe, expect, it } from "vitest";
import { createFirstChoiceActions, firstChoicePending, pausedSettingsBody, servicePaused, serviceStatus } from "../../store/desktopService";
import type { DesktopService, NotificationOwnership } from "../../types";
// Produced by the native passive_projection_tests from the root's passive status
// and the platform-manual activation view (Windows); never hand-written here.
import windowsPassive from "../../../fixtures/desktop-service-windows-passive.json";

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

const SETTINGS_BODY = "Zerus runs your sessions through a background service, which is paused on this computer. Settings shows its current state.";
const windows = (patch: Partial<DesktopService> = {}): DesktopService => ({ ...(structuredClone(windowsPassive) as unknown as DesktopService), ...patch });
const windowsOwnership = (patch: (view: NotificationOwnership) => NotificationOwnership) => windows({ notification_ownership: patch(windows().notification_ownership!) });

describe("passive service", () => {
  it("is paused only while connected, with no background work started and Resume offered", () => {
    expect(servicePaused(undefined)).toBeUndefined();
    expect(servicePaused(passive())).toBe("resume");
    expect(servicePaused(passive({ code: "explicit_handoff_required" }))).toBe("resume");
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
    expect(servicePaused(state)).toBeUndefined();
  });
  it("names why the activation helper was refused beside its code (task #59)", () => {
    const words = "C:\\Zerus\\hgs.exe could not be pinned: The process cannot access the file because it is being used by another process. (os error 32)";
    const status = serviceStatus(passive({}, { activation: { ...activation, code: "activation_helper_unavailable", reason: words } }));
    expect(status?.details).toContain("Background activation is unavailable.");
    expect(status?.details).toContain(`Why: ${words}`);
    const bare = serviceStatus(passive({}, { activation: { ...activation, code: "activation_helper_unavailable" } }));
    expect(bare?.details).not.toContain("Why:");
  });
  it("replaces the skeletons, Connecting and the empty detail only while paused without computers", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const read = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
    const component = read("./DesktopServiceStatus.tsx");
    expect(component).toContain("(!!servicePaused(desktopService.state()) || firstChoicePending(desktopService.state()))\n  && !Object.keys(catalog.computers).length");
    const detail = component.match(/export function PausedServiceDetail[\s\S]*?\n}\n/)?.[0] ?? "";
    expect(detail).toContain('tr("Background work is paused")');
    expect(detail).toContain('servicePaused(desktopService.state()) === "settings"');
    expect(detail).toContain('tr("Zerus runs your sessions through a background service, which stays paused until you resume it.")');
    expect(detail).toMatch(/class="btn primary" disabled=\{resumeBlocked\(\)\} onClick=\{resume\}>\{tr\("Resume background work"\)\}/);
    expect(detail).toContain("<p>{pausedSettingsBody(desktopService.state())}</p>");
    expect(detail).toMatch(/class="btn primary" onClick=\{openServiceSettings\}>\{tr\("Open background service settings"\)\}/);
    expect(component).toContain('const openServiceSettings = () => void import("../screens/settings/sections").then(({ openSettings }) => openSettings("about"));');
    expect(component).toContain("const resume = () => void ownershipActions.rearm()");
    expect(component).toContain("ownershipActions.busy() || !!desktopService.state()?.notification_ownership?.running");
    expect(read("../sessions/SessionDetail.tsx")).toMatch(/function EmptyDetail\(\) \{\s*return \(\s*<Show when=\{!servicePausedEmpty\(\)\} fallback=\{<PausedServiceDetail \/>\}>/);
    const list = read("../sessions/SessionList.tsx").match(/function ListPlaceholder\(\)[\s\S]*?\n}\n/)?.[0] ?? "";
    expect(list.indexOf("<Show when={!servicePausedEmpty()}")).toBeGreaterThan(-1);
    expect(list.indexOf("<Show when={!servicePausedEmpty()}")).toBeLessThan(list.indexOf("skel-card"));
    expect(read("./MachinesStatus.tsx")).toContain('fallback={<Show when={!servicePaused(desktopService.state()) && !firstChoicePending(desktopService.state())}><span>{props.mode() === "starting" ? tr("Starting…") : tr("Connecting…")}</span></Show>}');
  });
});

// Task 176 (owner, 2026-11-07): an inherited sandbox read on the session state
// folder left an endless skeleton. A refusal the repair did not fix is shown.
const REFUSAL = "desktop folders must be private directories: C:\\Users\\me\\.local\\state\\hgs\\sessions: it grants MSI\\CodexSandboxUsers (S-1-5-21-1-2-3-1009) read and execute (RX), mask 0x001200A9, inherited from its parent folder. Repair: starting the background service (or hgs desktop prepare-state --json) makes a folder you own private in place; a folder another account owns must be fixed by its owner";

describe("refused session state folder", () => {
  it("heads the footer with hgs's own words and offers Retry", () => {
    const state = healthy({ connected: false, background: {}, code: "state_folder_not_private", state_refusal: REFUSAL });
    const view = serviceStatus(state)!;
    expect(view.label).toBe("Session state folder refused");
    expect(view.details).toContain(REFUSAL);
    expect(view).toMatchObject({ healthy: false, retry: true });
    // Without a refusal the same disconnection reads as before.
    expect(serviceStatus(healthy({ connected: false, code: "service_unavailable" }))!.label).toBe("Background service unavailable");
  });
  it("replaces the skeleton rows with the refusal, its repair and Retry", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const read = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
    const list = read("../sessions/SessionList.tsx").match(/function ListPlaceholder\(\)[\s\S]*?\n}\n/)?.[0] ?? "";
    const refused = list.indexOf("<Show when={!stateRefusal()} fallback={<StateRefusalDetail />}>");
    expect(refused).toBeGreaterThan(-1);
    expect(refused).toBeLessThan(list.indexOf("skel-card"));
    expect(refused).toBeLessThan(list.indexOf("<Show when={!servicePausedEmpty()}"));
    const component = read("./DesktopServiceStatus.tsx");
    expect(component).toContain('export const stateRefusal = () => desktopService.state()?.state_refusal || "";');
    const detail = component.match(/export function StateRefusalDetail[\s\S]*?\n}\n/)?.[0] ?? "";
    expect(detail).toContain('role="alert"');
    expect(detail).toContain("{stateRefusal()}");
    expect(detail).toContain('tr("Sessions cannot load")');
    expect(detail).toContain("retryDesktopService()");
    expect(detail).not.toContain("skeleton");
  });
});

describe("Windows passive service (the actual native projection)", () => {
  it("has no native activation route and offers no Resume", () => {
    expect(windows().notification_ownership?.activation).toEqual({ pending: false, running: false, needs_resume: false, code: "activation_platform_manual",
      review: null, automatic_blocked: false, request_id: null, active: false, business_ready: true });
    expect(windows().code).toBe("background_unready");
  });
  it("is paused with the Settings action, and the footer offers neither Retry nor Resume", () => {
    expect(servicePaused(windows())).toBe("settings");
    expect(serviceStatus(windows())).toEqual({ label: "Background access paused", details: SETTINGS_BODY, healthy: false,
      retry: false, inputsReview: false, ownership: undefined, manualRefresh: false, modeReview: false });
  });
  it.each<[string, DesktopService]>([
    ["disconnected", windows({ connected: false })],
    ["one component started", windows({ background: { ...windows().background, fleet: { ready: false, code: "warming" } } })],
    ["ready", windows({ background: Object.fromEntries(Object.keys(windows().background).map((key) => [key, { ready: true, code: "running" }])), code: "" })],
    ["manual refresh", windows({ executable_upgrade: { state: "manual_refresh_required" } })],
    ["an unconfirmed ownership change", windowsOwnership((view) => ({ ...view, pending: true }))],
    ["revoked background access", windowsOwnership((view) => ({ ...view, status: { ...view.status!, state: "revoked", rearm_required: true } }))],
    ["native recovery", windowsOwnership((view) => ({ ...view, native_recovery: true }))],
    ["a changed frontend identity", windowsOwnership((view) => ({ ...view, status: { ...view.status!, state: "identity_changed", identity_current: false } }))],
    ["an unknown activation code", windowsOwnership((view) => ({ ...view, activation: { ...view.activation!, code: "activation_status_unavailable" } }))],
  ])("is not paused when %s", (_, state) => {
    expect(servicePaused(state)).toBeUndefined();
    expect(serviceStatus(state)?.details).not.toContain(SETTINGS_BODY);
  });
  it("keeps the existing recovery actions when ownership itself needs one", () => {
    const revoked = windowsOwnership((view) => ({ ...view, status: { ...view.status!, state: "revoked", rearm_required: true } }));
    expect(serviceStatus(revoked)).toMatchObject({ ownership: "rearm" });
    expect(serviceStatus(windows({ connected: false }))).toMatchObject({ retry: true, label: "Background service unavailable" });
  });
});

const TURN_ON_BODY = "Zerus runs your sessions through a background service, which stays paused until you turn it on in Settings.";
const OFFER = { configuration: "a".repeat(64), selection: "11111111-1111-4111-8111-111111111111" };
const offered = () => windowsOwnership((view) => ({ ...view, activation: { ...view.activation!, first: OFFER } }));

describe("Windows first background choice (zerus-yfbd.1)", () => {
  it("promises turning background mode on only while the choice is offered", () => {
    expect(pausedSettingsBody(windows())).toBe(SETTINGS_BODY);
    expect(pausedSettingsBody(offered())).toBe(TURN_ON_BODY);
    expect(servicePaused(offered())).toBe("settings");
    expect(serviceStatus(offered())?.details).toBe(TURN_ON_BODY);
    expect(serviceStatus(offered())).toMatchObject({ retry: false, ownership: undefined, label: "Background access paused" });
  });
  it("submits one confirmation at a time and keeps only its own original ID", async () => {
    let release: (value: { request_id: string }) => void = () => {};
    const calls: unknown[] = [];
    const actions = createFirstChoiceActions((display) => { calls.push(display); return new Promise((resolve) => { release = resolve; }); });
    const first = actions.submit(OFFER);
    expect(actions.busy()).toBe(true);
    expect(await actions.submit(OFFER)).toBeUndefined();
    release({ request_id: "22222222-2222-4222-8222-222222222222" });
    await first;
    expect(calls).toEqual([OFFER]);
    expect(actions.busy()).toBe(false);
    expect(actions.request()).toBe("22222222-2222-4222-8222-222222222222");
    const cancelled = createFirstChoiceActions(async () => null);
    await cancelled.submit(OFFER);
    expect(cancelled.request()).toBeUndefined();
  });
  it("shows the Settings group only for a healthy offer, and B2's interrupted group takes precedence", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const read = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
    const group = read("../screens/settings/ServiceFirstChoice.tsx");
    expect(group).toContain('(!manual() && activation()?.first && !ownershipNotice(ownership())) ? activation()!.first! : undefined');
    expect(group).toContain("const visible = () => !activation()?.review && (!!offer() || pending() || notStarted());");
    expect(group).toContain("const own = () => { const id = firstChoiceActions.request(); return !!id && activation()?.request_id === id; };");
    expect(group).toContain('<Group title={tr("Background service")}>');
    expect(group).toContain('<Status kind="warn">{tr("Background work is paused on this computer.")}</Status>');
    expect(group).toContain('tr("Turn on background mode to let Zerus run your sessions through the background service on this computer.")');
    expect(group).toMatch(/<Show when=\{offer\(\) && !pending\(\)\}>\s*<button type="button" class="btn" disabled=\{busy\(\)\} onClick=\{choose\}>\{tr\("Use background mode…"\)\}/);
    expect(group).toContain("ownershipActions.check()");
    const about = read("../screens/settings/About.tsx");
    expect(about.indexOf("<ServiceModeReview />")).toBeLessThan(about.indexOf("<ServiceFirstChoice />"));
    expect(read("../../store/desktopService.ts")).toContain('invoke("desktop_service_first_choice", { display })');
  });
});

const REQUEST = "22222222-2222-4222-8222-222222222222";
const PENDING_BODY = "Zerus is turning on background mode on this computer. Settings shows its progress.";
const inFlight = (patch: Partial<DesktopService> = {}) => windows({ ...patch, notification_ownership: { ...windows().notification_ownership!,
  activation: { ...windows().notification_ownership!.activation!, first: null, pending: true, request_id: REQUEST, code: "activation_unconfirmed" } } });

describe("Windows first background choice in flight", () => {
  it("is pending only for its own original ID, and review or manual refresh take over", () => {
    expect(firstChoicePending(inFlight(), REQUEST)).toBe(true);
    expect(firstChoicePending(inFlight(), undefined)).toBe(false);
    expect(firstChoicePending(inFlight(), "33333333-3333-4333-8333-333333333333")).toBe(false);
    expect(firstChoicePending(windows(), REQUEST)).toBe(false);
    const reviewed = inFlight();
    reviewed.notification_ownership!.activation!.review = { request_id: REQUEST, configuration: "a".repeat(64),
      selection: "s", disposition: "required", passive_available: true };
    expect(firstChoicePending(reviewed, REQUEST)).toBe(false);
    expect(firstChoicePending(inFlight({ executable_upgrade: { state: "manual_refresh_required" } }), REQUEST)).toBe(false);
  });
  it("source wiring: the footer, main area and Settings group show the explicit change, never Retry or Connecting", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const read = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/\r\n/g, "\n");
    const store = read("../../store/desktopService.ts");
    expect(store).toContain('return { label: tr("Turning on background mode"),');
    expect(store).toContain('healthy: false, retry: false, inputsReview, ownership: "check" as const, manualRefresh: false, modeReview: false };');
    expect(store).toContain(`if (firstChoicePending(state)) return tr("${PENDING_BODY}");`);
    expect(read("./DesktopServiceStatus.tsx")).toContain('servicePaused(desktopService.state()) === "settings" || firstChoicePending(desktopService.state());');
    expect(read("./DesktopServiceStatus.tsx")).toContain('<h2>{firstChoicePending(desktopService.state()) ? tr("Turning on background mode") : tr("Background work is paused")}</h2>');
    const group = read("../screens/settings/ServiceFirstChoice.tsx");
    expect(group).toContain("const pending = () => own() && !!activation()?.pending;");
    expect(group).toContain("const offer = () => (!manual() && activation()?.first && !ownershipNotice(ownership())) ? activation()!.first! : undefined;");
    expect(group).toContain('<Status kind="busy">{tr("Turning on background mode…")}</Status>');
    expect(group.indexOf('<Status kind="busy">')).toBeLessThan(group.indexOf('tr("The new mode choice is unconfirmed.'));
  });
});

describe("service status in the title bar (task 409)", () => {
  it("keeps healthy details available to About and offers no Retry", () => {
    expect(serviceStatus(undefined)).toBeUndefined();
    expect(serviceStatus(healthy())).toMatchObject({ healthy: true, retry: false, ownership: undefined, label: "Background service", details: "Background service connected" });
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
  it("keeps saved-read recovery visible with its Retry", () => {
    const status = serviceStatus(healthy({ pending_read_import: { required: true, complete: false, running: false, through: 0, superseded: 0, delivered: false, code: "pending" } }));
    expect(status).toMatchObject({ healthy: false, retry: true, label: "Read marks need attention" });
    expect(status?.details).toContain("Saved read marks");
    // Task #59: the G3 migration report review left with the Qt import.
    expect(status).not.toHaveProperty("review");
  });
  it("presents a passive service as paused work with Resume, not as a fault to Retry", () => {
    const status = serviceStatus(passive());
    expect(status).toMatchObject({ healthy: false, retry: false, ownership: "rearm", label: "Background access paused" });
    expect(status?.details).toBe("Background work is paused. Resume to activate the service explicitly.");
    // A durable handoff block keeps its own label and Retry.
    expect(serviceStatus(passive({ code: "explicit_handoff_required" }))).toMatchObject({ label: "Background handoff required", retry: true, ownership: "rearm" });
  });
  it("inserts status only in the title bar, never above the page", async () => {
    const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
    const app = fs.readFileSync(new URL("../../App.tsx", import.meta.url), "utf8");
    // Task 409: the footer is gone; the service status sits in the title bar.
    expect(app).not.toContain("DesktopServiceStatus");
    expect(app).not.toContain("<footer");
    const bar = fs.readFileSync(new URL("./TitleBar.tsx", import.meta.url), "utf8");
    expect(bar.match(/<DesktopServiceStatus\b/g)).toHaveLength(1);
    expect(bar.match(/<div class="tb-group tb-status">[\s\S]*?<\/div>/)?.[0]).toContain("<DesktopServiceStatus");
    expect(app.match(/<main\b[\s\S]*?<\/main>/)?.[0]).not.toContain("DesktopServiceStatus");
    expect(app).not.toContain("with-service-status");
    const css = fs.readFileSync(new URL("../styles/app.css", import.meta.url), "utf8");
    expect(css).not.toContain(".statusbar");
    expect(css).toContain(".tb-log {");
    expect(css).not.toContain(".page.with-service-status");
  });
});

describe("held message copies while the service cannot list them", () => {
  it("keep the paused label and offer no review a passive service cannot list", () => {
    const status = serviceStatus(passive({ task_message_inputs: { review: 2, full: true } }));
    expect(status).toMatchObject({ label: "Background access paused", inputsReview: false, ownership: "rearm" });
    expect(status?.details).toContain("Large task messages are paused: some held message copies need review.");
    expect(serviceStatus(healthy({ task_message_inputs: { review: 2, full: false } }))).toMatchObject({ label: "Held message copies", inputsReview: true });
  });
  it.each([
    { code: "background_unready" },
    { background: { fleet: { ready: false, code: "warming" } } },
    { code: "explicit_handoff_required" },
  ])("offer no review while the service is not ready: %j", (patch) => {
    const status = serviceStatus(healthy({ ...patch, task_message_inputs: { review: 1, full: false } }));
    expect(status).toMatchObject({ inputsReview: false });
    expect(status?.label).not.toBe("Held message copies");
    expect(status?.details).toContain("Some held task message copies need review.");
  });
});
