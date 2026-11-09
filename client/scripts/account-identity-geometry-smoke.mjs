#!/usr/bin/env node
// Capture the real Accounts identity header in an isolated Firefox profile.
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const CLIENT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ROOT = resolve(CLIENT, "..");
const PAGE = "scripts/account-identity-geometry-smoke.html";
const HARNESS = [
  "client/src/perf/accountIdentityGeometrySmoke.tsx",
  "client/scripts/account-identity-geometry-smoke.html",
  "client/scripts/account-identity-geometry-smoke.mjs",
];
const CASES = ["loading", "kimi", "codex", "claude", "unavailable", "claude-late"];
const TITLE_CASES = ["short", "email-label", "switch-long", "long-label", "localized-label", "hidden-short", "hidden-long", "unhidden-short"];
let titleLabels;
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const digest = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((resolve) => child.once("exit", resolve));
  child.kill("SIGTERM");
  if (!await Promise.race([exited.then(() => true), pause(8000).then(() => false)])) {
    child.kill("SIGKILL");
    await exited;
  }
}

class Bidi {
  constructor(ws) {
    this.ws = ws;
    this.pending = new Map();
    this.nextId = 0;
    ws.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      const request = this.pending.get(message.id);
      if (!request) return;
      this.pending.delete(message.id);
      clearTimeout(request.timer);
      if (message.error) request.reject(new Error(JSON.stringify(message)));
      else request.resolve(message.result);
    });
  }

  static async connect(port) {
    let lastError;
    for (let attempt = 0; attempt < 100; attempt++) {
      const ws = new WebSocket(`ws://127.0.0.1:${port}/session`);
      try {
        await new Promise((resolve, reject) => {
          ws.addEventListener("open", resolve, { once: true });
          ws.addEventListener("error", reject, { once: true });
          setTimeout(() => reject(new Error("BiDi open timed out")), 2000).unref();
        });
        const bidi = new Bidi(ws);
        await bidi.call("session.new", { capabilities: {} });
        return bidi;
      } catch (error) {
        lastError = error;
        ws.close();
        await pause(100);
      }
    }
    throw lastError;
  }

  call(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++this.nextId;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timed out`));
      }, 15000);
      this.pending.set(id, { resolve, reject, timer });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  async evaluate(expression) {
    const response = await this.call("script.evaluate", {
      expression, target: { context: this.context }, awaitPromise: true, resultOwnership: "none",
    });
    if (response.type === "exception") throw new Error(`Browser evaluation failed: ${JSON.stringify(response)}`);
    return response.result.value ?? response.result;
  }

  async json(expression) {
    return JSON.parse(await this.evaluate(`(async () => JSON.stringify(await (${expression})))()`));
  }

  async viewport(width) {
    await this.call("browsingContext.setViewport", {
      context: this.context, viewport: { width, height: 1000 }, devicePixelRatio: 1,
    });
    await pause(100);
  }

  async capture(path) {
    const { data } = await this.call("browsingContext.captureScreenshot", {
      context: this.context, format: { type: "png" },
    });
    const bytes = Buffer.from(data, "base64");
    writeFileSync(path, bytes);
    return createHash("sha256").update(bytes).digest("hex");
  }
}

async function waitReady(bidi) {
  for (let attempt = 0; attempt < 100; attempt++) {
    const state = await bidi.json("document.documentElement.dataset.fixtureReady || ''");
    if (state === "true") {
      const failures = await bidi.json("window.accountIdentityFixture.failures()");
      if (failures.length) throw new Error(`Fixture initialization failed: ${JSON.stringify(failures)}`);
      return;
    }
    if (state === "error") throw new Error(JSON.stringify(await bidi.json("window.accountIdentityFixture.failures()")));
    await pause(100);
  }
  throw new Error("Accounts fixture did not initialize");
}

function closeTo(before, after, label) {
  if (Math.abs(before - after) > 0.02) throw new Error(`${label}: ${before} != ${after}`);
}

function checkGeometry(details, baseline, label) {
  if (details.accountCount !== 3 || details.machineCount !== 1 || !details.ipcExpected) {
    throw new Error(`${label}: real screen counts or unexpected startup IPC: ${JSON.stringify(details)}`);
  }
  if (details.mainScrollWidth > details.mainClientWidth + 1) {
    throw new Error(`${label}: horizontal overflow ${details.mainScrollWidth} > ${details.mainClientWidth}`);
  }
  if (details.actions.length !== 2) throw new Error(`${label}: title actions missing`);
  closeTo(details.provider.left - details.accountLabel.right, 10, `${label}: provider follows the title`);
  closeTo(details.accountLabel.left - details.mark.right, 10, `${label}: one title gap after the mark`);
  if (details.provider.right > details.titleMain.right + 0.02) throw new Error(`${label}: provider overlaps actions`);
  if (baseline) {
    for (const key of ["machines", "usage"]) closeTo(details[key].top, baseline[key].top, `${label}: ${key} top`);
    for (const key of ["title", "identity"]) closeTo(details[key].height, baseline[key].height, `${label}: ${key} height`);
    for (const key of ["titleMain", "actionArea"]) {
      for (const metric of ["top", "height", "left", "right"]) closeTo(details[key][metric], baseline[key][metric], `${label}: ${key} ${metric}`);
    }
    for (const key of ["mark", "provider"]) {
      for (const metric of key === "mark" ? ["top", "height", "right"] : ["top", "height"]) closeTo(details[key][metric], baseline[key][metric], `${label}: ${key} ${metric}`);
    }
    closeTo(details.accountLabel.height, baseline.accountLabel.height, `${label}: account label height`);
    closeTo(details.identity.bottom, baseline.identity.bottom, `${label}: header bottom`);
  }
}

async function runCase(bidi, widthName, caseName, output) {
  let details;
  if (caseName === "loading" || caseName === "short") details = await bidi.json("window.accountIdentityFixture.reset()");
  else if (caseName === "kimi") details = await bidi.json("window.accountIdentityFixture.update('kimi')");
  else if (caseName === "codex") details = await bidi.json("window.accountIdentityFixture.select('Codex Work')");
  else if (caseName === "claude") details = await bidi.json("window.accountIdentityFixture.select('Claude Team')");
  else if (caseName === "unavailable") details = await bidi.json("window.accountIdentityFixture.update('unavailable')");
  else if (caseName === "claude-late") details = await bidi.json("window.accountIdentityFixture.update('claude-late')");
  else if (caseName === "switch-long") {
    await bidi.json("window.accountIdentityFixture.select('Codex Work')");
    details = await bidi.json(`window.accountIdentityFixture.select(${JSON.stringify(titleLabels["email-label"])})`);
  } else if (caseName === "email-label" || caseName === "long-label" || caseName === "localized-label") {
    details = await bidi.json(`window.accountIdentityFixture.setLabel(${JSON.stringify(titleLabels[caseName])})`);
  } else if (caseName === "hidden-short") {
    await bidi.json(`window.accountIdentityFixture.setLabel(${JSON.stringify(titleLabels.short)})`);
    details = await bidi.json("window.accountIdentityFixture.setHidden(true)");
  } else if (caseName === "hidden-long") {
    details = await bidi.json(`window.accountIdentityFixture.setLabel(${JSON.stringify(titleLabels["localized-label"])})`);
  } else if (caseName === "unhidden-short") {
    await bidi.json(`window.accountIdentityFixture.setLabel(${JSON.stringify(titleLabels.short)})`);
    details = await bidi.json("window.accountIdentityFixture.setHidden(false)");
  } else throw new Error(`Unknown fixture case: ${caseName}`);
  const hiddenExpected = caseName === "hidden-short" || caseName === "hidden-long" ? true
    : ["short", "email-label", "switch-long", "long-label", "localized-label", "unhidden-short"].includes(caseName) ? false : undefined;
  if (hiddenExpected !== undefined && !!details.hidden?.shown !== hiddenExpected) throw new Error(`${widthName}/${caseName}: hidden status mismatch`);
  return { details, image: `${output}/accounts-${widthName}-${caseName}.png` };
}

async function checkAccountFilters(bidi, output) {
  await bidi.viewport(1100);
  await bidi.json("window.accountIdentityFixture.reset()");
  const states = [];
  const read = () => bidi.json(`({ rows: [...document.querySelectorAll('.acct-row')].map(row => row.querySelector('b').textContent),
    selected: document.querySelector('.acct-row[aria-current="true"] b')?.textContent,
    detail: document.querySelector('.acct-title h2')?.textContent })`);
  const expectRows = async (name, rows) => {
    const state = await read();
    if (JSON.stringify(state.rows) !== JSON.stringify(rows)) throw Error(`${name}: ${JSON.stringify(state)}`);
    if (rows.length && state.selected !== state.detail) throw Error(`${name}: selection differs from displayed account`);
    states.push({name, ...state});
  };
  const search = async (value) => {
    await bidi.json(`(() => { const input = document.querySelector('.acct-search'); input.focus(); input.value = ${JSON.stringify(value)}; input.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
    await pause(30);
  };
  const choose = async (selector, label) => {
    await bidi.json(`(() => { document.querySelector(${JSON.stringify(selector)}).click(); return true; })()`);
    await bidi.json(`(() => { const option = [...document.querySelectorAll('[role="option"]')].find(item => item.textContent.trim() === ${JSON.stringify(label)}); if (!option) throw Error('missing option'); option.click(); return true; })()`);
    await pause(30);
  };
  await expectRows('default agent/name order', ['Claude Team', 'Codex Work', 'Kimi Personal']);
  await search('  cOdEx  ');
  await expectRows('case-insensitive search', ['Codex Work']);
  await choose('.acct-provider-filter', 'Kimi');
  await expectRows('combined query/provider gives no matches', []);
  await search('');
  await expectRows('provider filter', ['Kimi Personal']);
  await choose('.acct-filter', 'Workstation');
  await expectRows('combined machine/provider', ['Kimi Personal']);
  await bidi.json(`(() => { [...document.querySelectorAll('.acct-count button')].find(button => button.textContent === 'Clear filters').click(); return true; })()`);
  await expectRows('clear filters retains catalog accounts', ['Claude Team', 'Codex Work', 'Kimi Personal']);
  await bidi.json("window.accountIdentityFixture.setLabel('A personal')");
  await expectRows('default order still groups a renamed account by agent', ['Claude Team', 'Codex Work', 'A personal']);
  await choose('.acct-sort', 'Account name');
  await expectRows('explicit alphabetical sort', ['A personal', 'Claude Team', 'Codex Work']);
  await choose('.acct-sort', 'Agent type, then name');
  await expectRows('restore default order', ['Claude Team', 'Codex Work', 'A personal']);
  await bidi.json('window.accountIdentityFixture.reset()');
  await search('Codex');
  await choose('.acct-provider-filter', 'Kimi');
  await bidi.json("window.accountIdentityFixture.requestProfile('claude')");
  await expectRows('explicit requested account clears hiding filters', ['Claude Team', 'Codex Work', 'Kimi Personal']);
  if ((await read()).selected !== 'Claude Team') throw Error('explicit requested account was not selected');
  const screenshot = `${output}/accounts-list-filters.png`;
  return { states, screenshot, screenshot_sha256: await bidi.capture(screenshot) };
}

const args = process.argv.slice(2);
if (args.length !== 2 || args[0] !== "--output") throw new Error("Usage: account-identity-geometry-smoke.mjs --output NEW_DIRECTORY");
const output = resolve(args[1]);
mkdirSync(output, { recursive: false });
const firefox = execFileSync("sh", ["-c", "command -v firefox"], { encoding: "utf8" }).trim();
const vite = `${CLIENT}/node_modules/.bin/vite`;
if (!firefox || !readFileSync(vite)) throw new Error("Firefox and the client's locked Vite dependency are required");

const source = {
  commit: execFileSync("git", ["rev-parse", "HEAD"], { cwd: ROOT, encoding: "utf8" }).trim(),
  tree: execFileSync("git", ["rev-parse", "HEAD^{tree}"], { cwd: ROOT, encoding: "utf8" }).trim(),
};
const status = execFileSync("git", ["status", "--porcelain"], { cwd: ROOT, encoding: "utf8" });
source.clean = !status;
if (status) source.tracked_diff_sha256 = createHash("sha256").update(execFileSync("git", ["diff", "--binary"], { cwd: ROOT })).digest("hex");
const harness = Object.fromEntries(HARNESS.map((name) => [name, digest(`${ROOT}/${name}`)]));
harness["client/package-lock.json"] = digest(`${CLIENT}/package-lock.json`);

let server, browserProcess, bidi;
const scratch = mkdtempSync(`${tmpdir()}/zerus-account-identity-browser-`);
try {
  for (const name of ["home", "config", "data", "cache", "state", "runtime", "tmp", "profile"]) mkdirSync(`${scratch}/${name}`, { mode: 0o700 });
  const env = {
    ...process.env,
    HOME: `${scratch}/home`, XDG_CONFIG_HOME: `${scratch}/config`, XDG_DATA_HOME: `${scratch}/data`,
    XDG_CACHE_HOME: `${scratch}/cache`, XDG_STATE_HOME: `${scratch}/state`, XDG_RUNTIME_DIR: `${scratch}/runtime`,
    TMPDIR: `${scratch}/tmp`, NO_AT_BRIDGE: "1", GSETTINGS_BACKEND: "memory", MOZ_HEADLESS: "1", LANG: "C.UTF-8",
  };
  const port = await freePort();
  const url = `http://127.0.0.1:${port}/${PAGE}`;
  const viteLog = openSync(`${output}/vite.log`, "w");
  server = spawn(vite, ["--host", "127.0.0.1", "--port", String(port), "--strictPort"], {
    cwd: CLIENT, env, stdio: ["ignore", viteLog, viteLog],
  });
  closeSync(viteLog);
  let served = false;
  for (let attempt = 0; attempt < 100; attempt++) {
    if (server.exitCode !== null) throw new Error("Vite exited before the fixture was ready");
    try {
      const response = await fetch(url);
      if (response.ok) { served = true; break; }
    } catch { /* Vite is still starting. */ }
    await pause(100);
  }
  if (!served) throw new Error("Vite did not serve the Accounts fixture");

  const bidiPort = await freePort();
  const firefoxLog = openSync(`${output}/firefox.log`, "w");
  browserProcess = spawn(firefox, [
    "--no-remote", "--profile", `${scratch}/profile`, "--headless", "--window-size=1100,1000",
    "--remote-debugging-port", String(bidiPort), url,
  ], { cwd: scratch, env, stdio: ["ignore", firefoxLog, firefoxLog] });
  closeSync(firefoxLog);
  bidi = await Bidi.connect(bidiPort);
  const tree = await bidi.call("browsingContext.getTree");
  bidi.context = tree.contexts.find((item) => item.url.includes(PAGE))?.context;
  if (!bidi.context) throw new Error("Accounts fixture context not found");
  await waitReady(bidi);
  const inputs = await bidi.json("window.accountIdentityFixture.inputs()");
  titleLabels = inputs.titleLabels;
  const results = {};

  for (const [widthName, width] of [["wide", 1100], ["narrow", 720]]) {
    await bidi.viewport(width);
    let baseline;
    const captures = [];
    for (const caseName of [...CASES, ...TITLE_CASES]) {
      const { details, image } = await runCase(bidi, widthName, caseName, output);
      checkGeometry(details, baseline, `${widthName}/${caseName}`);
      baseline ??= details;
      details.screenshot = image;
      details.sha256 = await bidi.capture(image);
      captures.push({ case: caseName, geometry: details });
    }
    const keyboard = await bidi.json("window.accountIdentityFixture.machineKeyboard()");
    checkGeometry(keyboard, baseline, `${widthName}/keyboard`);
    results[widthName] = captures;
  }

  const listFilters = await checkAccountFilters(bidi, output);
  await bidi.call("browsingContext.navigate", { context: bidi.context, url: `${url}?lang=ru`, wait: "complete" });
  await bidi.viewport(720);
  await waitReady(bidi);
  let baseline;
  const captures = [];
  for (const caseName of ["loading", "kimi", "codex"]) {
    const { details, image } = await runCase(bidi, "narrow-ru", caseName, output);
    if (details.language !== "ru") throw new Error("Russian fixture did not load its catalog");
    checkGeometry(details, baseline, `narrow-ru/${caseName}`);
    baseline ??= details;
    details.screenshot = image;
    details.sha256 = await bidi.capture(image);
    captures.push({ case: caseName, geometry: details });
  }
  results["narrow-ru"] = captures;

  const failures = await bidi.json("window.accountIdentityFixture.failures()");
  const calls = await bidi.json("window.accountIdentityFixture.calls()");
  if (failures.length) throw new Error(`Fixture failures: ${JSON.stringify(failures)}`);
  writeFileSync(`${output}/provenance.json`, `${JSON.stringify({
    source, harness, inputs, url, firefox: execFileSync(firefox, ["--version"], { encoding: "utf8" }).trim(),
    results, list_filters: listFilters, failures, ipc_calls: calls,
  }, null, 2)}\n`);
  console.log(`PASS ${Object.values(results).flat().length} captures + ${listFilters.states.length} list-filter controls; stable Machines and usage tops; full metadata; no management IPC`);
  console.log(`${output}/provenance.json`);
} finally {
  if (bidi) bidi.ws.close();
  await stop(browserProcess);
  await stop(server);
  rmSync(scratch, { recursive: true, force: true });
}
