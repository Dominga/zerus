// Native WebKit survey for zerus-5wmh.28.2: no finite entrance of the
// stylesheets may present a frame dimmer than its finished state. WebKitGTK
// 2.54.1's Skia compositor painted one frame near an animation's start values
// when the animation ended (fixed in 2.54.2; backported to the arch prefix as
// patch 0005), so every rise, fadeIn, drop, pop, slideL and insp-in blinked
// at its end. Each entrance rule with a plain selector (a tag and classes, or
// a dialog's [open]) and existing keyframes is shown in turn as a magenta box
// on black over column 100; menu-smoke.py --paint-column 100 fails any
// presented frame that dims a box after it was fully painted (`shows`).
// Served by Vite, outside the app entry; no IPC. Run:
//   npx vite --port <free port> --strictPort, then
//   python3 scripts/menu-smoke.py --title "Zerus entrance survey" --paint-column 100 \
//     --url http://127.0.0.1:<port>/scripts/entrance-survey-smoke.html
// in its own private Xvfb by default; for a local WebKit prefix, with
// LD_LIBRARY_PATH and WEBKIT_EXEC_PATH set as packaging/desktop/linux-launcher.py sets them.
import { applySkin, skins } from "../skin/runtime";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import "../ui/styles/goto.css";
import "../ui/styles/screens.css";
import "../ui/styles/machines.css";
import "../ui/styles/settings.css";
import "../ui/styles/tasks.css";
import "../ui/styles/inspector.css";
import "../ui/styles/newsession.css";
import "../ui/styles/motion.css";

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const post = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(v: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(value));
const PLAIN = /^([a-z]+)?((?:\.[\w-]+)+)(\[open\])?$/;
/** Enough entrances that the survey has not silently lost the stylesheets. */
const MINIMUM = 20;

/** Every style rule, also inside layers, media and nesting. */
function* styleRules(rules: CSSRuleList): Generator<CSSStyleRule | CSSKeyframesRule> {
  for (const rule of rules) {
    if (rule instanceof CSSStyleRule || rule instanceof CSSKeyframesRule) yield rule;
    if ("cssRules" in rule && !(rule instanceof CSSKeyframesRule)) yield* styleRules((rule as CSSGroupingRule).cssRules);
  }
}

function entrances() {
  const rules = [...document.styleSheets].flatMap((sheet) => [...styleRules(sheet.cssRules)]);
  const keyframes = new Set<string>(), exits = new Set<string>();
  for (const rule of rules) {
    if (!(rule instanceof CSSKeyframesRule)) continue;
    keyframes.add(rule.name);
    // An exit (insp-out) ends hidden; its element leaves, so it has no finished state to keep.
    const last = rule.findRule("100%") ?? rule.findRule("to");
    if (last?.style.opacity === "0") exits.add(rule.name);
  }
  const found = new Set<string>(), missing = new Set<string>();
  for (const rule of rules) {
    if (!(rule instanceof CSSStyleRule)) continue;
    // A shorthand with var() leaves its longhands unparsed in the CSSOM: read what was declared.
    const tokens = (rule.style.getPropertyValue("animation") || rule.style.animationName).split(/[\s,]+/).filter(Boolean);
    const name = tokens.find((t) => keyframes.has(t)) ?? (/^[a-z][\w-]*$/i.test(tokens[0] ?? "") ? tokens[0] : "");
    if (!name || name === "none" || exits.has(name) || tokens.includes("infinite") || rule.style.animationIterationCount.includes("infinite")) continue;
    for (const selector of rule.selectorText.split(",").map((s) => s.trim()).filter((s) => PLAIN.test(s)))
      (keyframes.has(name) ? found : missing).add(selector);
  }
  return { cases: [...found], missing: [...missing].filter((s) => !found.has(s)) };
}

async function run() {
  await document.fonts.ready;
  applySkin(skins[0]);
  document.documentElement.dataset.platform = "linux";
  const style = document.createElement("style");
  style.textContent = "html, body, #root { background: #000 !important; } #root * { color: transparent !important; border-color: transparent !important; box-shadow: none !important; }";
  document.head.append(style);
  const { cases, missing } = entrances();
  const failures: string[] = [];
  if (cases.length < MINIMUM) failures.push(`only ${cases.length} entrances found`);
  const shows: number[] = [], ends: number[] = [], animations: Record<string, string> = {};
  for (const selector of cases) {
    const [, tag, classes, open] = PLAIN.exec(selector)!;
    const el = document.createElement(tag || "div");
    el.classList.add(...classes.slice(1).split("."));
    if (open) el.setAttribute("open", "");
    for (const [property, value] of Object.entries({ position: "fixed", inset: "auto", left: "40px", top: "100px", width: "300px", height: "200px",
      margin: "0", padding: "0", display: "block", visibility: "visible", "z-index": "10", background: "#ff00ff",
      "max-width": "none", "max-height": "none", "min-width": "0", "min-height": "0" }))
      el.style.setProperty(property, value, "important");
    shows.push(Date.now());
    document.getElementById("root")!.append(el);
    const computed = getComputedStyle(el);
    animations[selector] = `${computed.animationName} ${computed.animationDuration}`;
    if (computed.animationName === "none") failures.push(`${selector}: its entrance did not apply`);
    // The entrance and a margin after it; menu-smoke.py bounds each window by `ends`.
    await sleep(Math.max(350, parseFloat(computed.animationDuration) * 1000 + 250));
    ends.push(Date.now());
    el.remove();
    await sleep(60);
  }
  post({ failures, shows, ends, cases, animations, missing_keyframes: missing });
}
void run().catch((error) => post({ failures: [String(error)] }));
