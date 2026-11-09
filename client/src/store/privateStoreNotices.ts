import { tr } from "../i18n";
import { inTauri } from "../bridge";
import { invoke } from "@tauri-apps/api/core";

// Task 75: Qt PrivateStoreNotices, one notice per (computer, path, reason).
// A store is raised again only after it was resolved and comes back.
// Task 102: the set raised so far is kept on this device across restarts, as
// Qt keeps privateStores/notified (at most KNOWN_LIMIT keys, core
// private_notices.rs), and Qt's set seeds it once at the first start.

/** Keys kept, as the core's file keeps them. */
export const KNOWN_LIMIT = 512;

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

/** Where the set is kept between starts. */
export interface NoticesPort {
  load(): Promise<string[]>;
  save(known: string[]): Promise<unknown>;
}

export class PrivateStoreNotices {
  private known = new Set<string>();
  /** Observations made before the kept set arrived, in order; null once it has. */
  private waiting: (() => void)[] | null = null;
  constructor(private readonly raised: (notice: PrivateStoreNotice) => void, private readonly port?: NoticesPort) {
    if (!port) return;
    this.waiting = [];
    const done = (keys: readonly string[]) => {
      for (const key of keys.slice(0, KNOWN_LIMIT)) if (typeof key === "string" && key) this.known.add(key);
      const queued = this.waiting ?? [];
      this.waiting = null;
      for (const run of queued) run();
    };
    port.load().then(done, () => done([]));
  }

  /** Whether the kept set has arrived (always, without a port). */
  ready(): boolean {
    return this.waiting === null;
  }

  /**
   * Raises the stores not seen yet; `ok` false (offline) resolves nothing.
   * Before the kept set arrives the observation waits for it (and returns
   * nothing now), so a restart never raises a notice again.
   */
  observe(computer: string, name: string, ok: boolean, stores: readonly PrivateStore[] = []): PrivateStoreNotice[] {
    if (this.waiting) {
      const copy = stores.map((store) => ({ ...store }));
      this.waiting.push(() => { this.observe(computer, name, ok, copy); });
      return [];
    }
    const fresh: PrivateStoreNotice[] = [];
    if (!ok) return fresh;
    const before = this.signature();
    const prefix = `${computer}\n`;
    const current = new Set<string>();
    // Task #383: once per path - a store whose reason changes is not raised
    // again - and the stores new in one look are one notice.
    const raised: { key: string; store: PrivateStore }[] = [];
    for (const store of stores) {
      if (!store.path) continue;
      const key = `${prefix}${store.path}\n${store.reason}`;
      current.add(key);
      const path = `${prefix}${store.path}\n`;
      const seen = [...this.known].some((known) => known.startsWith(path));
      this.known.add(key);
      if (seen || raised.some((entry) => entry.store.path === store.path)) continue;
      raised.push({ key, store });
    }
    if (raised.length > 0) {
      const where = name || (computer ? computer : tr("this computer"));
      const notice = raised.length === 1
        ? {
            key: raised[0].key,
            title: tr("Zerus: a private store is refused"),
            body: tr("%1: %2\n%3\nFix: %4", where, raised[0].store.path, raised[0].store.reason, raised[0].store.fix),
          }
        : {
            key: raised[0].key,
            title: tr("Zerus: %1 private stores are refused", raised.length),
            body: groupedBody(where, raised.map((entry) => entry.store)),
          };
      this.raised(notice);
      fresh.push(notice);
    }
    for (const key of [...this.known]) {
      if (key.startsWith(prefix) && !current.has(key)) this.known.delete(key);
    }
    this.keep(before);
    return fresh;
  }

  forget(computer: string) {
    if (this.waiting) { this.waiting.push(() => this.forget(computer)); return; }
    const before = this.signature();
    const prefix = `${computer}\n`;
    for (const key of [...this.known]) if (key.startsWith(prefix)) this.known.delete(key);
    this.keep(before);
  }

  knownKeys(): string[] {
    return [...this.known].sort().slice(0, KNOWN_LIMIT);
  }

  private signature(): string {
    return this.knownKeys().join("\u0000");
  }

  /** Saves the set when it changed (Qt's TrayAgent.cpp:593). */
  private keep(before: string) {
    if (this.port && this.signature() !== before) void this.port.save(this.knownKeys()).catch(() => {});
  }
}

/** Task #383: several refused stores on one line, the first one's fix after. */
export function groupedBody(where: string, stores: readonly PrivateStore[]): string {
  const shown = stores.slice(0, 3).map((store) => store.path).join(", ");
  const list = stores.length > 3 ? tr("%1 and %2 more", shown, stores.length - 3) : shown;
  return tr("%1: %2\nFix: %3\nEach store has a fix of its own; the session details list them.", where, list, stores[0]?.fix ?? "");
}

const devicePort: NoticesPort = {
  load: () => (inTauri() ? invoke<string[]>("private_store_notices_known") : Promise.resolve([])),
  save: (known) => (inTauri() ? invoke("private_store_notices_save", { known }) : Promise.resolve()),
};

let shared: PrivateStoreNotices | undefined;
export const privateStoreNotices = (raised: (notice: PrivateStoreNotice) => void) =>
  (shared ??= new PrivateStoreNotices(raised, devicePort));
