// Synthetic data for Settings, Machines and Accounts. Every name, path,
// domain and number is invented. Field names follow the hgs JSON replies the
// Qt pages read (machines ls/resolve, accounts catalog, speech config_list,
// analytics summary, approval journal pages, phone lists).
import type { AgentKind } from "./types";

// ---------------------------------------------------------------- Updates (7.1)

export type Channel = "stable" | "nightly" | "dev";
export interface Feed { version: string; buildNumber?: number; release: string; }

export const INSTALLED_VERSION = "0.48.0-dev.912";
export const INSTALLED_DISPLAY = "0.48.0-dev.912 (Dev build 912, source 4f1c2a9)";
export const HGS_VERSION = "0.48.0-dev.912";
export const LAST_CHECK = "Oct 27, 2026, 15:42";
export const FEEDS: Record<Channel, Feed> = {
  stable: { version: "0.47.3", release: "https://github.com/example/zerus/releases/tag/v0.47.3" },
  nightly: { version: "0.48.0-nightly.20261026", release: "https://github.com/example/zerus/releases/tag/nightly-20261026" },
  dev: { version: "0.48.0-dev.931", buildNumber: 931, release: "https://github.com/example/zerus/actions/runs/931" },
};

// ---------------------------------------------------------------- Recovery (5.3)

export interface RecoveryRule { id: "service" | "rate_limit" | "session_limit" | "network"; label: string; on: boolean; delays: string; }
export const RECOVERY_RULES: RecoveryRule[] = [
  { id: "service", label: "Overload / server unavailable", on: true, delays: "15, 30, 60, 300, -1" },
  { id: "rate_limit", label: "Temporary rate limit", on: true, delays: "60, 300, 900, -1" },
  { id: "session_limit", label: "Session usage limit", on: true, delays: "300, 0" },
  { id: "network", label: "Connection / timeout", on: true, delays: "15, 30, 60, 300, -1" },
];

// ---------------------------------------------------------------- Mobile (6.2-6.6)

export const OFFICIAL_RELAY = "https://relay.example.org";
export const WORKSPACE = { name: "Example team", computer: "workstation", generation: 7 };
export const CONNECTOR_CONFIG = "~/.config/zerus-mobile/connector.toml";

export interface Phone { id: string; name: string; platform: string; paired: string; lastActive: string; addedFrom: string; group: string; }
export const PHONES: Phone[] = [
  { id: "ph1", name: "Pixel 9", platform: "Android 16", paired: "Oct 12", lastActive: "4 min ago", addedFrom: "workstation", group: "Example team · this computer" },
  { id: "ph2", name: "iPhone 17", platform: "iOS 26.1", paired: "Sep 30", lastActive: "Yesterday", addedFrom: "macbook", group: "Example team · this computer" },
  { id: "ph3", name: "Phone", platform: "Android 15", paired: "Aug 2", lastActive: "Oct 1", addedFrom: "", group: "Example team · this computer" },
  { id: "ph4", name: "Lab tablet", platform: "iPadOS 26.0", paired: "Oct 20", lastActive: "2 h ago", addedFrom: "build-01", group: "Lab · via build-01" },
];
export const PAIR_WORKSPACES = ["Example team", "Lab (via build-01)"];

export const DELIVERY_OPTIONS = [
  { id: "immediate", label: "Immediately, even if I'm at a computer" },
  { id: "away", label: "As soon as I'm away" },
  { id: "away_5", label: "After 5 minutes away" },
  { id: "away_10", label: "After 10 minutes away" },
  { id: "away_15", label: "After 15 minutes away" },
  { id: "away_30", label: "After 30 minutes away" },
];

// ---------------------------------------------------------------- Files (5.7)

export const DOWNLOAD_FOLDER = { linux: "/home/example/Downloads/Zerus Files", macos: "/Users/example/Downloads/Zerus Files", windows: "C:\\Users\\example\\Downloads\\Zerus Files" };
export const DOWNLOAD_USAGE = "Downloaded copies: 1.8 GiB · 214 files";

// ---------------------------------------------------------------- Worktrees (2.7)

export interface CleanupPolicy { enabled: boolean; after_session: boolean; unused_integrated: boolean; missing: boolean; grace_hours: number; }
export const CLEANUP_POLICIES: Record<string, CleanupPolicy | "unavailable" | "invalid"> = {
  "": { enabled: true, after_session: true, unused_integrated: true, missing: true, grace_hours: 24 },
  macbook: { enabled: true, after_session: true, unused_integrated: false, missing: true, grace_hours: 72 },
  "build-01": { enabled: false, after_session: true, unused_integrated: true, missing: true, grace_hours: 24 },
  "build-02": "unavailable",
  "win-lab": "invalid",
};

// ---------------------------------------------------------------- Approvals (5.9)

export type ApprovalResult = "Approved once" | "Failed" | "Cancelled" | "Unconfirmed";
export interface ApprovalEntry { id: string; time: string; computer: string; session: string; question: string; result: ApprovalResult; answer: string; detail: string; }
const Q = [
  ["Run command? cargo test --workspace", "Yes, once"],
  ["Edit file src/api/pagination.rs?", "Yes, once"],
  ["Run command? sqlx migrate run", "Yes, once"],
  ["Run command? podman build -t runner:26.10 .", "Yes, once"],
  ["Write file docs/migration.md?", "Yes, once"],
  ["Run command? ./scripts/smoke-test.sh", "Yes, once"],
] as const;
const R: ApprovalResult[] = ["Approved once", "Approved once", "Unconfirmed", "Approved once", "Failed", "Approved once", "Cancelled", "Approved once"];
const S = [["workstation", "api-refactor"], ["build-01", "migrate-db"], ["workstation", "runner-images"], ["macbook", "release-notes"]] as const;
export const APPROVALS: ApprovalEntry[] = Array.from({ length: 23 }, (_, i) => {
  const [question, answer] = Q[i % Q.length];
  const [computer, session] = S[i % S.length];
  const result = R[i % R.length];
  const minute = 59 - ((i * 7) % 60);
  return {
    id: `ap${i}`,
    time: `Oct ${27 - Math.floor(i / 6)} ${String(16 - (i % 6)).padStart(2, "0")}:${String(minute).padStart(2, "0")}`,
    computer, session, question, result, answer,
    detail: result === "Unconfirmed"
      ? "Delivery was not confirmed. Check Activity or Terminal; this attempt is not retried."
      : result === "Failed" ? "The agent closed the question before the approval was delivered."
      : result === "Cancelled" ? "The question was answered in the session before the approval was sent."
      : "Delivered once and confirmed by the agent.",
  };
});

// ---------------------------------------------------------------- Voice (5.10)

export type SpeechKind = "gigaam" | "openai_http";
export interface SpeechProvider { id: string; name: string; kind: SpeechKind; model: string; language: string; device?: string; baseUrl?: string; allowHttp?: boolean; apiKey?: boolean; ready: boolean; }
export const SPEECH: Record<string, SpeechProvider[]> = {
  "": [
    { id: "gigaam-ru", name: "Russian speech", kind: "gigaam", model: "v3_e2e_rnnt", language: "ru", device: "cuda", ready: true },
    { id: "team-http", name: "Team transcription server", kind: "openai_http", model: "whisper-large-v3", language: "", baseUrl: "https://speech.example.com/v1", apiKey: true, ready: true },
  ],
  macbook: [{ id: "gigaam-mps", name: "Russian on macbook", kind: "gigaam", model: "v3_e2e_rnnt", language: "ru", device: "mps", ready: false }],
  "build-01": [],
};
export const MICROPHONES = ["System default microphone", "USB Microphone (Example)", "Built-in Audio Analog Stereo"];
export const VOICE_STATS = "This computer (last 90 days)\n142 recordings, 136 completed, 2 failed, 3 cancelled, 1 uncertain\n63.4 minutes of audio, 412.8 seconds processing\nThese counters stay on the speech computer and contain no audio or recognized text.";

// ---------------------------------------------------------------- Keyboard (5.11)

export interface ShortcutCommand { id: string; group: string; title: string; scope: "window" | "composer"; linux: string[]; mac?: string[]; }
export const SHORTCUTS: ShortcutCommand[] = [
  { id: "go.overview", group: "Go to", title: "Go to Overview", scope: "window", linux: ["Ctrl+1"] },
  { id: "go.sessions", group: "Go to", title: "Go to Sessions", scope: "window", linux: ["Ctrl+2"] },
  { id: "go.projects", group: "Go to", title: "Go to Projects", scope: "window", linux: ["Ctrl+3"] },
  { id: "go.machines", group: "Go to", title: "Go to Machines", scope: "window", linux: ["Ctrl+4"] },
  { id: "go.accounts", group: "Go to", title: "Go to Accounts", scope: "window", linux: ["Ctrl+5"] },
  { id: "go.settings", group: "Go to", title: "Open Settings", scope: "window", linux: ["Ctrl+,"] },
  { id: "go.anywhere", group: "Go to", title: "Go to…", scope: "window", linux: ["Ctrl+K"] },
  { id: "help.shortcuts", group: "Go to", title: "Keyboard shortcuts", scope: "window", linux: ["F1", "Ctrl+/"] },
  { id: "session.next", group: "Sessions", title: "Next session", scope: "window", linux: ["Alt+Down", "Ctrl+PgDown"], mac: ["Alt+Down", "Ctrl+Alt+Down"] },
  { id: "session.previous", group: "Sessions", title: "Previous session", scope: "window", linux: ["Alt+Up", "Ctrl+PgUp"], mac: ["Alt+Up", "Ctrl+Alt+Up"] },
  { id: "session.nextAttention", group: "Sessions", title: "Next session that needs attention", scope: "window", linux: ["Alt+Shift+Down"] },
  { id: "session.previousAttention", group: "Sessions", title: "Previous session that needs attention", scope: "window", linux: ["Alt+Shift+Up"] },
  { id: "session.new", group: "Sessions", title: "New session", scope: "window", linux: ["Ctrl+N"] },
  { id: "session.rename", group: "Sessions", title: "Rename session", scope: "window", linux: ["F2"] },
  { id: "session.search", group: "Sessions", title: "Search sessions", scope: "window", linux: ["Ctrl+F"] },
  { id: "view.refresh", group: "Sessions", title: "Refresh", scope: "window", linux: ["Ctrl+R", "F5"] },
  { id: "tab.next", group: "Session view", title: "Next tab", scope: "window", linux: ["Ctrl+Tab"], mac: ["Meta+Tab"] },
  { id: "tab.previous", group: "Session view", title: "Previous tab", scope: "window", linux: ["Ctrl+Shift+Tab"], mac: ["Meta+Shift+Tab"] },
  { id: "tab.terminal", group: "Session view", title: "Switch between Activity and Terminal", scope: "window", linux: ["Ctrl+`"], mac: ["Meta+`"] },
  { id: "panel.toggle", group: "Session view", title: "Show or hide the side panel", scope: "window", linux: ["Ctrl+I"] },
  { id: "focus.next", group: "Session view", title: "Next area", scope: "window", linux: ["F6"] },
  { id: "focus.previous", group: "Session view", title: "Previous area", scope: "window", linux: ["Shift+F6"] },
  { id: "message.send", group: "Messages", title: "Send message", scope: "composer", linux: ["Ctrl+Return"] },
  { id: "queue.sendNow", group: "Messages", title: "Send queued messages now", scope: "window", linux: ["Ctrl+Shift+Return"] },
  { id: "message.push", group: "Messages", title: "Send and push now", scope: "composer", linux: ["Ctrl+X, Ctrl+S"], mac: ["Meta+X, Meta+S"] },
  { id: "zoom.in", group: "Content scale", title: "Increase content scale", scope: "window", linux: ["Ctrl+=", "Ctrl++"] },
  { id: "zoom.out", group: "Content scale", title: "Decrease content scale", scope: "window", linux: ["Ctrl+-"] },
  { id: "zoom.reset", group: "Content scale", title: "Reset content scale", scope: "window", linux: ["Ctrl+0"] },
];

// ---------------------------------------------------------------- Analytics (5.12)

export interface AnalyticsRow { day: string; computer: string; provider: string; producer: "main" | "child"; turns: number; work: string; approval: string; input: string; }
const PROVIDERS = ["claude", "codex", "kimi", "deepseek"] as const;
export const ANALYTICS_ROWS: AnalyticsRow[] = (() => {
  const rows: AnalyticsRow[] = [];
  for (let d = 0; d < 14; d++) {
    const day = `2026-10-${String(27 - d).padStart(2, "0")}`;
    for (const [ci, computer] of ["Computer 1", "Computer 2", "Computer 3"].entries()) {
      for (const [pi, provider] of PROVIDERS.entries()) {
        if ((d + ci + pi) % 3 === 0) continue;
        const turns = 6 + ((d * 7 + ci * 5 + pi * 3) % 31);
        rows.push({
          day, computer, provider, producer: (d + pi) % 4 === 0 ? "child" : "main", turns,
          work: provider === "codex" ? "Unavailable" : `${turns * 3 + ci} min`,
          approval: `${(turns % 7) + 1} min`, input: provider === "deepseek" ? "Unknown" : `${(turns % 11) + 2} min`,
        });
      }
    }
  }
  return rows;
})();
export const ANALYTICS_OVERVIEW =
  "2026-10-14 through 2026-10-27 UTC (3 selected route(s), 3 current unique computer(s)). Recorded current totals: 1 284 completed turns; 61 h 12 min work.\nRecorded fleet work histogram: 1 106 samples; weighted mean 3 318 ms.\nCoverage below applies to every total and recorded sample.";
export const ANALYTICS_COVERAGE = [
  "Computer 1 (Current); generated 15:40; revision 412\nCoverage 2026-10-14 to 2026-10-27; reported complete\nGaps: none\nUnsupported: codex work duration\nCollection: enabled",
  "Computer 2 (Current); generated 15:39; revision 207\nCoverage 2026-10-14 to 2026-10-27; partial\nGaps: 2026-10-19\nUnsupported: none\nCollection: enabled",
  "Computer 3 (Last known (excluded)); generated Oct 26 21:02; revision 88\nSummary is stale or its clock is unconfirmed.",
];
export const ANALYTICS_VOICE = "Computer 1 (latest read; speech collection enabled)\nAccepted audio: 63.4 min · Total queue time: 41.2 s · Total processing time: 412.8 s\nTyped errors: none reported";

// ---------------------------------------------------------------- Machines (5.14)

export interface MachineConnection {
  alias: string;
  hostname: string;
  user: string;
  port: number;
  key: string;
  enabled: boolean;
  source: "local" | "config";
  inherited: boolean;
  effective?: { user: string; hostname: string; port: number; keys: string[]; jump?: string; proxyCommand?: boolean };
  os?: string;
  tmux?: string;
}
export const MACHINE_CONNECTIONS: MachineConnection[] = [
  { alias: "macbook", hostname: "", user: "", port: 0, key: "", enabled: true, source: "config", inherited: false,
    effective: { user: "example", hostname: "macbook.example.net", port: 22, keys: ["~/.ssh/id_ed25519"] }, os: "macOS 26.1 arm64", tmux: "tmux 3.5a" },
  { alias: "build-01", hostname: "10.0.12.21", user: "builder", port: 0, key: "", enabled: true, source: "local", inherited: true,
    effective: { user: "builder", hostname: "10.0.12.21", port: 22, keys: ["~/.ssh/id_ed25519", "~/.ssh/lab_ed25519"], jump: "bastion.example.net" }, os: "Linux x86_64", tmux: "tmux 3.4" },
  { alias: "build-02", hostname: "10.0.12.22", user: "builder", port: 2222, key: "~/.ssh/lab_ed25519", enabled: true, source: "local", inherited: false,
    effective: { user: "builder", hostname: "10.0.12.22", port: 2222, keys: ["~/.ssh/lab_ed25519"] }, os: "Linux x86_64", tmux: "tmux 3.4" },
  { alias: "win-lab", hostname: "win-lab.example.net", user: "example", port: 0, key: "", enabled: true, source: "local", inherited: false,
    effective: { user: "example", hostname: "win-lab.example.net", port: 22, keys: ["~/.ssh/id_ed25519"], proxyCommand: true }, os: "Windows 11 x86_64", tmux: "" },
];
export const LABEL_SWATCHES = ["#ffcf33", "#ff9234", "#f85c63", "#e45dbf", "#a96bf5", "#6677ff", "#2b97ff", "#23c5dd", "#00aa7f", "#41c86b", "#a6cc38", "#8796aa"];

export const INSTALL_LOG = [
  "==> Copying HGS source to build-01:~/.local/src/hgs (rsync, 1 284 files)",
  "==> cargo build --release --locked -p hgs",
  "   Compiling serde v1.0.228",
  "   Compiling tokio v1.48.0",
  "   Compiling hgs v0.48.0 (/home/example/.local/src/hgs)",
  "    Finished `release` profile [optimized] target(s) in 1m 12s",
  "==> Installing ~/.local/bin/hgs (existing sessions are preserved)",
];

// ---------------------------------------------------------------- Accounts (5.15)

export type MemberState = "ready" | "offline" | "not-installed" | "apikey" | "signed-out" | "keychain" | "desktop" | "unavailable" | "expired" | "checking";
export interface AccountMember {
  host: string; // machine alias
  id: string;
  state: MemberState;
  isDefault: boolean;
  permission: "provider" | "bypass";
  permissionDetail?: string;
  model?: string;
  effort?: string;
}
export interface UsageWindow { label: string; used: number; resets: string; resetIn: string; elapsed: number; }
export interface AgentAccount {
  key: string;
  label: string;
  provider: AgentKind;
  email?: string;
  plan?: string;
  members: AccountMember[];
  usage: { windows: UsageWindow[]; balance?: string; credits?: string; updated?: string; state?: string };
  openCodeProviders?: { name: string; models: number; detail: string }[];
}

export const AGENT_ACCOUNTS: AgentAccount[] = [
  {
    key: "a-claude-eng", label: "Aurora engineering", provider: "claude", email: "eng@example.com", plan: "Max",
    members: [
      { host: "workstation", id: "acc-claude-ws", state: "ready", isDefault: true, permission: "provider", permissionDetail: "Ask before tools (default mode).", model: "claude-opus-4-6", effort: "high" },
      { host: "build-01", id: "acc-claude-b1", state: "ready", isDefault: false, permission: "bypass" },
      { host: "build-02", id: "acc-claude-b2", state: "offline", isDefault: false, permission: "provider" },
    ],
    usage: { windows: [{ label: "5h", used: 34, resets: "27 Oct 17:50", resetIn: "2h 8m", elapsed: 58 }, { label: "7d", used: 18, resets: "1 Nov 09:00", resetIn: "4d 17h", elapsed: 33 }], updated: "15:41" },
  },
  {
    key: "a-codex-eng", label: "Aurora engineering", provider: "codex", email: "eng@example.com", plan: "Pro",
    members: [
      { host: "workstation", id: "acc-codex-ws", state: "ready", isDefault: true, permission: "provider", permissionDetail: "Workspace write with approval prompts." },
      { host: "win-lab", id: "acc-codex-win", state: "checking", isDefault: true, permission: "bypass" },
      { host: "build-01", id: "acc-codex-b1", state: "not-installed", isDefault: false, permission: "provider" },
    ],
    usage: { windows: [{ label: "5h", used: 52, resets: "27 Oct 16:47", resetIn: "1h 5m", elapsed: 78 }, { label: "7d", used: 31, resets: "30 Oct 11:20", resetIn: "2d 19h", elapsed: 60 }], credits: "Credits available: 120", updated: "15:40" },
  },
  {
    key: "a-codex-rel", label: "Release review", provider: "codex", email: "release@example.com", plan: "Pro",
    members: [{ host: "macbook", id: "acc-codex-mac", state: "ready", isDefault: true, permission: "provider", permissionDetail: "Workspace write with approval prompts." }],
    usage: { windows: [{ label: "5h", used: 43, resets: "27 Oct 19:22", resetIn: "3h 40m", elapsed: 27 }, { label: "7d", used: 25, resets: "2 Nov 08:00", resetIn: "5d 16h", elapsed: 21 }], updated: "15:38" },
  },
  {
    key: "a-claude-rel", label: "Release review", provider: "claude", email: "release@example.com", plan: "Max",
    members: [{ host: "macbook", id: "acc-claude-mac", state: "ready", isDefault: true, permission: "provider", permissionDetail: "Ask before tools (default mode)." }],
    usage: { windows: [{ label: "5h", used: 12, resets: "27 Oct 20:12", resetIn: "4h 30m", elapsed: 10 }, { label: "7d", used: 9, resets: "31 Oct 14:00", resetIn: "3d 22h", elapsed: 44 }], updated: "15:39" },
  },
  {
    key: "a-kimi-research", label: "Research", provider: "kimi", email: "research@example.org", plan: "Moderato",
    members: [
      { host: "workstation", id: "acc-kimi-ws", state: "ready", isDefault: true, permission: "provider", permissionDetail: "Ask before tools." },
      { host: "macbook", id: "acc-kimi-mac", state: "expired", isDefault: true, permission: "provider" },
    ],
    usage: { windows: [{ label: "5h", used: 61, resets: "27 Oct 16:34", resetIn: "52m", elapsed: 82 }, { label: "7d", used: 39, resets: "29 Oct 10:00", resetIn: "1d 18h", elapsed: 74 }], updated: "15:41" },
  },
  {
    key: "a-ds", label: "DeepSeek workspace", provider: "deepseek",
    members: [{ host: "build-01", id: "acc-ds-b1", state: "apikey", isDefault: true, permission: "bypass" }],
    usage: { windows: [], balance: "Balance: 18.40 USD", state: "API key configured. Usage is available in the DeepSeek console.", updated: "15:30" },
  },
  {
    key: "a-oc", label: "Local models", provider: "opencode",
    members: [{ host: "workstation", id: "native-opencode", state: "ready", isDefault: true, permission: "provider" }],
    usage: { windows: [], state: "OpenCode uses its native provider configuration. Usage is not available here." },
    openCodeProviders: [
      { name: "Ollama (local)", models: 12, detail: "http://127.0.0.1:11434" },
      { name: "OpenRouter", models: 48, detail: "API key in OpenCode" },
      { name: "Example Labs", models: 3, detail: "OpenAI-compatible" },
    ],
  },
  {
    key: "a-claude-personal", label: "Personal", provider: "claude", email: "me@example.org",
    members: [
      { host: "macbook", id: "acc-claude-mac-2", state: "keychain", isDefault: false, permission: "provider" },
      { host: "workstation", id: "acc-claude-ws-2", state: "signed-out", isDefault: false, permission: "provider" },
    ],
    usage: { windows: [], state: "Saved sign-in is in the locked macOS Keychain. Unlock it in Keychain Access, then refresh." },
  },
];

export const ACCOUNT_MODELS: Record<string, { id: string; efforts: string[] }[]> = {
  claude: [{ id: "claude-opus-4-6", efforts: ["low", "medium", "high", "max"] }, { id: "claude-sonnet-4-6", efforts: ["low", "medium", "high"] }],
  codex: [{ id: "gpt-6.1-sol", efforts: ["minimal", "low", "medium", "high"] }, { id: "gpt-6.1-mini", efforts: ["low", "medium"] }],
  kimi: [{ id: "kimi-k2.5", efforts: [] }, { id: "kimi-k2.5-turbo", efforts: [] }],
  deepseek: [{ id: "deepseek-v4-pro", efforts: ["low", "high"] }, { id: "deepseek-v4-flash", efforts: [] }],
  opencode: [],
};
