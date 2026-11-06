import { createRoot, createSignal } from "solid-js";
import * as bridge from "../bridge";
import { tr } from "../i18n";

// WhiteWh fork: the SocratiCode card of the session inspector
// (tray/src/SocratiCodePanel.h). The card reads `hgs socraticode status` for the
// selected session's folder and runs the one next step the CLI offers; the
// webview only shows the core's projection. Reads are on demand and repeat
// every 10 seconds only while the runner works or the index is being built,
// as Qt polls.

/** The CLI verbs the card may run. */
export type SocratiCodeVerb = "status" | "start" | "stop" | "connect" | "install" | "install-qdrant";

/** The panel's state line, as parts the webview words (zerus_client_core::socraticode::State). */
export type SocratiCodeState =
  | { kind: "checking" }
  | { kind: "not_checked" }
  | { kind: "unavailable" }
  | { kind: "indexed"; chunks: number; files: number; last_indexed_at: string; watching: boolean; qdrant: boolean; startable: boolean }
  | { kind: "indexing"; indexed: number; total: number; qdrant: boolean; startable: boolean }
  | { kind: "not_indexed"; qdrant: boolean; startable: boolean };

export type SocratiCodeDetail =
  | { code: "plugin_missing" }
  | { code: "docker_missing" }
  | { code: "native_missing"; installable: boolean }
  | { code: "agents_docker"; url: string }
  | { code: "node_missing" }
  | { code: "ollama_model_missing"; model: string }
  | { code: "runner_failed"; error: string };

/** The one next step (`install`, `install-qdrant`, or a start). */
export type SocratiCodeNext =
  | { kind: "install" }
  | { kind: "install_qdrant"; version: string }
  | { kind: "start"; fresh: boolean; qdrant: boolean };

export interface SocratiCodePanel {
  state: SocratiCodeState;
  details: SocratiCodeDetail[];
  action: SocratiCodeNext | null;
  connect: boolean;
  stop: boolean;
  poll: boolean;
}

export interface Acted { installed: boolean }

export interface SocratiCodeDeps {
  bridge: Pick<typeof bridge, "socraticodeStatus" | "socraticodeAct">;
  /** Qt's delayed status read after an action: 1.5 s for a start, 0.5 s otherwise. */
  later(run: () => void, ms: number): void;
}

export interface SocratiCodeStore {
  context(): { host: string; path: string; native: boolean } | undefined;
  data(): SocratiCodePanel | undefined;
  error(): string;
  /** A status read is in flight. */
  busy(): boolean;
  /** An action is in flight. */
  acting(): boolean;
  /** The panel before any answer: Qt's Checking…, Not checked or Unavailable. */
  panel(): SocratiCodePanel;
  open(host: string, path: string, native: boolean): void;
  close(): void;
  read(): void;
  /** Runs one verb; the panel reads its new status afterwards. */
  act(verb: SocratiCodeVerb): Promise<boolean>;
}

const blank = (state: SocratiCodeState): SocratiCodePanel => ({
  state, details: [], action: null, connect: false, stop: false, poll: false,
});

export function createSocratiCode(deps: SocratiCodeDeps): SocratiCodeStore {
  const [context, setContext] = createSignal<{ host: string; path: string; native: boolean }>();
  const [data, setData] = createSignal<SocratiCodePanel>();
  const [error, setError] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [acting, setActing] = createSignal(false);
  let request = 0;

  function panel(): SocratiCodePanel {
    const held = data();
    if (held) return held;
    return blank(busy() ? { kind: "checking" } : error() ? { kind: "unavailable" } : { kind: "not_checked" });
  }

  async function read() {
    const target = context();
    if (!target || !target.path || busy()) return;
    const mine = ++request;
    setBusy(true);
    try {
      const next = await deps.bridge.socraticodeStatus(target.host, target.path, target.native);
      if (mine !== request) return;
      setData(next);
      setError("");
    } catch (failure) {
      // Qt keeps the last status and shows the failure as a detail line.
      if (mine === request) setError(String(failure));
    } finally {
      if (mine === request) setBusy(false);
    }
  }

  async function act(verb: SocratiCodeVerb): Promise<boolean> {
    const target = context();
    if (!target || !target.path || busy() || acting()) return false;
    setActing(true);
    try {
      const reply = await deps.bridge.socraticodeAct(verb, target.host, target.path, target.native);
      if (verb === "install" && !reply.installed) {
        setError(tr("The plugin was not installed. Run: claude plugin install socraticode@socraticode"));
      } else if (verb === "install-qdrant" && !reply.installed) {
        setError(tr("Qdrant was not installed."));
      }
      deps.later(() => void read(), verb === "start" ? 1500 : 500);
      return true;
    } catch (failure) {
      setError(String(failure));
      return false;
    } finally {
      setActing(false);
    }
  }

  return {
    context,
    data,
    error,
    busy,
    acting,
    panel,
    open(host, path, native) {
      const current = context();
      if (current && current.host === host && current.path === path && current.native === native) return;
      request++;
      setContext({ host, path, native });
      setData(undefined);
      setError("");
      setBusy(false);
      void read();
    },
    close() {
      request++;
      setContext(undefined);
      setData(undefined);
      setError("");
      setBusy(false);
      setActing(false);
    },
    read: () => void read(),
    act,
  };
}

/** The window's one card, wired to the app's bridge. */
export const socraticode = createRoot(() => createSocratiCode({
  bridge: { socraticodeStatus: bridge.socraticodeStatus, socraticodeAct: bridge.socraticodeAct },
  later: (run, ms) => { setTimeout(run, ms); },
}));
