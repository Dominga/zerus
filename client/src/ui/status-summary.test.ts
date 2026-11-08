import { describe, expect, it } from "vitest";
import { connectionError, staleNote, statusSummary } from "./status-summary";

// Task 72 (F1): a cold start's first seconds, while this computer's stream
// builds its first snapshot, read as connecting, never as offline.

const label = (id: string) => id || "this computer";

describe("the status bar's connection line", () => {
  it("says connecting until the first answer, never offline", () => {
    const starting = [{ id: "", status: "probing" }];
    expect(statusSummary(starting, label)).toBe("Connecting to this computer…");
    expect(statusSummary(starting, label)).not.toContain("offline");
    expect(connectionError(starting)).toBe(false);
  });

  it("names the computers still connecting beside the connected count", () => {
    const fleet = [{ id: "", status: "live" }, { id: "studio", status: "polled" }, { id: "build-02", status: "probing" }];
    expect(statusSummary(fleet, label)).toBe("2 of 3 machines connected, build-02 connecting");
    expect(statusSummary(fleet.slice(0, 2), label)).toBe("2 of 2 machines connected");
  });

  it("a real problem is the error line, not the calm one", () => {
    expect(connectionError([{ id: "", status: "live" }, { id: "studio", status: "reconnecting" }])).toBe(true);
    expect(connectionError([{ id: "", status: "ended" }])).toBe(true);
    expect(connectionError([{ id: "", status: "live" }])).toBe(false);
  });
});

describe("stale note (task 235)", () => {
  it("names the oldest saved list while cached rows show, and nothing after", () => {
    const at = (seconds: number) => `t${seconds}`;
    expect(staleNote({}, at)).toBe("");
    expect(staleNote({ "": 200, studio: 100 }, at)).toBe("Last list from t100, updating…");
  });
});
