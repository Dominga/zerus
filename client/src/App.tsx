import { For, Match, Show, Switch, createEffect, createMemo, onCleanup, onMount } from "solid-js";
import { Rail } from "./ui/components/Rail";
import { MenuLayer, Toasts } from "./ui/components/Overlays";
import { ActionDialogs, ActionNotices } from "./ui/sessions/ActionDialogs";
import { SessionList } from "./ui/sessions/SessionList";
import { SessionDetail } from "./ui/sessions/SessionDetail";
import { PerfHost } from "./ui/PerfHost";
import { OverviewScreen } from "./ui/screens/Overview";
import { MachinesScreen } from "./ui/screens/Machines";
import { AccountsScreen } from "./ui/screens/Accounts";
import { ProjectsScreen } from "./ui/screens/Projects";
import { SettingsScreen } from "./ui/screens/Settings";
import { applySkin, skin } from "./skin/runtime";
import { catalog } from "./store/catalog";
import { contentScale, page, setContentScale, setPage, toast } from "./store/ui";
import { num, setSetting } from "./store/settings";
import { installShortcutRouter, onCommand } from "./store/shortcuts";
import type { BridgeMode } from "./bridge";

// The client shell: the rail, the pages and the status bar.

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
              <SessionDetail />
            </div>
          </Match>
          <Match when={page() === "dashboard"}><OverviewScreen /></Match>
          <Match when={page() === "machines"}><MachinesScreen /></Match>
          <Match when={page() === "accounts"}><AccountsScreen /></Match>
          <Match when={page() === "projects"}><ProjectsScreen /></Match>
          <Match when={page() === "settings"}><SettingsScreen /></Match>
        </Switch>
      </main>
      <StatusBar mode={props.mode} />
      <MenuLayer />
      <Toasts />
      <ActionDialogs />
      <ActionNotices />
      <PerfHost />
    </div>
  );
}
