// Synthetic browser coverage: no CLI, installed app or user data. Run from client:
// node scripts/team-help-smoke.mjs ../artifacts/team-help-review
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve, join } from "node:path";
import { createServer } from "vite";
import solid from "vite-plugin-solid";

const out = resolve(process.argv[2] || "../artifacts/team-help-review");
mkdirSync(out, { recursive: true });
const chrome = process.argv[3] || "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";
const server = await createServer({ configFile: false, root: resolve(import.meta.dirname, ".."), logLevel: "error",
  server: { port: 0, host: "127.0.0.1" }, plugins: [solid()] });
await server.listen();
const port = server.httpServer.address().port;
const debug = 9600 + Math.floor(Math.random() * 300);
const browser = spawn(chrome, ["--headless=new", `--remote-debugging-port=${debug}`, `--user-data-dir=${join(out, "chrome-profile")}`,
  "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--force-device-scale-factor=1", "about:blank"], { stdio: "ignore", windowsHide: true });
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));
let socket;
const summary = [];
try {
  let tabs;
  for (let n = 0; n < 100 && !tabs; n++) {
    try { tabs = await (await fetch(`http://127.0.0.1:${debug}/json/list`)).json(); } catch { await sleep(100); }
  }
  assert.ok(tabs, "Chrome started");
  socket = new WebSocket(tabs.find((tab) => tab.type === "page").webSocketDebuggerUrl);
  await new Promise((done) => { socket.onopen = done; });
  const pending = new Map();
  const exceptions = [];
  socket.onmessage = (message) => {
    const data = JSON.parse(message.data);
    if (data.id) { const done = pending.get(data.id); pending.delete(data.id); done?.(data); }
    else if (data.method === "Runtime.exceptionThrown") exceptions.push(data.params.exceptionDetails.text);
  };
  let id = 0;
  const send = async (method, params = {}) => {
    const next = ++id;
    const answer = new Promise((done) => pending.set(next, done));
    socket.send(JSON.stringify({ id: next, method, params }));
    const result = await answer;
    if (result.error) throw new Error(JSON.stringify(result.error));
    return result.result;
  };
  const evaluate = async (expression) => {
    const result = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
    assert.equal(result.exceptionDetails, undefined, expression);
    return result.result.value;
  };
  const wait = async (expression) => {
    for (let n = 0; n < 200; n++) { if (await evaluate(expression)) return; await sleep(100); }
    throw new Error(`Timed out: ${expression}`);
  };
  const shot = async (name) => {
    await sleep(150);
    const png = await send("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(out, `${name}.png`), Buffer.from(png.data, "base64"));
  };
  await send("Page.enable");
  await send("Runtime.enable");
  for (const sample of [
    { name: "english-dark", locale: "en-US", lang: "en", skin: "whitewh-dark", width: 1440, height: 1000 },
    { name: "russian-dark", locale: "ru-RU", lang: "ru", skin: "whitewh-dark", width: 1440, height: 1000 },
    { name: "english-light", locale: "en-GB", lang: "en", skin: "whitewh-light", width: 1280, height: 900 },
    { name: "russian-narrow", locale: "ru-RU", lang: "ru", skin: "whitewh-light", width: 520, height: 850 },
  ]) {
    await send("Emulation.setDeviceMetricsOverride", { width: sample.width, height: sample.height, deviceScaleFactor: 1, mobile: false });
    await send("Page.navigate", { url: `http://127.0.0.1:${port}/scripts/new-team-smoke.html?case=new&locale=${sample.locale}&skin=${sample.skin}` });
    await wait("document.title.endsWith(': ready') && document.querySelectorAll('.tg-inline [data-guide-chapter]').length === 11");
    assert.equal(await evaluate("document.querySelector('.tg-inline').lang"), sample.lang);
    assert.equal(await evaluate("document.documentElement.scrollWidth <= innerWidth"), true, "no window overflow");
    await shot(`${sample.name}-new-team`);
    await evaluate("document.querySelector('.tg-inline [data-guide-chapter=broker]').click()");
    assert.match(await evaluate("document.querySelector('.tg-inline .tg-article').textContent"), /snapshot|снимка/);
    await evaluate("const search = document.querySelector('.tg-inline input[type=search]'); search.value = 'request_id'; search.dispatchEvent(new Event('input', {bubbles:true}))");
    assert.equal(await evaluate("document.querySelectorAll('.tg-inline [data-guide-chapter]').length"), 1);
    assert.match(await evaluate("document.querySelector('.tg-inline .tg-article').textContent"), /cargo-test/);
    await evaluate("document.querySelector('.tg-inline .tg-cookie button').click()");
    assert.match(await evaluate("document.querySelector('.tg-inline .tg-cookie [role=status]').textContent"), /cookie|печеньку/);
    await evaluate("document.querySelector('.nt-head [data-team-help]').focus(); document.querySelector('.nt-head [data-team-help]').click()");
    await wait("document.querySelector('dialog.team-help-dialog[open] [data-guide-chapter=profile]')");
    assert.equal(await evaluate("!!document.activeElement.closest('dialog.team-help-dialog')"), true);
    await evaluate("document.querySelector('dialog [data-guide-chapter=profile]').click()");
    assert.match(await evaluate("document.querySelector('dialog .tg-article').textContent"), /Overrun/);
    await shot(`${sample.name}-help`);
    await send("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    await send("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
    await wait("!document.querySelector('dialog.team-help-dialog[open]')");
    assert.equal(await evaluate("document.activeElement === document.querySelector('.nt-head [data-team-help]')"), true, "Escape restores focus");
    summary.push({ ...sample, chapters: 11, search: "passed", modalAndFocus: "passed", cookie: "passed" });
  }
  assert.deepEqual(exceptions, []);
  writeFileSync(join(out, "results.json"), JSON.stringify(summary, null, 2));
  console.log(JSON.stringify(summary, null, 2));
} finally {
  socket?.close();
  browser.kill();
  await server.close();
}
