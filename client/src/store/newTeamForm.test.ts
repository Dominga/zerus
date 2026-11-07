import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import {
  applyPolicy, branchHint, canAdd, createReason, effectivePolicy, folderPolicy, leadMember, memberId, memberOf, memberRoles, membersWithIds,
  memoryMode, pickTemplate, startReason, templateTasks, withMemory, type LeadChoice,
} from "./newTeamForm";
import { createNewTeam, type NewTeamPort } from "./teamsNew";
import { followDraft } from "./newTeamFollow";
import { createTeams } from "./teamsView";
import { parseCatalog } from "../ui/teams/catalog";
import type { NewTeamView, TeamCall, TeamOutcome } from "../ui/teams/types";

// Task 42: Qt TeamNewTab's fields and actions on the Tauri New team form:
// templates, folder policy, members (+ Member, without a role, remove),
// the lead (this session or a new one), max members, Discard draft, and the
// creation rules (opening creates nothing, Start planning only the draft and
// its lead, Create team the members). The lead's draft edits reach the form.

const TEAM = "33fbafeb-8291-4eda-a00c-6e1448ec6f20";

const catalog = parseCatalog({
  version: 3, builtin: false,
  roles: [
    { id: "lead", name: "Lead", agent: "claude", model: "opus", prompt: "You lead.", effort: "", account: "", permissions: "", computer: "", folder: "" },
    { id: "backend", name: "Backend", agent: "codex", model: "gpt-5.5", prompt: "Build it.", effort: "high", account: "work", permissions: "auto", computer: "", folder: "worktree" },
    { id: "reviewer", name: "Reviewer", agent: "claude", model: "sonnet", prompt: "Review it.", effort: "", account: "", permissions: "", computer: "studio", folder: "read_only" },
  ],
  templates: [
    { id: "feature", name: "Feature delivery", description: "Build and review", planning: "template", approval: "auto", max_members: 4, pipeline: "code",
      phase_approval: "review", lead_protocol: "Split by layer.", roster: [{ role: "backend", count: 2, optional: false }, { role: "reviewer", count: 1, optional: false }] },
  ],
  routing: [], planning_rules: "", pipelines: [], draft_base: 0, proposals: [],
} as unknown as Record<string, unknown>);

const NEW: LeadChoice = { mode: "new", session: null, agent: "dsh", model: " deepseek-flash ", effort: "high", account: "work" };

describe("New team form rules", () => {
  it("a template seats its roster as members and brings its planning, approvals, limit and protocol", () => {
    const picked = pickTemplate(catalog, "feature", { settings: { folder_policy: "auto" }, addendum: "", addendumEdited: false }, true);
    expect(picked.members.map((member) => [member.role, member.agent, member.model, member.effort, member.account, member.permissions, member.folder])).toEqual([
      ["Backend", "codex", "gpt-5.5", "high", "work", "auto", "worktree"],
      ["Backend", "codex", "gpt-5.5", "high", "work", "auto", "worktree"],
      ["Reviewer", "claude", "sonnet", "", "", "", "shared"],
    ]);
    expect(picked.members[2].computer).toBe("studio");
    expect(picked.settings).toEqual({ folder_policy: "auto", approval: "auto", phase_approval: "review", max_members: 4 });
    expect(picked.planning).toBe("template");
    expect(picked.addendum).toBe("Split by layer.");
    // The owner's own protocol stays; No template seats nobody.
    expect(pickTemplate(catalog, "feature", { settings: {}, addendum: "Mine", addendumEdited: true }, true).addendum).toBe("Mine");
    expect(pickTemplate(catalog, "-", { settings: { max_members: 6 }, addendum: "", addendumEdited: false }, true)).toEqual({ members: [], settings: { max_members: 6 }, planning: "lead", addendum: "" });
    // Without worktrees every member shares the folder.
    expect(pickTemplate(catalog, "feature", { settings: {}, addendum: "", addendumEdited: false }, false).members.every((member) => member.folder === "shared")).toBe(true);
  });

  it("+ Member offers every role but the lead's, or a member without a role, within the limit", () => {
    expect(memberRoles(catalog).map((role) => role.id)).toEqual(["backend", "reviewer"]);
    expect(memberOf(undefined, true)).toMatchObject({ role: "", agent: "claude", folder: "worktree", session: null, lead: false });
    expect(canAdd([memberOf(undefined, true)], { max_members: 2 })).toBe(true);
    expect(canAdd([memberOf(undefined, true), memberOf(undefined, true)], { max_members: 2 })).toBe(false);
    expect(canAdd(new Array(5).fill(memberOf(undefined, true)), {})).toBe(true);   // 6 by default, the lead not counted
  });

  it("new rows get ids by role, numbered when taken; the draft's ids stay", () => {
    expect(memberId("Backend dev", new Set())).toBe("backend-dev");
    expect(memberId("", new Set())).toBe("member");
    expect(memberId("Backend", new Set(["backend"]))).toBe("backend-2");
    const ids = membersWithIds([{ ...memberOf(undefined, true), id: "backend", role: "Backend" }, { ...memberOf(undefined, true), role: " Backend " },
      { ...memberOf(undefined, true), role: "" }]).map((member) => [member.id, member.role]);
    expect(ids).toEqual([["backend", "Backend"], ["backend-2", "Backend"], ["member", ""]]);
  });

  it("the folder policy: Auto by default, Shared puts everyone in the team folder, a non-Git folder forces Shared", () => {
    expect(folderPolicy({})).toBe("auto");
    expect(folderPolicy({ folder_policy: "worktree" })).toBe("worktree");
    expect(effectivePolicy("worktree", "C:\\work\\app", true)).toEqual({ policy: "worktree", forced: false });
    expect(effectivePolicy("auto", "C:\\plain", false)).toEqual({ policy: "shared", forced: true });
    const members = [memberOf(undefined, true), { ...memberOf(undefined, true), folder: "read_only" }];
    expect(applyPolicy(members, "shared", true).map((member) => member.folder)).toEqual(["shared", "shared"]);
    expect(applyPolicy(members, "worktree", true).map((member) => member.folder)).toEqual(["worktree", "read_only"]);
    expect(applyPolicy(members, "auto", false).map((member) => member.folder)).toEqual(["shared", "shared"]);
    expect(branchHint("Checkout Redesign", true)).toBe("branch team/checkout-redesign/<role>");
    expect(branchHint("x", false)).toBe("");
  });

  it("the lead is this session (an existing one) or a new one with its agent, model, effort and account", () => {
    const self: LeadChoice = { ...NEW, mode: "this", session: { name: "claude/app/coord", agent: "claude" } };
    expect(leadMember(self, "You lead.")).toMatchObject({ id: "lead", role: "Lead", agent: "claude", model: "", account: "",
      session: { host: "", name: "claude/app/coord" }, prompt: "You lead." });
    expect(leadMember(NEW, "")).toMatchObject({ agent: "dsh", model: "deepseek-flash", effort: "high", account: "work", session: null });
    expect(startReason("", false, NEW)).toBe("Choose an existing folder");
    expect(startReason("C:\\app", true, { ...NEW, mode: "this", session: null })).toBe("Choose who leads");
    expect(startReason("C:\\app", true, NEW)).toBe("");
  });

  it("Create team waits, in Qt's order, for the draft, the goal, the name, the folder, the rows, the limit and the preflight", () => {
    const ok = { drafting: true, goal: "Ship", name: "Orbit", folderOk: true, badRows: 0, members: 2, max: 6, preflightErrors: 0 };
    expect(createReason({ ...ok, drafting: false })).toBe("Start planning first");
    expect(createReason({ ...ok, goal: " " })).toBe("Write the goal, or let the lead write it");
    expect(createReason({ ...ok, name: "" })).toBe("Name the team");
    expect(createReason({ ...ok, badRows: 2 })).toBe("Fix 2 member rows");
    expect(createReason({ ...ok, members: 7 })).toBe("More members than the limit (the lead is not counted)");
    expect(createReason({ ...ok, preflightErrors: 1 })).toBe("Fix 1 preflight error");
    expect(createReason(ok)).toBe("");
  });

  it("Use template tasks assigns the template's tasks to the members it seats", () => {
    const withTasks = parseCatalog({ ...catalog, roles: catalog.roles.map((role) => role.raw),
      templates: [{ ...catalog.templates[0].raw, tasks: [{ title: "API", role: "backend" }, { title: "Review", role: "reviewer", deps: [0] }] }] } as unknown as Record<string, unknown>);
    const members = pickTemplate(withTasks, "feature", { settings: {}, addendum: "", addendumEdited: false }, true).members;
    expect(templateTasks(withTasks, "feature", "lead", members)).toEqual([]);
    expect(templateTasks(withTasks, "feature", "template", members).map((task) => [task.id, task.title, task.assignee, task.deps])).toEqual([
      [1, "API", "backend", []], [2, "Review", "reviewer", [{ task: 1, hard: true }]]]);
  });
});

class Port implements NewTeamPort {
  calls: TeamCall[] = [];
  replies: TeamOutcome[] = [];
  async action(call: TeamCall): Promise<TeamOutcome> {
    this.calls.push(call);
    return this.replies.shift() ?? { outcome: "done", receipt: call.action.kind === "draft_start" ? { team: TEAM, revision: 1 } : { draft_revision: 2 }, request_id: "r" };
  }
}

function view(over: Partial<NewTeamView> = {}): NewTeamView {
  return { team: TEAM, name: "Orbit", goal: "Ship it", addendum: "", folder: "C:\\app", settings: { folder_policy: "auto" }, members: [], tasks: [],
    draft: { revision: 2, unseen: [], owner_edited: [] }, validation: { errors: [], warnings: [] }, updated: 0, ...over };
}

describe("New team creation rules", () => {
  it("opening creates nothing; Start planning creates only the draft and its lead; the form's fields save next, without sessions; Create team creates the members", async () => {
    const port = new Port();
    const store = createNewTeam(port);
    store.openNew();
    const picked = pickTemplate(catalog, "feature", { settings: {}, addendum: "", addendumEdited: false }, true);
    store.editMany({ members: picked.members, settings: picked.settings, addendum: picked.addendum });
    store.edit("goal", "Ship the checkout");
    expect(port.calls).toEqual([]);
    await store.startPlanning({ folder: "C:\\app", name: "Orbit", lead: leadMember(NEW, ""), template: "feature" });
    expect(port.calls.map((call) => call.action.kind)).toEqual(["draft_start"]);
    const start = port.calls[0].action;
    if (start.kind !== "draft_start") throw new Error("kind");
    expect([start.lead.id, start.lead.agent, start.template]).toEqual(["lead", "dsh", "feature"]);
    expect(JSON.stringify(start)).not.toContain("Backend");   // the members are not started with the draft
    expect([...store.dirty()].sort()).toEqual(["addendum", "goal", "members", "settings"]);
    await store.flush();
    const update = port.calls[1].action;
    if (update.kind !== "draft_update") throw new Error("kind");
    expect(update.revision).toBe(1);
    const members = update.set.members as { id: string; session: unknown }[];
    expect(members.map((member) => member.id)).toEqual(["backend", "backend-2", "reviewer"]);
    expect(members.every((member) => member.session === null)).toBe(true);
    expect(update.set.goal).toBe("Ship the checkout");
    expect(store.dirty().size).toBe(0);
    expect(store.revision()).toBe(2);
    expect(port.calls.some((call) => call.action.kind === "draft_create")).toBe(false);
    store.applyView(view({ members: members as never }));
    await store.createTeam();
    expect(port.calls.map((call) => call.action.kind)).toEqual(["draft_start", "draft_update", "draft_create"]);
  });

  it("a save refused for a newer revision goes again once on top of it", async () => {
    const port = new Port();
    const store = createNewTeam(port);
    store.applyView(view());
    store.edit("name", " Orbit 2 ");
    port.replies = [{ outcome: "failed", detail: "hgs: Draft revision is 5 now; read it again", request_id: "r" }];
    await store.flush();
    expect(port.calls.map((call) => call.action.kind === "draft_update" ? [call.action.revision, call.action.set.name] : null)).toEqual([[2, "Orbit 2"], [5, "Orbit 2"]]);
    expect(store.dirty().size).toBe(0);
  });

  it("Discard draft sends the owner's confirmed discard, with the lead's session stopped or kept, and the form starts over", async () => {
    const port = new Port();
    const store = createNewTeam(port);
    store.openNew();
    store.applyView(view());
    await store.discard(true);
    expect(port.calls).toEqual([{ team: TEAM, team_name: "Orbit", confirmed: true, action: { kind: "draft_discard", stop_lead: true } }]);
    expect(store.draftId()).toBe("");
    expect(store.open()).toBe(true);
  });

  it("the lead's draft edits appear in the open form; the owner's unsaved field stays, and Show the lead's version drops it", () => {
    const port = new Port();
    const store = createNewTeam(port);
    const teams = createTeams({ follow: async () => {} });
    const shown: string[] = [];
    store.applyView(view());
    // The effects run once the root is set up, as in a mounted form.
    const dispose = createRoot((done) => {
      followDraft({ computer: () => "", draft: store.draftId, show: (_, team) => { shown.push(team); return () => {}; },
        view: (computer, id) => teams.newTeam(computer, id), apply: store.applyView });
      return done;
    });
    expect(shown).toEqual([TEAM]);
    store.edit("goal", "My unsaved goal");
    // The lead runs `hgs team draft` update: a new revision arrives in a batch.
    teams.apply([{ computer: "", key: `new:${TEAM}`, kind: "new_team",
      payload: view({ name: "Orbit by the lead", goal: "The lead's goal", draft: { revision: 3, unseen: ["name", "goal"], owner_edited: [] } }) }]);
    expect(store.fields().name).toBe("Orbit by the lead");
    expect(store.fields().goal).toBe("My unsaved goal");
    expect(store.unseen()).toEqual(["name", "goal"]);
    // An older read changes nothing.
    teams.apply([{ computer: "", key: `new:${TEAM}`, kind: "new_team", payload: view({ name: "Old", draft: { revision: 1, unseen: [], owner_edited: [] } }) }]);
    expect(store.fields().name).toBe("Orbit by the lead");
    store.useLeadVersion();
    expect(store.fields().goal).toBe("The lead's goal");
    expect(store.dirty().size).toBe(0);
    dispose();
  });
});

describe("task 126: the team's memory mode", () => {
  it("is RustCodeLore unless plain was chosen, and keeps the other settings", () => {
    expect(memoryMode({})).toBe("lore");
    expect(memoryMode({ memory: { mode: "lore" } })).toBe("lore");
    expect(memoryMode({ memory: { mode: "plain" } })).toBe("plain");
    expect(withMemory({ max_members: 4, memory: { mode: "lore", store: "/old" } }, "plain")).toEqual({ max_members: 4, memory: { mode: "plain" } });
  });
});
