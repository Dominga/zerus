// Real Accounts screen with synthetic profiles, no CLI or account writes.
// Start Vite, then: dbus-run-session -- python3 scripts/menu-smoke.py
//   --url http://127.0.0.1:1420/scripts/accounts-keyboard-smoke.html
//   --title "Zerus Accounts keyboard regression (synthetic)"
import { render } from "solid-js/web";
import { setAccountsPage, setRequestedProfile } from "../store/accounts";
import { emptyAccounts, emptyUsage, type Account, type Profile } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/screens.css";
import "../ui/styles/machines.css";

const failures: string[] = [], calls: string[] = [];
const startupReads = new Set(["pending_read_marks", "attention_marks", "cancel_content_search"]);
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string) => {
  calls.push(command);
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "cancel_content_search") return null;
  throw Error(`Unexpected synthetic IPC: ${command}`);
} } });

function profile(index: number): Profile {
  const host = `workstation-${index < 2 ? 0 : index}`;
  return {
    key: `${host}\u001fclaude-${index}`, host, machine: host, id: `claude-${index}`, label: "Example", provider: "claude", native: false,
    installed: true, is_default: index === 0, home: "", auth_revision: "", online: index !== 2, state: index === 2 ? "offline" : "ready",
    detail: { source: `Synthetic profile ${index}` }, signed_in: true, permission: "auto", permission_label: { source: "Auto" },
    permission_hint: { source: "" }, model_supported: false, model_text: { source: "" }, usage: emptyUsage("claude"), opencode: null,
  };
}
const members = Array.from({ length: 6 }, (_, index) => profile(index));
const account: Account = { key: "example", label: "Example", provider: "claude", installed: true, machines: members.map((m) => m.machine), members, usage: emptyUsage("claude"), permission: { source: "Auto" }, identity_lines: [], identity_note: null };
const single: Account = { ...account, key: "single", label: "Single profile", members: [profile(7)] };

async function run() {
  setAccountsPage({ ...emptyAccounts(), snapshot: true, accounts: [account, single] });
  const { AccountsScreen } = await import("../ui/screens/Accounts");
  const dispose = render(() => <AccountsScreen />, document.getElementById("root")!);
  try {
    await frame();
    const group = () => document.querySelector<HTMLElement>(".acct-mrows")!;
    const radios = () => [...group().querySelectorAll<HTMLButtonElement>('[role="radio"]')];
    const state = (at: number, profile: number, focused = true) => {
      const rows = radios();
      check(rows.filter((r) => r.getAttribute("aria-checked") === "true").length === 1 && rows[at]?.getAttribute("aria-checked") === "true", `profile ${profile}: selection`);
      check(rows.filter((r) => r.tabIndex === 0).length === 1 && rows[at]?.tabIndex === 0, `profile ${profile}: exactly one selected Tab entry`);
      if (focused) check(document.activeElement === rows[at], `profile ${profile}: focus follows selection`);
      check(document.querySelector(".acct-status")?.textContent?.includes(`Synthetic profile ${profile}`), `profile ${profile}: exact detail target`);
    };
    const key = (key: string, expected: number, profile = expected) => {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      document.activeElement!.dispatchEvent(event);
      check(event.defaultPrevented, `${key}: browser scroll prevented`);
      state(expected, profile);
    };
    check(radios().length === 4, "only the bounded profile window is shown");
    state(0, 0, false); radios()[0].focus();
    key("ArrowRight", 1); // Same machine, different profile key.
    key("ArrowDown", 2); // Offline data remains selectable for inspection.
    key("ArrowLeft", 1); key("ArrowUp", 0);
    key("ArrowUp", 3); key("ArrowDown", 0);
    key("End", 3); key("Home", 0);
    for (const modifier of ["altKey", "ctrlKey", "metaKey"]) {
      const event = new KeyboardEvent("keydown", { key: "ArrowDown", [modifier]: true, bubbles: true, cancelable: true });
      radios()[0].dispatchEvent(event); check(!event.defaultPrevented, `${modifier}: shortcut preserved`); state(0, 0);
    }
    const defaultButton = group().querySelector<HTMLButtonElement>(".acct-mrow-default button")!;
    check(defaultButton.tabIndex === 0, "separate default action remains tabbable");
    defaultButton.focus();
    const event = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    defaultButton.dispatchEvent(event); check(!event.defaultPrevented, "default action does not navigate profiles");
    check(document.activeElement === defaultButton, "default action keeps focus"); state(0, 0, false);
    radios()[2].focus(); radios()[2].click(); state(2, 2);

    setRequestedProfile(members[5].key); await frame();
    check(radios().length === 4, "hidden selection keeps the window bounded");
    state(3, 5, false); radios()[3].focus(); key("ArrowLeft", 2);
    check(radios()[3].textContent?.includes("workstation-3"), "leaving hidden selection restores the visible window");
    const more = document.querySelector<HTMLButtonElement>(".acct-more-anchor > button")!;
    more.click(); await frame();
    const hidden = document.querySelectorAll<HTMLButtonElement>(".acct-more-row")[4];
    hidden.click(); await frame(); state(3, 4, false);
    radios()[3].focus(); key("ArrowRight", 0);

    const accountRows = document.querySelectorAll<HTMLButtonElement>(".acct-row");
    accountRows[1].click(); await frame(); state(0, 7, false);
    radios()[0].focus(); key("ArrowDown", 0, 7); key("End", 0, 7);
    accountRows[0].click(); await frame(); state(0, 0, false);
    check(calls.every((command) => startupReads.has(command)), `profile navigation submitted IPC: ${calls.filter((command) => !startupReads.has(command)).join(", ")}`);
  } finally { dispose(); }
  report({ failures, cases: ["roving Tab entry", "arrows wrap and Home/End", "exact profile and offline selection", "separate default action", "hidden profile and account reset", "no management IPC"] });
}
void run().catch((error) => report({ failures: [String(error)] }));
