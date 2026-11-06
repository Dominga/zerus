import { describe, expect, it } from "vitest";
import { PrivateStoreNotices } from "./privateStoreNotices";

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
});
