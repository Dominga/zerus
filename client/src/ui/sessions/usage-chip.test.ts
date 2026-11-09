import { describe, expect, it } from "vitest";
import { usageChipView } from "../lib/usage";
import { emptyUsage, type ProjectedWindow, type Usage } from "../../types";

// Task #351: the composer's usage chip. Claude and Codex show the account's
// windows (two bars, the weekly percent and the days to its reset); a DeepSeek
// (dsh) session shows the account balance, or why it is unknown. The bars are
// the same UsageMeters the status bar drew; what the chip says is this view.
const NOW = 1_800_000_000;
const window = (id: string, minutes: number, used: number, resetsInMinutes: number): ProjectedWindow =>
  ({ id, label: "", used, minutes, resets_at: NOW + resetsInMinutes * 60 });

function usage(provider: string, windows: ProjectedWindow[], extra: Partial<Usage> = {}): Usage {
  return { ...emptyUsage(provider), status: "ok", windows, ...extra };
}

describe("the composer's usage chip (task #351)", () => {
  it("draws Claude's two bars, its weekly percent and the days to the weekly reset", () => {
    const claude = usage("claude", [window("five_hour", 300, 34, 120), window("seven_day", 10080, 73, 3 * 1440 + 4 * 60)]);
    const view = usageChipView(claude, NOW, "claude");
    expect(view.kind).toBe("windows");
    // Both current windows draw a bar; the weekly one is the longer.
    expect(view.windows).toHaveLength(2);
    expect(view.windows.map((w) => w.minutes)).toEqual([300, 10080]);
    expect(view.weekly?.minutes).toBe(10080);
    expect(view.percent).toBe(73);
    expect(view.reset).toBe("3d 4h");
    expect(view.label).toBe("Usage 73%, resets in 3d 4h");
  });

  it("draws Codex's weekly window on its own, with its percent and reset", () => {
    const codex = usage("codex", [window("weekly", 10080, 41, 5 * 1440 + 2 * 60)]);
    const view = usageChipView(codex, NOW, "codex");
    expect(view.kind).toBe("windows");
    expect(view.windows).toHaveLength(1);
    expect(view.percent).toBe(41);
    expect(view.reset).toBe("5d 2h");
    expect(view.label).toBe("Usage 41%, resets in 5d 2h");
    // An ended window is not a current bar and never reaches the chip.
    const ended = usage("codex", [window("weekly", 10080, 41, -30)]);
    expect(usageChipView(ended, NOW, "codex").windows).toHaveLength(0);
  });

  it("shows a DeepSeek session's balance instead of bars", () => {
    const dsh = usage("dsh", [], { balances: [{ kind: "balance", balance: "12.50", currency: "USD" }] });
    const view = usageChipView(dsh, NOW, "dsh");
    expect(view.kind).toBe("balance");
    expect(view.windows).toHaveLength(0);
    expect(view.balance).toBe("12.50 USD");
    expect(view.label).toBe("Balance 12.50 USD");
    // The session's agent names the provider when the account read has none yet.
    expect(usageChipView(usage("", []), NOW, "dsh").kind).toBe("balance");
  });

  it("says why a DeepSeek balance is unknown", () => {
    const signedOut = usage("dsh", [], { status: "signed_out" });
    const view = usageChipView(signedOut, NOW, "dsh");
    expect(view.kind).toBe("balance");
    expect(view.balance).toBeUndefined();
    expect(view.label).toBe("Balance unknown");
    expect(view.reason).toContain("DeepSeek account is signed out");
  });

  it("lives in the composer, and the status bar keeps no agent or usage block", () => {
    // Read from disk, as the other style guards do (no Node types here).
    const read = (path: string) => fs.readFileSync(new URL(path, repo), "utf8");
    expect(read("client/src/App.tsx")).not.toContain("SessionUsageStrip");
    expect(read("client/src/ui/sessions/Composer.tsx")).toContain("UsageLimitChip");
  });
});

const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const repo = new URL("../../../../", import.meta.url);
