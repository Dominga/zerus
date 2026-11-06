import { describe, expect, it } from "vitest";
import { APPROVALS_TITLE, APPROVALS_BODY, APPROVALS_CONFIRM, APPROVALS_WARNING, APPROVALS_HINT } from "./approval-opt-in";
import contract from "./fixtures/text-contracts.json";
import catalog from "../../../i18n.ru.json";

// These exported translation keys are shared by the reviewed consent dialog.
// Ordinary Settings labels/order no longer mirror the retired Qt UI.
const keys = { APPROVALS_TITLE, APPROVALS_BODY, APPROVALS_CONFIRM, APPROVALS_WARNING, APPROVALS_HINT };
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as { readFileSync(path: URL, encoding: "utf8"): string };
const repo = new URL("../../../../../", import.meta.url);
const mockup = ["design/webui/src/screens/settings/Policies.tsx", "design/webui/src/dialogs/SettingsDialogs.tsx"]
  .map((file) => fs.readFileSync(new URL(file, repo), "utf8")).join("\n");

describe("Automatic approval consent translation keys", () => {
  it("keeps the exported keys used by the consent dialog", () => {
    expect(keys).toEqual(contract.approvalTranslationKeys);
  });
  it("Processes keeps Qt's timing ranges", () => {
    const TIMINGS = [...source("Processes.tsx").matchAll(/\{ key: "([^"]+)", label: (?:trNoop\()?"([^"]+)"\)?, min: ([\d.]+), max: ([\d.]+) \}/g)]
      .map((m) => ({ key: m[1], label: m[2], min: Number(m[3]), max: Number(m[4]) }));
    const qtTimings = [...qt.matchAll(/timing\("(\w+)",tr\("([^"]+)"\),ProcessSettings::\w+\(\),([\d.]+),([\d.]+)\)/g)]
      .map((m) => ({ key: `processes/${m[1]}`, label: m[2], min: Number(m[3]), max: Number(m[4]) }));
    expect(qtTimings.length).toBe(4);
    expect(TIMINGS.map((t) => ({ key: t.key, label: t.label, min: t.min, max: t.max }))).toEqual(qtTimings);
  });
  it.each(PROCESS_TAB)("the Processes tab shows Qt's %s", (text) => {
    expect(processView).toContain(`tr("${text}`);
    expect(sessionsUi("Processes.tsx")).toContain(text);
  });
  it("the Processes tab fills in Qt's counts", () => {
    expect(processView).toContain('tr("Terminate selected (%1)")');
    expect(sessionsUi("Processes.tsx")).toContain(`tr("Terminate selected (%1)", ids.length)`);
    expect(sessionsWindow).toContain('tr("Processes (%1)")');
    expect(processStore).toContain('tr("Processes (%1)", count)');
  });
  it.each(PROCESS_MENU)("the session menu shows Qt's %s", (text) => {
    expect(sessionsWindow).toContain(`tr("${text}")`);
    expect(processStore).toContain(text);
  });

  it.each(ANALYTICS)("Private analytics shows Qt's %s", (text) => {
    expect(analyticsQt).toContain(`"${text}"`);
    expect(source("Analytics.tsx") + source("analytics-text.ts")).toContain(text);
  });
  it.each(ANALYTICS_CORE)("the analytics core says Qt's %s", (text) => {
    expect(analyticsQt).toContain(`"${text}"`);
    expect(analyticsCore).toContain(`"${text}"`);
  });
  it("the analytics core counts the routes it reads as Qt does", () => {
    expect(analyticsQt).toContain('"Reading %1 configured route(s)…"');
    expect(analyticsCore).toContain('"Reading {} configured route(s)…"');
  });
  it.each([APPROVALS_TITLE, APPROVALS_BODY, APPROVALS_CONFIRM, APPROVALS_WARNING, APPROVALS_HINT])("the dangerous opt-in uses Qt and the mockup's text: %s", (text) => {
    const mockup = fs.readFileSync(new URL("design/webui/src/screens/settings/Policies.tsx", repo), "utf8") + fs.readFileSync(new URL("design/webui/src/dialogs/SettingsDialogs.tsx", repo), "utf8");
    expect(approvalsQt).toContain(text); expect(mockup).toContain(text);
  });
  it.each(APPROVALS)("Automatic approvals shows Qt's %s", (text) => {
    expect(approvalsQt).toContain(`"${text}"`);
    expect(source("Approvals.tsx") + source("approvals-journal.ts")).toContain(text);
  });
  it.each(APPROVALS_CORE)("the approval journal says Qt's %s", (text) => {
    expect(approvalsQt).toContain(`"${text}"`);
    expect(approvalsCore).toContain(`"${text}"`);
  });
  it("the approval journal fills in Qt's counts and answers", () => {
    expect(approvalsQt).toContain('"%1 entries. Unconfirmed attempts require checking Activity or Terminal; they are not retried."');
    expect(source("approvals-journal.ts")).toContain("${page.entries.length} entries. Unconfirmed attempts require checking Activity or Terminal; they are not retried.");
    expect(approvalsQt).toContain('"Selected answer: %1"');
    expect(approvalsCore).toContain('"Selected answer: {}"');
  });
  it.each(WORKTREES)("Worktrees shows Qt's %s", (text) => {
    expect(worktreesQt).toContain(`"${text}"`);
    expect(source("Worktrees.tsx")).toContain(text);
  });
  it.each(WORKTREES_CORE)("the cleanup policy says Qt's %s", (text) => {
    expect(worktreesQt).toContain(`"${text}"`);
    expect(worktreesCore).toContain(`"${text}"`);
  });
  it.each(MOBILE)("Mobile connection shows Qt's %s", (text) => {
    expect(mobileQt).toContain(`"${text}"`);
    expect(source("Mobile.tsx") + source("ComputerInvitations.tsx") + source("WorkspacePanel.tsx")).toContain(text);
  });
  it.each(MOBILE_CORE)("the mobile core says Qt's %s", (text) => {
    expect(mobileQt.replaceAll("\\n", "\n")).toContain(text);
    expect(mobileCore).toContain(text);
  });
  it("Mobile connection shows Qt's Phones introduction, which Qt writes in two pieces", () => {
    expect(mobileQt).toContain('"Phones paired with your computers. A phone can control every computer of its workspace; "\n                                "revoke one you no longer use."');
    expect(source("Mobile.tsx")).toContain("Phones paired with your computers. A phone can control every computer of its workspace; revoke one you no longer use.");
  });
  it("the mobile core fills in Qt's formats", () => {
    expect(mobileQt).toContain('"Update Zerus on %1 to manage phones from here."');
    expect(mobileCore).toContain('"Update Zerus on {computer} to manage phones from here."');
    expect(mobileQt).toContain('"Could not read the machine list: %1"');
    expect(mobileCore).toContain('"Could not read the machine list: {error}"');
    expect(mobileQt).toContain('"%1 phones"');
    expect(mobileCore).toContain('"{count} phones"');
    expect(mobileQt).toContain('"%1 (via %2)"');
    expect(source("Mobile.tsx")).toContain("(via ${list.host})");
    expect(mobileQt).toContain('"Could not save: %1"');
    expect(mobileCore).toContain('"Could not save: {}"');
    expect(mobileQt).toContain('"Could not read the shared setting: %1"');
    expect(mobileCore).toContain('"Could not read the shared setting: {}"');
    expect(mobileQt).toContain('"Update Zerus on %1, a computer this one synchronizes with directly. Until then it keeps syncing projects but does not receive this choice."');
    expect(mobileCore).toContain('"Update Zerus on {one}, a computer this one synchronizes with directly. Until then it keeps syncing projects but does not receive this choice."');
    expect(mobileQt).toContain('"Update Zerus on these computers this one synchronizes with directly: %1. Until then they keep syncing projects but do not receive this choice."');
    expect(mobileCore).toContain('"Update Zerus on these computers this one synchronizes with directly: {}. Until then they keep syncing projects but do not receive this choice."');
    expect(mobileQt).toContain('"The sync service on this computer is outdated and must be restarted (%1, or sign out and back in). Until then this computer may stop syncing projects and this choice."');
    expect(mobileCore).toContain('"The sync service on this computer is outdated and must be restarted ({}, or sign out and back in). Until then this computer may stop syncing projects and this choice."');
  });
  it.each(FILES)("Files shows Qt's %s", (text) => {
    expect(filesQt).toContain(`"${text}"`);
    expect(source("Files.tsx")).toContain(text);
  });
  it("Files fills in Qt's limit question", () => {
    expect(filesQt).toContain('"Zerus will automatically delete its oldest downloaded copies when they exceed %1. Files you added or edited will be kept."');
    expect(source("Files.tsx")).toContain('tr("Zerus will automatically delete its oldest downloaded copies when they exceed %1. Files you added or edited will be kept.", formatDataSize(props.bytes))');
  });
  it.each(FILES_CORE)("the download cache says Qt's %s", (text) => {
    expect(cacheQt).toContain(`"${text}"`);
    expect(cacheCore).toContain(`"${text}"`);
  });
  it.each(RECOVERY)("Automatic recovery shows Qt's %s", (text) => {
    expect(recoveryQt).toContain(`"${text}"`);
    expect(source("Recovery.tsx")).toContain(text);
  });
  it.each(RECOVERY_CORE)("the recovery core says Qt's %s", (text) => {
    expect(recoveryQt).toContain(`"${text}"`);
    expect(recoveryCore).toContain(`"${text}"`);
  });
  it.each(VOICE)("Voice input shows Qt's %s", (text) => {
    expect(speech).toContain(`"${text}"`);
    expect(source("Voice.tsx")).toContain(text);
  });
  it.each(VOICE_CORE)("the Voice input core says Qt's %s", (text) => {
    expect(speech).toContain(`"${text}"`);
    expect(speechCore).toContain(`"${text}"`);
  });
  it.each(VOICE_CORE_FILLED)("the Voice input core fills in Qt's %s", (template, shown) => {
    expect(speech).toContain(`"${template}"`);
    expect(speechCore).toContain(shown);
  });
  it.each(ABOUT)("About shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
    expect(source("About.tsx")).toContain(text);
  });
  it.each(ABOUT_FILLED)("About fills in Qt's %s", (template, shown) => {
    expect(qt).toContain(`"${template}"`);
    expect(source("About.tsx")).toContain(shown);
  });
  it("About names the build as UpdateController::installedVersion does", () => {
    const updates = fs.readFileSync(new URL("tray/src/UpdateController.cpp", repo), "utf8");
    expect(updates).toContain('tr("%1 (local changes %2)")');
    expect(source("about-version.ts")).toContain('tr("%1 (local changes %2)", version, source)');
    expect(updates).toContain('QString("%1 (%2)")');
    expect(source("about-version.ts")).toContain("`${version} (${source})`");
  });
  it.each(KEYBOARD)("Keyboard shows Qt's %s", (text) => {
    expect(keyboard).toContain(`"${text}"`);
    expect(source("Keyboard.tsx")).toContain(text);
  });
  it("uses Shortcuts.cpp's titles, groups, reasons and Terminal rules", () => {
    const qtTexts = [...shortcuts.matchAll(/(?:QT_TRANSLATE_NOOP\("Shortcuts", |translate\("Shortcuts", )"((?:[^"\\]|\\.)+)"/g)].map((m) => m[1]);
    expect(qtTexts.length).toBeGreaterThan(40);
    for (const text of qtTexts) expect(store, text).toContain(`"${text}"`);
  });
  it.each(APPEARANCE)("Appearance shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
    expect(source("Appearance.tsx")).toContain(text);
  });
  it.each(APPEARANCE_FILLED)("Appearance fills in Qt's %s", (template, shown) => {
    expect(qt).toContain(`"${template}"`);
    expect(source("Appearance.tsx")).toContain(shown);
  });
  it.each(WINDOW_LAYER)("Keep Above shows WindowLayer's %s", (text) => {
    expect(layer).toContain(`"${text}"`);
    expect(clientLayer).toContain(text);
  });
  it.each(SESSIONS)("Sessions shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
    expect(source("Sessions.tsx")).toContain(text);
  });
  it("lists the sections in Qt's order", () => {
    const labels = [...source("sections.ts").matchAll(/label: (?:trNoop\()?"([^"]+)"/g)].map((m) => m[1]);
    const nav = /nav->addItems\(\{([^}]*)\}\)/.exec(qt)![1];
    const qtOrder = [...nav.matchAll(/tr\("([^"]+)"\)/g)].map((m) => m[1]);
    // Worktrees goes in before Automatic approvals (SettingsPage.h:140).
    qtOrder.splice(qtOrder.indexOf("Automatic approvals"), 0, "Worktrees");
    // The fork's Extensions page (SettingsPage.h:146, 300) is in the nav list
    // already, so the client shows it there too.
    // The fork appends Teams and Model servers after the nav list (SessionsWindow.cpp:2235, 2241).
    const sessionsWindow = fs.readFileSync(new URL("tray/src/SessionsWindow.cpp", repo), "utf8");
    const appended = [...sessionsWindow.matchAll(/addSection\(tr\("([^"]+)"\)/g)].map((match) => match[1]);
    qtOrder.push(...appended);
    expect(labels).toEqual(qtOrder);
  });

  it.each(Object.values(contract.approvalTranslationKeys))("retains the reviewed and translated consent: %s", (key) => {
    expect(mockup).toContain(key);
    expect(Object.hasOwn(catalog, key)).toBe(true);
  });
});
