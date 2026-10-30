import { createSignal } from "solid-js";
import * as bridge from "../bridge";
import { emptyUpdatesView, type UpdateChannel, type UpdatesView } from "../types";

export interface UpdatesPort {
  view(): Promise<UpdatesView>;
  listen(listener: (view: UpdatesView) => void): Promise<() => void>;
  check(): Promise<void>;
  setChannel(channel: UpdateChannel): Promise<void>;
  setAutomatic(on: boolean): Promise<void>;
  open(target: "release" | "download" | "instructions"): Promise<void>;
  copyCommand(): Promise<void>;
  restart(): Promise<void>;
  fixLoginItem(restart: boolean): Promise<void>;
}

const native: UpdatesPort = {
  view: bridge.updatesView,
  listen: bridge.onUpdates,
  check: bridge.updatesCheck,
  setChannel: bridge.updatesSetChannel,
  setAutomatic: bridge.updatesSetAutomatic,
  open: bridge.updatesOpen,
  copyCommand: bridge.updatesCopyCommand,
  restart: bridge.updatesRestart,
  fixLoginItem: bridge.updatesFixLoginItem,
};

export function createUpdates(port: UpdatesPort) {
  const [view, setView] = createSignal<UpdatesView>(emptyUpdatesView());
  const [loaded, setLoaded] = createSignal(false);
  let reads = 0;
  let events = 0;
  let starting: Promise<() => void> | undefined;

  async function refresh(): Promise<void> {
    const read = ++reads;
    const event = events;
    try {
      const next = await port.view();
      if (read === reads && event === events) setView(next);
    } catch (error) {
      console.warn(`Zerus Next: updates_view failed: ${String(error)}`);
    } finally {
      if (read === reads) setLoaded(true);
    }
  }

  function start(): Promise<() => void> {
    if (!starting) {
      starting = (async () => {
        const stop = await port.listen((next) => {
          events++;
          setView(next);
          setLoaded(true);
        });
        await refresh();
        return stop;
      })().catch((error) => {
        starting = undefined;
        throw error;
      });
    }
    return starting;
  }

  return {
    view,
    loaded,
    start,
    refresh,
    check: () => port.check(),
    setChannel: (channel: UpdateChannel) => port.setChannel(channel),
    setAutomatic: (on: boolean) => port.setAutomatic(on),
    open: (target: "release" | "download" | "instructions") => port.open(target),
    copyCommand: () => port.copyCommand(),
    restart: () => port.restart(),
    fixLoginItem: (restart: boolean) => port.fixLoginItem(restart),
  };
}

const store = createUpdates(native);
export const updates = store.view;
export const updatesLoaded = store.loaded;
export const startUpdates = store.start;
export const refreshUpdates = store.refresh;
export const updateCommands = {
  check: store.check,
  setChannel: store.setChannel,
  setAutomatic: store.setAutomatic,
  open: store.open,
  copyCommand: store.copyCommand,
  restart: store.restart,
  fixLoginItem: store.fixLoginItem,
};
