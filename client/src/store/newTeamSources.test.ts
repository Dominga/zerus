import { describe, expect, it } from "vitest";
import { accountChoices, catalogOf, computerChoices, coordinatorOf } from "./newTeamSources";
import { memberRoles } from "./newTeamForm";
import type { AccountsView } from "../types";

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
      { id: "work", label: "Work", provider: "codex", signedIn: true },
      { id: "ds", label: "DeepSeek", provider: "dsh", signedIn: false },
    ]);
    expect(accountChoices(undefined)).toEqual([]);
  });

  it("offers the catalog's roles once it is read", () => {
    expect(catalogOf(undefined)).toBeUndefined();
    const catalog = catalogOf({ version: 3, roles: [{ id: "lead", name: "Lead" }, { id: "backend", name: "Backend", agent: "codex" }],
      templates: [{ id: "feature", name: "Feature", roster: [{ role: "backend", count: 1 }] }] });
    expect(memberRoles(catalog).map((role) => role.name)).toEqual(["Backend"]);
    expect(catalog!.templates.map((template) => template.id)).toEqual(["feature"]);
  });

  it("the selected local session may lead; a peer's or archived one may not", () => {
    const row = { computer: "", name: "dsh/app/coord", model: "deepseek-flash", status: "working", state: "running" } as never;
    expect(coordinatorOf(row)).toEqual({ name: "dsh/app/coord", agent: "dsh", model: "deepseek-flash", busy: true });
    expect(coordinatorOf({ ...(row as object), computer: "studio" } as never)).toBeUndefined();
    expect(coordinatorOf({ ...(row as object), state: "archived" } as never)).toBeUndefined();
    expect(coordinatorOf(undefined)).toBeUndefined();
  });
});
