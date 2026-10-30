import { For, Match, Show, Suspense, Switch, createEffect, createMemo, lazy, onCleanup, onMount } from "solid-js";
import { Rail } from "./ui/components/Rail";
import { MenuLayer, Toasts } from "./ui/components/Overlays";
import { ActionDialogs, ActionNotices } from "./ui/sessions/ActionDialogs";
import { newSessionFromShortcut, newSessionOpened } from "./ui/sessions/newsession-view";
import { OrganizationDialogs } from "./ui/sessions/OrganizationDialogs";
import { SessionList } from "./ui/sessions/SessionList";
import { SessionDetail } from "./ui/sessions/SessionDetail";
import { CreationPanel } from "./ui/sessions/CreationPanel";
import { selectedCreation } from "./store/creations";
import { OverviewScreen } from "./ui/screens/Overview";
import { MachinesScreen } from "./ui/screens/Machines";
import { AccountsScreen } from "./ui/screens/Accounts";
import { ProjectsScreen } from "./ui/screens/Projects";
import { applySkin, skin } from "./skin/runtime";
import { catalog } from "./store/catalog";
import { contentScale, page, selectedKey, setContentScale, setPage, toast } from "./store/ui";
import { num, setSetting } from "./store/settings";
import { installShortcutRouter, onCommand } from "./store/shortcuts";
import { openGoTo, openShortcutHelp } from "./store/goTo";
import { rememberRecentSession } from "./ui/sessions/goTo";
import { GoToDialog } from "./ui/sessions/GoToDialog";
import { ShortcutHelpDialog } from "./ui/sessions/ShortcutHelpDialog";
import { updateCommands, updates } from "./store/updates";
import { openUpdates } from "./store/desk";
import { ZerusLogo } from "./ui/components/ZerusLogo";
import type { BridgeMode } from "./bridge";

// The client shell: the rail, the pages and the status bar.

// Settings and its sections load the first time Settings opens; their
// state lives in their modules, so it stays once loaded.
const SettingsScreen = lazy(() => import("./ui/screens/Settings").then((m) => ({ default: m.SettingsScreen })));
// New session and its child dialogs load the first time it opens
// (`newSessionOpened`), and stay mounted from then on.
const NewSessionDialog = lazy(() => import("./ui/sessions/NewSessionDialog").then((m) => ({ default: m.NewSessionDialog })));
const PerfHost = lazy(() => import("./ui/PerfHost").then((m) => ({ default: m.PerfHost })));

const STATE_TEXT: Record<string, string> = {
  probing: "checking",
  unsupported: "no event stream (update hgs)",
  polled: "polled every 60 s (update hgs for live updates)",
  reconnecting: "reconnecting",
  ended: "stream ended",
};

function StatusBar(props: { mode: () => BridgeMode | "starting" }) {
  const computers = createMemo(() => Object.values(catalog.computers));
  const connected = (status: string) => status === "live" || status === "polled";
  const live = () => computers().filter((c) => connected(c.status)).length;
  const problems = () => computers().filter((c) => !connected(c.status));
  const label = (id: string) => id || "this computer";
  return (
    <footer class="statusbar">
      <Show when={computers().length} fallback={<span>{props.mode() === "starting" ? "Starting…" : "Connecting…"}</span>}>
        <Show when={problems().length} fallback={<span class="conn-ok"><span class="state-dot ready" />{computers().length === 1 ? "Connected" : `${live()} of ${computers().length} machines connected`}</span>}>
          <span classList={{ danger: problems().some((c) => c.status !== "probing") }}>
            <For each={problems()}>{(c, i) => <>{i() ? "; " : ""}{label(c.id)}: {STATE_TEXT[c.status] ?? c.status}{c.status === "reconnecting" && c.attempt ? ` (attempt ${c.attempt})` : ""}</>}</For>
          </span>
        </Show>
      </Show>
      <Show when={props.mode() === "replay"}>
        <span class="chip-accent" title="Synthetic data from the replay fixture; no hgs is running">Replay</span>
      </Show>
      <span class="grow" />
      <Show when={!updates().quiet && updates().available}>
        <button type="button" class="btn ghost small status-update" title={`Zerus ${updates().version} is available. Open Updates.`}
          aria-label={`Zerus ${updates().version} is available. Open Updates.`} onClick={openUpdates}>
          <ZerusLogo attention={false} motion="spin" size={14} />New version
        </button>
      </Show>
      <Show when={!updates().quiet && updates().restart_required}>
        <button type="button" class="btn ghost small status-restart" title="Zerus was updated on disk. Drafts are kept; sessions keep running."
          aria-label="Zerus was updated on disk. Drafts are kept; sessions keep running."
          onClick={() => { void updateCommands.restart().catch((error) => toast("Updates", String(error))); }}>
          <ZerusLogo attention={false} motion="spin" size={14} />Restart to finish update
        </button>
      </Show>
    </footer>
  );
}

/** Keyboard zoom changes Settings > Appearance > Content scale (Qt's syncContentScale). */
function zoom(scale: number) {
  setContentScale(scale);
  void setSetting("workspace/contentScale", scale);
}

export function App(props: { mode: () => BridgeMode | "starting" }) {
  createEffect(() => applySkin(skin()));
  createEffect(() => setContentScale(num("workspace/contentScale")));
  createEffect(() => rememberRecentSession(selectedKey()));
  // Window commands through the shortcut router (store/shortcuts.ts), with the
  // chords of Settings > Keyboard.
  const zoomBy = (step: number) => {
    zoom(Math.max(0.75, Math.min(2, +(contentScale() + step).toFixed(2))));
    toast("Content scale", `${Math.round(contentScale() * 100)}%`);
  };
  const commands: [string, () => void][] = [
    ["go.overview", () => setPage("dashboard")],
    ["go.sessions", () => setPage("sessions")],
    ["go.projects", () => setPage("projects")],
    ["go.machines", () => setPage("machines")],
    ["go.accounts", () => setPage("accounts")],
    ["go.settings", () => setPage("settings")],
    ["go.anywhere", openGoTo],
    ["help.shortcuts", openShortcutHelp],
    // session.new (Shortcuts.cpp:29, 58): New session from every page.
    ["session.new", newSessionFromShortcut],
    ["zoom.in", () => zoomBy(0.1)],
    ["zoom.out", () => zoomBy(-0.1)],
    ["zoom.reset", () => zoom(1)],
  ];
  onMount(() => {
    const off = [installShortcutRouter(), ...commands.map(([id, run]) => onCommand(id, run))];
    onCleanup(() => off.forEach((undo) => undo()));
  });
  return (
    <div class="app" classList={{ "rail-labeled": skin().layout.rail === "labeled" }}>
      <Rail />
      <main class="page">
        <Switch>
          <Match when={page() === "sessions"}>
            <div class="sessions-page" classList={{ "layout-right": skin().layout["sessions-pane"] === "right" }}>
              <SessionList />
              <Show when={selectedCreation() !== undefined} fallback={<SessionDetail />}><CreationPanel /></Show>
            </div>
          </Match>
          <Match when={page() === "dashboard"}><OverviewScreen /></Match>
          <Match when={page() === "machines"}><MachinesScreen /></Match>
          <Match when={page() === "accounts"}><AccountsScreen /></Match>
          <Match when={page() === "projects"}><ProjectsScreen /></Match>
          <Match when={page() === "settings"}><Suspense><SettingsScreen /></Suspense></Match>
        </Switch>
      </main>
      <StatusBar mode={props.mode} />
      <MenuLayer />
      <Toasts />
      <ActionDialogs />
      <GoToDialog />
      <ShortcutHelpDialog />
      <Show when={newSessionOpened()}><Suspense><NewSessionDialog /></Suspense></Show>
      <OrganizationDialogs />
      <ActionNotices />
      <Suspense><PerfHost /></Suspense>
    </div>
  );
}
