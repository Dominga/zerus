import { For, Show } from "solid-js";
import { Icon } from "../lib/icons";
import { ZerusLogo } from "./ZerusLogo";
import { page, setPage, counts, setMenu, logoMotion, type Page } from "../../store/ui";
import { markAllEntry } from "../sessions/mark-entries";
import { expandAllCollapsedEntry } from "../sessions/background-menu";
import { windowLayer, setWindowLayer } from "../../store/windowLayer";
import { skin } from "../../skin/runtime";
import { newSessionFromRail } from "../sessions/newsession-view";
import { openTasks } from "../../store/taskNavigation";

// The side screen switcher stays on the side (owner decision 2026-10-27).
// Order follows SessionsWindow: brand -> Overview, New session, Sessions,
// Projects, Machines, Accounts; window pin, Settings and the version at the bottom.
// Glyphs and the 24px icon size follow tray/src/SessionsWindow.cpp:328-351.
// Sessions is the one live screen of this phase; the others show where they will be.

const ITEMS: { id: Page; icon: string; label: string; tip: string }[] = [
  { id: "sessions", icon: "sessions", label: "Sessions", tip: "Sessions" },
  { id: "tasks", icon: "list-todo", label: "Tasks", tip: "Project tasks" },
  { id: "projects", icon: "projects", label: "Projects", tip: "Projects" },
  { id: "machines", icon: "machines", label: "Machines", tip: "Machines and connection status" },
  { id: "accounts", icon: "accounts", label: "Accounts", tip: "Agent accounts" },
];


/** "0.1.0" -> "0.1 Dev": the rail shows major.minor and the channel. */
const short = __APP_VERSION__.split(".").slice(0, 2).join(".");

export function Rail() {
  const labeled = () => skin().layout.rail === "labeled";
  return (
    <nav class="rail" classList={{ labeled: labeled() }} aria-label="Screens">
      <button class="rail-brand" classList={{ on: page() === "dashboard" }} data-tip={`Home — Zerus Next ${__APP_VERSION__}`} data-tip-pos="right" onClick={() => setPage("dashboard")}>
        <ZerusLogo attention={counts().attention > 0} motion={logoMotion()} size={28} />
        <Show when={labeled()}><span>Overview</span></Show>
      </button>
      <button class="rail-new" data-tip="New session" data-tip-pos="right" onClick={newSessionFromRail}>
        <Icon name="add" size={24} />
        <Show when={labeled()}><span>New</span></Show>
      </button>
      <For each={ITEMS}>
        {(item) => (
          <button class="rail-btn" classList={{ on: page() === item.id }} aria-current={page() === item.id ? "page" : undefined}
            aria-label={item.label}
            data-tip={labeled() ? undefined : item.tip} data-tip-pos="right" onClick={() => item.id === "tasks" ? openTasks() : setPage(item.id)}
            onContextMenu={(e) => { if (item.id !== "sessions") return; e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, items: [markAllEntry(), expandAllCollapsedEntry()] }); }}>
            <Icon name={item.icon} size={24} />
            <Show when={item.id === "sessions" && counts().attention > 0}>
              <span class="rail-badge">{counts().attention}</span>
            </Show>
            <Show when={labeled()}><span>{item.label}</span></Show>
          </button>
        )}
      </For>
      <div class="grow" />
      <button class="rail-btn pin" classList={{ pinned: windowLayer().on }} aria-pressed={windowLayer().on} aria-label="Keep Zerus above other windows" disabled={!windowLayer().supported}
        data-tip={!windowLayer().supported ? windowLayer().hint : windowLayer().on ? "Unpin Zerus" : "Keep Zerus above other windows"} data-tip-pos="right"
        onClick={() => void setWindowLayer(!windowLayer().on)}>
        <Icon name={windowLayer().on ? "pinned" : "unpinned"} size={24} />
      </button>
      <button class="rail-btn" classList={{ on: page() === "settings" }} data-tip={labeled() ? undefined : "Settings"} data-tip-pos="right" onClick={() => setPage("settings")}>
        <Icon name="settings" size={24} />
        <Show when={labeled()}><span>Settings</span></Show>
      </button>
      <button class="rail-version" data-tip={`Zerus Next ${__APP_VERSION__} (development build)`} data-tip-pos="right" onClick={() => setPage("settings")}>{short} Dev</button>
    </nav>
  );
}
