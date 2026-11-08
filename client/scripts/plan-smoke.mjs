// Task 197: screenshots of the plan's Graph, Board and Layers (scripts/plan-smoke.html,
// src/perf/planSmoke.tsx) beside Qt's shots of the same team; task 206: the side
// panel's sections and tabs (page "side-panel": scripts/side-panel-smoke.html,
// src/perf/sidePanelSmoke.tsx) beside Qt's ux-6-inspector shots; task 208: the
// Activity view's system notices (page "activity-notices") beside Qt's
// activity-whitewh shot; task 248: the media an agent names (page "agent-media").
// Starts Vite and a
// headless Chrome of its own (a new, empty profile under the output folder),
// drives it over CDP only (Page.captureScreenshot; no OS input, no screen
// capture), and closes both. No CLI, session or owner data is opened.
//   node scripts/plan-smoke.mjs <output folder> [chrome.exe] [plan|side-panel|activity-notices|agent-media|subagent-card|lead-notices|title-bar]
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { createServer } from "vite";
import solid from "vite-plugin-solid";

const out = resolve(process.argv[2] || "plan-smoke-shots");
const chrome = process.argv[3] || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const page = process.argv[4] || "plan";
mkdirSync(out, { recursive: true });
const profile = join(out, "chrome-profile");
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

const server = await createServer({ configFile: false, root: resolve(import.meta.dirname, ".."), logLevel: "error",
  server: { port: 0, host: "127.0.0.1", strictPort: false }, plugins: [solid()] });
await server.listen();
const port = server.httpServer.address().port;
const debug = 9400 + Math.floor(Math.random() * 400);
const browser = spawn(chrome, ["--headless=new", `--remote-debugging-port=${debug}`, `--user-data-dir=${profile}`, "--no-first-run",
  "--no-default-browser-check", "--disable-extensions", "--hide-scrollbars", "--force-device-scale-factor=1", "about:blank"], { stdio: "ignore" });

async function json(path) {
  for (let i = 0; i < 100; i++) {
    try { return await (await fetch(`http://127.0.0.1:${debug}${path}`)).json(); } catch { await sleep(100); }
  }
  throw new Error("Chrome's debugging port did not answer");
}

let id = 0;
function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  const events = [];
  socket.onmessage = (message) => {
    const data = JSON.parse(message.data);
    if (data.id && pending.has(data.id)) { pending.get(data.id)(data); pending.delete(data.id); } else events.push(data);
  };
  const ready = new Promise((done) => { socket.onopen = done; });
  const send = async (method, params = {}) => {
    await ready;
    const n = ++id;
    socket.send(JSON.stringify({ id: n, method, params }));
    const answer = await new Promise((done) => pending.set(n, done));
    if (answer.error) throw new Error(`${method}: ${answer.error.message}`);
    return answer.result;
  };
  return { send, close: () => socket.close(), events };
}

const shots = [];
try {
  const target = (await json("/json/list")).find((page) => page.type === "page");
  const cdp = connect(target.webSocketDebuggerUrl);
  await cdp.send("Page.enable");
  await cdp.send("Emulation.setDeviceMetricsOverride", page === "side-panel"
    ? { width: 440, height: 1000, deviceScaleFactor: 1, mobile: false }
    : page === "activity-notices" || page === "lead-notices" ? { width: 760, height: 1100, deviceScaleFactor: 1, mobile: false }
    : page === "agent-media" ? { width: 760, height: 1000, deviceScaleFactor: 1, mobile: false }
    : page === "subagent-card" ? { width: 760, height: 520, deviceScaleFactor: 1, mobile: false }
    : page === "title-bar" ? { width: 1440, height: 640, deviceScaleFactor: 1, mobile: false }
    : { width: 1180, height: 900, deviceScaleFactor: 1, mobile: false });
  const cases = page === "title-bar" ? [["unified-dark", "skin=whitewh-dark"], ["unified-hidden-dark", "skin=whitewh-dark&strip=1"], ["unified-light", "skin=whitewh-light"], ["system-dark", "skin=zerus-dark"], ["system-collapsed-dark", "skin=zerus-dark&strip=1"]]
    : page === "lead-notices" ? [["lead-full-dark", "skin=whitewh-dark&mode=full"], ["lead-collapsed-dark", "skin=whitewh-dark&mode=collapsed"], ["lead-silent-dark", "skin=whitewh-dark&mode=silent"], ["lead-collapsed-light", "skin=whitewh-light&mode=collapsed"]]
    : page === "subagent-card" ? [["subagents-dark", "skin=whitewh-dark"], ["subagents-light", "skin=whitewh-light"], ["subagents-confirm-dark", "skin=whitewh-dark&confirm=1"]]
    : page === "agent-media" ? [["media-dark", "skin=whitewh-dark"], ["media-light", "skin=whitewh-light"], ["media-viewer-dark", "skin=whitewh-dark&viewer=1"]]
    : page === "activity-notices" ? [["activity-dark", "skin=whitewh-dark"], ["activity-light", "skin=whitewh-light"]] : page === "side-panel" ? [
    ["sections-dark", "skin=whitewh-dark"], ["sections-light", "skin=whitewh-light"], ["sections-folded-dark", "skin=whitewh-dark&fold=1"],
    ["tabs-dark", "skin=zerus-dark"], ["sections-finished-dark", "skin=whitewh-dark&finished=1&tasks=1"], ["tabs-finished-dark", "skin=zerus-dark&finished=1"],
  ] : [
    ["graph-dark", "mode=graph"], ["board-dark", "mode=board"], ["layers-dark", "mode=layers"],
    ["graph-light", "mode=graph&skin=whitewh-light"], ["board-light", "mode=board&skin=whitewh-light"], ["layers-light", "mode=layers&skin=whitewh-light"],
    ["graph-tester", "mode=graph&member=tester"], ["graph-drawers-dark", "mode=graph&drawers=1"], ["graph-drawers-light", "mode=graph&drawers=1&skin=whitewh-light"],
  ];
  for (const [name, query] of cases) {
    await cdp.send("Page.navigate", { url: `http://127.0.0.1:${port}/scripts/${page}-smoke.html?${query}` });
    let title = "";
    for (let i = 0; i < 150 && !title.endsWith(": ready"); i++) {
      await sleep(100);
      title = (await cdp.send("Runtime.evaluate", { expression: "document.title", returnByValue: true })).result.value;
    }
    if (!title.endsWith(": ready")) throw new Error(`${name}: the page never said it was ready (${title})`);
    await sleep(600);
    const errors = (await cdp.send("Runtime.evaluate", {
      expression: page === "title-bar"
        ? "JSON.stringify({bar: !!document.querySelector('.titlebar'), rail: !!document.querySelector('.rail'), paneWidth: Math.round(document.querySelector('.sessions-pane')?.getBoundingClientRect().width ?? -1), cards: document.querySelectorAll('.sessions-pane [data-key], .sessions-pane .sc').length})"
        : page === "lead-notices"
        ? "JSON.stringify({rows: [...document.querySelectorAll('.act-msg, .act-tools')].map((e) => e.className.replace(/^act-(msg|tools) ?/, '') + ': ' + (e.querySelector('header b, .act-tools-head b')?.textContent ?? '')), mode: document.querySelector('[data-notice-mode]')?.value ?? 'none'})"
        : page === "subagent-card"
        ? "JSON.stringify({rows: document.querySelectorAll('.qcard-subagent').length, buttons: [...document.querySelectorAll('.qcard-actions button')].map((b) => b.textContent)})"
        : page === "agent-media"
        ? "JSON.stringify({tiles: [...document.querySelectorAll('.media-tile')].map((e) => e.dataset.media + ': ' + (e.querySelector('.media-note')?.textContent ?? 'shown')), viewer: document.querySelector('dialog.media-viewer[open] h2')?.textContent ?? ''})"
        : page === "activity-notices"
        ? "JSON.stringify([...document.querySelectorAll('.act-msg')].map((e) => e.className.replace('act-msg ', '') + ': ' + (e.querySelector('header b')?.textContent ?? '')))"
        : page === "side-panel"
        ? "JSON.stringify({sections: [...document.querySelectorAll('.insp-sections > [data-section]')].map((e) => e.dataset.section + (e.classList.contains('closed') ? ' (folded)' : '')), tabs: document.querySelectorAll('.insp-tabs .tab').length})"
        : "JSON.stringify({cards: document.querySelectorAll('[data-task]').length, edges: document.querySelectorAll('[data-edge]').length})",
      returnByValue: true })).result.value;
    const png = await cdp.send("Page.captureScreenshot", { format: "png" });
    const file = join(out, `${name}.png`);
    writeFileSync(file, Buffer.from(png.data, "base64"));
    shots.push({ name, file, counts: JSON.parse(errors) });
  }
  cdp.close();
} finally {
  browser.kill();
  await server.close();
  await sleep(500);
  try { rmSync(profile, { recursive: true, force: true }); } catch { /* Chrome may still hold it */ }
}
writeFileSync(join(out, "shots.json"), JSON.stringify(shots, null, 1));
for (const shot of shots) console.log(`${shot.name}: ${shot.file} ${JSON.stringify(shot.counts)}`);
