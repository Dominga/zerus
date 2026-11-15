import { expect, it } from "vitest";
import { emptyAccounts, type Profile } from "../../types";
import { memberAccounts, supportedChoice, swapBlocked } from "./member-edit";
import { memberActions } from "./actions";
import { memberMenu } from "./members";
import { teamView } from "./fixtures";
it("only offers current installed local accounts for the selected agent", () => {
  const p = (id: string, change: Partial<Profile> = {}) => ({ id, label: id, host: "", provider: "claude", state: "ready", signed_in: true, installed: true, ...change }) as Profile;
  const rows = [p("local"), p("remote", { host: "example-host" }), p("other", { provider: "codex" }), p("absent", { installed: false })];
  const view = { ...emptyAccounts(), accounts: [{ members: rows }] } as ReturnType<typeof emptyAccounts>;
  expect(memberAccounts(view, "claude").map((a) => a.id)).toEqual(["local"]);
});
it("swap requires the full writer's zero-work fact, a person stop and a binding", () => {
  const stopped = { ...teamView.members[1], state: "paused", person_stopped: true, open_work: 0 };
  expect(swapBlocked(stopped)).toBeUndefined();
  expect(memberActions(stopped, "running").find((a) => a.id === "swap_member")?.disabled).toBe(false);
  expect(memberMenu(stopped).find((a) => a.id === "swap")?.enabled).toBe(true);
  for (const change of [{ state: "active" }, { person_stopped: false }, { open_work: 1 }, { open_work: undefined }, { session: null }])
    expect(swapBlocked({ ...stopped, ...change })).toBeTruthy();
});

it("keeps only choices the native launch path supports", () => {
  const choice = { agent: "dsh", model: "example-model", effort: "high", account: { mode: "freest" as const } };
  expect(supportedChoice(choice)).toEqual({ ...choice, model: "", effort: "" });
  expect(supportedChoice({ ...choice, agent: "kimi" })).toEqual({ ...choice, agent: "kimi", effort: "" });
  expect(supportedChoice({ ...choice, agent: "codex" })).toEqual({ ...choice, agent: "codex" });
});
