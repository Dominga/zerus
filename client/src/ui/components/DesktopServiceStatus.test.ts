import { describe, expect, it } from "vitest";
import { serviceStatus } from "../../store/desktopService";
import type { DesktopService, NotificationOwnership } from "../../types";

const healthy = (patch: Partial<DesktopService> = {}): DesktopService => ({ connected: true, epoch: "fixture", generation: 1,
  background: Object.fromEntries(["fleet", "presence", "reads", "attention", "approvals"].map((key) => [key, { ready: true, code: "ready" }])),
  attention_counts: null, automatic_approvals: false, details_available: true, code: "", ...patch });
const rearm: NotificationOwnership = { pending: false, running: false, code: "", request_id: null,
  status: { abi: "fixture", configuration: "fixture", journal: null, revision: 1, state: "revoked", rearm_required: true, request_id: null, owner_epoch: null, identity_current: true } };

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
  it("keeps report review and saved-read recovery visible together", () => {
    const status = serviceStatus(healthy({ retained_source_import: { required: true, complete: false, running: false, code: "retained_import_qt_unavailable", phase: null },
      pending_read_import: { required: true, complete: false, running: false, through: 0, superseded: 0, delivered: false, code: "pending" } }));
    expect(status).toMatchObject({ healthy: false, retry: true, review: true });
    expect(status?.details).toContain("Saved read marks");
    expect(status?.details).toContain("migration report needs review");
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
