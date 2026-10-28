import { describe, expect, it } from "vitest";
import { ModelPicker, MAX_ROWS } from "./opencode-models";
import { POPOVER_HEIGHT, POPOVER_WIDTH, enterRow, pickerValues, popoverBox, starText, stepRow } from "./model-picker";

// The model popover's list state (ModelPicker.tsx over opencode-models.ts):
// the rows it lists and the keyboard moves over, in the picker's display
// order, and its place under the model button. Synthetic models only.

type Json = Record<string, unknown>;

/** A ready catalog of `count` models across two providers, the second a folder's, and `gone` unavailable favorites. */
function catalog(count: number, gone = 0): Json {
  const models = Array.from({ length: count }, (_, i) => ({ value: `${i % 2 ? "local" : "cloud"}/m${i}`, label: `Model ${i}`, provider: i % 2 ? "local" : "cloud", provider_label: i % 2 ? "Local" : "Cloud", free: i === 0, default: i === 1 }));
  const favorites = Array.from({ length: gone }, (_, i) => ({ value: `gone/g${i}`, available: false }));
  return { catalog_state: "ready", models, providers: [{ id: "local", label: "Local", folder: true }], models_truncated: false, favorites };
}

describe("the model popover's list", () => {
  it("lists the groups' rows in display order, the folder's provider named for it", () => {
    const picker = new ModelPicker();
    picker.setShortList([], false);
    picker.setCatalog(catalog(6, 1));
    picker.toggleAll();
    const view = picker.view();
    expect(view.groups.map((g) => g.label)).toEqual(["Cloud", "Local (this folder)", "Unavailable favorites"]);
    expect(pickerValues(view)).toEqual(["cloud/m0", "cloud/m2", "cloud/m4", "local/m1", "local/m3", "local/m5", "gone/g0"]);
    expect(view.groups[0].models[0]).toMatchObject({ marks: "Free", unavailable: false });
    expect(view.groups[1].models[0].marks).toBe("Default");
    expect(view.groups[2].models[0]).toMatchObject({ marks: "Unavailable", unavailable: true, favorite: true, tooltip: "OpenCode does not offer gone/g0 here. Its star removes the favorite." });
    picker.setQuery("model 3");
    expect(pickerValues(picker.view())).toEqual(["local/m3"]);
    expect(picker.view().query).toBe("model 3");
  });

  it("keeps the five hundred rows the picker renders", () => {
    const picker = new ModelPicker();
    picker.setShortList([], false);
    picker.setCatalog(catalog(1200, 3));
    picker.toggleAll();
    expect(pickerValues(picker.view()).length).toBe(MAX_ROWS);
    expect(picker.view().notice).toBe("Showing the first 500 matches. Search to narrow the list.");
  });

  // OpenCodeModelPicker::render 252-283: loading, a failure, the catalog's reason, the cut, then the truncated lists.
  it("shows one notice, in Qt's order", () => {
    const picker = new ModelPicker();
    picker.setShortList([{ value: "cloud/a", label: "A" }], true);
    expect(picker.view().notice).toBe("Some models are not in this short list. Use All models to search every model.");
    picker.toggleAll();
    expect(picker.view().notice).toBe("Loading all models…");
    picker.setCatalog({ catalog_state: "not_running", catalog_reason: "OpenCode is not running on mac", models: [] });
    expect(picker.view().notice).toBe("OpenCode is not running on mac");
    picker.setCatalogFailed("mac is offline.");
    expect(picker.view()).toMatchObject({ notice: "mac is offline.", all: { checked: false } });
    picker.setCatalog({ ...catalog(600), models_truncated: true });
    picker.toggleAll();
    expect(picker.view().notice).toBe("Showing the first 500 matches. Search to narrow the list.");
    picker.setQuery("model 1");
    expect(picker.view().notice).toBe("Showing the first 600 models. Search to narrow the list.");
    picker.toggleAll();
    expect(picker.view().notice).toBe("Some models are not in this short list. Use All models to search every model.");
    picker.setShortList([{ value: "cloud/a", label: "A" }], false);
    expect(picker.view().notice).toBe("");
  });

  it("moves over the model rows only and stays inside the list", () => {
    const picker = new ModelPicker();
    picker.setShortList([], false);
    picker.setCatalog(catalog(4, 1));
    picker.toggleAll();
    const view = picker.view();
    expect(stepRow(view, undefined, 1)).toBe("cloud/m0");
    expect(stepRow(view, undefined, -1)).toBe("gone/g0");
    expect(stepRow(view, "cloud/m2", 1)).toBe("local/m1");
    expect(stepRow(view, "cloud/m0", -1)).toBe("cloud/m0");
    expect(stepRow(view, "local/m3", 10)).toBe("gone/g0");
    expect(stepRow({ groups: [] }, undefined, 1)).toBeUndefined();
    expect(enterRow(view, "local/m1")).toBe("local/m1");
    // Enter never chooses an unavailable favorite, nor a row no longer listed.
    expect(enterRow(view, "gone/g0")).toBeUndefined();
    expect(enterRow(view, "missing/model")).toBeUndefined();
  });

  it("shows a star per row, and its change on its way", () => {
    expect(starText({ pending: false, favorite: false })).toBe("☆");
    expect(starText({ pending: false, favorite: true })).toBe("★");
    expect(starText({ pending: true, favorite: true })).toBe("…");
  });
});

describe("the model popover's place (openModelPicker 1266-1274)", () => {
  const window = { width: 1200, height: 900 };
  it("opens under the model button, at least 440 by 420", () => {
    expect(popoverBox({ left: 300, top: 400, bottom: 432, width: 360 }, window)).toEqual({ left: 300, top: 436, width: POPOVER_WIDTH, height: POPOVER_HEIGHT });
    expect(popoverBox({ left: 300, top: 400, bottom: 432, width: 520 }, window).width).toBe(520);
  });

  it("stays inside the window", () => {
    expect(popoverBox({ left: 1000, top: 100, bottom: 132, width: 200 }, window).left).toBe(1200 - 8 - 440);
    // No room below: above the button.
    expect(popoverBox({ left: 100, top: 700, bottom: 732, width: 200 }, window).top).toBe(700 - 4 - 420);
    // A small window: as large as it allows.
    expect(popoverBox({ left: 0, top: 200, bottom: 232, width: 200 }, { width: 400, height: 300 })).toEqual({ left: 8, top: 8, width: 384, height: 284 });
  });
});
