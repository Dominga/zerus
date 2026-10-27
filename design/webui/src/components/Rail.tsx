import { For, Show } from "solid-js";
import { Icon, ZerusMark } from "../lib/icons";
import { createSignal } from "solid-js";
import { page, setPage, setDialog, counts, setMenu, toast, type Page } from "../state";
import { skin } from "../skin/runtime";
import { setSection } from "../screens/settings/store";

// The side screen switcher stays on the side (owner decision 2026-10-27).
// Order follows SessionsWindow: brand -> Overview, New session, Sessions,
// Projects, Machines, Accounts; Settings and the version at the bottom.

const ITEMS: { id: Page; icon: string; label: string; tip: string }[] = [
  { id: "sessions", icon: "rows-3", label: "Sessions", tip: "Sessions (Ctrl+2)" },
  { id: "projects", icon: "folder", label: "Projects", tip: "Projects (Ctrl+3)" },
  { id: "machines", icon: "monitor", label: "Machines", tip: "Machines and connection status (Ctrl+4)" },
  { id: "accounts", icon: "user-round-cog", label: "Accounts", tip: "Agent accounts (Ctrl+5)" },
];
const [pinned, setPinned] = createSignal(false);

export function Rail() {
  const labeled = () => skin().layout.rail === "labeled";
  return (
    <nav class="rail" classList={{ labeled: labeled() }} aria-label="Screens">
      <button class="rail-brand" classList={{ on: page() === "dashboard" }} data-tip="Home — Zerus 0.48.0-dev.912 (Ctrl+1)" data-tip-pos="right" onClick={() => setPage("dashboard")}>
        <ZerusMark size={26} />
        <Show when={labeled()}><span>Overview</span></Show>
      </button>
      <button class="rail-new" data-tip="New session (Ctrl+N)" data-tip-pos="right" onClick={() => setDialog("new-session")}>
        <Icon name="plus" size={18} />
        <Show when={labeled()}><span>New</span></Show>
      </button>
      <For each={ITEMS}>
        {(item) => (
          <button class="rail-btn" classList={{ on: page() === item.id }} aria-current={page() === item.id ? "page" : undefined}
            data-tip={labeled() ? undefined : item.tip} data-tip-pos="right" onClick={() => setPage(item.id)}
            onContextMenu={(e) => { if (item.id !== "sessions") return; e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, items: [{ label: `Mark all as read (${counts().attention})`, icon: "check-check" }, { label: "Expand all collapsed", icon: "rows-3" }] }); }}>
            <Icon name={item.icon} size={18} stroke={1.7} />
            <Show when={item.id === "sessions" && counts().attention > 0}>
              <span class="rail-badge">{counts().attention}</span>
            </Show>
            <Show when={labeled()}><span>{item.label}</span></Show>
          </button>
        )}
      </For>
      <div class="grow" />
      <button class="rail-btn pin" classList={{ pinned: pinned() }} data-tip={pinned() ? "Unpin Zerus" : "Keep Zerus above other windows"} data-tip-pos="right"
        onClick={() => { setPinned(!pinned()); toast(pinned() ? "Pinned" : "Unpinned", pinned() ? "Zerus stays above other windows (KWin)" : "Normal window stacking"); }}>
        <Icon name={pinned() ? "pin" : "pin-off"} size={17} stroke={1.7} />
      </button>
      <button class="rail-btn" classList={{ on: page() === "settings" }} data-tip={labeled() ? undefined : "Settings (Ctrl+,)"} data-tip-pos="right" onClick={() => setPage("settings")}>
        <Icon name="settings" size={18} stroke={1.7} />
        <Show when={labeled()}><span>Settings</span></Show>
      </button>
      <button class="rail-version" data-tip="Zerus 0.48.0-dev.912 · Open About" data-tip-pos="right" onClick={() => { setSection("about"); setPage("settings"); }}>0.48 Dev</button>
    </nav>
  );
}
