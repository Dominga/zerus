import { describe, expect, it, vi } from "vitest";
import effortIds from "../../../tests/fixtures/opencode-effort-ids.json";
import { settle, setup, type World } from "./newSessionFake";

// New session's OpenCode part (newSessionOpencode.ts), through the store:
// tray/tests/test_newsessiondialog.cpp's OpenCode cases over the recorded
// bridge of newSessionFake.ts.

vi.mock("./ui", () => ({ selected: () => undefined, machineFilter: () => [], setPage: () => {} }));
vi.mock("../bridge", () => ({}));

describe("OpenCode (NewSessionDialog.cpp 1145-1313)", () => {
  const opencode = (w: World) => { w.flags.add("opencode"); w.flags.add("opencode-models"); };
  const efforts = (t: ReturnType<typeof setup>) => t.state().efforts.map((e) => e.label);

  it("openCodeEffortFollowsSelectedModel", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    expect(t.state().models.some((m) => m.value === "fixture/local")).toBe(true);
    expect(t.state().accounts[t.state().account].id).toBe("native-opencode");
    expect(t.state()).toMatchObject({ modelVisible: true, model: "", effortVisible: false });
    t.act.chooseModel("fixture/local");
    expect(t.state().effortVisible).toBe(true);
    expect(efforts(t)).toEqual(["Default", "low", "medium", "high"]);
    expect(t.state().effort).toBe("");
    t.act.chooseEffort("high");
    t.act.chooseModel("fixture/other");
    expect(efforts(t)).toEqual(["Default", "High", "Maximum"]);
    expect(t.state().effort).toBe("high");
    t.act.chooseEffort("max");
    t.act.chooseModel("fixture/local");
    expect(t.state().effort).toBe("");
    t.act.chooseModel("fixture/many");
    expect(t.state().efforts.length).toBe(17);
    expect(t.state().efforts[16].value).toBe("level-15");
    t.act.chooseEffort("level-3");
    t.act.chooseModel("fixture/plain");
    expect(t.state()).toMatchObject({ effortVisible: false, effort: "" });
    t.act.chooseModel("fixture/many");
    expect(t.state().effort).toBe("");
    await t.choose("work");
    expect(t.state()).toMatchObject({ effortVisible: false, modelVisible: false });
    await t.choose("native-opencode");
    t.act.chooseModel("fixture/local");
    t.act.chooseEffort("high");
    await settle();
    await t.start();
    expect(t.choices()).toMatchObject({ agent: "opencode", account: "native-opencode", model: "fixture/local", effort: "high" });
  });

  it("openCodeEffortIdsFollowSharedRule", async () => {
    const t = setup((w) => {
      opencode(w);
      const ids = [...effortIds.reject, ...effortIds.accept];
      w.extraModel = { id: "fixture/ids", value: "fixture/ids", label: "Ids fixture", provider: "fixture", effort_options: ids, effort_labels: ids };
    });
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.chooseModel("fixture/ids");
    expect(t.state().efforts.map((e) => e.value)).toEqual(["", ...effortIds.accept]);
    t.act.chooseEffort("think:deep");
    await settle();
    await t.start();
    expect(t.choices().effort).toBe("think:deep");
  });

  it("openCodeStartsStoppedServiceExplicitly", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("opencode-stopped"); });
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    expect(t.state().service).toMatchObject({ visible: true, offer: true, text: "Start OpenCode on mac", enabled: true, reason: "OpenCode is not running on mac" });
    expect(t.world.count("openCodeStart")).toBe(0);
    t.act.startOpenCode();
    await settle();
    expect(t.world.of("openCodeStart").map((c) => c.args)).toEqual([["mac"]]);
    expect(t.world.of("launchAccounts").at(-1)?.args).toEqual(["mac", true, "opencode"]);
    expect(t.state().service).toMatchObject({ visible: false, offer: false });
    // Other agents never offer it.
    t.world.flags.add("opencode-stopped");
    t.act.refreshAccounts();
    await settle();
    expect(t.state().service.offer).toBe(true);
    await t.choose("work");
    expect(t.state().service).toMatchObject({ visible: false, offer: false });
  });

  it("openCodeStartNeedsOnlineComputer", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("opencode-stopped"); w.online.mac = false; });
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    expect(t.state().service).toMatchObject({ offer: true, enabled: false, tooltip: "mac is offline." });
    t.act.startOpenCode();
    await settle();
    expect(t.world.count("openCodeStart")).toBe(0);
    t.world.online.mac = true;
    t.store.fleetChanged();
    await settle();
    expect(t.state().service.enabled).toBe(true);
  });

  it("openCodePickerUsesTheSelectedFolderCatalog", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    expect(t.state()).toMatchObject({ modelEnabled: true, modelButton: "Native default model" });
    t.act.openModelPicker();
    await settle();
    expect(t.state().pickerOpen).toBe(true);
    expect(t.values()).toContain("folderonly/proj");
    expect(t.world.of("openCodeModels").map((c) => c.args)).toEqual([["mac", "/workspace/example", "mac"]]);
    expect(t.state().picker.groups.map((g) => g.label)).toContain("Folder-only fixture (this folder)");
    t.act.pickerSelect("folderonly/proj");
    expect(t.state()).toMatchObject({ pickerOpen: false, modelButton: "Project model", effortVisible: false });
    await settle();
    await t.start();
    expect(t.choices()).toMatchObject({ model: "folderonly/proj", effort: "" });
  });

  it("openCodeAllModelsSearchesTheFolderCatalog", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("folderonly/proj");
    t.act.pickerToggleAll();
    t.act.pickerSearch("solo");
    await settle();
    expect(t.values()).toEqual(["solo/solo-model"]);
    // The folder catalog is fetched once.
    expect(t.world.count("openCodeModels")).toBe(1);
    t.act.pickerSelect("solo/solo-model");
    expect(t.state()).toMatchObject({ pickerOpen: false, effortVisible: true, model: "solo/solo-model" });
    expect(t.state().efforts.length).toBe(2);
    // Reopened, the search the field keeps still narrows the list, and nothing is read again.
    t.act.openModelPicker();
    await settle();
    expect(t.state().picker.query).toBe("solo");
    expect(t.world.count("openCodeModels")).toBe(1);
    expect(t.world.creating()).toEqual([]);
  });

  it("openCodeChoicesOutsideTheRowSurviveAnAccountRefresh", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerSelect("folderonly/proj");
    expect(t.state().modelButton).toBe("Project model");
    const before = t.world.count("openCodeModels");
    t.act.refreshAccounts();
    await settle();
    expect(t.state()).toMatchObject({ modelButton: "Project model", model: "folderonly/proj" });
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("folderonly/proj");
    expect(t.world.count("openCodeModels")).toBe(before);
    // An All models choice is kept the same way, with its efforts.
    t.act.pickerToggleAll();
    t.act.pickerSelect("solo/solo-model");
    expect(t.state().effortVisible).toBe(true);
    t.act.chooseEffort("low");
    t.act.refreshAccounts();
    await settle();
    expect(t.state()).toMatchObject({ modelButton: "Solo model", effort: "low" });
    await t.start();
    expect(t.choices()).toMatchObject({ model: "solo/solo-model", effort: "low" });
  });

  it("openCodeStoppedServiceKeepsTheRowListAndNeverStarts", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("models-stopped"); });
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    expect(t.state().picker.notice).toBe("OpenCode is not running on pc");
    expect(t.values()).toContain("fixture/local");
    // Opening again shows that answer instead of asking again.
    t.act.closeModelPicker();
    t.act.openModelPicker();
    await settle();
    expect(t.state().pickerOpen).toBe(true);
    expect(t.world.count("openCodeModels")).toBe(1);
    expect(t.world.count("openCodeStart")).toBe(0);
  });

  it("openCodeOlderComputerOffersNoStarsOrAllModels", async () => {
    const t = setup((w) => { opencode(w); w.flags.add("old-hgs"); });
    await t.open({ agent: "opencode", computer: "mac", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("fixture/local");
    expect(t.world.count("openCodeModels")).toBe(0);
    expect(t.state().picker.all).toMatchObject({ enabled: false, tooltip: "Update Zerus on mac to search all models and star favorites." });
    t.act.pickerClick("fixture/plain", true);
    expect(t.star("fixture/plain")?.starTooltip).toBe("Update Zerus on mac to search all models and star favorites.");
    t.act.pickerSelect("fixture/local");
    await settle();
    expect(t.state().form?.start_enabled).toBe(true);
    expect(t.world.count("openCodeFavorite")).toBe(0);
  });

  it("openCodeStarSendsOneFavoriteRequestWithIdentity", async () => {
    const t = setup(opencode);
    t.world.hold("models");
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.world.release("models");
    await settle();
    t.world.hold("models");
    t.act.pickerClick("fixture/plain", true);
    await settle();
    expect(t.world.of("openCodeFavorite").map((c) => c.args)).toEqual([["", "fixture/plain", true, "pc"]]);
    // The folder catalog is read again, and it marks the favorite.
    expect(t.world.count("openCodeModels")).toBe(2);
    t.world.release("models");
    await settle();
    expect(t.star("fixture/plain")?.favorite).toBe(true);
    t.world.flags.add("favorite-fails");
    t.act.pickerClick("fixture/other", true);
    await settle();
    expect(t.state().error.startsWith("Could not change the favorite on pc:")).toBe(true);
    expect(t.star("fixture/other")).toMatchObject({ favorite: false, pending: false });
    // A failure is never sent again on its own.
    await settle();
    expect(t.world.count("openCodeFavorite")).toBe(2);
  });

  it("openCodeLateFolderCatalogStaysWithItsFolder", async () => {
    const t = setup(opencode);
    t.world.hold("models");
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("fixture/local");
    expect(t.world.count("openCodeModels")).toBe(1);
    t.act.selectPath("/workspace/other", false);
    expect(t.state().pickerOpen).toBe(false);
    expect(t.values()).toContain("fixture/local");
    expect(t.values()).not.toContain("folderonly/proj");
    expect(t.state().picker.selected).toBeUndefined();
    expect(t.state().modelButton).toBe("Native default model");
    t.world.release("models");
    await settle();
    expect(t.values()).not.toContain("folderonly/proj");
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("folderonly/other");
    expect(t.values()).not.toContain("folderonly/proj");
    expect(t.world.count("openCodeModels")).toBe(2);
    // Back in the first folder, its late catalog is shown without another request.
    t.act.closeModelPicker();
    t.act.selectPath("/workspace/example", false);
    expect(t.values()).toContain("folderonly/proj");
    expect(t.values()).not.toContain("folderonly/other");
    expect(t.world.count("openCodeModels")).toBe(2);
  });

  it("openCodeLateFavoriteAnswerStaysWithItsComputer", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerSelect("folderonly/proj");
    expect(t.state().modelButton).toBe("Project model");
    t.act.openModelPicker();
    t.world.hold("favorite");
    t.act.pickerClick("fixture/plain", true);
    expect(t.star("fixture/plain")?.pending).toBe(true);
    await settle();
    t.act.chooseComputer(t.state().computers.findIndex((c) => c.target === "mac"));
    expect(t.state()).toMatchObject({ pickerOpen: false, modelButton: "Native default model" });
    expect(t.state().picker.selected).toBeUndefined();
    expect(t.values()).not.toContain("folderonly/proj");
    await settle();
    expect(t.state().modelEnabled).toBe(true);
    t.act.openModelPicker();
    await settle();
    expect(t.star("fixture/plain")).toMatchObject({ favorite: false, pending: false });
    t.world.release("favorite");
    await settle();
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.state().error.startsWith("Could not change the favorite")).toBe(false);
    expect(t.world.count("openCodeFavorite")).toBe(1);
  });

  it("openCodeCurrentModelSurvivesItsFavoriteChanges", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerToggleAll();
    t.act.pickerSelect("solo/solo-model");
    expect(t.state().modelButton).toBe("Solo model");
    t.act.openModelPicker();
    for (const on of [true, false]) {
      const calls = t.world.count("openCodeModels");
      t.act.pickerClick("solo/solo-model", true);
      await settle();
      expect(t.world.count("openCodeModels")).toBe(calls + 1);
      expect(t.star("solo/solo-model")?.favorite).toBe(on);
    }
    expect(t.state().picker.selected).toBe("solo/solo-model");
    expect(t.state()).toMatchObject({ modelButton: "Solo model", model: "solo/solo-model" });
    expect(t.world.count("openCodeFavorite")).toBe(2);
    await t.start();
    expect(t.choices().model).toBe("solo/solo-model");
  });

  it("openCodeOlderFolderAnswerNeverReplacesANewerOne", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/example" });
    t.act.openModelPicker();
    await settle();
    t.act.pickerSelect("folderonly/proj");
    t.act.openModelPicker();
    // The star's folder read is built at once, with the favorite on, and held.
    t.world.flags.add("hold-next-models");
    t.world.hold("next-models");
    t.act.pickerClick("fixture/plain", true);
    await settle();
    expect(t.world.flags.has("models-held")).toBe(true);
    expect(t.star("fixture/plain")?.favorite).toBe(true);
    // The unstar's newer read answers first and is applied.
    t.act.pickerClick("fixture/plain", true);
    await settle();
    expect(t.world.count("openCodeFavorite")).toBe(2);
    expect(t.world.count("openCodeModels")).toBe(3);
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.values()).not.toContain("stale/old");
    // The older answer arrives last and is dropped.
    t.world.release("next-models");
    await settle();
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.values()).not.toContain("stale/old");
    expect(t.values()).toContain("folderonly/proj");
    expect(t.state()).toMatchObject({ modelButton: "Project model", model: "folderonly/proj" });
    // The folder's stored catalog is the newer one too.
    t.act.closeModelPicker();
    t.act.selectPath("/workspace/other", false);
    t.act.selectPath("/workspace/example", false);
    expect(t.values()).toContain("folderonly/proj");
    expect(t.values()).not.toContain("stale/old");
    expect(t.star("fixture/plain")?.favorite).toBe(false);
    expect(t.world.count("openCodeModels")).toBe(3);
  });

  it("openCodeFolderChangeKeepsOnlyAChoiceTheNewFolderOffers", async () => {
    const t = setup(opencode);
    await t.open({ agent: "opencode", path: "/workspace/other" });
    const visit = async (path: string) => { t.act.selectPath(path, false); t.act.openModelPicker(); await settle(); };
    // Read two folders first: one offers the row's fixture/local, one does not.
    t.act.openModelPicker();
    await settle();
    expect(t.values()).toContain("folderonly/other");
    expect(t.values()).toContain("fixture/local");
    t.act.closeModelPicker();
    await visit("/workspace/nolocal");
    expect(t.values()).not.toContain("fixture/local");
    t.act.closeModelPicker();
    // Uncached: a folder without a ready catalog does not take the choice.
    await visit("/workspace/example");
    t.act.pickerSelect("fixture/local");
    expect(t.state().modelButton).toBe("Local fixture");
    t.act.selectPath("/workspace/fresh", false);
    expect(t.state()).toMatchObject({ modelButton: "Native default model", model: "" });
    t.act.selectPath("/workspace/example", false);
    expect(t.state().modelButton).toBe("Native default model");
    // Not offering: a ready catalog without the model does not take it either.
    t.act.openModelPicker();
    t.act.pickerSelect("fixture/local");
    t.act.selectPath("/workspace/nolocal", false);
    expect(t.state()).toMatchObject({ modelButton: "Native default model", model: "" });
    // Offering: a ready catalog that lists it keeps the choice, and the session starts with it.
    t.act.selectPath("/workspace/example", false);
    t.act.openModelPicker();
    t.act.pickerSelect("fixture/local");
    t.act.selectPath("/workspace/other", false);
    expect(t.state()).toMatchObject({ modelButton: "Local fixture", model: "fixture/local" });
    expect(t.state().picker.selected).toBe("fixture/local");
    expect(t.world.count("openCodeModels")).toBe(3);
    await settle();
    await t.start();
    expect(t.choices()).toMatchObject({ path: "/workspace/other", model: "fixture/local" });
  });
});
