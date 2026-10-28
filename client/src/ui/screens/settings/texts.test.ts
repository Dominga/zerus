import { describe, expect, it } from "vitest";

// Every text Settings shows for a Qt control is Qt's own: it appears in the
// Qt source it comes from and in the section that shows it.

// Read from disk, as styles/motion.test.ts does: the specifier is built so
// TypeScript (no Node types here) leaves it alone.
const fs = (await import(/* @vite-ignore */ ["node", "fs"].join(":"))) as {
  readFileSync(path: URL, encoding: "utf8"): string;
};
const here = new URL("./", import.meta.url);
const repo = new URL("../../../../../", import.meta.url);
const qt = ["tray/src/SettingsPage.h", "tray/src/UiLanguage.h"].map((file) => fs.readFileSync(new URL(file, repo), "utf8")).join("\n");
const source = (file: string) => fs.readFileSync(new URL(file, here), "utf8");
const layer = fs.readFileSync(new URL("tray/src/WindowLayer.cpp", repo), "utf8");
const keyboard = fs.readFileSync(new URL("tray/src/KeyboardSettings.cpp", repo), "utf8");
const shortcuts = fs.readFileSync(new URL("tray/src/Shortcuts.cpp", repo), "utf8");
const store = fs.readFileSync(new URL("../../../store/shortcuts.ts", here), "utf8");
const speech = fs.readFileSync(new URL("tray/src/speech/SpeechSettings.cpp", repo), "utf8");
const speechCore = fs.readFileSync(new URL("client/core/src/speech_settings.rs", repo), "utf8");

const APPEARANCE = [
  "Follow system appearance", "Dark", "Light", "Interface language",
  "Restart Zerus to apply the interface language. Agent sessions keep running.",
  "Russian is a preview. Untranslated text stays in English.", "Russian (preview)",
  "Usage limit indicator", "Usage and elapsed time bars", "Pace marker", "Pace marker and reset countdown",
  "Compares used quota with the elapsed part of each limit window. The bars show usage above elapsed time; the marker shows elapsed time on the usage bar. Colors warn when the current pace would use up the limit before it resets.",
  "Content scale",
  "Adjusts Activity, the message field and Terminal from 75% to 200%. The session list and the side panel keep their size.",
  "Appearance and session list preferences apply to this Zerus.",
  "Activity width", "Maximum Activity width", "Full-width Activity", "Keep Zerus above other windows",
];
// Qt texts with an argument, as Appearance fills it in.
const APPEARANCE_FILLED: [string, string][] = [
  ["Reset to %1 px", "Reset to 900 px"],
  ["In a wide window, Activity, questions and the message field stay centered at this width, which grows with content scale. Drag the column edges in Activity to adjust it, or double-click an edge to restore %1 px. Full-width Activity fills the whole pane.",
    "In a wide window, Activity, questions and the message field stay centered at this width, which grows with content scale. Drag the column edges in Activity to adjust it, or double-click an edge to restore 900 px. Full-width Activity fills the whole pane."],
];
const WINDOW_LAYER = [
  "Files and terminals opened from Zerus may appear behind it while this is on.",
  "Keep Above is unavailable in this Zerus session. On Wayland, KDE Plasma 6 with KWin scripting is required. You can use your desktop's window menu or rules.",
];
const SESSIONS = [
  "Expand the collapsed session list on hover", "Hide projects without active sessions", "Animate working sessions",
  "Show completed subagents",
  "Include agents whose latest turn has finished in the session list and its counts. Plan & agents always shows all reported agents. Agents with unknown status remain visible.",
  "Notify about optional questions",
  "Off by default. Shows a system notification once for each new optional question, which the agent keeps working without. It is withdrawn when the questions are answered or skipped, or the session is marked as read. Questions that need an answer always notify.",
  "Closing Zerus keeps agents running. Projects and the arrangement of your workspace are saved on this device.",
];

const KEYBOARD = [
  "Keyboard", "Command", "Shortcut", "Alternate", "Reset", "Reset all", "Restore the default chords of the selected command",
  "Chords marked ✓ work in Terminal.", "Double-click a shortcut, or press Enter on it, to change it.", "Works in Terminal",
  "Reset all shortcuts", "Restore the default chords of every command?", "Change shortcut", "Clear", "Shortcut in use",
];
const KEYBOARD_FILLED: [string, string][] = [
  ["Press the new shortcut for %1.", "Press the new shortcut for {title}."],
  ["Already used by %1. Reassign?", "Already used by ${separatedList("],
];

const ABOUT_FILLED: [string, string][] = [
  ["Zerus %1", "`Zerus ${__APP_VERSION__}`"],
  ["hgs CLI %1", "`hgs CLI ${version}`"],
];
const ABOUT = ["About", "hgs CLI version unavailable", "hgs CLI: checking…", "Open Zerus repository on GitHub"];

const VOICE = [
  "Voice input", "Preview before sending",
  "Show Stop instead of Send while recording audio or video. Stop retains a preview for review and a separate Send. This preference applies on this device.",
  "Manage recognition providers on a processing computer. Recording starts on this device; Send voice uses the provider selected beside your recording.",
  "Configure providers on", "Add", "Edit", "Test", "Remove", "Use as default",
  "Test sends a short synthetic recording from the selected computer. It may load a prepared model or use the remote API; it does not use your microphone or send anything to an agent.",
  "Speech provider documentation", "Refresh providers and microphones", "Check operation status", "Microphone on this device",
  "System default microphone", "Selected microphone (unavailable)",
  "Provider settings and credentials stay on their processing computer. Recordings are limited to two minutes. Original audio and recognized text remain in conversation history; your typed draft stays separate.",
  "Remove speech provider?", "Remove provider", "Name", "Profile ID", "Type", "Model", "Language", "Device", "Base URL",
  "Allow unencrypted HTTP for a local or private server", "API key", "Keep stored API key", "No API key", "Replace API key",
  "Remove API key", "New API key", "Auto-detect", "e.g. Team transcription server", "e.g. Russian speech",
  "Unsupported provider type", "HTTP transcription documentation", "Local GigaAM documentation",
  "This provider type cannot be edited by this version of Zerus. See the provider documentation before changing it.",
  String.raw`The processing computer sends audio to the server's /audio/transcriptions endpoint. Use the server's model ID and a two-letter language code, or leave language empty.\n\nThe optional API key is saved on the processing computer. Prefer HTTPS; private HTTP requires explicit consent. Saving sends no audio.`,
  String.raw`Local inference runs on the processing computer named above. Saving creates a profile; it does not install the service or download model weights.\n\nv3_e2e_rnnt is the default for Russian with punctuation; use ru for Russian.\n\nCPU uses the processor. CUDA requires a supported NVIDIA GPU and CUDA-enabled PyTorch. MPS requires a supported Apple GPU/runtime. Use Test to verify the chosen model and device.`,
];
// Texts the core's state machine (speech_settings.rs) shows.
const VOICE_CORE = [
  "Checking provider configuration…", "Loading this computer's voice statistics…", "Voice statistics unavailable.",
  "The computer returned an invalid provider configuration. Update hgs and refresh.",
  "The recognition service is not ready. You can still add and edit providers.",
  "No providers yet. Add Local GigaAM or an OpenAI-compatible server.",
  "Provider configuration is ready. Test a saved provider before using it.",
  "Use as default selects a provider for new recordings. Existing recordings keep their selected provider.",
  "The selected default provider is unavailable. Choose another explicitly or restore it on this computer.",
  "The default provider is configured on another computer. Browsing these providers does not change it.",
  "Working on the provider request. You can keep using Zerus.",
  "That profile ID already exists. Edit the existing provider or choose another ID.",
  "Saving on the selected computer. If the result is not confirmed, use Check operation status.",
  "The operation is not confirmed. Check its status before making another change.",
  "The saved configuration changed. Your edits are kept here. Close and reopen the provider to review its current values before saving.",
  "Could not save the operation identity. No provider request was sent.", "The provider request failed.",
  "Prepare the speech service on this processing computer before testing.", "HTTP server", "Local GigaAM", "Ready", "Not ready",
  "Add speech provider", "Edit speech provider", "This Mac", "This Windows computer", "This computer (Linux)",
];
const VOICE_CORE_FILLED: [string, string][] = [
  ["Default for new recordings: %1", "Default for new recordings: {}"],
  ["Processing computer: %1", "Processing computer: {computer}"],
  ["The provider test on %1 completed successfully.", "The provider test on {computer} completed successfully."],
  ["Provider configuration saved on %1.", "Provider configuration saved on {computer}."],
  ["A provider request on %1 is not yet confirmed. Check its status; it will not be sent again automatically.",
    "A provider request on {} is not yet confirmed. Check its status; it will not be sent again automatically."],
  ["Recognition service is available on %1. Test the saved profile to verify the selected provider.",
    "Recognition service is available on {computer}. Test the saved profile to verify the selected provider."],
  ["Recognition service is not ready on %1. %2", "Recognition service is not ready on {computer}. {}"],
];

const recoveryQt = fs.readFileSync(new URL("tray/src/RecoverySettings.h", repo), "utf8");
const recoveryCore = fs.readFileSync(new URL("client/core/src/recovery_settings.rs", repo), "utf8");
const RECOVERY = [
  "Automatic recovery", "Shared by all connected machines. Agents finish their own retries first; recovery continues the same task.",
  "Recover automatically after temporary failures", "Overload / server unavailable", "Temporary rate limit", "Session usage limit",
  "Connection / timeout", "Wait for reset", "Retry on a schedule",
  "Wait until the provider's reset time by default. If no reset time is available, recovery needs your attention. Choose a schedule to retry at your own intervals.",
  "Retry after (seconds)", "Save recovery settings", "Reload", "Saved.",
  String.raw`Each positive number is a delay before the next attempt. End with −1 to stop, or 0 to repeat the previous delay forever.\nExample: 300, 0 retries every 5 minutes until stopped.\n\nNew messages and Stop cancel recovery. Sign-in, balance, other quota and context errors require your action.`,
];
const RECOVERY_CORE = [
  "To configure automatic recovery, enable local sessions for this computer in Machines or add a remote machine.",
  "Add a remote machine in Machines to configure automatic recovery; this computer runs no agents.",
  "Synchronizing recovery settings…", "Synchronized with 1 machine. Changes from any Zerus are shared.",
  "Wait for synchronization to finish before saving.", "Settings changed in another Zerus. Reload before saving.",
  "Connect a remote machine to save recovery settings.", "Update HGS on this machine to synchronize recovery settings.",
  "this machine", "Use 1–16 values: delays from 1 to 86400 seconds; −1 or 0 may only be last, and 0 needs a preceding delay.",
];

const filesQt = fs.readFileSync(new URL("tray/src/DownloadCacheSettings.cpp", repo), "utf8");
const cacheQt = fs.readFileSync(new URL("tray/src/DownloadCache.cpp", repo), "utf8");
const cacheCore = fs.readFileSync(new URL("client/core/src/download_cache.rs", repo), "utf8");
const FILES = [
  "Files",
  "Download and open reuses a verified saved copy from the same machine and path. Show file selects it in your file manager. Download again gets a fresh copy; Download… lets you choose where to save. Existing files are preserved.",
  "Downloaded files", "Never clear", "Limit size", "Keep copies", "Maximum size",
  "Never clear is the default. With a limit, Zerus removes its least recently used downloaded copies first. Files you add or edit are kept. Downloads in progress and copies used in the last minute are protected, so usage can briefly exceed the limit. This setting applies to this computer.",
  "Checking downloaded files…", "Apply", "Limit downloaded files?", "Apply limit",
];
const FILES_CORE = [
  "Cannot create the downloaded files folder.", "The downloaded files folder is unavailable or is a link.",
  "The downloaded files cache is busy.", "Cannot read the downloaded files index.",
  "The downloaded files index is invalid. Existing files were preserved.", "Cannot save the downloaded files index.",
  "Some older downloaded copies could not be removed.", "The downloaded files index is a link.",
  "Cannot prepare the local download.", "The downloaded file source is invalid.", "The prepared download is unavailable.",
  "The downloaded copy could not be verified.", "Cannot save the downloaded copy in its cache folder.",
  "Cannot find an unused name for the downloaded copy.",
];

describe("Settings texts", () => {
  it.each(FILES)("Files shows Qt's %s", (text) => {
    expect(filesQt).toContain(`"${text}"`);
    expect(source("Files.tsx")).toContain(text);
  });
  it("Files fills in Qt's limit question", () => {
    expect(filesQt).toContain('"Zerus will automatically delete its oldest downloaded copies when they exceed %1. Files you added or edited will be kept."');
    expect(source("Files.tsx")).toContain("Zerus will automatically delete its oldest downloaded copies when they exceed ${formatDataSize(props.bytes)}. Files you added or edited will be kept.");
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
  it.each(KEYBOARD)("Keyboard shows Qt's %s", (text) => {
    expect(keyboard).toContain(`"${text}"`);
    expect(source("Keyboard.tsx")).toContain(text);
  });
  it.each(KEYBOARD_FILLED)("Keyboard fills in Qt's %s", (template, shown) => {
    expect(keyboard).toContain(`"${template}"`);
    expect(source("Keyboard.tsx")).toContain(shown);
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
  it.each(WINDOW_LAYER)("Appearance shows WindowLayer's %s", (text) => {
    expect(layer).toContain(`"${text}"`);
    expect(source("Appearance.tsx")).toContain(text);
  });
  it.each(SESSIONS)("Sessions shows Qt's %s", (text) => {
    expect(qt).toContain(`"${text}"`);
    expect(source("Sessions.tsx")).toContain(text);
  });
  it("lists the sections in Qt's order", () => {
    const labels = [...source("sections.ts").matchAll(/label: "([^"]+)"/g)].map((m) => m[1]);
    const nav = /nav->addItems\(\{([^}]*)\}\)/.exec(qt)![1];
    const qtOrder = [...nav.matchAll(/tr\("([^"]+)"\)/g)].map((m) => m[1]);
    // Worktrees goes in before Automatic approvals (SettingsPage.h:140).
    qtOrder.splice(qtOrder.indexOf("Automatic approvals"), 0, "Worktrees");
    expect(labels).toEqual(qtOrder);
  });
});
