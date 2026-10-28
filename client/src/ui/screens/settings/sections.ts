import { createSignal, type JSX } from "solid-js";
import { setPage } from "../../../store/ui";
import { AppearanceSection } from "./Appearance";
import { SessionsSection } from "./Sessions";
import { KeyboardSection } from "./Keyboard";
import { VoiceSection, refreshVoice } from "./Voice";
import { RecoverySection, refreshRecovery } from "./Recovery";
import { FilesSection } from "./Files";
import { AboutSection, refreshAbout } from "./About";

// Settings sections in Qt's order (tray/src/SettingsPage.h:43, Worktrees
// inserted at :140). A section shows once it has a component; `refresh` runs
// whenever it is opened (SettingsPage.h:157). Phase 4 Task 11 adds Processes
// here as ProcessesSection (./Processes.tsx).

export type SectionId =
  | "appearance" | "sessions" | "recovery" | "processes" | "updates" | "mobile" | "files"
  | "worktrees" | "approvals" | "voice" | "keyboard" | "analytics" | "about";

export interface Section {
  id: SectionId;
  label: string;
  icon: string;
  component?: () => JSX.Element;
  refresh?: () => void;
}

export const SECTIONS: readonly Section[] = [
  { id: "appearance", label: "Appearance", icon: "palette", component: AppearanceSection },
  { id: "sessions", label: "Sessions", icon: "rows-3", component: SessionsSection },
  { id: "recovery", label: "Automatic recovery", icon: "rotate-ccw", component: RecoverySection, refresh: refreshRecovery },
  { id: "processes", label: "Processes", icon: "activity" },
  { id: "updates", label: "Updates", icon: "download" },
  { id: "mobile", label: "Mobile connection", icon: "smartphone" },
  { id: "files", label: "Files", icon: "hard-drive", component: FilesSection },
  { id: "worktrees", label: "Worktrees", icon: "folder-git-2" },
  { id: "approvals", label: "Automatic approvals", icon: "shield-check" },
  { id: "voice", label: "Voice input", icon: "mic", component: VoiceSection, refresh: refreshVoice },
  { id: "keyboard", label: "Keyboard", icon: "keyboard", component: KeyboardSection },
  { id: "analytics", label: "Private analytics", icon: "gauge" },
  { id: "about", label: "About", icon: "info", component: AboutSection, refresh: refreshAbout },
];

/** The sections the client has so far, in Qt's order. */
export const available = (): Section[] => SECTIONS.filter((s) => s.component);

const params = new URLSearchParams(typeof location === "undefined" ? "" : location.search);
const initial = available().find((s) => s.id === params.get("section"))?.id ?? "appearance";
export const [section, setSection] = createSignal<SectionId>(initial);

/** Opens Settings at `id` (Qt's openUpdates, openKeyboard, openRecovery, openAbout). */
export function openSettings(id: SectionId = section()) {
  const target = available().find((s) => s.id === id) ?? available()[0];
  setSection(target.id);
  target.refresh?.();
  setPage("settings");
}
