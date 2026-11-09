// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { measureTeamUpdates } from "./teamUpdate";

// Task #9: the team update smoke (teamUpdateSmoke.tsx) in happy-dom, so the
// suite keeps the in-place guarantee for a 465-task team and prints the
// before/after numbers; the browser page measures with real layout.

vi.mock("../i18n", () => ({ trNoop: (s: string) => s, tr: (s: string, ...args: unknown[]) => s.replace(/%(\d+)/g, (_, n) => String(args[Number(n) - 1])) }));
vi.mock("../store/settings", () => ({ settings: {}, settingsReady: new Promise(() => {}) }));
vi.mock("../ui/sessions/SessionCard", () => ({ AgentBadge: () => null }));
vi.mock("../store/catalog", () => ({ agentKind: (agent: string) => agent || "claude" }));
vi.mock("../ui/teams/TimeView", () => ({ ProfileTime: () => null }));
vi.mock("../ui/teams/actions", () => ({ stageName: (kind: string) => kind }));
vi.mock("../skin/runtime", () => ({ skin: () => ({ appearance: "light" }) }));
vi.mock("../ui/lib/Dropdown", () => ({ Dropdown: () => null }));

afterEach(() => document.body.replaceChildren());

describe("a 465-task team's update (task #9)", () => {
  it.each(["graph", "board"] as const)("keeps every task card when one member's field changes, and is cheaper than a replace (%s)", (mode) => {
    const host = document.createElement("div"); document.body.append(host);
    const before = measureTeamUpdates(host, "replace", "member", mode, 4);
    const after = measureTeamUpdates(host, "reconcile", "member", mode, 4);
    console.log("team-update", mode, JSON.stringify({ before, after }));
    expect(after.cards).toBe(465);
    expect(after.kept).toBe(after.cards);
    expect(before.kept).toBe(0);
    expect(after.median_ms).toBeLessThan(before.median_ms);
  }, 60_000);
});
