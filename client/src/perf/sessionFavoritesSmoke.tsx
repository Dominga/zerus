// Private native WebKit fixture for zerus-ux18t. No hgs or native action runs.
import { render } from "solid-js/web";
import { SessionList } from "../ui/sessions/SessionList";
import { MenuLayer } from "../ui/components/Overlays";
import { applyBatch, catalog } from "../store/catalog";
import { applyMachines } from "../store/machines";
import { applyGrouping, emptyGrouping } from "../store/organization";
import { favorite, setFavorite } from "../store/favorites";
import { settingsReady, setSetting } from "../store/settings";
import { setTeamCollapsed } from "../store/teamCollapse";
import { listRows, filterCounts } from "../ui/sessions/model";
import { setStateFilter, setStripMode, setSessionsWidth, setMachineFilter, setProjectFilter, setAccountFilter, setFolderFilter } from "../store/ui";
import { emptyBatch, type ViewRow } from "../types";
import { applySkin, skins } from "../skin/runtime";
import { tr, uiLanguage, installUiLanguage } from "../i18n";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/sessions.css";
import "../ui/styles/client.css";
import "../ui/styles/detail.css";
import "../ui/styles/motion.css";

const params = new URLSearchParams(location.search), restarted = params.has("restart");
const MACHINE = "11111111-1111-4111-8111-111111111111";
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const key = (n: number) => `\nrun:favorite-${n}`;
const favoriteKey = restarted ? "\nrun:renamed" : key(2);
const failures: string[] = JSON.parse(sessionStorage.getItem("favorite-smoke-failures") ?? "[]");
const check = (condition: unknown, message: string) => { if (!condition) failures.push(message); };
// An offscreen WKWebView may suspend rAF; timers still let its DOM settle.
const frame = () => new Promise<void>((resolve) => {
  const timer = setTimeout(resolve, 100);
  requestAnimationFrame(() => requestAnimationFrame(() => { clearTimeout(timer); resolve(); }));
});
const cards = () => listRows().flatMap((r) => r.type === "card" ? [r.id] : []);
const raw = (n: number, extra: Partial<ViewRow> = {}): ViewRow => ({ name: `codex/example/session-${n}`, title: `Example session ${n}`,
  agent: "codex", session_id: id(n), run_id: `favorite-${n}`, conversation_id: `conversation-${n}`, state: "running", status: "ready", ...extra });
function group(collapsed: boolean) {
  applyGrouping({ ...emptyBatch(), grouping: { ...emptyGrouping(), loaded: true, ready: true, default_project: "example", headers: [
    { id: "example", name: "Example project", palette: 0, color: null, accessible: true, is_default: true, collapsed },
    { id: "other", name: "Other project", palette: 1, color: null, accessible: true, is_default: false, collapsed: false },
  ] } });
}
async function choose(label: string) {
  const hit = document.querySelector<HTMLElement>(`.vrow[data-key="${CSS.escape(favoriteKey)}"] .rowhit`);
  check(hit, `favorite row has a real context target: ${label}`);
  hit?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 120 }));
  await frame();
  const button = [...document.querySelectorAll<HTMLButtonElement>(".menu-item")].find((node) => node.textContent?.trim() === label);
  check(button && button.getAttribute("aria-disabled") !== "true", `usable translated menu item: ${label}`);
  button?.click(); await frame();
}
async function run() {
  installUiLanguage();
  await settingsReady;
  if (restarted) for (let i = 0; uiLanguage() !== "ru" && i < 50; i++) await frame();
  applySkin(skins[0]); setStripMode(false); setSessionsWidth(Number(params.get("width") ?? 640)); setStateFilter("all");
  if (params.get("scale") === "150") document.documentElement.style.zoom = "1.5";
  const link = { machine: MACHINE, session_id: id(1), conversation_id: "conversation-1", run_id: "favorite-1", name: raw(1).name, state: "saved" as const };
  applyMachines({ ...emptyBatch(), machines: [{ computer: "", machine: { machine_id: MACHINE } as never }] });
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], replaces: [{ computer: "", rows: [
    { computer: "", key: "run:favorite-1", row: raw(1, { state: "paused", status: "paused" }) },
    { computer: "", key: favoriteKey.slice(1), row: raw(2, { parent: link, ...(restarted ? { name: "codex/renamed/child", title: "Renamed favorite child" } : {}) }) },
    { computer: "", key: "run:favorite-3", row: raw(3, { parent: link }) },
  ] }] });
  group(false);
  render(() => <><style>{"#root { display:flex; height:100vh; } .sessions-pane { width:100%; max-width:100%; }"}</style><SessionList /><MenuLayer /></>, document.getElementById("root")!);
  await frame(); await frame();
  if (!restarted) {
    setTeamCollapsed(key(1), false); await frame();
    await choose(tr("Add to favorites"));
    check(favorite(catalog.rows[favoriteKey]), "add from independent child menu");
    setTeamCollapsed(key(1), true); group(true); await frame();
    check(JSON.stringify(cards()) === JSON.stringify([key(1), favoriteKey]), "both collapsed: only parent and favorite");
    sessionStorage.setItem("favorite-smoke-failures", JSON.stringify(failures));
    await setSetting("workspace/language", "ru");
    params.set("restart", "1"); location.search = params.toString(); return;
  }
  check(favorite(catalog.rows[favoriteKey]), "fresh module/settings load preserves preference across rename and row-key change");
  setTeamCollapsed(key(1), true); group(true); await frame();
  const counts = JSON.stringify(filterCounts());
  check(document.querySelectorAll(".sc-favorite").length === 1, "one small star, no duplicate favorite");
  await choose(tr("Remove from favorites"));
  check(cards().length === 0, "removal under both collapsed levels hides row immediately");
  check(JSON.stringify(filterCounts()) === counts, "favorites do not change filter counts");
  setFavorite(catalog.rows[favoriteKey], true); await frame();
  for (const [set, reset] of [
    [() => setStateFilter("paused"), () => setStateFilter("all")],
    [() => setMachineFilter(["absent"]), () => setMachineFilter([])],
    [() => setProjectFilter(["other"]), () => setProjectFilter([])],
    [() => setAccountFilter(["absent"]), () => setAccountFilter([])],
    [() => setFolderFilter({ computer: "", path: "/example/missing", style: "posix", checkout: false }), () => setFolderFilter(undefined)],
  ]) { set(); check(!cards().includes(favoriteKey), "ordinary filter still hides favorite"); reset(); }
  applyBatch({ ...emptyBatch(), upserts: [{ computer: "", key: "run:favorite-2", row: raw(2, { parent: link, session_id: id(20) }) }] });
  check(!favorite(catalog.rows[key(2)]), "same old name/key with another session identity never inherits favorite");
  check(new Set(listRows().map((r) => r.id)).size === listRows().length, "no duplicate row identities");
  await setSetting("workspace/language", "ru"); await frame(); await frame();
  for (let i = 0; uiLanguage() !== "ru" && i < 50; i++) await frame();
  check(uiLanguage() === "ru", "Russian catalog loaded");
  await choose(tr("Remove from favorites"));
  setFavorite(catalog.rows[favoriteKey], true); await frame();
  const stars = [...document.querySelectorAll<HTMLElement>(".sc-favorite")].map((el) => ({ label: el.getAttribute("aria-label"), width: el.getBoundingClientRect().width }));
  check(stars.length === 1 && stars[0].width > 0, "native star remains visible at the selected scale");
  const pane = document.querySelector<HTMLElement>(".sessions-pane")!;
  const result = { failures, restarted, width: innerWidth, scale: params.get("scale") ?? "100", stars,
    rows: cards(), mounted: document.querySelectorAll(".vrow").length, paneWidth: pane.getBoundingClientRect().width,
    bodyWidth: document.body.clientWidth, bodyScroll: document.body.scrollWidth, source: "actual SessionList, MenuLayer, settings and grouping; synthetic ViewRows; native reads unavailable" };
  (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify(result));
}
void run().catch((error) => (window as unknown as { webkit: { messageHandlers: { result: { postMessage(text: string): void } } } }).webkit.messageHandlers.result.postMessage(JSON.stringify({ failures: [...failures, String(error)] })));
