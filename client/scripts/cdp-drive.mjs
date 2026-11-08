// Task 205: drive a running Zerus Next window over CDP only (no OS input, no
// screen capture) and save one screenshot per step of a plan. The app is the
// development-identity build started by tauri-launch with
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>
// so this script only attaches; it starts and stops nothing.
//   node scripts/cdp-drive.mjs <port> <plan.json> <output folder>
// plan.json: { "width"?: n, "height"?: n, "steps": [ { "name": "...",
//   "eval"?: "<expression run in the page; may return a promise>",
//   "until"?: "<expression that turns truthy when the step is ready>",
//   "wait"?: ms, "probe"?: "<expression whose JSON value is kept>",
//   "shot"?: false } ] }
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const [portText, planPath, outArg] = process.argv.slice(2);
if (!portText || !planPath) {
  console.error("usage: node scripts/cdp-drive.mjs <port> <plan.json> <output folder>");
  process.exit(2);
}
const port = Number(portText);
const plan = JSON.parse(readFileSync(planPath, "utf8"));
const out = resolve(outArg || "cdp-shots");
mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

async function json(path) {
  for (let i = 0; i < 150; i++) {
    try { return await (await fetch(`http://127.0.0.1:${port}${path}`)).json(); } catch { await sleep(200); }
  }
  throw new Error(`the debugging port ${port} did not answer`);
}

let id = 0;
function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  socket.onmessage = (message) => {
    const data = JSON.parse(message.data);
    if (data.id && pending.has(data.id)) { pending.get(data.id)(data); pending.delete(data.id); }
  };
  const ready = new Promise((done, fail) => { socket.onopen = done; socket.onerror = fail; });
  const send = async (method, params = {}) => {
    await ready;
    const n = ++id;
    socket.send(JSON.stringify({ id: n, method, params }));
    const answer = await new Promise((done) => pending.set(n, done));
    if (answer.error) throw new Error(`${method}: ${answer.error.message}`);
    return answer.result;
  };
  return { send, close: () => socket.close() };
}

async function evaluate(cdp, expression) {
  const result = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  return result.result.value;
}

// The app's own window: a page target that is not DevTools or about:blank.
const targets = await json("/json/list");
const target = targets.find((page) => page.type === "page" && !page.url.startsWith("devtools:") && page.url !== "about:blank")
  ?? targets.find((page) => page.type === "page");
if (!target) throw new Error("no page target on the debugging port");
const cdp = connect(target.webSocketDebuggerUrl);
await cdp.send("Page.enable");
await cdp.send("Runtime.enable");
if (plan.width && plan.height)
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: plan.width, height: plan.height, deviceScaleFactor: 1, mobile: false });

const shots = [];
try {
  for (const step of plan.steps) {
    const entry = { name: step.name };
    try {
      if (step.eval) entry.eval = await evaluate(cdp, step.eval);
      if (step.until) {
        let ok = false;
        for (let i = 0; i < 100 && !ok; i++) { ok = !!(await evaluate(cdp, step.until)); if (!ok) await sleep(100); }
        entry.ready = ok;
      }
      await sleep(step.wait ?? 500);
      if (step.probe) entry.probe = await evaluate(cdp, `JSON.stringify(${step.probe})`).then((text) => JSON.parse(text ?? "null"));
      if (step.shot !== false) {
        const png = await cdp.send("Page.captureScreenshot", { format: "png" });
        entry.file = join(out, `${step.name}.png`);
        writeFileSync(entry.file, Buffer.from(png.data, "base64"));
      }
    } catch (error) {
      entry.error = String(error.message ?? error);
    }
    shots.push(entry);
    console.log(`${entry.name}: ${entry.error ? "ERROR " + entry.error : entry.file ?? "no shot"} ${entry.probe ? JSON.stringify(entry.probe) : ""}`);
  }
} finally {
  if (plan.width && plan.height) await cdp.send("Emulation.clearDeviceMetricsOverride").catch(() => {});
  cdp.close();
}
writeFileSync(join(out, "shots.json"), JSON.stringify({ target: target.url, shots }, null, 1));
