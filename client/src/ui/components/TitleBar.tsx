import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { refreshAppBuildInfo } from "../../store/buildInfo";
import { Icon } from "../lib/icons";
import { ZerusLogo } from "./ZerusLogo";
import { ITEMS } from "./Rail";
import { page, setPage, counts, logoMotion, setMenu, stripMode, toggleSessionsList } from "../../store/ui";
import { markAllEntry } from "../sessions/mark-entries";
import { expandAllCollapsedEntry } from "../sessions/background-menu";
import { windowLayer, setWindowLayer } from "../../store/windowLayer";
import { newSessionFromRail } from "../sessions/newsession-view";
import { shortcutTooltip, chord } from "../../store/shortcuts";
import { openGoTo } from "../../store/goTo";
import { tr } from "../../i18n";
import { waiting } from "../../store/teams";
import { modelServers } from "../../store/modelServers";
import { windowControl } from "../../bridge";

// Task 243 (owner, 2026-11-07): the Compact skins' one title bar, as Qt's
// WindowTitleBar with the rail's controls moved into it (placeRailControls):
// the mark and name, the sessions toggle, New session and the screens on the
// left, the Go to search in the middle, the pin and Settings before the
// window buttons. The bar is the window's drag region; a double click
// maximizes (Tauri's data-tauri-drag-region). The native decorations are off
// while it shows (App, `chrome: unified`).

export function TitleBar() {
  const [maximized, setMaximized] = createSignal(false);
  const control = (action: "minimize" | "maximize" | "close" | "state") => {
    void windowControl(action).then((on) => setMaximized(on)).catch(() => {});
  };
  onMount(() => {
    control("state");
    const resized = () => control("state");
    addEventListener("resize", resized);
    onCleanup(() => removeEventListener("resize", resized));
  });
  // What the rail reads when it shows: the build (the status bar's version) and the model servers.
  onMount(() => { void refreshAppBuildInfo(); void modelServers.refreshList(); });
  createEffect(() => { if (!modelServers.available() && modelServers.listed() && page() === "modelservers") setPage("sessions"); });
  const shown = () => ITEMS.filter((item) => item.id !== "modelservers" || modelServers.available());
  const goTo = () => chord("go.anywhere");
  return (
    <header class="titlebar" data-tauri-drag-region aria-label={tr("Window title bar")}>
      <button class="tb-btn tb-brand" classList={{ on: page() === "dashboard" }} data-tip={shortcutTooltip(tr("Home"), "go.overview")} onClick={() => setPage("dashboard")}>
        <ZerusLogo attention={counts().attention > 0} motion={logoMotion()} size={20} />
      </button>
      <span class="tb-name" data-tauri-drag-region>Zerus</span>
      <div class="tb-group">
        <Show when={page() === "sessions"}>
          <button class="tb-btn" data-sessions-toggle aria-pressed={stripMode()} aria-label={stripMode() ? tr("Show the session list") : tr("Hide the session list")}
            data-tip={stripMode() ? tr("Show the session list") : tr("Hide the session list")} onClick={toggleSessionsList}>
            <Icon name={stripMode() ? "expand-sessions" : "collapse-sessions"} size={17} />
          </button>
        </Show>
        <button class="tb-btn" data-tip={shortcutTooltip(tr("New session"), "session.new")} aria-label={tr("New session")} onClick={newSessionFromRail}><Icon name="add" size={17} /></button>
        <For each={shown()}>{(item) => (
          <button class="tb-btn" classList={{ on: page() === item.id }} aria-current={page() === item.id ? "page" : undefined} aria-label={tr(item.label)}
            data-page={item.id} data-tip={shortcutTooltip(tr(item.tip), item.command)} onClick={() => setPage(item.id)}
            onContextMenu={(e) => { if (item.id !== "sessions") return; e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY, items: [markAllEntry(), expandAllCollapsedEntry()] }); }}>
            <Icon name={item.icon} size={17} />
            <Show when={item.id === "sessions" && counts().attention > 0}><span class="tb-badge">{counts().attention}</span></Show>
            <Show when={item.id === "waiting" && (waiting()?.total ?? 0) > 0}><span class="tb-badge">{waiting()!.total}</span></Show>
          </button>
        )}</For>
      </div>
      <span class="grow" data-tauri-drag-region />
      <button class="tb-search" aria-label={tr("Search sessions, IDs and messages")} onClick={openGoTo}>
        <Icon name="search" size={14} />
        <span class="tb-search-text">{tr("Search sessions, IDs and messages…")}</span>
        <Show when={goTo()}><span class="tb-shortcut">{goTo()}</span></Show>
      </button>
      <span class="grow" data-tauri-drag-region />
      <div class="tb-group">
        <button class="tb-btn" classList={{ pinned: windowLayer().on }} aria-pressed={windowLayer().on} aria-label={tr("Keep Zerus above other windows")} disabled={!windowLayer().supported}
          data-tip={!windowLayer().supported ? windowLayer().hint : windowLayer().on ? tr("Unpin Zerus") : tr("Keep Zerus above other windows")}
          onClick={() => void setWindowLayer(!windowLayer().on)}>
          <Icon name={windowLayer().on ? "pinned" : "unpinned"} size={17} />
        </button>
        <button class="tb-btn" classList={{ on: page() === "settings" }} aria-label={tr("Settings")} data-tip={shortcutTooltip(tr("Settings"), "go.settings")} onClick={() => setPage("settings")}>
          <Icon name="settings" size={19} />
        </button>
      </div>
      <div class="tb-window">
        <button class="tb-win" data-window="minimize" aria-label={tr("Minimize")} title={tr("Minimize")} onClick={() => control("minimize")}><Icon name="minus" size={14} /></button>
        <button class="tb-win" data-window="maximize" aria-label={maximized() ? tr("Restore") : tr("Maximize")} title={maximized() ? tr("Restore") : tr("Maximize")} onClick={() => control("maximize")}>
          <Icon name={maximized() ? "copy" : "square"} size={12} />
        </button>
        <button class="tb-win close" data-window="close" aria-label={tr("Close window")} title={tr("Close window")} onClick={() => control("close")}><Icon name="x" size={15} /></button>
      </div>
    </header>
  );
}
