import { describe, expect, it } from "vitest";
import contract from "../../../../tests/fixtures/opencode-provider-catalog.json";
import effortIds from "../../../../tests/fixtures/opencode-effort-ids.json";
import {
  addModelOptions, catalogSupported, effortValid, filter, group, kindText, markCatalogFavorite, marks, ModelPicker, parse, providers,
  updateText, type ModelOption,
} from "./opencode-models";

// tray/tests/test_opencodemodelpicker.cpp and the dialog's model rows
// (NewSessionDialog.cpp 51-105), over the shared contract
// tests/fixtures/opencode-provider-catalog.json.

type Json = Record<string, unknown>;
const row = contract.row as Json;
const shortList = row.model_options as Json[];

/** A ready catalog: the row's list plus solo/solo-model, its folder provider and an unavailable favorite. */
function catalog(): Json {
  const models = [...shortList, { id: "solo/solo-model", value: "solo/solo-model", label: "Solo model", provider: "solo", provider_label: "Solo fixture", free: false, effort_options: ["low"], effort_labels: ["low"] }];
  const list = [...(row.providers as Json[])];
  list[list.length - 1] = { ...list[list.length - 1], folder: true };
  return { catalog_state: "ready", model_options: shortList, model_options_truncated: false, providers: list, models, models_truncated: true, favorites: [{ value: "gone/missing", available: false }] };
}

/** A ready catalog of `count` models p/m0… and `gone` unavailable favorites gone/g0…. */
function numbered(count: number, gone: number): Json {
  const models = Array.from({ length: count }, (_, i) => ({ value: `p/m${i}`, label: `Model ${i}`, provider: "p", provider_label: "P" }));
  const favorites = Array.from({ length: gone }, (_, i) => ({ value: `gone/g${i}`, available: false }));
  return { catalog_state: "ready", models, models_truncated: false, favorites };
}

function find(picker: ModelPicker, value: string) {
  for (const g of picker.view().groups) for (const m of g.models) if (m.value === value) return { group: g, row: m };
  return undefined;
}

/** A picker that records what it asks its owner. */
function recorded() {
  const picker = new ModelPicker();
  const chosen: string[] = [], favorites: [string, boolean][] = [];
  let catalogs = 0;
  picker.onChosen = (value) => chosen.push(value);
  picker.onFavoriteRequested = (value, favorite) => favorites.push([value, favorite]);
  picker.onCatalogRequested = () => { catalogs++; };
  return { picker, chosen, favorites, catalogs: () => catalogs };
}

describe("OpenCodeModels", () => {
  it("parses the shared short list in order with its marks", () => {
    const models = parse(shortList, 256);
    expect(models.map((m) => m.value)).toEqual(["fixture/local", "keyed/paid-model", "groq/openai/gpt-oss-20b", "fixture/alternate",
      "ollama/qwen3-coder:30b", "opencode/glm-5.3-flash-free", "opencode/big-pickle", "groq/groq/compound"]);
    expect(marks(models[0])).toBe("Default");
    expect(marks(models[1])).toBe("Favorite");
    expect(marks(models[5])).toBe("Free");
    // Ruling D4: the marks join with commas, never dots.
    expect(marks({ ...models[0], favorite: true, free: true })).toBe("Default, Favorite, Free");
    expect(models[2].efforts).toEqual(["low", "medium", "high"]);
    const groups = group(models);
    expect(groups.map((g) => g.label)).toEqual(["Private keyless fixture", "Keyed fixture", "Groq", "Ollama", "OpenCode"]);
    expect(groups[2].models.length).toBe(2);
    expect(groups[2].models[1].value).toBe("groq/groq/compound");
  });

  it("names each provider's connection", () => {
    const list = providers(row.providers as Json[]);
    expect(list.length).toBe(6);
    expect(kindText(list[0].kind)).toBe("Free, no key");
    expect(kindText(list[3].kind)).toBe("Detected locally");
    expect(list[4].credentials).toBe(2);
    expect(list[5].activeLabel).toBe("Work");
    expect(kindText("future-kind")).toBe("Connected");
  });

  it("matches every word of a search", () => {
    const models = parse(shortList, 256);
    const values = (q: string) => filter(models, q).map((m) => m.value);
    expect(values("gpt 20B")).toEqual(["groq/openai/gpt-oss-20b"]);
    expect(values("opencode")).toEqual(["opencode/glm-5.3-flash-free", "opencode/big-pickle"]);
    expect(values("keyless alternate")).toEqual(["fixture/alternate"]);
    expect(values("  ").length).toBe(8);
  });

  it("drops invalid entries", () => {
    const models = parse([{ value: "noslash" }, { value: `p/${"m".repeat(255)}` }, { value: "p/a\u0007" }, { id: "p/ok", label: "OK" }, { value: "p/ok" },
      { value: "p/e", effort_options: ["low", "has space", "a/b", "low"] }], 256);
    expect(models.length).toBe(2);
    expect(models[0].value).toBe("p/ok");
    expect(models[1].efforts).toEqual(["low"]);
  });

  // openCodeEffortIdsFollowSharedRule: the shared variant shape rule.
  it("accepts exactly the shared effort ids", () => {
    expect(effortIds.accept.length).toBeGreaterThanOrEqual(10);
    expect(effortIds.reject.length).toBeGreaterThanOrEqual(13);
    for (const id of effortIds.accept) expect(effortValid(id), JSON.stringify(id)).toBe(true);
    for (const id of effortIds.reject) expect(effortValid(id), JSON.stringify(id)).toBe(false);
    // The dialog offers the accepted ones, in order, after Default.
    const options = addModelOptions([], [{ id: "fixture/ids", value: "fixture/ids", label: "Ids fixture", effort_options: [...effortIds.reject, ...effortIds.accept], effort_labels: [...effortIds.reject, ...effortIds.accept] }]);
    expect(options[0].efforts).toEqual(effortIds.accept.slice(0, 16));
  });

  it("offers older computers neither All models nor stars", () => {
    expect(catalogSupported({ model_options: [] })).toBe(false);
    expect(catalogSupported({ model_options_truncated: false })).toBe(true);
    expect(updateText("mac")).toBe("Update Zerus on mac to search all models and star favorites.");
    const { picker, catalogs } = recorded();
    picker.setShortList(shortList, false);
    picker.setAllModelsAvailable(false, updateText("mac"));
    expect(picker.view().all).toEqual({ checked: false, enabled: false, tooltip: "Update Zerus on mac to search all models and star favorites." });
    picker.toggleAll();
    expect(catalogs()).toBe(0);
  });
});

describe("the dialog's model rows (NewSessionDialog.cpp 51-105)", () => {
  // openCodeEffortFollowsSelectedModel: only valid, distinct variants, at most 16, keep their labels.
  it("keeps each model's valid variants and its label", () => {
    const many = Array.from({ length: 20 }, (_, n) => `level-${n}`);
    const options = addModelOptions([], [
      { id: "fixture/local", value: "fixture/local", label: "Local fixture", effort_options: ["low", "medium", "high"], effort_labels: ["low", "medium", "high"] },
      { id: "fixture/other", value: "fixture/other", label: "Other fixture", effort_options: ["high", "max", "", "bad value", "x".repeat(80), "max"], effort_labels: ["High", "Maximum", "", "", "", ""] },
      { id: "fixture/many", value: "fixture/many", label: "Many fixture", effort_options: many, effort_labels: many },
      { id: "fixture/local", value: "fixture/local", label: "Duplicate" },
      { value: "plain/id" },
      { value: "noslash" },
    ]);
    expect(options.map((o: ModelOption) => o.value)).toEqual(["fixture/local", "fixture/other", "fixture/many", "plain/id"]);
    expect(options[0].text).toBe("Local fixture (fixture/local)");
    expect(options[1].effortLabels).toEqual(["High", "Maximum"]);
    expect(options[1].efforts).toEqual(["high", "max"]);
    expect(options[2].efforts.length).toBe(16);
    expect(options[2].efforts[15]).toBe("level-15");
    expect(options[3]).toEqual({ value: "plain/id", text: "plain/id", label: "plain/id", efforts: [], effortLabels: [] });
  });

  it("marks a confirmed favorite in both lists and its favorites", () => {
    const ready = catalog();
    const on = markCatalogFavorite(ready, "fixture/alternate", true);
    expect((on.model_options as Json[]).find((m) => m.value === "fixture/alternate")?.favorite).toBe(true);
    expect((on.models as Json[]).find((m) => m.value === "fixture/alternate")?.favorite).toBe(true);
    expect(on.favorites).toEqual([{ value: "gone/missing", available: false }, { value: "fixture/alternate", available: true }]);
    const off = markCatalogFavorite(on, "keyed/paid-model", false);
    expect((off.model_options as Json[]).find((m) => m.value === "keyed/paid-model")).not.toHaveProperty("favorite");
    const gone = markCatalogFavorite(on, "gone/missing", false);
    expect(gone.favorites).toEqual([{ value: "fixture/alternate", available: true }]);
    // The original stays as it was.
    expect(ready.favorites).toEqual([{ value: "gone/missing", available: false }]);
  });
});

describe("the model picker (OpenCodeModelPicker.cpp)", () => {
  it("groups, marks and stars request favorites", () => {
    const { picker, chosen, favorites } = recorded();
    picker.setShortList(shortList, false);
    expect(picker.view().groups.length).toBe(5);
    expect(find(picker, "fixture/local")?.row.marks).toBe("Default");
    expect(find(picker, "keyed/paid-model")?.row.favorite).toBe(true);
    expect(find(picker, "fixture/alternate")?.row.favorite).toBe(false);
    picker.click("fixture/alternate", true);
    expect(favorites).toEqual([["fixture/alternate", true]]);
    expect(chosen).toEqual([]);
    picker.setFavoritePending("fixture/alternate", true);
    expect(find(picker, "fixture/alternate")?.row.pending).toBe(true);
    picker.click("fixture/alternate", true);
    expect(favorites.length).toBe(1);
    picker.setFavorite("fixture/alternate", true);
    expect(find(picker, "fixture/alternate")?.row).toMatchObject({ favorite: true, pending: false });
    picker.click("groq/groq/compound", false);
    expect(chosen).toEqual(["groq/groq/compound"]);
    expect(picker.current()).toBe("groq/groq/compound");
    picker.setFavoritesAvailable(false, "mac is offline.");
    picker.click("groq/groq/compound", true);
    expect(favorites.length).toBe(1);
    expect(find(picker, "groq/groq/compound")?.row.starTooltip).toBe("mac is offline.");
  });

  it("asks for the catalog once and searches it", () => {
    const { picker, catalogs, favorites } = recorded();
    picker.setShortList(shortList, true);
    expect(picker.view().notice).toBe("Some models are not in this short list. Use All models to search every model.");
    picker.toggleAll();
    expect(catalogs()).toBe(1);
    expect(picker.view().notice).toBe("Loading all models…");
    picker.setCatalog(catalog());
    expect(picker.showingAll()).toBe(true);
    expect(picker.values()).toContain("solo/solo-model");
    expect(picker.view().notice.startsWith("Showing the first 9 models.")).toBe(true);
    // Ruling D4: a folder's provider reads "{provider} (this folder)".
    expect(picker.view().groups.map((g) => g.label)).toContain("Solo fixture (this folder)");
    picker.setQuery("solo");
    expect(picker.values()).toEqual(["solo/solo-model"]);
    picker.setQuery("");
    const gone = find(picker, "gone/missing");
    expect(gone?.row).toMatchObject({ favorite: true, unavailable: true, marks: "Unavailable" });
    picker.click("gone/missing", false);
    expect(picker.current()).not.toBe("gone/missing");
    picker.click("gone/missing", true);
    expect(favorites[0]).toEqual(["gone/missing", false]);
    picker.toggleAll();
    expect(picker.showingAll()).toBe(false);
    picker.toggleAll();
    expect(catalogs()).toBe(1);
    expect(picker.model("solo/solo-model")?.efforts).toEqual(["low"]);
  });

  it("keeps an unknown current model listed and explains failures", () => {
    const { picker } = recorded();
    picker.setShortList(shortList, false);
    picker.setCurrent("custom/model");
    expect(picker.values()[0]).toBe("custom/model");
    picker.toggleAll();
    picker.setCatalogFailed("OpenCode is not running on mac");
    expect(picker.view().notice).toBe("OpenCode is not running on mac");
    expect(picker.showingAll()).toBe(false);
    expect(picker.values()).toContain("fixture/local");
  });

  it("renders at most five hundred matches", () => {
    const { picker } = recorded();
    picker.setShortList([], false);
    picker.toggleAll();
    picker.setCatalog(numbered(2000, 0));
    expect(picker.values().length).toBe(500);
    expect(picker.view().notice).toBe("Showing the first 500 matches. Search to narrow the list.");
    picker.setQuery("model 1999");
    expect(picker.values()).toEqual(["p/m1999"]);
  });

  it("lists the shared account models answer", () => {
    const answer = contract.account_models.answer as Json;
    const { picker, catalogs, chosen } = recorded();
    picker.setShortList(answer.model_options, answer.model_options_truncated === true);
    picker.setCatalog(answer);
    expect(picker.showingAll()).toBe(false);
    expect(picker.values()).not.toContain("gone/missing");
    picker.toggleAll();
    expect(catalogs()).toBe(0);
    expect(picker.showingAll()).toBe(true);
    expect(picker.values()).toContain("workspace/team-coder");
    const headers = picker.view().groups.map((g) => g.label);
    expect(headers).toContain("Workspace models (this folder)");
    expect(headers[headers.length - 1]).toBe("Unavailable favorites");
    expect(picker.view().groups[headers.length - 1].models.length).toBe(1);
    expect(find(picker, "fixture/local")?.row.marks).toContain("Default");
    expect(picker.model("workspace/team-coder")?.efforts).toEqual(["low", "high"]);
    picker.click("solo/solo-model", false);
    expect(chosen).toEqual(["solo/solo-model"]);
    picker.click("gone/missing", false);
    expect(chosen.length).toBe(1);
    picker.setFavorite("gone/missing", false);
    expect(find(picker, "gone/missing")).toBeUndefined();
    const after = picker.view().groups;
    expect(after[after.length - 1].label).toBe("Workspace models (this folder)");
    // A context without a catalog: the current choice stays listed first, and All models asks again.
    picker.setCatalog({});
    expect(picker.showingAll()).toBe(false);
    expect(picker.values()[0]).toBe("solo/solo-model");
    expect(picker.values()).toContain("workspace/team-coder");
    picker.toggleAll();
    expect(catalogs()).toBe(1);
  });

  it("clears pending stars on a context reset but keeps them through a refresh", () => {
    const { picker, favorites } = recorded();
    picker.setShortList(shortList, false);
    picker.setCatalog(catalog());
    picker.setFavoritePending("fixture/alternate", true);
    picker.setCatalog(catalog());
    expect(find(picker, "fixture/alternate")?.row.pending).toBe(true);
    picker.click("fixture/alternate", true);
    expect(favorites.length).toBe(0);
    picker.setCatalog({});
    picker.setShortList([shortList[3]], false);
    expect(picker.values()).toEqual(["fixture/alternate"]);
    expect(find(picker, "fixture/alternate")?.row).toMatchObject({ pending: false, favorite: false });
    picker.click("fixture/alternate", true);
    expect(favorites).toEqual([["fixture/alternate", true]]);
  });

  it("shares the five hundred rows with unavailable favorites", () => {
    const { picker } = recorded();
    picker.setShortList([], false);
    picker.setCatalog(numbered(500, 64));
    picker.toggleAll();
    expect(picker.values().length).toBe(500);
    expect(picker.values()).not.toContain("gone/g0");
    expect(picker.view().notice).toBe("Showing the first 500 matches. Search to narrow the list.");
    picker.setCurrent("p/m499");
    expect(picker.values()).toContain("p/m499");
    picker.setCurrent("gone/g63");
    expect(picker.values().length).toBe(500);
    expect(picker.values()[0]).toBe("gone/g63");
    expect(picker.values().filter((v) => v === "gone/g63").length).toBe(1);
    picker.setQuery("gone");
    expect(picker.values().length).toBe(64);
    expect(picker.values()[0]).toBe("gone/g63");
    expect(picker.view().notice).toBe("");
  });

  it("keeps a current model past the cut visible and selected", () => {
    const { picker } = recorded();
    picker.setShortList([], false);
    picker.setCurrent("p/m1500");
    picker.toggleAll();
    picker.setCatalog(numbered(2000, 0));
    expect(picker.values().length).toBe(500);
    expect(picker.values()).toContain("p/m1500");
    expect(picker.values()).not.toContain("p/m499");
    expect(picker.view().groups.length).toBe(1);
    expect(picker.values()[499]).toBe("p/m1500");
    expect(picker.view().selected).toBe("p/m1500");
  });

  it("keeps an unavailable current favorite starred and never chooses it", () => {
    const { picker, chosen, favorites } = recorded();
    picker.setShortList(shortList, false);
    picker.setCatalog(catalog());
    picker.setCurrent("gone/missing");
    expect(picker.values()[0]).toBe("gone/missing");
    expect(find(picker, "gone/missing")?.row.favorite).toBe(true);
    expect(picker.model("gone/missing")?.favorite).toBe(true);
    picker.click("gone/missing", false);
    picker.select("gone/missing");
    expect(chosen.length).toBe(0);
    picker.click("gone/missing", true);
    expect(favorites).toEqual([["gone/missing", false]]);
    picker.toggleAll();
    expect(picker.values().filter((v) => v === "gone/missing").length).toBe(1);
    picker.setFavorite("gone/missing", false);
    expect(picker.values()[0]).toBe("gone/missing");
    expect(find(picker, "gone/missing")?.row.favorite).toBe(false);
  });

  it("bounds an owner's reasons as one line", () => {
    const { picker } = recorded();
    picker.setShortList(shortList, false);
    picker.setAllModelsAvailable(false, "<i>older</i>\nhgs");
    expect(picker.view().all.tooltip).toBe("<i>older</i> hgs");
    picker.setAllModelsAvailable(false, "y".repeat(1000));
    expect(picker.view().all.tooltip).toBe(`${"y".repeat(399)}…`);
    picker.setFavoritesAvailable(false, "y".repeat(1000));
    expect(find(picker, "fixture/local")?.row.starTooltip).toBe(`${"y".repeat(399)}…`);
    picker.setAllModelsAvailable(true);
    expect(picker.view().all.tooltip).toBe("Search every model OpenCode offers");
  });
});
