import { describe, expect, it } from "vitest";
import { machineAppearance } from "./machineAppearance";

const fallback = "var(--z-entity-palette-3)";
describe("local machine appearance imported from Qt", () => {
  it("applies only an exact stored machine key", () => {
    const values = { "machines/colors/peer": "#abcdef", "machines/vivid/peer": true };
    expect(machineAppearance(values, "peer", fallback)).toEqual({ color: "#abcdef", vivid: true });
    expect(machineAppearance(values, "other", fallback)).toEqual({ color: fallback, vivid: false });
  });
  it("keeps the palette fallback for malformed colors", () => {
    expect(machineAppearance({ "machines/colors/peer": "red; background: url(example)" }, "peer", fallback).color).toBe(fallback);
  });
});
