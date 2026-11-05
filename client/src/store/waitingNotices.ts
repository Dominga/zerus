import type { TeamWaitingItem } from "../types";
import { tr } from "../i18n";

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

const notifications = (): typeof Notification | undefined =>
  typeof globalThis !== "undefined" && "Notification" in globalThis ? globalThis.Notification : undefined;
const open = new Map<string, Notification>();

/** The webview's OS notifications; no-ops where the API or permission is absent. */
export const browserNotifications: WaitingNotificationPort = {
  post(notice) {
    const NotificationCtor = notifications();
    if (!NotificationCtor) return;
    if (NotificationCtor.permission === "granted") {
      const posted = new NotificationCtor(notice.title, { body: notice.body, tag: notice.token });
      open.set(notice.token, posted);
      return;
    }
    if (NotificationCtor.permission === "default") {
      void NotificationCtor.requestPermission().then((permission) => {
        if (permission !== "granted") return;
        const posted = new NotificationCtor(notice.title, { body: notice.body, tag: notice.token });
        open.set(notice.token, posted);
      });
    }
  },
  withdraw(token) {
    open.get(token)?.close();
    open.delete(token);
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
  shared ??= new WaitingNotices(browserNotifications);
  return shared;
}
