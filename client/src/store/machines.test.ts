import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createCatalog } from "./catalog";
import { createMachineCards } from "./machines";
import { machineMeta } from "./testing";
import { emptyBatch } from "../types";

describe("machine cards", () => {
  it("apply right after the computer statuses and drop with an ended computer", () => {
    createRoot((dispose) => {
      const machines = createMachineCards();
      const seen: string[] = [];
      const { catalog, applyBatch } = createCatalog(undefined, (b) => {
        seen.push(Object.keys(catalog.computers).join(","));
        machines.applyMachines(b);
      });
      applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }, { computer: "studio", status: "live", attempt: 0 }],
        machines: [{ computer: "", machine: machineMeta("workstation") }, { computer: "studio", machine: machineMeta("studio") }] });
      expect(seen).toEqual([",studio"]);
      expect(machines.cards[""].host).toBe("workstation");
      applyBatch({ ...emptyBatch(), machines: [{ computer: "", machine: machineMeta("workstation", { cpu_percent: 40 }) }] });
      expect(machines.cards[""].metrics.cpu_percent).toBe(40);
      applyBatch({ ...emptyBatch(), computers: [{ computer: "studio", status: "ended", attempt: 0 }] });
      expect(machines.cards.studio).toBeUndefined();
      expect(machines.cards[""]).toBeDefined();
      dispose();
    });
  });
});
