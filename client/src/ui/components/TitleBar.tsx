import { For, Show, createEffect, createSignal, onCleanup, onMount } from "solid-js";
import { appBuildInfo, refreshAppBuildInfo } from "../../store/buildInfo";
import { Icon } from "../lib/icons";
import { ZerusLogo } from "./ZerusLogo";
import { ITEMS } from "./Rail";
import { page, setPage, counts, logoMotion, setMenu, stripMode, toggleSessionsList, toast } from "../../store/ui";
import { markAllEntry } from "../sessions/mark-entries";
import { expandAllCollapsedEntry } from "../sessions/background-menu";
import { windowLayer, setWindowLayer } from "../../store/windowLayer";
import { newSessionFromRail } from "../sessions/newsession-view";
import { shortcutTooltip } from "../../store/shortcuts";
import { tr } from "../../i18n";
import { waiting } from "../../store/teams";
import { modelServers } from "../../store/modelServers";
import { windowControl, type BridgeMode } from "../../bridge";
import { appLog } from "../../store/appLog";
import { updates, updateCommands } from "../../store/updates";
import { openAbout, openUpdates } from "../../store/desk";
import { installedAppName, railVersion } from "../screens/settings/about-version";
import { DesktopServiceStatus } from "./DesktopServiceStatus";
import { BackgroundTasksChip } from "../screens/BackgroundTasks";
import { CleanupChip } from "../screens/CleanupChip";
import { TeamHelpButton } from "../teams/TeamHelpButton";

// Task 243 (owner, 2026-11-07): the Compact skins' one title bar, as Qt's
// WindowTitleBar with the rail's controls moved into it (placeRailControls):
// the mark and name, the sessions toggle, New session and the screens on the
// left, the Go to search in the middle, the pin and Settings before the
// window buttons. The bar is the window's drag region; a double click
// maximizes (Tauri's data-tauri-drag-region). The native decorations are off
// while it shows (App, `chrome: unified`).

export function TitleBar(props: { mode?: () => BridgeMode | "starting"; logTitle?: string }) {
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
      {/* Owner, 2026-11-09: no search in the title bar; the session list has
          its own search, and Ctrl+K still opens Go to. */}
      {/* Task 409: the client log's last line, beside the search; clicking it
          opens the whole log (the title bar takes the place of the status bar).
          Task 424: the line and its icon are pale grey (muted), brighter only on
          hover/focus; an error line is a muted red instead. */}
      <button type="button" class="tb-log muted" classList={{ "tb-log-error": appLog.level() === "error" }} data-log-line title={props.logTitle ?? tr("Open log")} aria-label={tr("Open log")} onClick={appLog.open}>
        <Icon name="file-text" size={14} />
        <span class="tb-log-text ell">{appLog.latest()}</span>
      </button>
      <span class="grow" data-tauri-drag-region />
      <div class="tb-group tb-status">
        <Show when={props.mode?.() === "replay"}>
          <span class="chip-accent" title={tr("Synthetic data from the replay fixture; no hgs is running")}>{tr("Replay")}</span>
        </Show>
        <DesktopServiceStatus />
        <Show when={!updates().quiet && updates().available}>
          <button type="button" class="chip-accent status-update" title={tr("Zerus %1 is available. Open Updates.", updates().version)}
            aria-label={tr("Zerus %1 is available. Open Updates.", updates().version)} onClick={openUpdates}>{tr("Update available")}</button>
        </Show>
        <Show when={!updates().quiet && updates().restart_required}>
          <button type="button" class="chip-accent status-restart" title={tr("Zerus was updated on disk. Drafts are kept; sessions keep running.")}
            aria-label={tr("Zerus was updated on disk. Drafts are kept; sessions keep running.")}
            onClick={() => { void updateCommands.restart().catch((error) => toast(tr("Updates"), String(error), "error", true)); }}>{tr("Restart to finish update")}</button>
        </Show>
        <Show when={updates().login_item_deprioritized}>
          <button type="button" class="chip-accent warn-chip" title={tr("macOS limits background priority for this login item")} onClick={openUpdates}>{tr("Fix login item")}</button>
        </Show>
        <BackgroundTasksChip />
        <CleanupChip />
        <button type="button" class="status-version tb-version" data-version data-tip={tr("%1\nOpen About", installedAppName(appBuildInfo()))} onClick={openAbout}>{railVersion(appBuildInfo()) || tr("About")}</button>
      </div>
      <div class="tb-group">
        <TeamHelpButton titlebar />
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
