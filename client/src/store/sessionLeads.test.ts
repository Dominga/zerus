import { describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => ({ settings: {} as Record<string, unknown>, writes: [] as [string, unknown][] }));
vi.mock("./settings", () => ({
  settings: store.settings,
  setSetting: async (key: string, value: unknown) => { store.writes.push([key, value]); store.settings[key] = value; return true; },
}));

import { leadLinks, leadOf, leadRefusal, setLead, subordinatesOf } from "./sessionLeads";

describe("session leads", () => {
  it("reads Qt's identity-to-lead object and lists subordinates", () => {
    store.settings["workspace/sessionLeads"] = JSON.stringify({ child: "lead", other: "lead" });
    expect(leadLinks()).toEqual({ child: "lead", other: "lead" });
    expect(leadOf("child")).toBe("lead");
    expect(subordinatesOf("lead").sort()).toEqual(["child", "other"]);
  });

  it("refuses a lead that reports, a session that already leads and itself", () => {
    const links = { child: "lead", lead: "boss" };
    expect(leadRefusal("x", "lead", links)).toContain("already reports");
    expect(leadRefusal("lead", "x", links)).toContain("already leads");
    expect(leadRefusal("x", "x", links)).toContain("itself");
    expect(leadRefusal("x", "y", {})).toBe("");
  });

  it("writes one level and detaches", async () => {
    store.settings["workspace/sessionLeads"] = "{}";
    expect(await setLead("child", "lead")).toBe(true);
    expect(store.settings["workspace/sessionLeads"]).toBe(JSON.stringify({ child: "lead" }));
    expect(await setLead("child", "")).toBe(true);
    expect(store.settings["workspace/sessionLeads"]).toBe("{}");
  });
});
