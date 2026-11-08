import { describe, expect, it } from "vitest";
import { DESIGNER_AGENTS, agentName, designerEfforts, designerModelLabel, effortAllowed, effortLabel, modelSections } from "./designerModels";

describe("the designer's model menu", () => {
  it("offers every agent Qt's list names, DeepSeek with its own default model only", () => {
    const sections = modelSections();
    expect(sections.map((section) => section.agent)).toEqual(DESIGNER_AGENTS.map(([agent]) => agent));
    const dsh = sections.find((section) => section.agent === "dsh")!;
    expect(dsh.models).toEqual([""]);
    expect(sections.find((section) => section.agent === "claude")!.models).toContain("sonnet");
    expect(sections.find((section) => section.agent === "codex")!.models).toEqual(["gpt-5.5-codex", "gpt-5.5"]);
    expect(agentName("dsh")).toBe("DeepSeek");
  });

  it("shows the model (or the agent) and the effort, as Qt's button does", () => {
    expect(designerModelLabel("claude", "sonnet", "high")).toBe("sonnet high");
    expect(designerModelLabel("claude", "", "high")).toBe("Claude Code high");
    expect(designerModelLabel("dsh", "deepseek-reasoner", "high")).toBe("DeepSeek");
    expect(designerModelLabel("kimi", "kimi-code/k3", "low")).toBe("kimi-code/k3");
  });

  it("offers efforts except for the agents Qt disables them for", () => {
    expect(effortAllowed("claude")).toBe(true);
    expect(effortAllowed("dsh")).toBe(false);
    expect(effortAllowed("kimi")).toBe(false);
    expect(designerEfforts("claude")).toEqual(["low", "medium", "high", "xhigh"]);
    expect(designerEfforts("claude", "custom")).toEqual(["low", "medium", "high", "xhigh", "custom"]);
    expect(designerEfforts("dsh", "high")).toEqual([]);
    expect(effortLabel("high")).toBe("Effort: high");
    expect(effortLabel("")).toBe("Effort: —");
  });
});
