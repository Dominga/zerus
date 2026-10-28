import { For, Match, Show, Switch, createEffect, createMemo, onCleanup, onMount } from "solid-js";
import { Rail } from "./ui/components/Rail";
import { MenuLayer, Toasts } from "./ui/components/Overlays";
import { ActionDialogs, ActionNotices } from "./ui/sessions/ActionDialogs";
import { NewSessionDialog } from "./ui/sessions/NewSessionDialog";
import { windowKey } from "./ui/sessions/newsession-view";
import { OrganizationDialogs } from "./ui/sessions/OrganizationDialogs";
import { SessionList } from "./ui/sessions/SessionList";
import { SessionDetail } from "./ui/sessions/SessionDetail";
import { PerfHost } from "./ui/PerfHost";
import { OverviewScreen } from "./ui/screens/Overview";
import { MachinesScreen } from "./ui/screens/Machines";
import { AccountsScreen } from "./ui/screens/Accounts";
import { ProjectsScreen } from "./ui/screens/Projects";
import { SettingsScreen } from "./ui/components/SettingsScreen";
import { applySkin, skin } from "./skin/runtime";
import { catalog } from "./store/catalog";
import { contentScale, page, setContentScale, setPage, toast } from "./store/ui";
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

export function App(props: { mode: () => BridgeMode | "starting" }) {
  createEffect(() => applySkin(skin()));
  const keys = (e: KeyboardEvent) => {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) return;
    // session.new (Shortcuts.cpp:29, 58): a Window shortcut, on every page.
    if (windowKey(e)) return;
    if (/^[1-5]$/.test(e.key)) { e.preventDefault(); setPage((["dashboard", "sessions", "projects", "machines", "accounts"] as const)[Number(e.key) - 1]); }
    else if (e.key === ",") { e.preventDefault(); setPage("settings"); }
    else if (e.key === "=" || e.key === "+") { e.preventDefault(); setContentScale(Math.min(2, +(contentScale() + 0.1).toFixed(2))); toast("Content scale", `${Math.round(contentScale() * 100)}%`); }
    else if (e.key === "-") { e.preventDefault(); setContentScale(Math.max(0.75, +(contentScale() - 0.1).toFixed(2))); toast("Content scale", `${Math.round(contentScale() * 100)}%`); }
    else if (e.key === "0") { e.preventDefault(); setContentScale(1); }
  };
  onMount(() => addEventListener("keydown", keys));
  onCleanup(() => removeEventListener("keydown", keys));
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
      <NewSessionDialog />
      <OrganizationDialogs />
      <ActionNotices />
      <PerfHost />
    </div>
  );
}
