import { createSignal, createRoot, createEffect, on } from "solid-js";
import { createStore } from "solid-js/store";
import { skins, skin, setSkin, registerSkin as registerRuntimeSkin } from "../../skin/runtime";
import type { ResolvedSkin } from "../../skin/skin";
import { SHORTCUTS, type Channel, type CleanupPolicy } from "../../mock/settings-extra";

// Local UI state for Settings, Machines and Accounts. In the Tauri client the
// GUI-local values come from the migrated QSettings keys named beside each
// field; shared values (recovery, phone timing, cleanup rules) are read from
// and written through hgs.

const params = new URLSearchParams(location.search);

export type SectionId =
  | "appearance" | "sessions" | "recovery" | "processes" | "updates" | "mobile" | "files"
  | "worktrees" | "approvals" | "voice" | "keyboard" | "analytics" | "about";

// Order of SettingsPage.h's navigation list (Worktrees is inserted after Files).
export const SECTIONS: { id: SectionId; label: string; icon: string }[] = [
  { id: "appearance", label: "Appearance", icon: "palette" },
  { id: "sessions", label: "Sessions", icon: "rows-3" },
  { id: "recovery", label: "Automatic recovery", icon: "rotate-ccw" },
  { id: "processes", label: "Processes", icon: "activity" },
  { id: "updates", label: "Updates", icon: "download" },
  { id: "mobile", label: "Mobile connection", icon: "smartphone" },
  { id: "files", label: "Files", icon: "hard-drive" },
  { id: "worktrees", label: "Worktrees", icon: "folder-git-2" },
  { id: "approvals", label: "Automatic approvals", icon: "shield-check" },
  { id: "voice", label: "Voice input", icon: "mic" },
  { id: "keyboard", label: "Keyboard", icon: "keyboard" },
  { id: "analytics", label: "Private analytics", icon: "gauge" },
  { id: "about", label: "About", icon: "info" },
];

const initialSection = params.get("section") as SectionId | null;
export const [section, setSection] = createSignal<SectionId>(
  initialSection && SECTIONS.some((s) => s.id === initialSection) ? initialSection : "appearance",
);

/** Platform preview for platform-specific rows (?platform=linux|macos|windows). */
export type Platform = "linux" | "macos" | "windows";
const p = params.get("platform");
export const platform: Platform = p === "macos" || p === "windows" ? p : "linux";
export const isMac = platform === "macos";

/** Display a portable chord ("Ctrl+K") in the platform's notation. */
export function chordText(chord: string): string {
  if (!isMac) return chord;
  return chord
    .split(", ")
    .map((part) => part
      .replace(/Ctrl\+/g, "⌘").replace(/Meta\+/g, "⌃").replace(/Alt\+/g, "⌥").replace(/Shift\+/g, "⇧")
      .replace(/Return/g, "↩").replace(/Down/g, "↓").replace(/Up/g, "↑").replace(/Tab/g, "⇥"))
    .join(", ");
}

// ------------------------------------------------------------------ preferences

export type Theme = "system" | "dark" | "light";

export const [prefs, setPrefs] = createStore({
  // Appearance (workspace/*)
  theme: "system" as Theme,
  darkSkin: "zerus-dark",
  lightSkin: "zerus-light",
  language: "en",
  usageIndicator: "rails",
  activityWidth: 900,
  fullWidth: false,
  // Overview: the default width of its centred column (720-2400) and full width.
  overviewWidth: 1200,
  overviewFullWidth: false,
  onTop: false,
  // Sessions
  expandOnHover: true,
  hideEmptyProjects: true,
  animateActivity: true,
  showCompletedSubagents: false,
  optionalQuestions: false,
  // Processes (processes/*)
  processesEnabled: false,
  procActive: 2.5,
  procBackground: 30,
  procUnknown: 10,
  procLoading: 0.5,
  // Updates (updates/*)
  updatesAutomatic: true,
  channel: "dev" as Channel,
  // Files (downloads/cacheBytes: 0 = never)
  cacheLimitGiB: 0,
  // Automatic approvals (automaticApprovals/enabled)
  approvals: false,
  // Voice (speech/*)
  speechPreview: false,
  speechDefault: "gigaam-ru",
  // Phone timing, shared through the swarm catalog
  delivery: "away",
});

// ------------------------------------------------------------------ skins

const media = matchMedia("(prefers-color-scheme: light)");
const [systemLight, setSystemLight] = createSignal(media.matches);
media.addEventListener("change", (e) => setSystemLight(e.matches));
export { systemLight };

/** Built-in plus imported skins, in display order. */
export const [skinList, setSkinList] = createSignal<ResolvedSkin[]>([...skins]);
export const [importedIds, setImportedIds] = createSignal<string[]>([]);

const skinById = (id: string) => skinList().find((s) => s.id === id);

/** The skin in effect for a theme choice: one dark and one light skin form a pair. */
export function skinFor(theme: Theme): string {
  if (theme === "dark") return prefs.darkSkin;
  if (theme === "light") return prefs.lightSkin;
  return systemLight() ? prefs.lightSkin : prefs.darkSkin;
}

export function chooseTheme(theme: Theme) {
  setPrefs("theme", theme);
  setSkin(skinFor(theme));
}

/** Choose a skin for its appearance. Returns false when it waits for the other system appearance. */
export function chooseSkin(id: string): boolean {
  const s = skinById(id);
  if (!s) return false;
  setPrefs(s.appearance === "dark" ? "darkSkin" : "lightSkin", id);
  if (prefs.theme !== "system" && prefs.theme !== s.appearance) setPrefs("theme", s.appearance);
  const active = skinFor(prefs.theme);
  setSkin(active);
  return active === id;
}

/** Register a validated imported skin: its compiled custom properties plus the list entry. */
export function registerSkin(resolved: ResolvedSkin, _css?: string) {
  registerRuntimeSkin(resolved);
  // Keep the local list (used by the skin cards) in step with the runtime.
  const existing = skins.findIndex((s) => s.id === resolved.id);
  if (existing >= 0) skins.splice(existing, 1, resolved);
  else skins.push(resolved);
  setSkinList([...skins]);
  setImportedIds((ids) => (ids.includes(resolved.id) ? ids : [...ids, resolved.id]));
}

createRoot(() => {
  // Start from the skin in effect (URL, saved choice or system appearance).
  const start = skin();
  setPrefs(start.appearance === "dark" ? "darkSkin" : "lightSkin", start.id);
  if ((start.appearance === "light") !== systemLight()) setPrefs("theme", start.appearance);
  // The mockup's review bar can switch skins directly; keep the pair in step.
  createEffect(on(skin, (s) => {
    setPrefs(s.appearance === "dark" ? "darkSkin" : "lightSkin", s.id);
    if (prefs.theme === "system" ? (s.appearance === "light") !== systemLight() : prefs.theme !== s.appearance) setPrefs("theme", s.appearance);
  }, { defer: true }));
  createEffect(on(systemLight, () => { if (prefs.theme === "system") setSkin(skinFor("system")); }, { defer: true }));
});

// ------------------------------------------------------------------ keyboard

const defaults = (id: string) => {
  const c = SHORTCUTS.find((s) => s.id === id)!;
  return [...(isMac && c.mac ? c.mac : c.linux)];
};
export const [chords, setChords] = createStore<Record<string, string[]>>(
  Object.fromEntries(SHORTCUTS.map((c) => [c.id, defaults(c.id)])),
);
export const isCustomized = (id: string) => chords[id].join("|") !== defaults(id).join("|");
export const resetChords = (id: string) => setChords(id, defaults(id));
export const resetAllChords = () => { for (const c of SHORTCUTS) setChords(c.id, defaults(c.id)); };
export const [selectedCommand, setSelectedCommand] = createSignal<string | undefined>("session.new");
/** The chord being edited: command id and slot (0 = Shortcut, 1 = Alternate). */
export const [editing, setEditing] = createSignal<{ id: string; slot: number; captured: string; conflicts: string[] } | undefined>();

// ------------------------------------------------------------------ dialog context

/** Parameters shared by parameterized dialogs (which phone, which machine, which account). */
export const [dialogCtx, setDialogCtx] = createStore<{
  phone?: string;
  speechEdit?: string; // provider id, or "" for Add
  speechHost: string;
  analyticsRetention: number;
  retentionDraft: number;
  cacheDraftGiB: number;
}>({ speechHost: "", analyticsRetention: 90, retentionDraft: 90, cacheDraftGiB: 1 });

/** Worktree cleanup rules per computer route ("" = this computer). */
export const [cleanupDraft, setCleanupDraft] = createStore<{ route: string; policy?: CleanupPolicy; status: string; error: boolean }>({
  route: "", status: "Refresh to read this computer's cleanup rules.", error: false,
});
