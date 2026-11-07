// Real Machines screen, synthetic read replies and no machine writes or SSH.
// With Vite running: python3 scripts/menu-smoke.py --accessibility
//   --url http://127.0.0.1:1420/scripts/machines-accessibility-smoke.html
//   --title "Zerus Machines accessibility regression (synthetic)"
import { render } from "solid-js/web";
import { setMachinesPage } from "../store/machines";
import { emptyMachines, type MachinesView } from "../types";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/machines.css";

const failures: string[] = [], calls: string[] = [];
const startupReads = new Set(["pending_read_marks", "attention_marks", "cancel_content_search", "settings_get", "machine_resolve"]);
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
const view: MachinesView = { ...emptyMachines(), loaded: true, local: "fixture-local", connection_revision: "synthetic-revision", machines: [
  { alias: "fixture-peer", enabled: true, source: "local", inherited: false, transport: "ssh", wsl: null, test_runner: "", hostname: "peer.example.invalid", user: "fixture", port: null, identity_file: "", origin: "Synthetic machine" },
] };
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string, args: { alias?: string } = {}) => {
  calls.push(command);
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "cancel_content_search") return null;
  if (command === "settings_get") return { values: {}, revision: 0 };
  if (command === "machine_resolve" && args.alias === "fixture-peer") return { ...view, effective: { alias: args.alias, pending: false, error: false, text: "Synthetic connection read", hostname: "peer.example.invalid", user: "fixture", port: "22", key: "" } };
  failures.push(`Unexpected synthetic IPC: ${command}`);
  throw Error(`Unexpected synthetic IPC: ${command}`);
} } });
async function run() {
  setMachinesPage(view);
  const { MachinesScreen } = await import("../ui/screens/Machines");
  const dispose = render(() => <MachinesScreen />, document.getElementById("root")!);
  try {
    await frame();
    const cards = () => [...document.querySelectorAll<HTMLButtonElement>(".mach-card")];
    const button = (text: string) => [...document.querySelectorAll<HTMLButtonElement>("button")].find((row) => row.textContent?.trim() === text)!;
    const initial = cards();
    check(initial.length === 2, "local and saved peer are listed");
    for (const [index, name] of ["fixture-local", "fixture-peer"].entries()) {
      initial[index].focus(); initial[index].click(); await frame();
      check(document.activeElement === initial[index] && initial[index].getAttribute("aria-current") === "true", `${name}: selected button keeps focus`);
      check(document.querySelector(".mach-title h2")?.textContent === name, `${name}: exact editor target`);
    }
    button("Add machine").click(); await frame();
    check(cards().length === 3 && cards()[2].classList.contains("draft"), "unsaved draft is listed");
    check(document.activeElement === document.getElementById("mach-alias"), "draft focuses its alias field");
    button("Discard draft").click(); await frame(); check(cards().length === 2, "discard removes only the draft card");
    button("Add machine").click(); await frame();
    check(cards()[0] === initial[0] && cards()[1] === initial[1], "selection and draft toggles never remount the saved cards");
    check(calls.every((command) => startupReads.has(command)), `unexpected IPC: ${calls.filter((command) => !startupReads.has(command)).join(", ")}`);
    const card_geometry = cards().map((card) => { const { x, y, width, height } = card.getBoundingClientRect(); return { x, y, width, height }; });
    // The fixture window owns this render while the native accessibility read runs.
    report({ failures, card_geometry, accessibility_lists: [{ name: "Machines", buttons: ["Computer, Not connected fixture-local", "Computer, Not connected fixture-peer", "New machine"] }], cases: ["local and peer selection", "draft add/discard", "saved buttons remain mounted", "no machine write or SSH"] });
  } catch (error) { dispose(); throw error; }
}
void run().catch((error) => report({ failures: [String(error)] }));
