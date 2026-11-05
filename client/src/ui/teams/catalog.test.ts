import { describe, expect, it } from "vitest";
import {
  applyConfirmation,
  parseCatalog,
  parseRole,
  proposalCounts,
  roleById,
  route,
  templateById,
  visibleTemplates,
  withProposals,
  type Catalog,
} from "./catalog";

function catalog(): Catalog {
  return parseCatalog({
    version: 3,
    builtin: false,
    roles: [
        { id: "tester", name: "Tester", agent: "kimi", model: "kimi-k2", prompt: "Test it." },
        { id: "backend", name: "Backend", agent: "dsh", model: "deepseek-v4" },
      ],
      templates: [
        {
          id: "feature",
          name: "Feature delivery",
          description: "Build a feature",
          roster: [{ role: "tester", count: 1 }],
          planning: "lead",
          approval: "review",
          max_members: 6,
          pipeline: "code",
        },
        {
          id: "docs",
          name: "Documentation",
          description: "Write the docs",
          roster: [{ role: "backend", count: 1 }],
          planning: "lead",
          approval: "auto",
          max_members: 3,
        },
      ],
      routing: [
        { id: "add-tester", when: "tests", use: "+tester", approval: "" },
        { id: "features", when: "feature", template: "", use: "feature", approval: "review" },
      ],
      pipelines: [
        {
          id: "code",
          name: "Code: review, test",
          stages: [
            { kind: "review", role: "tester" },
            { kind: "test", role: "tester" },
          ],
          max_rounds: 3,
          folder: "hold",
          accept: "lead",
        },
      ],
    planning_rules: "",
    draft_base: 2,
    proposals: [
        {
          id: "p1",
          kind: "role",
          action: "update",
          target: "tester",
          title: "Tester model",
          state: "proposed",
          value: { id: "tester", name: "Tester", agent: "kimi", model: "Sonnet", prompt: "Test it." },
        },
        {
          id: "p2",
          kind: "role",
          action: "add",
          target: "security",
          title: "Security",
          state: "kept",
          value: { id: "security", name: "Security", agent: "claude", model: "claude-sonnet-4-6" },
        },
        {
          id: "p3",
          kind: "role",
          action: "remove",
          target: "backend",
          title: "Remove backend",
          state: "rejected",
          value: { id: "backend" },
        },
      ],
  });
}

describe("catalog model", () => {
  it("applies kept proposals, and undecided ones only when the draft shows them", () => {
    const c = catalog();
    const kept = withProposals(c, false);
    expect(roleById(kept, "security")?.name).toBe("Security");
    expect(roleById(kept, "tester")?.agent).toBe("kimi");
    expect(roleById(kept, "tester")?.model).toBe("kimi-k2");
    expect(roleById(kept, "backend")?.name).toBe("Backend");
    expect(withProposals(c, true).roles.find((role) => role.id === "tester")?.model).toBe("Sonnet");
    expect(c.roles.find((role) => role.id === "backend")?.name).toBe("Backend");
    // A role survives the trip through JSON.
    expect(parseRole(roleById(c, "tester")!.raw).model).toBe("kimi-k2");
  });

  it("removes a target on a kept remove proposal and never applies a rejected one", () => {
    const c = catalog();
    const remove = {
      ...c.proposals[0],
      id: "p4",
      action: "remove",
      target: "backend",
      state: "kept",
    };
    const withRemove = withProposals({ ...c, proposals: [remove] }, true);
    expect(roleById(withRemove, "backend")).toBeUndefined();
    expect(withProposals(c, true).roles.find((role) => role.id === "security")?.model).toBe(
      "claude-sonnet-4-6",
    );
  });

  it("counts kept and undecided changes, and Apply names what it drops", () => {
    const c = catalog();
    expect(proposalCounts(c)).toEqual({ kept: 1, undecided: 1, rejected: 1 });
    expect(applyConfirmation(c)).toBe(
      "Write 1 kept change as the next catalog version? The 1 undecided change is dropped with the draft.",
    );
    expect(applyConfirmation({ ...c, proposals: [] })).toBe(
      "Write 0 kept changes as the next catalog version? The 0 undecided changes are dropped with the draft.",
    );
  });

  it("lists templates by search, favourites first, with No template last", () => {
    const c = catalog();
    expect(visibleTemplates(c)).toEqual(["docs", "feature", "-"]);
    expect(visibleTemplates(c, { favourites: ["feature"] })).toEqual(["feature", "docs", "-"]);
    expect(visibleTemplates(c, { query: "documentation" })).toEqual(["docs", "-"]);
    expect(visibleTemplates(c, { query: "tester" })).toEqual(["feature", "-"]);
    expect(visibleTemplates(c, { favouritesOnly: true, favourites: ["docs"] })).toEqual(["docs", "-"]);
    expect(templateById(c, "feature")?.maxMembers).toBe(6);
  });

  it("routes a goal: a +role rule adds a seat and a template rule wins", () => {
    const c = catalog();
    const matched = route(c, "Add the feature tests");
    expect(matched.templateId).toBe("feature");
    expect(matched.added).toEqual(["tester"]);
    expect(matched.approval).toBe("review");
    expect(route(c, "Something else").templateId).toBe("feature");
  });

  it("reads the projected CatalogView shape", () => {
    const view = parseCatalog({
      version: 4,
      builtin: false,
      roles: [{ id: "tester", name: "Tester", agent: "kimi", model: "Sonnet" }],
      templates: [],
      routing: [],
      planning_rules: "",
      pipelines: [],
      draft_base: 3,
      proposals: [
        {
          id: "p1",
          kind: "role",
          action: "update",
          target: "tester",
          title: "Tester",
          state: "kept",
          value: { id: "tester", model: "Sonnet" },
        },
      ],
    });
    expect(view.version).toBe(4);
    expect(view.draftBase).toBe(3);
    expect(view.proposals[0].state).toBe("kept");
    expect(roleById(view, "tester")?.model).toBe("Sonnet");
  });
});
