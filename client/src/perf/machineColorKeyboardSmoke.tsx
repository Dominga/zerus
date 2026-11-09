import { chooseDropdown } from "./dropdown";
// Real color dialog, synthetic settings replies; no device preference is written.
// With Vite running: python3 scripts/menu-smoke.py
//   --url http://127.0.0.1:1420/scripts/machine-color-keyboard-smoke.html
//   --title "Zerus machine color keyboard regression (synthetic)"
import { render } from "solid-js/web";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/machines.css";

const failures: string[] = [], calls: string[] = [];
const allowedCommands = new Set(["pending_read_marks", "attention_marks", "cancel_content_search", "settings_get", "settings_set"]);
const writes: { key: string; value: unknown; finish(): void }[] = [];
let revision = 0;
const check = (value: unknown, message: string) => { if (!value) failures.push(message); };
const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
const report = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string, args: { key?: string; value?: unknown } = {}) => {
  calls.push(command);
  if (command === "pending_read_marks") return [];
  if (command === "attention_marks") return { marks: {}, replies: {} };
  if (command === "cancel_content_search") return null;
  if (command === "settings_get") return { values: {}, revision: 0 };
  if (command === "settings_set") return new Promise((resolve) => writes.push({ key: args.key!, value: args.value, finish: () => resolve({ value: args.value, revision: ++revision }) }));
  failures.push(`Unexpected synthetic IPC: ${command}`); throw Error(`Unexpected synthetic IPC: ${command}`);
} } });
async function run() {
  const { default: MachineColorDialog } = await import("../ui/screens/MachineColorDialog");
  const { settingsReady } = await import("../store/settings"); await settingsReady;
  let closed = 0;
  const dispose = render(() => <MachineColorDialog alias="fixture-machine" origin={document.getElementById("origin")!} close={() => closed++} />, document.getElementById("root")!);
  try {
    await frame();
    const rows = () => [...document.querySelectorAll<HTMLButtonElement>(".cd-swatch")];
    const hex = () => document.querySelector<HTMLInputElement>("#cd-hex")!;
    const apply = () => [...document.querySelectorAll<HTMLButtonElement>("dialog button")].find((row) => row.textContent === "Apply")!;
    const state = (at: number, focus = true) => {
      const radios = rows(), selected = radios.filter((row) => row.getAttribute("aria-checked") === "true");
      check(selected.length === (at < 0 ? 0 : 1) && (at < 0 || selected[0] === radios[at]), "swatch selection follows color");
      check(radios.filter((row) => row.tabIndex === 0).length === 1 && radios[Math.max(0, at)].tabIndex === 0, "one swatch Tab entry, with custom-color fallback");
      if (at >= 0) check(hex().value.toLowerCase() === radios[at].getAttribute("aria-label"), "hex follows the selected swatch");
      if (focus) check(document.activeElement === radios[Math.max(0, at)], "focus follows the selected swatch");
      check(document.querySelector<HTMLElement>(".cd-preview .mach-label")!.style.getPropertyValue("--entity").toLowerCase() === hex().value.toLowerCase(), "preview follows the staged color");
    };
    const key = (key: string, at: number) => {
      const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
      document.activeElement!.dispatchEvent(event); check(event.defaultPrevented, `${key}: browser scroll prevented`); state(at);
    };
    const enterHex = (value: string) => { hex().value = value; hex().dispatchEvent(new InputEvent("input", { bubbles: true })); };
    state(6, false); rows()[6].focus();
    key("ArrowRight", 7); key("ArrowDown", 8); key("ArrowLeft", 7); key("ArrowUp", 6);
    key("Home", 0); key("ArrowUp", 11); key("ArrowRight", 0); key("End", 11);
    enterHex("#123456"); state(-1, false); rows()[0].focus(); key("Home", 0);
    enterHex("#6677FF"); state(5, false); rows()[5].focus();
    for (const modifier of ["altKey", "ctrlKey", "metaKey"]) {
      const event = new KeyboardEvent("keydown", { key: "ArrowDown", [modifier]: true, bubbles: true, cancelable: true });
      rows()[5].dispatchEvent(event); check(!event.defaultPrevented, `${modifier}: shortcut preserved`); state(5);
    }
    hex().focus();
    const fieldKey = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    hex().dispatchEvent(fieldKey); check(!fieldKey.defaultPrevented, "hex field keeps its own keys"); state(5, false);
    const style = document.querySelector<HTMLButtonElement>("#cd-style")!;
    chooseDropdown(style, "bright");
    check(writes.length === 0 && closed === 0, "keyboard, hex and style changes remain staged");
    apply().click(); await frame();
    check(JSON.stringify(writes.map(({ key, value }) => ({ key, value }))) === JSON.stringify([
      { key: "machines/colors/fixture-machine", value: "#6677ff" }, { key: "machines/vivid/fixture-machine", value: true },
    ]), "explicit Apply saves exact machine/color/style only");
    check(rows().every((row) => row.disabled && row.tabIndex === -1) && apply().disabled, "saving disables the swatch group and Apply");
    rows()[5].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
    check(hex().value === "#6677FF" && writes.length === 2, "busy swatches cannot change the staged save");
    writes.forEach((write) => write.finish());
    const end = performance.now() + 5000; while (!closed && performance.now() < end) await frame();
    check(closed === 1 && writes.length === 2, "only the explicit Apply completes");
    check(calls.every((command) => allowedCommands.has(command)), "no unrelated IPC");
  } finally { dispose(); }
  report({ failures, cases: ["one Tab entry", "wrapped arrows and Home/End", "custom hex and preview", "staged until Apply", "busy guard and exact saved settings"] });
}
void run().catch((error) => report({ failures: [String(error)] }));
