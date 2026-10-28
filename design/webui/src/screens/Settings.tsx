import { For, Match, Switch } from "solid-js";
import { Icon } from "../lib/icons";
import { SECTIONS, section, setSection } from "./settings/store";
import { AppearanceSection } from "./settings/Appearance";
import { SessionsSection, ProcessesSection, FilesSection, AboutSection } from "./settings/General";
import { RecoverySection } from "./settings/Recovery";
import { UpdatesSection } from "./settings/Updates";
import { MobileSection } from "./settings/Mobile";
import { WorktreesSection, ApprovalsSection } from "./settings/Policies";
import { VoiceSection } from "./settings/Voice";
import { KeyboardSection } from "./settings/Keyboard";
import { AnalyticsSection } from "./settings/Analytics";
import "../styles/settings.css";

// Settings (SettingsPage.h): a fixed section list and one scrollable page at a
// time, at most 850 px wide. Pages that read live state refresh when opened.

export function SettingsScreen() {
  let scroller!: HTMLDivElement;
  const open = (id: (typeof SECTIONS)[number]["id"]) => { setSection(id); scroller.scrollTop = 0; };
  const onNavKey = (e: KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const i = SECTIONS.findIndex((s) => s.id === section());
    const next = SECTIONS[(i + (e.key === "ArrowDown" ? 1 : SECTIONS.length - 1)) % SECTIONS.length];
    open(next.id);
    (document.getElementById(`set-nav-${next.id}`) as HTMLElement | null)?.focus();
  };
  return (
    <div class="set-page">
      <div class="set-shell">
        <nav class="set-nav" aria-label="Settings sections" onKeyDown={onNavKey}>
          <h1>Settings</h1>
          <div role="tablist" aria-orientation="vertical" class="set-nav-list">
            <For each={SECTIONS}>
              {(s) => (
                <button id={`set-nav-${s.id}`} role="tab" class="set-nav-item" classList={{ on: section() === s.id }} aria-selected={section() === s.id}
                  tabindex={section() === s.id ? 0 : -1} aria-controls="set-content" onClick={() => open(s.id)}>
                  <Icon name={s.icon} size={15} stroke={1.8} />
                  <span class="ell">{s.label}</span>
                </button>
              )}
            </For>
          </div>
        </nav>
        <div class="set-scroll" ref={scroller} id="set-content" role="tabpanel" aria-labelledby={`set-nav-${section()}`}>
          <div class="set-content">
            <Switch>
              <Match when={section() === "appearance"}><AppearanceSection /></Match>
              <Match when={section() === "sessions"}><SessionsSection /></Match>
              <Match when={section() === "recovery"}><RecoverySection /></Match>
              <Match when={section() === "processes"}><ProcessesSection /></Match>
              <Match when={section() === "updates"}><UpdatesSection /></Match>
              <Match when={section() === "mobile"}><MobileSection /></Match>
              <Match when={section() === "files"}><FilesSection /></Match>
              <Match when={section() === "worktrees"}><WorktreesSection /></Match>
              <Match when={section() === "approvals"}><ApprovalsSection /></Match>
              <Match when={section() === "voice"}><VoiceSection /></Match>
              <Match when={section() === "keyboard"}><KeyboardSection /></Match>
              <Match when={section() === "analytics"}><AnalyticsSection /></Match>
              <Match when={section() === "about"}><AboutSection /></Match>
            </Switch>
          </div>
        </div>
      </div>
    </div>
  );
}
