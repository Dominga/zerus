import { describe, expect, it } from "vitest";
import { accountChoices, catalogOf, computerChoices, coordinatorLine, coordinatorOf, draftLedBy, draftsOf, projectOfRow, shownTemplates, templateSeats, templateTaskCount, toggledFavourite } from "./newTeamSources";
import { memberRoles } from "./newTeamForm";
import type { AccountsView, TeamSummary } from "../types";

// Task 56: New team from the rail offers what it offers from the Team tab:
// every computer (with its online state), this computer's accounts, the
// catalog's roles and templates, and the selected session as "This session".
// Before, the rail's page passed only this computer and nothing else.

const profile = (over: Record<string, unknown>) => ({ key: "k", host: "", machine: "", id: "work", label: "Work", provider: "codex", native: false,
  installed: true, is_default: false, home: "", auth_revision: "", online: true, state: "ok", detail: "", signed_in: true, ...over });

describe("New team sources", () => {
  it("offers this computer first, then every known computer once, with whether it is online", () => {
    const computers = computerChoices(
      { "": { status: "live" }, studio: { status: "live" }, laptop: { status: "ended" }, build: { status: "live" } },
      [{ computer: "studio", name: "studio", label: "Studio", online: true }, { computer: "laptop", name: "laptop", label: "", online: true }],
      (computer) => computer !== "laptop",
    );
    expect(computers).toEqual([
      { alias: "", label: "this computer", online: true },
      { alias: "studio", label: "Studio", online: true },
      { alias: "laptop", label: "laptop", online: false },
      { alias: "build", label: "build", online: true },
    ]);
  });

  it("offers this computer's accounts once each, signed out marked, native ones left to Default account", () => {
    const view = { accounts: [
      { key: "a", label: "Codex", provider: "codex", members: [profile({}), profile({ host: "studio", id: "remote" }), profile({ id: "native-codex", native: true })] },
      { key: "b", label: "DeepSeek", provider: "deepseek", members: [profile({ id: "ds", label: "", provider: "deepseek", signed_in: false }), profile({})] },
    ] } as unknown as AccountsView;
    expect(accountChoices(view)).toEqual([
      { id: "work", label: "Codex", provider: "codex", signedIn: true },
      { id: "ds", label: "DeepSeek", provider: "dsh", signedIn: false },
    ]);
    expect(accountChoices(undefined)).toEqual([]);
    // Task 200 (Qt m_accounts[host]): a member on another computer gets that computer's accounts.
    expect(accountChoices(view, "studio")).toEqual([{ id: "remote", label: "Codex", provider: "codex", signedIn: true }]);
  });

  it("finds the draft a session leads, and lists the drafts newest first (task 200)", () => {
    const lead = { id: "lead", role: "Lead", agent: "claude", state: "active", lead: true, session: { host: "", name: "claude/orbit/lead", run_id: "", conversation_id: "" } };
    const draft = { id: "t1", status: "draft", members: [lead] };
    expect(draftLedBy(draft, { name: "claude/orbit/lead" })).toBe("t1");
    expect(draftLedBy({ ...draft, status: "running" }, { name: "claude/orbit/lead" })).toBe("");
    expect(draftLedBy({ ...draft, members: [{ ...lead, lead: false }] }, { name: "claude/orbit/lead" })).toBe("");
    expect(draftLedBy(draft, { name: "codex/orbit/other" })).toBe("");
    expect(draftLedBy(undefined, { name: "claude/orbit/lead" })).toBe("");
    const teams = [{ id: "a", status: "draft", updated: 5, name: "A", folder: "" }, { id: "b", status: "running", updated: 9, name: "B", folder: "" },
      { id: "c", status: "draft", updated: 7, name: "C", folder: "" }] as unknown as TeamSummary[];
    expect(draftsOf({ teams }).map((team) => team.id)).toEqual(["c", "a"]);
    expect(draftsOf(undefined)).toEqual([]);
  });

  it("sends the selected session's project by name and id, the Inbox neither (task 200)", () => {
    const names: Record<string, string> = { p1: "Orbit" };
    expect(projectOfRow("p1", (id) => names[id])).toEqual({ project: "Orbit", projectId: "p1" });
    expect(projectOfRow("p2", (id) => names[id])).toEqual({ project: "p2", projectId: "p2" });
    expect(projectOfRow("ungrouped", (id) => names[id])).toEqual({});
    expect(projectOfRow("", (id) => names[id])).toEqual({});
  });

  it("searches templates by name, description and roles, favourites first (task 200, Qt TeamTemplateList)", () => {
    const catalog = catalogOf({ version: 3, roles: [{ id: "backend", name: "Backend engineer" }, { id: "qa", name: "Tester" }],
      templates: [{ id: "feature", name: "Feature", description: "One feature", roster: [{ role: "backend", count: 2 }], tasks: [{ title: "a" }] },
        { id: "audit", name: "Audit", description: "Review the code", roster: [{ role: "qa", count: 1 }] },
        { id: "bugfix", name: "Bug fix", description: "", roster: [] }] })!;
    expect(shownTemplates(catalog, "", [], false).map((found) => found.id)).toEqual(["audit", "bugfix", "feature"]);
    expect(shownTemplates(catalog, "", ["feature"], false).map((found) => found.id)).toEqual(["feature", "audit", "bugfix"]);
    expect(shownTemplates(catalog, "", ["feature"], true).map((found) => found.id)).toEqual(["feature"]);
    expect(shownTemplates(catalog, "ENGINEER", [], false).map((found) => found.id)).toEqual(["feature"]);
    expect(shownTemplates(catalog, "qa", [], false).map((found) => found.id)).toEqual(["audit"]);
    expect(shownTemplates(catalog, "review", [], false).map((found) => found.id)).toEqual(["audit"]);
    expect(shownTemplates(catalog, "nothing", [], false)).toEqual([]);
    expect([templateSeats(catalog.templates[0]), templateTaskCount(catalog.templates[0]), templateTaskCount(catalog.templates[1])]).toEqual([2, 1, 0]);
    expect(toggledFavourite(["a"], "b")).toEqual(["a", "b"]);
    expect(toggledFavourite(["a", "b"], "a")).toEqual(["b"]);
  });

  it("offers the catalog's roles once it is read", () => {
    expect(catalogOf(undefined)).toBeUndefined();
    const catalog = catalogOf({ version: 3, roles: [{ id: "lead", name: "Lead" }, { id: "backend", name: "Backend", agent: "codex" }],
      templates: [{ id: "feature", name: "Feature", roster: [{ role: "backend", count: 1 }] }] });
    expect(memberRoles(catalog).map((role) => role.name)).toEqual(["Backend"]);
    expect(catalog!.templates.map((template) => template.id)).toEqual(["feature"]);
  });

  it("Task #326: the lead pane names the session, agent, model and account", () => {
    expect(coordinatorLine({ name: "claude/XP/XP_Start", agent: "claude", model: "claude-opus-4", account: "work" },
      [{ id: "work", label: "Work" }])).toBe("claude/XP/XP_Start, Claude Code, claude-opus-4, Work");
    expect(coordinatorLine({ name: "codex/XP/XP_Start", agent: "codex" })).toBe("codex/XP/XP_Start, Codex");
  });

  it("the selected local session may lead; a peer's or archived one may not", () => {
    const row = { computer: "", name: "dsh/app/coord", model: "deepseek-flash", status: "working", state: "running" } as never;
    expect(coordinatorOf(row)).toEqual({ name: "dsh/app/coord", agent: "dsh", model: "deepseek-flash", busy: true });
    expect(coordinatorOf({ ...(row as object), computer: "studio" } as never)).toBeUndefined();
    expect(coordinatorOf({ ...(row as object), state: "archived" } as never)).toBeUndefined();
    expect(coordinatorOf(undefined)).toBeUndefined();
  });
});
