import { describe, expect, it } from "vitest";
import { KNOWN_LIMIT, PrivateStoreNotices, groupedBody, type NoticesPort } from "./privateStoreNotices";

const store = (path: string, reason = "not private", fix = "restore the ACL") => ({ path, reason, fix });

describe("private-store notices", () => {
  it("raises once per computer, path and reason", () => {
    const seen: string[] = [];
    const notices = new PrivateStoreNotices((notice) => seen.push(`${notice.key}\n${notice.body}`));
    expect(notices.observe("", "this computer", true, [store("C:\\state")])).toHaveLength(1);
    expect(notices.observe("", "this computer", true, [store("C:\\state")])).toHaveLength(0);
    expect(seen[0]).toContain("Fix: restore the ACL");
    expect(notices.knownKeys()).toHaveLength(1);
  });

  it("raises again after the store was resolved, and forgets an offline peer's stores", () => {
    const notices = new PrivateStoreNotices(() => undefined);
    expect(notices.observe("mac", "studio", true, [store("/state")])).toHaveLength(1);
    expect(notices.observe("mac", "studio", true, [])).toHaveLength(0);
    expect(notices.knownKeys()).toEqual([]);
    expect(notices.observe("mac", "studio", true, [store("/state", "other")])).toHaveLength(1);
    expect(notices.observe("mac", "studio", false, [])).toHaveLength(0);
    expect(notices.knownKeys()).toHaveLength(1);
    notices.forget("mac");
    expect(notices.knownKeys()).toEqual([]);
  });

  it("keeps the raised set across restarts: a kept notice is not raised again (task 102)", async () => {
    let release!: (keys: string[]) => void;
    const saved: string[][] = [];
    const port: NoticesPort = { load: () => new Promise((resolve) => { release = resolve; }), save: async (known) => { saved.push(known); } };
    const seen: string[] = [];
    const notices = new PrivateStoreNotices((notice) => seen.push(notice.key), port);
    // Before the kept set arrives an observation waits: nothing is raised yet.
    expect(notices.ready()).toBe(false);
    expect(notices.observe("", "this computer", true, [store("C:\\state"), store("C:\\new")])).toEqual([]);
    release(["\nC:\\state\nnot private"]);
    await Promise.resolve(); await Promise.resolve();
    expect(notices.ready()).toBe(true);
    expect(seen).toEqual(["\nC:\\new\nnot private"]);
    expect(saved.at(-1)).toEqual(["\nC:\\new\nnot private", "\nC:\\state\nnot private"]);
    // An unchanged set is not saved again.
    const writes = saved.length;
    notices.observe("", "this computer", true, [store("C:\\state"), store("C:\\new")]);
    expect(saved.length).toBe(writes);
    // A resolved store leaves the kept set.
    notices.observe("", "this computer", true, [store("C:\\new")]);
    expect(saved.at(-1)).toEqual(["\nC:\\new\nnot private"]);
  });

  it("keeps at most the core's 512 keys and starts empty when the kept set cannot be read", async () => {
    const many = Array.from({ length: KNOWN_LIMIT + 30 }, (_, n) => `\n/p/${String(n).padStart(4, "0")}\nreason`);
    const notices = new PrivateStoreNotices(() => undefined, { load: async () => many, save: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    expect(notices.knownKeys()).toHaveLength(KNOWN_LIMIT);
    const broken = new PrivateStoreNotices(() => undefined, { load: () => Promise.reject(new Error("unreadable")), save: async () => undefined });
    await Promise.resolve(); await Promise.resolve();
    expect(broken.ready()).toBe(true);
    expect(broken.observe("", "", true, [store("/x")])).toHaveLength(1);
  });
});

describe("private-store notices are once per path and grouped (task #383)", () => {
  it("does not raise a path again when only its reason changes", () => {
    const notices = new PrivateStoreNotices(() => undefined);
    expect(notices.observe("", "this computer", true, [store("C:\\state")])).toHaveLength(1);
    expect(notices.observe("", "this computer", true, [store("C:\\state", "owned by another user")])).toHaveLength(0);
    expect(notices.knownKeys()).toEqual(["\nC:\\state\nowned by another user"]);
  });

  it("raises the stores new in one look as one notice on one line", () => {
    const seen: { title: string; body: string }[] = [];
    const notices = new PrivateStoreNotices((notice) => seen.push(notice));
    const many = ["C:\\a", "C:\\b", "C:\\c", "C:\\d", "C:\\e"].map((path) => store(path, "not private", `icacls ${path}`));
    const raised = notices.observe("", "msi", true, many);
    expect(raised).toHaveLength(1);
    expect(seen).toHaveLength(1);
    expect(seen[0].title).toBe("Zerus: 5 private stores are refused");
    expect(seen[0].body.split("\n")[0]).toBe("msi: C:\\a, C:\\b, C:\\c and 2 more");
    expect(seen[0].body).toContain("Fix: icacls C:\\a");
    // Each one is known: the next look raises nothing.
    expect(notices.observe("", "msi", true, many)).toHaveLength(0);
    expect(notices.knownKeys()).toHaveLength(5);
    // A single new store keeps its own notice with its own fix.
    const one = notices.observe("", "msi", true, [...many, store("C:\\f", "not private", "icacls C:\\f")]);
    expect(one).toHaveLength(1);
    expect(one[0].title).toBe("Zerus: a private store is refused");
    expect(one[0].body).toContain("Fix: icacls C:\\f");
  });

  it("lists up to three paths and counts the rest", () => {
    expect(groupedBody("studio", [store("/a"), store("/b")]).split("\n")[0]).toBe("studio: /a, /b");
  });
});
