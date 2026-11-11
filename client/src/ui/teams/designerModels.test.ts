import { describe, expect, it } from "vitest";
import { agentName, designerModelLabel, effortAllowed, effortLabel } from "./designerModels";

describe("the designer's model menu", () => {
  it("shows the model (or the agent) and the effort, as Qt's button does", () => {
    expect(agentName("dsh")).toBe("DeepSeek");
    expect(designerModelLabel("claude", "sonnet", "high")).toBe("sonnet high");
    expect(designerModelLabel("claude", "", "high")).toBe("Claude Code high");
    expect(designerModelLabel("dsh", "deepseek-reasoner", "high")).toBe("DeepSeek");
    expect(designerModelLabel("kimi", "kimi-code/k3", "low")).toBe("kimi-code/k3");
  });

  it("offers efforts except for the agents whose native settings manage them", () => {
    expect(effortAllowed("claude")).toBe(true);
    expect(effortAllowed("codex")).toBe(true);
    expect(effortAllowed("dsh")).toBe(false);
    expect(effortAllowed("kimi")).toBe(false);
    expect(effortLabel("high")).toBe("Effort: high");
    expect(effortLabel("")).toBe("Effort: —");
  });
});
