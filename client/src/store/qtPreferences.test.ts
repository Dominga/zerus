import { describe, expect, it } from "vitest";
import { qtPreferenceRows, type QtPreferenceReport } from "./qtPreferences";

// zerus-5wmh.66.5.3: the Migration report's local Qt preferences section.
describe("Qt preferences in the Migration report", () => {
  it("lists imported, kept, waiting and unused keys by name", () => {
    const report: QtPreferenceReport = {
      families: [
        { family: "inspector-panel", state: "imported", keys: ["workspace/inspectorTab", "workspace/inspectorVisible"],
          imported: ["workspace/inspectorTab"], kept: ["workspace/inspectorVisible"] },
        { family: "window-geometry", state: "imported", keys: ["sessions/geometry"] },
        { family: "project-view", state: "imported", keys: [], imported: [], kept: [] },
        { family: "session-splitter", state: "waiting", keys: ["workspace/splitter"], reason: "invalid Qt splitter" },
      ],
      unused: ["workspace/expandedSessions"],
    };
    expect(qtPreferenceRows(report)).toEqual([
      { label: "Side panel", status: "Imported", keys: ["workspace/inspectorTab"] },
      { label: "Side panel", status: "Kept your Zerus choice", keys: ["workspace/inspectorVisible"] },
      // Handled before the split was recorded: shown as imported earlier.
      { label: "Window size and position", status: "Imported earlier", keys: ["sessions/geometry"] },
      { label: "Sessions list width", status: "Waiting to retry", keys: ["workspace/splitter"], reason: "invalid Qt splitter" },
      { label: "Other preferences", status: "Not imported: no Zerus equivalent", keys: ["workspace/expandedSessions"] },
    ]);
  });
  it("has nothing to show without a report or with an empty one", () => {
    expect(qtPreferenceRows(undefined)).toEqual([]);
    expect(qtPreferenceRows({ families: [], unused: [] })).toEqual([]);
  });
  it("names an unknown family by its identity", () => {
    expect(qtPreferenceRows({ families: [{ family: "future-family", state: "imported", keys: ["x/y"] }], unused: [] }))
      .toEqual([{ label: "future-family", status: "Imported earlier", keys: ["x/y"] }]);
  });
});
