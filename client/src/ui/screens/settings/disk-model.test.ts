import { describe, expect, it } from "vitest";
import { DiskModel, decisionsLine, driveLine, folderKind, folderState, lastUse, size, type DiskFolder, type DiskPolicy } from "./disk-model";
import { opensDisk, waitingKindIcon, waitingKindLabel } from "../../teams/decisions";
import { WaitingNotices, type WaitingNotice } from "../../../store/waitingNotices";
import type { TeamWaitingItem } from "../../../types";
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
    expect(folderState(folder({}))).toBe("Removable");
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

// Task 109: busy, last use and what Zerus holds per drive; the open view reads
// again by itself; the policy round-trips through hgs; a low drive opens the
// section and its notice names the drive.
describe("build folders, live (task 109)", () => {
  const folder = (over: Partial<DiskFolder>): DiskFolder => ({ path: "D:/b", kind: "cargo_target", origin: "hgs_run", owned: true,
    owned_reason: "a Zerus run created it", state: "kept", last_used: 0, ...over });

  it("says busy, kept for review, not Zerus's, removable and when it was last used", () => {
    expect(folderKind(folder({ busy: "cargo is building in it", due: "unused for 9 days" }))).toBe("busy");
    expect(folderState(folder({ busy: "cargo is building in it" }))).toBe("Busy: cargo is building in it");
    expect(folderKind(folder({ busy: null }))).toBe("removable");
    expect(folderKind(folder({ state: "review", busy: "x" }))).toBe("review");
    expect(folderKind(folder({ owned: false, busy: "x" }))).toBe("foreign");
    expect(folderKind(folder({ exists: false }))).toBe("gone");
    const now = 1_800_000_000;
    expect(lastUse(0, now)).toBe("never used");
    expect(lastUse(now - 20, now)).toBe("used just now");
    expect(lastUse(now - 5 * 60, now)).toBe("used 5 min ago");
    expect(lastUse(now - 3 * 3600, now)).toBe("used 3 h ago");
    expect(lastUse(now - 2 * 86400, now)).toBe("used 2 d ago");
    expect(driveLine({ drive: "D:", free: 400 * GB, total: 900 * GB, threshold: 90 * GB, low: false, held: 33 * GB }))
      .toBe("D: 400.0 GB free of 900.0 GB, keeps 90.0 GB; Zerus holds 33.0 GB here");
  });

  /** A fake hgs that stores the policy and measures a folder on a live read. */
  function hgs() {
    let policy: DiskPolicy = { enabled: true, unused_days: 7, free_gb: 50, free_percent: 10 };
    let sized = false;
    const sent: TeamCall[] = [];
    const fake: DecisionPort = {
      async send(call): Promise<TeamOutcome> {
        sent.push(call);
        const action = call.action as { action: string; measure?: boolean; policy?: DiskPolicy };
        if (action.action === "list" && action.measure) {
          sized = true;
        }
        if (action.action === "policy") policy = { ...action.policy! };
        const listing = { policy: { ...policy }, drives: [], held: sized ? 30 * GB : 0, last_pass: 0,
          teams: [{ team: "", team_name: "Sessions", bytes: 0, folders: [folder({ path: "D:/core", ...(sized ? { size: 30 * GB } : {}) })] }] };
        return { outcome: "done", receipt: (action.action === "list" ? listing : { status: "updated", policy }) as unknown as Record<string, unknown>, request_id: `r${sent.length}` };
      },
      async confirm() { return true; },
      async read() { throw new Error("no team"); },
    };
    return { fake, sent, stored: () => policy, setStored: (next: DiskPolicy) => { policy = next; } };
  }

  it("reads again by itself and shows a newer size, without touching the buttons or a notice", async () => {
    const h = hgs();
    let redraws = 0;
    const model = new DiskModel(h.fake, () => { redraws += 1; });
    await model.load();
    expect(model.view?.teams[0].folders[0].size).toBeUndefined();
    model.notice = "earlier";
    const before = redraws;
    await model.poll();
    expect((h.sent.at(-1)!.action as { measure?: boolean }).measure).toBe(true);
    expect(model.view?.teams[0].folders[0].size).toBe(30 * GB);
    expect(model.notice).toBe("earlier");
    expect(model.busy).toBe(false);
    expect(redraws).toBe(before + 1);
    // The same view again redraws nothing.
    await model.poll();
    expect(redraws).toBe(before + 1);
    // While an action runs, the live read is skipped.
    model.busy = true;
    const count = h.sent.length;
    await model.poll();
    expect(h.sent).toHaveLength(count);
  });

  it("drops a live read an action overtook", async () => {
    const h = hgs();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const slow: DecisionPort = { ...h.fake, async send(call) {
      if ((call.action as { measure?: boolean }).measure) { await gate; return { outcome: "done", receipt: { policy: {}, drives: [], teams: [], held: 999, last_pass: 0 }, request_id: "late" }; }
      return h.fake.send(call);
    } };
    const raced = new DiskModel(slow, () => {});
    await raced.load();
    const polling = raced.poll();
    await raced.load();
    release();
    await polling;
    expect(raced.view?.held).not.toBe(999);
  });

  it("sends the policy as set and shows it as hgs stored it", async () => {
    const h = hgs();
    const model = new DiskModel(h.fake, () => {});
    await model.load();
    await model.savePolicy({ enabled: false, unused_days: 3, free_gb: 80, free_percent: 10 });
    expect(h.stored()).toEqual({ enabled: false, unused_days: 3, free_gb: 80, free_percent: 10 });
    expect(model.view?.policy).toEqual(h.stored());
    // Changed elsewhere: the next live read brings it.
    h.setStored({ enabled: true, unused_days: 14, free_gb: 20, free_percent: 5 });
    await model.poll();
    expect(model.view?.policy).toEqual({ enabled: true, unused_days: 14, free_gb: 20, free_percent: 5 });
  });

  it("a low drive opens Build folders and its notice names the drive", () => {
    const low = { id: "disk/D:", team: "", team_name: "D:", kind: "disk_low", action: "open_disk", since: 1, proposal: null,
      title: "Drive D: has 12.0 GB free, under 90.0 GB; Zerus's build folders there hold 31.0 GB" } as unknown as TeamWaitingItem;
    expect(opensDisk(low)).toBe(true);
    expect(opensDisk({ action: "decide_proposal" })).toBe(false);
    expect(waitingKindLabel(low)).toBe("Disk space");
    expect(waitingKindIcon("disk_low")).toBe("hard-drive");
    const posted: WaitingNotice[] = [];
    const notices = new WaitingNotices({ post: (notice) => posted.push(notice), withdraw: () => {} });
    notices.observe([low], true);
    expect(posted).toEqual([{ token: "team-waiting:disk/D:", title: "Low disk space on D:", body: low.title }]);
  });
});
