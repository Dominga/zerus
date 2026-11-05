import type { TeamWaitingItem } from "../types";
import { tr } from "../i18n";
import { teamWaitingNotify } from "../bridge";

// Task 51: one desktop notification for each Waiting item seen for the first
// time, withdrawn when it is answered or no longer waits (Qt's
// TeamWaitingNotices). The browser's Notification API is the client's OS
// notification module; a test injects a fake port.

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
  private posted = new Map<string, string>();
  constructor(private readonly port: WaitingNotificationPort) {}

  /** A notice for each new item; `complete` drops ids that no longer wait. */
  observe(items: readonly TeamWaitingItem[], complete: boolean): WaitingNotice[] {
    const present = new Set(items.map((item) => item.id));
    const fresh: WaitingNotice[] = [];
    for (const item of items) {
      if (this.posted.has(item.id)) continue;
      const notice = {
        token: waitingToken(item),
        title: tr("Waiting for you in %1", item.team_name || item.team),
        body: item.title,
      };
      this.posted.set(item.id, notice.token);
      this.port.post(notice);
      fresh.push(notice);
    }
    if (complete) {
      for (const [id, token] of this.posted) {
        if (present.has(id)) continue;
        this.port.withdraw(token);
        this.posted.delete(id);
      }
    }
    return fresh;
  }

  /** The item was answered here: withdraw its banner now. */
  withdraw(id: string) {
    const token = this.posted.get(id);
    if (!token) return;
    this.port.withdraw(token);
    this.posted.delete(id);
  }

  known(): string[] {
    return [...this.posted.keys()];
  }
}

/** The window's one notice set, shared by the banner, page and answers. */
let shared: WaitingNotices | undefined;
export function waitingNotices(): WaitingNotices {
  shared ??= new WaitingNotices(desktopNotifications);
  return shared;
}
