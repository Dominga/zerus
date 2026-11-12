import { describe, expect, it } from "vitest";
import {
  DEFAULT_TASK_TEMPLATE, KNOWN_MODELS, ROLE_ICONS, SWATCHES, agentChange, chosenIcon, chosenSwatch, createModelCatalogs, dropBlock, effortOptions,
  fieldDiff, iconValue, includeBlock, modelKey, modelOptions, moveBlock, plateAnswers, promptBlocks, promptOrder, proposalMark, proposalOf, swatchColor,
  type ModelChoice,
} from "./catalogControls";
import type { CatalogProposal } from "./catalog";

// Task 50: Qt TeamCatalogEditor's role controls, one block per control.

describe("icon and colour pickers (Qt teamRoleSwatch, teamRoleIcon)", () => {
  it("offers the palette's 16 colours and 10 icons in the prototype's order", () => {
    expect(SWATCHES.map((swatch) => swatch.name)).toEqual(["lead", "architect", "backend", "frontend", "tester", "reviewer", "security", "integrator",
      "researcher", "secretary", "coder", "qa", "designer", "leadgpt", "expert", "proexpert"]);
    expect([...ROLE_ICONS]).toEqual(["lead", "architect", "backend", "frontend", "tester", "reviewer", "security", "integrator", "researcher", "secretary"]);
  });

  it("marks the role's own colour, else the one its name keys to; an older #hex shows as it is", () => {
    expect(chosenSwatch({ name: "Backend", color: "qa" })).toBe("qa");
    expect(chosenSwatch({ name: "Front End" })).toBe("frontend");
    expect(swatchColor("tester", true)).toBe("#56c8e0");
    expect(swatchColor("tester", false)).toBe("#137f95");
    expect(swatchColor("#123456", true)).toBe("#123456");
    expect(swatchColor("unknown", true)).toBe("");
  });

  it("marks the role's own icon (stored as r_<icon>), else the one its id or name names, and stores a pick as r_<icon>", () => {
    expect(chosenIcon({ id: "x", name: "X", icon: "r_security" })).toBe("security");
    expect(chosenIcon({ id: "x", name: "X", icon: "reviewer" })).toBe("reviewer");
    expect(chosenIcon({ id: "api-backend", name: "API" })).toBe("backend");
    expect(chosenIcon({ id: "docs", name: "Docs" })).toBe("");
    expect(iconValue("tester")).toBe("r_tester");
  });
});

describe("prompt order rows (Qt teamPromptOrder)", () => {
  const role = { name: "Backend", notes: { note1: "Use the v2 API" } };

  it("shows the fixed blocks then the role's notes, each titled as in Qt, with what is switched off", () => {
    expect(promptOrder(role)).toEqual(["rules", "role", "protocol", "task", "note1"]);
    expect(promptOrder({ ...role, prompt_order: ["task", "role"] })).toEqual(["task", "role", "rules", "protocol", "note1"]);
    expect(promptBlocks({ ...role, prompt_off: ["protocol"] }).map((block) => [block.key, block.title, block.off, block.fixed])).toEqual([
      ["rules", "Project rules", false, true], ["role", "Role Backend", false, true], ["protocol", "Team protocol", true, true],
      ["task", "Task (per attempt)", false, true], ["note1", "Note", false, false]]);
  });

  it("moves a block up or down, never past either end, and writes the whole order", () => {
    expect(moveBlock(role, 0, -1)).toBeUndefined();
    expect(moveBlock(role, 4, 1)).toBeUndefined();
    expect(moveBlock(role, 1, -1)).toEqual({ prompt_order: ["role", "rules", "protocol", "task", "note1"], prompt_off: [], task_template: DEFAULT_TASK_TEMPLATE });
  });

  it("a dragged row drops at its place", () => {
    expect(dropBlock(role, 3, 0)?.prompt_order).toEqual(["task", "rules", "role", "protocol", "note1"]);
    expect(dropBlock(role, 2, 2)).toBeUndefined();
  });

  it("include switches a block on or off; a role without a task text sends the default one", () => {
    expect(includeBlock(role, "rules", false)).toMatchObject({ prompt_off: ["rules"], task_template: DEFAULT_TASK_TEMPLATE });
    expect(includeBlock({ ...role, prompt_off: ["rules"], task_template: "#{id}" }, "rules", true)).toEqual({ prompt_order: promptOrder(role), prompt_off: [] });
  });
});

describe("model and effort menus (Qt teamRoleModel, teamRoleEffort)", () => {
  const catalog: ModelChoice[] = [{ id: "gpt-5.5", label: "GPT-5.5", effort_options: ["low", "high"] }, { id: "o4", label: "o4", effort_options: [] }];

  it("offers the account's models, else the agent's known ones, and keeps the role's own model", () => {
    expect(modelOptions({ agent: "codex", model: "gpt-5.5" }, catalog)).toEqual([{ id: "gpt-5.5", label: "GPT-5.5" }, { id: "o4", label: "o4" }]);
    expect(modelOptions({ agent: "claude" }, undefined).map((option) => option.id)).toEqual(KNOWN_MODELS.claude);
    expect(modelOptions({ agent: "codex", model: "custom" }, []).map((option) => option.id)).toEqual(["gpt-5.5-codex", "gpt-5.5", "custom"]);
  });

  it("offers the chosen model's own efforts and uses the static list only without a catalog", () => {
    expect(effortOptions("gpt-5.5", catalog)).toEqual(["", "low", "high"]);
    expect(effortOptions("o4", catalog)).toEqual([""]);
    expect(effortOptions("gpt-5.5", catalog, "max")).toEqual(["", "low", "high", "max"]);
    expect(effortOptions("o4", undefined)).toEqual(["", "low", "medium", "high", "xhigh"]);
  });

  it("another agent takes its first model and the default account", () => {
    expect(agentChange("kimi")).toEqual({ agent: "kimi", account: "", model: "kimi-code/kimi-for-coding" });
    expect(agentChange("opencode")).toEqual({ agent: "opencode", account: "", model: "" });
  });

  it("reads each computer's account catalog once, the native one without an account, and none for OpenCode", async () => {
    const reads: string[] = [];
    let changes = 0;
    const catalogs = createModelCatalogs(async (computer, agent, account) => {
      reads.push(`${computer}|${agent}|${account}`);
      if (account === "broken") throw new Error("no");
      return catalog;
    }, () => changes++);
    catalogs.request({ agent: "codex", account: "" });
    catalogs.request({ agent: "codex", account: "" });
    catalogs.request({ agent: "opencode" });
    catalogs.request({ agent: "claude", account: "broken", computer: "studio" });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(reads).toEqual(["|codex|", "studio|claude|broken"]);
    expect(modelKey({ agent: "codex" })).toBe("\nnative-codex");
    expect(catalogs.get({ agent: "codex" })).toEqual(catalog);
    expect(catalogs.get({ agent: "claude", account: "broken", computer: "studio" })).toEqual([]);   // the known models stay offered
    expect(changes).toBe(2);
    catalogs.request({ agent: "codex" });
    expect(reads).toHaveLength(2);
  });
});

describe("per-item proposal plates (Qt teamProposalPlate)", () => {
  const proposal = (over: Partial<CatalogProposal>): CatalogProposal => ({
    id: "p1", kind: "role", action: "update", target: "tester", title: "Tester model", reason: "Cheaper", state: "proposed", value: {}, text: "", at: 0, ...over,
  });

  it("finds the proposal on an item by kind and id", () => {
    const list = [proposal({}), proposal({ id: "p2", kind: "template", target: "tester" })];
    expect(proposalOf(list, "template", "tester")?.id).toBe("p2");
    expect(proposalOf(list, "role", "lead")).toBeUndefined();
  });

  it("marks NEW, REMOVED or CHANGED, kept once kept, rejected; Keep and Reject while proposed, Undo after", () => {
    expect(proposalMark(proposal({ action: "add" }))).toEqual({ text: "NEW", tone: "accent" });
    expect(proposalMark(proposal({ action: "remove" }))).toEqual({ text: "REMOVED", tone: "danger" });
    expect(proposalMark(proposal({ state: "kept" }))).toEqual({ text: "CHANGED, kept", tone: "folder" });
    expect(proposalMark(proposal({ state: "rejected" }))).toEqual({ text: "rejected", tone: "dim" });
    expect(plateAnswers("proposed")).toEqual(["reject", "keep"]);
    expect(plateAnswers("kept")).toEqual(["undo"]);
  });

  it("diffs only the fields the proposal changes", () => {
    const item = { id: "tester", name: "Tester", model: "kimi-k2", skills: ["e2e"] };
    expect(fieldDiff(item, proposal({ value: { id: "tester", name: "Tester", model: "sonnet", skills: ["e2e", "load"] } }))).toEqual([
      { field: "model", from: "kimi-k2", to: "sonnet" }, { field: "skills", from: "e2e", to: "e2e, load" }]);
    expect(fieldDiff(undefined, proposal({ action: "add", value: { id: "qa", name: "QA" } }))).toEqual([{ field: "name", from: "", to: "QA" }]);
    expect(fieldDiff(item, proposal({ action: "remove" }))).toEqual([]);
  });
});
