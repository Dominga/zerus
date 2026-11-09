// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { createComponent } from "solid-js";
import { render } from "solid-js/web";
import { createCatalog } from "./catalog";
import { emptyBatch, isEmptyBatch, type Batch, type GitOut, type RowOut } from "../types";
import { normalize } from "../bridge";
import { SessionCard, gitMarks, gitTooltip } from "../ui/sessions/SessionCard";
import { setNow } from "./ui";

const row = (key: string, computer = "", path = "/fixture/project"): RowOut => ({ computer, key,
  row: { name: "codex/fixture/" + key, agent: "codex", status: "ready", state: "running", folder: "project",
    branch: "topic", cwd: path, git_root: path, git_metadata_state: "ok", run_id: key } });
const update = (computer = "", key = "/fixture/project", value: Record<string, unknown> = {}): GitOut => ({
  computer, key, kind: "state", payload: { rev: 1, value: { state: "ok", root: key, branch: "topic", upstream: "published/integration",
    changed_files: 4, conflicts: 1, ahead: 2, behind: 3, remote_state: "verified", remote_age_seconds: 2, ...value } },
});
const batch = (partial: Partial<Batch>): Batch => ({ ...emptyBatch(), ...partial });
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; document.body.replaceChildren(); });

describe("visible Git watch projection", () => {
  it("renders changed files and commits on the real card and keeps them through catalog updates", () => {
    const c = createCatalog();
    c.applyBatch(batch({ upserts: [row("run:a")], computers: [{ computer: "", status: "live", attempt: 0 }] }));
    expect(c.gitInterests(["\nrun:a"])).toEqual([{ computer: "", key: "run:a" }]);
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(SessionCard, {
      get session() { return c.catalog.rows["\nrun:a"]; }, selected: false, expandable: false, expanded: false, onToggleChildren() {},
    }), host);
    expect(host.querySelector(".gmarks")).toBeNull();
    c.applyBatch(batch({ git: [update()] })); setNow(Date.now() / 1000);
    expect(host.querySelector(".gmarks"), "the maintained catalog must apply the native Git batch").not.toBeNull();
    const card = host.querySelector(".scard"), marks = host.querySelector(".gmarks")!;
    expect(marks.querySelector(".t-conflict")?.textContent).toBe("4");
    expect([...marks.querySelectorAll(".gmark.counted")].map((m) => m.textContent)).toEqual(["4", "2", "3"]);
    expect(marks.getAttribute("title")).toContain("published/integration");
    expect(marks.getAttribute("title")).not.toContain("origin/topic");
    const glyph = marks.querySelector(".gmark");
    setNow(Date.now() / 1000 + 1);
    expect(marks.querySelector(".gmark")).toBe(glyph);
    c.applyBatch(batch({ upserts: [{ ...row("run:a"), row: { ...row("run:a").row, summary: "metadata changed" } }] }));
    expect(host.querySelector(".scard")).toBe(card);
    expect(host.querySelector(".gmarks")?.textContent).toContain("4");
    c.applyBatch(batch({ git: [{ computer: "", key: "/fixture/project", kind: "unavailable", payload: { code: "git_status_failed" } }] }));
    expect(host.querySelector(".gmarks")).toBeNull();
  });

  it("separates computers, retires offscreen rows and refuses stale checkout/branch facts", () => {
    const c = createCatalog();
    c.applyBatch(batch({ upserts: [row("run:a"), row("run:b", "peer")],
      computers: ["", "peer"].map((computer) => ({ computer, status: "live", attempt: 0 })) }));
    c.gitInterests(["\nrun:a", "peer\nrun:b"]);
    c.applyBatch(batch({ git: [update(), update("peer", undefined, { changed_files: 9 })] }));
    expect(c.catalog.rows["\nrun:a"].git?.changed).toBe(4);
    expect(c.catalog.rows["peer\nrun:b"].git?.changed).toBe(9);
    c.gitInterests(["peer\nrun:b"]);
    expect(c.catalog.rows["\nrun:a"].git).toBeUndefined();
    c.applyBatch(batch({ git: [update()] }));
    expect(c.catalog.rows["\nrun:a"].git).toBeUndefined();
    c.applyBatch(batch({ upserts: [row("run:b", "peer", "/fixture/replacement")] }));
    expect(c.catalog.rows["peer\nrun:b"].git).toBeUndefined();
    c.applyBatch(batch({ git: [update("peer")] }));
    expect(c.catalog.rows["peer\nrun:b"].git).toBeUndefined();
    c.applyBatch(batch({ git: [update("peer", "/fixture/replacement", { branch: "different" })] }));
    expect(c.catalog.rows["peer\nrun:b"].git?.localFresh).toBe(false);
    expect(gitMarks(c.catalog.rows["peer\nrun:b"].git!).some((m) => m.icon === "git-synced")).toBe(false);
    c.applyBatch(batch({ computers: [{ computer: "peer", status: "reconnecting", attempt: 1 }] }));
    expect(c.catalog.rows["peer\nrun:b"].git).toBeUndefined();
  });

  it("retains at most the selected/visible budget, including after many scrolls", () => {
    const c = createCatalog();
    c.applyBatch(batch({ upserts: Array.from({ length: 100 }, (_, n) => row("run:" + n, "", "/fixture/" + n)),
      computers: [{ computer: "", status: "live", attempt: 0 }] }));
    const ids = Array.from({ length: 100 }, (_, n) => "\nrun:" + n);
    expect(c.gitInterests(ids)).toHaveLength(48);
    c.applyBatch(batch({ git: Array.from({ length: 48 }, (_, n) => update("", "/fixture/" + n)) }));
    c.gitInterests(ids.slice(50));
    expect(ids.slice(0, 48).every((id) => !c.catalog.rows[id].git)).toBe(true);
    c.applyBatch(batch({ git: [update("", "/fixture/0"), update("", "/fixture/50")] }));
    expect(c.catalog.rows[ids[0]].git).toBeUndefined();
    expect(c.catalog.rows[ids[50]].git?.changed).toBe(4);
    c.gitInterests([]);
    expect(ids.every((id) => !c.catalog.rows[id].git)).toBe(true);
  });

  it("keeps unchanged watched facts and expires remote evidence independently and never invents an upstream or clean count", () => {
    const c = createCatalog();
    c.applyBatch(batch({ upserts: [row("run:a")], computers: [{ computer: "", status: "live", attempt: 0 }] }));
    c.gitInterests(["\nrun:a"]);
    c.applyBatch(batch({ git: [update("", undefined, { remote_age_seconds: 59, changed_files: 0, ahead: 0, behind: 0 })] }));
    const s = c.catalog.rows["\nrun:a"], at = s.git!.receivedAt!;
    expect(gitMarks(s.git!, at).map((m) => m.icon)).toEqual(["git-synced"]);
    expect(gitMarks(s.git!, at + 1001).map((m) => m.icon)).toEqual(["git-unknown"]);
    expect(gitMarks({ ...s.git!, remoteAge: 0 }, at + 30_000).map((m) => m.icon)).toEqual(["git-synced"]);
    expect(gitMarks({ ...s.git!, streamed: false, remoteAge: 0 }, at + 30_000).map((m) => m.icon)).toEqual(["git-unknown"]);
    c.applyBatch(batch({ git: [update("", undefined, { changed_files: 0, ahead: null, behind: null, upstream: "", remote_state: "no_upstream" })] }));
    expect(gitMarks(s.git!).map((m) => m.icon)).toEqual(["git-unknown"]);
    expect(gitTooltip(s)).not.toContain("origin/topic");
    expect(gitTooltip(s)).not.toContain("0 outgoing commits");
  });

  it("keeps a Git-only batch in the maintained bridge pump", () => {
    const b = normalize({ git: [update()] });
    expect(isEmptyBatch(b)).toBe(false);
    expect(b.git).toEqual([update()]);
  });

  it.each([
    ["empty", ""], ["missing", undefined], ["blank", " \t"],
    ["null", null], ["number", 42], ["object", { name: "published/integration" }],
  ])("keeps a reported verified remote unknown when its upstream is %s", (_label, upstream) => {
    const c = createCatalog();
    c.applyBatch(batch({ upserts: [row("run:a")], computers: [{ computer: "", status: "live", attempt: 0 }] }));
    c.gitInterests(["\nrun:a"]);
    const host = document.createElement("div"); document.body.append(host);
    dispose = render(() => createComponent(SessionCard, {
      get session() { return c.catalog.rows["\nrun:a"]; }, selected: false, expandable: false, expanded: false, onToggleChildren() {},
    }), host);
    const clean = { changed_files: 0, conflicts: 0, ahead: 0, behind: 0, remote_state: "verified", remote_age_seconds: 0 };
    c.applyBatch(batch({ git: [update("", undefined, { ...clean, upstream })] }));
    // Advance the card clock with each newly observed batch.
    setNow((c.catalog.rows["\nrun:a"].git!.receivedAt! + 1) / 1000);
    expect(host.querySelector(".gmark.t-synced"), "no verified clean mark without a reported upstream").toBeNull();
    expect(host.querySelector(".gmarks .gmark.t-muted")).not.toBeNull();
    expect(host.querySelector(".gmarks")?.getAttribute("title")).toContain("Remote publication has not been verified recently.");
    expect(c.catalog.rows["\nrun:a"].git?.remoteVerified).toBe(false);

    c.applyBatch(batch({ git: [update("", undefined, { ...clean, upstream, changed_files: 2 })] }));
    setNow((c.catalog.rows["\nrun:a"].git!.receivedAt! + 1) / 1000);
    expect(host.querySelector(".gmark.t-attention")?.textContent).toBe("2");
    expect(host.querySelector(".gmark.t-synced")).toBeNull();

    c.applyBatch(batch({ git: [update("", undefined, { ...clean, upstream: "published/integration" })] }));
    setNow((c.catalog.rows["\nrun:a"].git!.receivedAt! + 1) / 1000);
    expect(host.querySelector(".gmark.t-synced")).not.toBeNull();
    expect(c.catalog.rows["\nrun:a"].git?.remoteVerified).toBe(true);
  });
});
