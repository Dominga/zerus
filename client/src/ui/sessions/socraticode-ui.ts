import { tr } from "../../i18n";
import type { SocratiCodeDetail, SocratiCodeNext, SocratiCodePanel, SocratiCodeState } from "../../store/socraticode";

// WhiteWh fork: the SocratiCode card's texts (tray/src/SocratiCodePanel.h
// render 171-246, act 134-151). Qt's wording, split into the separate parts the
// client lays out without separator dots: the state parts, the detail lines
// and the one next step. Ages come from the clock here, never from the core.

/** The state line as separate parts, in Qt's order. */
export function stateParts(state: SocratiCodeState, now: number): string[] {
  switch (state.kind) {
    case "checking": return [tr("Checking…")];
    case "not_checked": return [tr("Not checked")];
    case "unavailable": return [tr("Unavailable")];
  }
  const parts: string[] = [];
  if (state.kind === "indexed") {
    parts.push(tr("Indexed"));
    parts.push(tr("%n chunks", state.chunks));
    if (state.files > 0) parts.push(tr("%n files", state.files));
    const age = indexedAgo(state.last_indexed_at, now);
    if (age) parts.push(age);
    parts.push(state.watching ? tr("watching") : tr("not watched"));
  } else if (state.kind === "indexing") {
    parts.push(tr("Indexing…"));
    if (state.total > 0) parts.push(tr("%1 of %2 files", state.indexed, state.total));
  } else {
    parts.push(tr("Not indexed"));
  }
  if (!state.qdrant) parts.push(state.startable ? tr("Qdrant is not running") : tr("no Qdrant"));
  return parts;
}

/** "5 min ago" for the index's clock value; nothing when it does not parse. */
export function indexedAgo(at: string, now: number): string {
  const then = Date.parse(at);
  if (!Number.isFinite(then)) return "";
  const seconds = now - Math.floor(then / 1000);
  if (seconds < 0) return "";
  if (seconds < 90) return tr("just now");
  if (seconds < 5400) return tr("%1 min ago", Math.floor(seconds / 60));
  if (seconds < 129_600) return tr("%1 h ago", Math.floor(seconds / 3600));
  return tr("%1 days ago", Math.floor(seconds / 86_400));
}

/** One detail line. */
export function detailText(detail: SocratiCodeDetail): string {
  switch (detail.code) {
    case "plugin_missing": return tr("The Claude Code plugin is not installed, so agents do not use this index.");
    case "docker_missing": return tr("SocratiCode keeps its index in a Qdrant that runs in Docker. Install Docker, or turn on a native Qdrant in Settings > Extensions.");
    case "native_missing":
      return detail.installable ? tr("The native Qdrant is not installed yet.") : tr("Install qdrant on PATH for a native Qdrant.");
    case "agents_docker": return tr("Zerus uses the native Qdrant at %1; this folder's agents still look for SocratiCode's Docker one.", detail.url);
    case "node_missing": return tr("SocratiCode needs Node.js with npx on this computer.");
    case "ollama_model_missing": return tr("Ollama has no %1 yet; SocratiCode downloads it with the first index.", detail.model);
    case "runner_failed": return detail.error;
  }
}

/** The one next step's label and tooltip (Qt's setAction calls). */
export function nextAction(next: SocratiCodeNext): { text: string; tip: string } {
  switch (next.kind) {
    case "install":
      return { text: tr("Install plugin"), tip: "claude plugin install socraticode@socraticode" };
    case "install_qdrant":
      return { text: tr("Install Qdrant…"), tip: tr("Download the pinned native Qdrant for SocratiCode") };
    case "start":
      return {
        text: next.fresh ? tr("Index and watch") : next.qdrant ? tr("Watch") : tr("Start"),
        tip: tr("Bring up Qdrant and Ollama when they are down, then run a SocratiCode server that keeps this folder's index current"),
      };
  }
}

/** The step's verb for `hgs socraticode`. */
export function nextVerb(next: SocratiCodeNext): "install" | "install-qdrant" | "start" {
  return next.kind === "install" ? "install" : next.kind === "install_qdrant" ? "install-qdrant" : "start";
}

/** The confirmation a step asks first, as Qt's QMessageBox texts. */
export function stepConfirm(next: SocratiCodeNext, path: string): { title: string; text: string } | undefined {
  if (next.kind === "install_qdrant") {
    return {
      title: tr("Install Qdrant?"),
      text: tr("Zerus downloads Qdrant %1 for Windows (about 30 MB) from its GitHub release, checks the archive and qdrant.exe against pinned SHA-256 hashes, and puts it in %LOCALAPPDATA%\\Programs\\qdrant. Its data stays in %LOCALAPPDATA%\\qdrant, and it listens only on 127.0.0.1:6333.", next.version),
    };
  }
  if (next.kind === "start" && next.fresh) {
    return {
      title: tr("Index this folder?"),
      text: tr("SocratiCode reads every source file of %1 and keeps its index in a local Qdrant container. The first index can take several minutes; Zerus then keeps watching the folder until you stop it.", path),
    };
  }
  return undefined;
}

/** The Connect agents confirmation and its settings file. */
export function connectConfirm(path: string): { title: string; text: string } {
  return {
    title: tr("Connect this folder's agents?"),
    text: tr("Agents use SocratiCode's Docker Qdrant unless the folder says otherwise. Zerus adds QDRANT_MODE, QDRANT_URL and the Ollama settings to %1, which stays out of Git; variables the folder already sets are kept. Sessions started after this use the index.", settingsFile(path)),
  };
}

/** Qt's `m_path + "/.claude/settings.local.json"`. */
export function settingsFile(path: string): string {
  return `${path}/.claude/settings.local.json`;
}

/** The panel before a status answer: Qt's Checking…, Not checked or Unavailable. */
export function blankState(busy: boolean, error: string): SocratiCodeState {
  return busy ? { kind: "checking" } : error ? { kind: "unavailable" } : { kind: "not_checked" };
}

/** Every control waits while a read or an action runs. */
export function controlsEnabled(panel: SocratiCodePanel | undefined, busy: boolean, acting: boolean): boolean {
  return !!panel && !busy && !acting;
}
