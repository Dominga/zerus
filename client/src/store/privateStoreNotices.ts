import { tr } from "../i18n";

// Task 75: Qt PrivateStoreNotices, one notice per (computer, path, reason).
// A store is raised again only after it was resolved and comes back.

export interface PrivateStore {
  path: string;
  reason: string;
  fix: string;
}

export interface PrivateStoreNotice {
  key: string;
  title: string;
  body: string;
}

export class PrivateStoreNotices {
  private known = new Set<string>();
  constructor(private readonly raised: (notice: PrivateStoreNotice) => void) {}

  /** Raises the stores not seen yet; `ok` false (offline) resolves nothing. */
  observe(computer: string, name: string, ok: boolean, stores: readonly PrivateStore[] = []): PrivateStoreNotice[] {
    const fresh: PrivateStoreNotice[] = [];
    if (!ok) return fresh;
    const prefix = `${computer}\n`;
    const current = new Set<string>();
    for (const store of stores) {
      if (!store.path) continue;
      const key = `${prefix}${store.path}\n${store.reason}`;
      current.add(key);
      if (this.known.has(key)) continue;
      this.known.add(key);
      const where = name || (computer ? computer : tr("this computer"));
      const notice = {
        key,
        title: tr("Zerus: a private store is refused"),
        body: tr("%1: %2\n%3\nFix: %4", where, store.path, store.reason, store.fix),
      };
      this.raised(notice);
      fresh.push(notice);
    }
    for (const key of [...this.known]) {
      if (key.startsWith(prefix) && !current.has(key)) this.known.delete(key);
    }
    return fresh;
  }

  forget(computer: string) {
    const prefix = `${computer}\n`;
    for (const key of [...this.known]) if (key.startsWith(prefix)) this.known.delete(key);
  }

  knownKeys(): string[] {
    return [...this.known].sort().slice(0, 128);
  }
}

let shared: PrivateStoreNotices | undefined;
export const privateStoreNotices = (raised: (notice: PrivateStoreNotice) => void) =>
  (shared ??= new PrivateStoreNotices(raised));
