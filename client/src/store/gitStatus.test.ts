import { beforeEach, describe, expect, it } from "vitest";
import { fresh, gitFor, gitOf, gitPath, markAsked, resetGitStatus, storeAnswer, verified, visibleFolders } from "./gitStatus";

// Task 252: a folder's `hgs git-status` answer as a row's git, with Qt's
// rules (SessionPresentation::gitStatus, GitStatusBadge fresh/verified/marks).
const NOW = 1_800_000_000_000;
const ok = (over: Record<string, unknown> = {}) => ({ state: "ok", root: "/work/orbit", branch: "main", changed_files: 0, ahead: 0, behind: 0,
  remote_state: "verified", remote_age_seconds: 5, received_at_ms: NOW - 2_000, ...over });
const row = { computer: "", state: "running", branch: "main", gitRoot: "/work/orbit", cwdCanonical: "/work/orbit/src", cwdPath: "/work/orbit/src" };

describe("git status of a session row (task 252)", () => {
  beforeEach(() => resetGitStatus());

  it("reads the checkout's root, else its folder, and nothing for an archived row", () => {
    expect(gitPath(row)).toBe("/work/orbit");
    expect(gitPath({ ...row, gitRoot: "" })).toBe("/work/orbit/src");
    expect(gitPath({ ...row, gitRoot: "", cwdCanonical: "" })).toBe("/work/orbit/src");
    expect(gitPath({ ...row, state: "archived" })).toBe("");
  });

  it("is fresh under 30 s on a computer online and verified while the remote check is under 60 s", () => {
    expect(fresh(ok(), NOW, true)).toBe(true);
    expect(fresh(ok(), NOW, false)).toBe(false);
    expect(fresh(ok({ received_at_ms: NOW - 30_000 }), NOW, true)).toBe(false);
    expect(fresh(ok({ state: "unavailable" }), NOW, true)).toBe(false);
    expect(verified(ok(), NOW, true)).toBe(true);
    expect(verified(ok({ remote_age_seconds: 58 }), NOW, true)).toBe(false);
    expect(verified(ok({ remote_state: "no_upstream" }), NOW, true)).toBe(false);
  });

  it("maps an answer to the chip's state: changes, outgoing, incoming, in sync or not checked", () => {
    expect(gitOf(ok(), row, NOW, true)).toEqual({ ahead: 0, behind: 0, changed: 0, state: "synced" });
    expect(gitOf(ok({ changed_files: 3, conflicts: 1 }), row, NOW, true)).toEqual({ ahead: 0, behind: 0, changed: 3, conflicts: 1, state: "diff" });
    expect(gitOf(ok({ ahead: 2 }), row, NOW, true)?.state).toBe("push");
    expect(gitOf(ok({ behind: 4 }), row, NOW, true)?.state).toBe("pull");
    // Unverified: counts kept, not checked.
    expect(gitOf(ok({ changed_files: 1, remote_state: "changed" }), row, NOW, true)).toEqual({ ahead: 0, behind: 0, changed: 1, state: "unknown" });
    expect(gitOf(ok(), row, NOW, false)?.state).toBe("unknown");
    // Another branch than the row's reads as not checked (Qt scope_changed).
    expect(gitOf(ok({ branch: "feature" }), row, NOW, true)?.state).toBe("unknown");
    expect(gitOf({ state: "unavailable", received_at_ms: NOW }, row, NOW, true)).toEqual({ ahead: 0, behind: 0, changed: 0, state: "unknown" });
    expect(gitOf({ state: "not_repo" }, row, NOW, true)).toBeUndefined();
    // Asked, no answer yet: Qt's "checking".
    expect(gitOf(undefined, row, NOW, true)?.state).toBe("unknown");
  });

  it("gives a row its git only once its folder was asked for", () => {
    expect(gitFor(row, true, NOW)).toBeUndefined();
    markAsked("", ["/work/orbit"]);
    expect(gitFor(row, true, NOW)?.state).toBe("unknown");
    storeAnswer("", "/work/orbit", ok({ changed_files: 2 }));
    expect(gitFor(row, true, NOW)).toEqual({ ahead: 0, behind: 0, changed: 2, state: "diff" });
    expect(gitFor({ ...row, computer: "studio" }, true, NOW)).toBeUndefined();
    // The answer ages into not checked, as Qt's badge does.
    expect(gitFor(row, true, NOW + 40_000)?.state).toBe("unknown");
  });

  it("asks each visible folder once per computer, never for a computer offline", () => {
    const rows = [row, { ...row, cwdPath: "/work/orbit/docs" }, { ...row, gitRoot: "/work/api" }, { ...row, computer: "studio", gitRoot: "/srv/a" },
      { ...row, computer: "laptop", gitRoot: "/l" }, { ...row, state: "archived", gitRoot: "/old" }];
    expect([...visibleFolders(rows, (computer) => computer !== "laptop")]).toEqual([["", ["/work/orbit", "/work/api"]], ["studio", ["/srv/a"]]]);
  });
});
