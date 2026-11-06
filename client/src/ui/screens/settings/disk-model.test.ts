import { describe, expect, it } from "vitest";
import { DiskModel, decisionsLine, driveLine, folderState, size, type DiskFolder } from "./disk-model";
import type { DecisionPort } from "../../teams/decisions";
import type { TeamCall, TeamConfirmation, TeamOutcome } from "../../../types";

// Task 108: Settings > Build folders reads `hgs team disk`, runs a dry run at
// once and Clean now only after the owner's Remove; no dots between parts.

const GB = 1024 * 1024 * 1024;

function port(answers: Record<string, unknown>, yes: boolean) {
  const sent: TeamCall[] = [];
  const asked: TeamConfirmation[] = [];
  const fake: DecisionPort = {
    async send(call): Promise<TeamOutcome> {
      const action = call.action as { kind: string; action: string; dry_run?: boolean };
      if (action.action === "clean" && action.dry_run === false && !call.confirmed) {
        return { outcome: "confirm", confirmation: { name: "diskCleanConfirm", title: "Remove build folders?", text: "Remove…", action: "Remove" } };
      }
      sent.push(call);
      return { outcome: "done", receipt: (answers[action.action] ?? {}) as Record<string, unknown>, request_id: `r${sent.length}` };
    },
    async confirm(confirmation) { asked.push(confirmation); return yes; },
    async read() { throw new Error("no team"); },
  };
  return { fake, sent, asked };
}

const view = {
  policy: { enabled: true, unused_days: 7, free_gb: 50, free_percent: 10 },
  drives: [{ drive: "D:", free: 12 * GB, total: 900 * GB, threshold: 90 * GB, low: true }],
  teams: [{ team: "t1", team_name: "Checkout", bytes: 31 * GB, folders: [] }], held: 31 * GB, last_pass: 0,
};

describe("build folders", () => {
  it("words sizes, drives, folders and decisions without dots", () => {
    expect(size(undefined)).toBe("not measured yet");
    expect(size(31.25 * GB)).toBe("31.3 GB");
    expect(size(5 * 1024 * 1024)).toBe("5 MB");
    expect(driveLine(view.drives[0])).toBe("D: 12.0 GB free of 900.0 GB, keeps 90.0 GB; low on space");
    const folder = (over: Partial<DiskFolder>): DiskFolder => ({ path: "D:/b", kind: "cargo_target", origin: "hgs_run", owned: true,
      owned_reason: "a Zerus run created it", state: "kept", last_used: 0, ...over });
    expect(folderState(folder({ due: "its team is finished" }))).toBe("The policy removes it: its team is finished");
    expect(folderState(folder({ owned: false, owned_reason: "it existed before Zerus used it" }))).toBe("Kept: not Zerus's (it existed before Zerus used it)");
    expect(folderState(folder({ state: "review", note: "removing failed" }))).toBe("Waits for your review: removing failed");
    expect(folderState(folder({}))).toBe("Kept");
    const decisions = [
      { path: "D:/a", team_name: "", reason: "unused for 9 days", action: "remove" as const, detail: "", bytes: 2 * GB },
      { path: "D:/b", team_name: "", reason: "its team is finished", action: "keep" as const, detail: "in use: cargo is building in it", bytes: 0 },
    ];
    expect(decisionsLine(decisions, true)).toBe("Would free 2.0 GB in 1 folders; 1 kept");
    expect(decisionsLine([], false)).toBe("Nothing to remove now");
  });

  it("lists at once, runs a dry run at once and cleans only after Remove", async () => {
    const decided = { decisions: [{ path: "D:/a", team_name: "", reason: "r", action: "remove", detail: "", bytes: GB }] };
    const cancel = port({ list: view, clean: decided }, false);
    const model = new DiskModel(cancel.fake, () => {});
    await model.load();
    expect(model.view?.drives[0].low).toBe(true);
    await model.clean(true);
    expect(model.decisions).toHaveLength(1);
    expect(cancel.sent.map((call) => (call.action as { action: string }).action)).toEqual(["list", "clean"]);
    expect(cancel.sent.every((call) => call.team === "")).toBe(true);
    // Clean now: Cancel sends nothing.
    await model.clean(false);
    expect(cancel.asked.map((asked) => asked.name)).toEqual(["diskCleanConfirm"]);
    expect(cancel.sent).toHaveLength(2);
    // Remove: sent confirmed, then the view is read again.
    const yes = port({ list: view, clean: decided }, true);
    const confirmed = new DiskModel(yes.fake, () => {});
    await confirmed.clean(false, "all");
    expect(yes.sent[0]).toMatchObject({ confirmed: true, action: { kind: "disk", action: "clean", scope: "all", dry_run: false } });
    expect((yes.sent[1].action as { action: string }).action).toBe("list");
    // The policy goes as the owner set it.
    await confirmed.savePolicy({ enabled: false, unused_days: 0, free_gb: 20, free_percent: 5 });
    expect(yes.sent.find((call) => (call.action as { action: string }).action === "policy")?.action)
      .toEqual({ kind: "disk", action: "policy", policy: { enabled: false, unused_days: 0, free_gb: 20, free_percent: 5 } });
  });
});
