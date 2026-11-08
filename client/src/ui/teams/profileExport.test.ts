import { describe, expect, it } from "vitest";
import type { TeamProfileView } from "../../types";
import { profileExport, profileFileName } from "./profileExport";

function profile(): TeamProfileView {
  return {
    elapsed_s: 60,
    worked_s: 30,
    accepted: 1,
    tasks: 2,
    tokens: 100,
    tokens_partial: false,
    losses: [],
    roles: [],
    task_rows: [],
    task_rows_more: false,
  } as unknown as TeamProfileView;
}

describe("the profile export", () => {
  it("holds the team, its scope and the profile Qt exports", () => {
    const value = profileExport("33fbafeb-8291-4eda-a00c-6e1448ec6f20", "plan", profile());
    expect(value.team).toBe("33fbafeb-8291-4eda-a00c-6e1448ec6f20");
    expect(value.scope).toBe("plan");
    expect(value.profile?.accepted).toBe(1);
    expect("spans" in value).toBe(false);
    expect("compare" in value).toBe(false);
  });

  it("keeps the time view's spans and a comparison when the view has them", () => {
    const value = profileExport("t", "phase:2", null, { spans: { spans: [] }, compare: { to: 1 } });
    expect(value.profile).toBeUndefined();
    expect(value.spans).toEqual({ spans: [] });
    expect(value.compare).toEqual({ to: 1 });
  });

  it("names the file as Qt's save dialog proposes", () => {
    expect(profileFileName("33fbafeb-8291-4eda", "plan")).toBe("profile-33fbafeb-plan.json");
    expect(profileFileName("33fbafeb-8291-4eda", "phase:2")).toBe("profile-33fbafeb-phase-2.json");
  });
});
