import { chooseDropdown } from "./dropdown";
// Real Add account dialog, synthetic choices and a captured submit: no account writes.
// Start Vite, then: dbus-run-session -- python3 scripts/menu-smoke.py
//   --url http://127.0.0.1:1420/scripts/account-agent-keyboard-smoke.html
//   --title "Zerus account agent keyboard regression (synthetic)"
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import type { AccountSetupHost, AccountSetupValues } from "../bridge";
import type { SetupPrompt } from "../store/accountSetup";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/machines.css";

const failures: string[] = [], calls: string[] = [];
const startupReads = new Set(["pending_read_marks", "attention_marks", "cancel_content_search", "settings_get"]);
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string) => {
  calls.push(command);
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "cancel_content_search") return null;
  if (command === "settings_get") return { values: {}, revision: 0 };
  throw Error(`Unexpected synthetic IPC: ${command}`);
} } });
const saved = (id: string, provider: string) => ({ id, provider, label: id, installed: true });
const host = (key: string, providers: string[], profiles: AccountSetupHost["saved"] = []): AccountSetupHost => ({ key, machine: key, label: key, online: true, providers, saved: profiles, listed: [], deepseek_installed: false });
const hosts = [
  host("workstation", ["codex", "opencode", "claude", "dsh", "kimi"], [saved("codex-a", "codex"), saved("codex-b", "codex"), saved("claude-a", "claude")]),
  host("other", ["opencode", "claude"], [saved("native-opencode", "opencode"), saved("claude-other", "claude")]),
  host("unavailable", ["opencode"]), host("fallback", ["opencode", "kimi"]),
];
async function run() {
  const { default: AccountSetupDialog } = await import("../ui/screens/AccountSetupDialog");
  const prompt: SetupPrompt = { handle: "synthetic-account-setup", kind: "add", hosts, host: "workstation", preferredHost: "workstation", provider: "codex", label: "Example", origin: document.getElementById("origin")! };
  const submitted: { values: AccountSetupValues; handle: string }[] = [];
  const closed: string[] = [];
  const [busy, setBusy] = createSignal(false);
  const dispose = render(() => <AccountSetupDialog prompt={prompt} busy={busy()} close={(handle) => closed.push(handle)} confirm={(values, handle) => submitted.push({ values, handle })} />, document.getElementById("root")!);
  try {
    await frame();
    const options = () => [...document.querySelectorAll<HTMLButtonElement>('.acct-agents [role="option"]')];
    const option = (label: string) => options().find((row) => row.querySelector("b")?.textContent === label)!;
    const account = () => document.querySelector<HTMLButtonElement>("#aa-saved")!;
    const name = () => document.querySelector<HTMLInputElement>("#aa-name")!;
    const state = (label: string, id: string, focus = true) => {
      check(options().filter((row) => row.getAttribute("aria-selected") === "true").length === 1 && option(label).getAttribute("aria-selected") === "true", `${label}: selected`);
      check(options().filter((row) => row.tabIndex === 0).length === 1 && option(label).tabIndex === 0, `${label}: one selected Tab entry`);
      if (focus) check(document.activeElement === option(label), `${label}: focus follows selection`);
      check(account().value === id, `${label}: exact saved profile ${id}`);
      check(name().disabled === !!id, `${label}: new versus saved profile form`);
    };
    const key = (key: string, label: string, id: string) => {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      document.activeElement!.dispatchEvent(event); check(event.defaultPrevented, `${key}: browser scroll prevented`); state(label, id);
    };
    const changeHost = (key: string) => {
      const select = document.querySelector<HTMLButtonElement>("#aa-host")!;
      chooseDropdown(select, key);
    };
    state("Codex", "codex-a", false); check(option("OpenCode").disabled, "unavailable OpenCode stays disabled");
    option("Codex").focus(); key("ArrowDown", "Claude", "claude-a"); key("ArrowUp", "Codex", "codex-a");
    chooseDropdown(account(), "codex-b"); check(document.activeElement === account(), "dropdown returns focus to its trigger"); option("Codex").focus();
    key("ArrowLeft", "Codex", "codex-b"); key("Home", "Codex", "codex-b");
    for (const modifier of ["altKey", "ctrlKey", "metaKey"]) {
      const event = new KeyboardEvent("keydown", { key: "ArrowDown", [modifier]: true, bubbles: true, cancelable: true });
      option("Codex").dispatchEvent(event); check(!event.defaultPrevented, `${modifier}: shortcut preserved`); state("Codex", "codex-b");
    }
    key("ArrowRight", "Claude", "claude-a"); key("End", "Kimi", ""); key("ArrowDown", "Kimi", "");
    name().value = "Synthetic account"; name().dispatchEvent(new InputEvent("input", { bubbles: true }));
    name().focus();
    const fieldKey = new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true });
    name().dispatchEvent(fieldKey); check(!fieldKey.defaultPrevented, "form field keeps its own keys"); state("Kimi", "", false);
    changeHost("other"); state("OpenCode", "native-opencode", false);
    option("OpenCode").focus(); key("ArrowDown", "Claude", "claude-other"); key("Home", "OpenCode", "native-opencode");
    changeHost("unavailable"); check(options().every((row) => row.disabled && row.tabIndex === -1), "no unavailable option becomes a Tab entry");
    changeHost("fallback"); check(option("OpenCode").disabled && option("Kimi").tabIndex === 0, "unavailable retained selection leaves an enabled Tab entry");
    option("Kimi").focus(); key("End", "Kimi", "");
    changeHost("other"); state("OpenCode", "native-opencode", false);
    check(submitted.length === 0, "navigation must not submit account setup");
    check(closed.length === 0, "navigation must not close account setup");
    document.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    check(JSON.stringify(submitted) === JSON.stringify([{ values: { host: "other", provider: "opencode", saved: "native-opencode", label: "Synthetic account", api_key: "" }, handle: prompt.handle }]), "one explicit submit keeps exact host/provider/profile/handle");
    setBusy(true); document.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    check(submitted.length === 1, "busy submit remains refused");
    check(calls.every((command) => startupReads.has(command)), `unexpected IPC: ${calls.filter((command) => !startupReads.has(command)).join(", ")}`);
  } finally { dispose(); }
  report({ failures, cases: ["single Tab entry", "arrows and Home/End skip unavailable agents", "edge retains saved profile", "host and form binding", "only explicit submit"] });
}
void run().catch((error) => report({ failures: [String(error)] }));
