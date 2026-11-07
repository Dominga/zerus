import type { TeamWaitingItem } from "../types";
import { tr } from "../i18n";
import { teamWaitingNotify } from "../bridge";
import { setSetting, text } from "./settings";

// Task 51: one desktop notification for each Waiting item seen for the first
// time, withdrawn when it is answered or no longer waits (Qt's
// TeamWaitingNotices). The browser's Notification API is the client's OS
// notification module; a test injects a fake port. W2: the shown set is saved
// under `teams/attentionNotified` (Qt's own key), so a restart or a window
// opened again does not raise the same item twice.

export interface WaitingNotice {
  token: string;
  title: string;
  body: string;
}

export interface WaitingNotificationPort {
  post(notice: WaitingNotice): void;
  withdraw(token: string): void;
}

export const waitingToken = (item: Pick<TeamWaitingItem, "id">) => `team-waiting:${item.id}`;

// Task 159: hgs holds the owner's cards while the owner is away
// (`notify: false`) and gives the ones shown together one `batch`.
const detailOf = (item: TeamWaitingItem) => (item.detail ?? {}) as Record<string, unknown>;
const held = (item: TeamWaitingItem) => detailOf(item).notify === false;
const batchOf = (item: TeamWaitingItem) => {
  const batch = detailOf(item).batch;
  return typeof batch === "string" ? batch : "";
};

/** The client's native notifier; no-ops outside the desktop app. */
export const desktopNotifications: WaitingNotificationPort = {
  post(notice) {
    void teamWaitingNotify([notice], []).catch(() => undefined);
  },
  withdraw(token) {
    void teamWaitingNotify([], [token]).catch(() => undefined);
  },
};

/** The ids seen and the notices they posted. */
export class WaitingNotices {
  private posted: Map<string, string>;
  constructor(
    private readonly port: WaitingNotificationPort,
    initial: Iterable<[string, string]> = [],
    /** Called after every change to the shown set (W2's settings key). */
    private readonly persist: (posted: Map<string, string>) => void = () => {},
  ) {
    this.posted = new Map(initial);
  }

  private changed() {
    this.persist(this.posted);
  }

  /** A notice for each new item; `complete` drops ids that no longer wait. */
  observe(items: readonly TeamWaitingItem[], complete: boolean): WaitingNotice[] {
    const present = new Set(items.map((item) => item.id));
    const fresh: WaitingNotice[] = [];
    // Task 159: new items of one batch are one notice; a held card waits.
    const waiting = items.filter((item) => !this.posted.has(item.id) && item.kind !== "team_paused" && !held(item));
    const batches = new Map<string, TeamWaitingItem[]>();
    for (const item of waiting) {
      const batch = batchOf(item);
      if (batch) batches.set(batch, [...(batches.get(batch) ?? []), item]);
    }
    const done = new Set<string>();
    for (const item of waiting) {
      if (done.has(item.id)) continue;
      // Task 112: a paused team is a state, not a question: listed, never notified.
      const group = batches.get(batchOf(item)) ?? [];
      const together = group.length > 1 ? group : [item];
      const notice = {
        token: together.length > 1 ? `team-waiting:batch:${batchOf(item)}` : waitingToken(item),
        // Task 109: a drive low on space is no team's; its notice says what it is.
        title: item.kind === "disk_low" ? tr("Low disk space on %1", item.team_name) : tr("Waiting for you in %1", item.team_name || item.team),
        body: together.map((found) => found.title).join("\n"),
      };
      for (const found of together) {
        this.posted.set(found.id, notice.token);
        done.add(found.id);
      }
      this.port.post(notice);
      fresh.push(notice);
    }
    if (complete) {
      let dropped = false;
      for (const [id, token] of this.posted) {
        if (present.has(id)) continue;
        this.posted.delete(id);
        dropped = true;
        // A batch's notice stays while any of its cards still waits.
        if (![...this.posted.values()].includes(token)) this.port.withdraw(token);
      }
      if (dropped) this.changed();
    }
    if (fresh.length) this.changed();
    return fresh;
  }

  /** The item was answered here: withdraw its banner now. */
  withdraw(id: string) {
    const token = this.posted.get(id);
    if (!token) return;
    this.posted.delete(id);
    if (![...this.posted.values()].includes(token)) this.port.withdraw(token);
    this.changed();
  }

  known(): string[] {
    return [...this.posted.keys()];
  }
}

/** The window's one notice set, shared by the banner, page and answers. */
const NOTIFIED = "teams/attentionNotified";

/** The saved item id -> token pairs; malformed or older values read as none. */
function savedNotices(): [string, string][] {
  try {
    const value: unknown = JSON.parse(text(NOTIFIED));
    if (!value || Array.isArray(value) || typeof value !== "object") return [];
    return Object.entries(value).flatMap(([id, token]) => typeof token === "string" ? [[id, token] as [string, string]] : []);
  } catch {
    return [];
  }
}

let shared: WaitingNotices | undefined;
export function waitingNotices(): WaitingNotices {
  shared ??= new WaitingNotices(desktopNotifications, savedNotices(), (posted) => {
    void setSetting(NOTIFIED, JSON.stringify(Object.fromEntries(posted)));
  });
  return shared;
}
