// Actual browser geometry for the production TeamWorkspace. Native calls are
// unavailable; action/read callbacks only record local test arguments.
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import { createComponent, createSignal } from "solid-js";
import { render } from "solid-js/web";
import { TeamWorkspace } from "../ui/teams/TeamTab";
import { workspaceLayoutView } from "../ui/teams/fixtures/layout";
import { applySkin, skins } from "../skin/runtime";
import { setOpenTeamNode } from "../ui/teams/openNode";

applySkin(skins[0]);
document.documentElement.dataset.motion = "off";
const host = document.getElementById("fixture")!;
const [view, setView] = createSignal(workspaceLayoutView());
const operations: unknown[] = [], pages: unknown[] = [];
render(() => createComponent(TeamWorkspace, {
  get view() { return view(); }, busy: false, failure: null,
  onAction: (...args) => operations.push(args), onPage: (cursor) => pages.push(cursor),
  onAddTask: () => operations.push("add"),
  onSettingsSave: async () => { operations.push("save"); throw Error("Read-only layout fixture"); },
}), host);
const checks: { name: string; ok: boolean }[] = [], failures: string[] = [], measurements: unknown[] = [];
const check = (name: string, ok: unknown) => { checks.push({ name, ok: !!ok }); if (!ok) failures.push(name); };
const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 80));
const near = (a: number, b: number) => Math.abs(a - b) <= 2;
const pick = <T extends Element = HTMLElement>(selector: string) => host.querySelector<T>(selector)!;
const button = (label: string) => [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label)!;
const box = (element: Element) => { const r = element.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height, right: r.right, bottom: r.bottom }; };
const detail = () => host.querySelector<HTMLElement>("[data-team-detail]");
const close = () => pick<HTMLButtonElement>('[data-team-detail] button[aria-label="Close"]');
const key = (element: Element, init: KeyboardEventInit = {}) => element.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true, ...init }));
const clickCard = (node = 1) => { const card = pick<HTMLButtonElement>(`.team-task-card[data-node="${node}"]`); card.focus({ preventScroll: true }); card.click(); return card; };
const emit = (value: unknown) => (window as unknown as { webkit: { messageHandlers: { proof: { postMessage: (message: string) => void } } } }).webkit.messageHandlers.proof.postMessage(JSON.stringify(value));
const resumes = new Map<string, () => void>();
const snapshot = (name: string) => new Promise<void>((resolve, reject) => {
  const timeout = setTimeout(() => reject(Error(`Snapshot ${name} timed out`)), 15000);
  resumes.set(name, () => { clearTimeout(timeout); resumes.delete(name); resolve(); });
  emit({ type: "snapshot", name });
});
(window as unknown as { teamsLayoutProof: { resume: (name: string) => void } }).teamsLayoutProof = { resume: (name) => resumes.get(name)?.() };
async function size(width: number, height: number) { host.style.width = `${width}px`; host.style.height = `${height}px`; await pause(); }
function geometry(name: string) {
  const board = pick<HTMLElement>("[data-team-board]"), canvas = pick<HTMLElement>(".team-canvas"), workspace = pick<HTMLElement>("[data-team-tab]");
  const b = box(board), c = box(canvas), w = box(workspace), toolbar = box(pick(".team-toolbar"));
  const style = getComputedStyle(board);
  const nativeGutter = board.offsetHeight - board.clientHeight - parseFloat(style.borderTopWidth || "0") - parseFloat(style.borderBottomWidth || "0");
  const record = { name, outerWidth: window.innerWidth, workspace: w, board: b, canvas: c, toolbar,
    boardClient: [board.clientWidth, board.clientHeight], boardScroll: [board.scrollWidth, board.scrollHeight],
    offsets: [board.scrollLeft, board.scrollTop], horizontalNativeGutter: nativeGutter,
    boardOverflow: [style.overflowX, style.overflowY], canvasOverflow: getComputedStyle(canvas).overflow,
    workspaceOverflow: workspace.scrollHeight - workspace.clientHeight };
  measurements.push(record);
  check(`${name}: Board viewport reaches available bottom`, near(b.bottom, c.bottom));
  check(`${name}: Board viewport fills available height`, near(b.height, c.height));
  check(`${name}: toolbar stays inside workspace`, toolbar.bottom <= w.bottom + 2);
  check(`${name}: workspace height does not grow with cards/detail`, workspace.scrollHeight <= workspace.clientHeight + 2);
  return record;
}
async function run() {
  try {
    await document.fonts.ready; await size(1100, 650);
    const short = geometry("wide-short");
    check("wide short has real horizontal overflow", short.boardScroll[0] > short.boardClient[0]);
    check("sparse cards leave space within the viewport", pick<HTMLElement>(".team-column-cards").getBoundingClientRect().bottom < short.board.bottom - 100);
    check("sparse Board does not need vertical scrolling", short.boardScroll[1] <= short.boardClient[1] + 2);
    await snapshot("wide-short");
    clickCard(); await pause();
    const selected = geometry("wide-selected");
    check("detail is in the right inspector", !!detail()?.closest("[data-team-inspector]") && !pick(".team-canvas").contains(detail()));
    check("wide selection keeps Board region stable", near(short.board.width, selected.board.width) && near(short.board.height, selected.board.height));
    await snapshot("wide-selected");
    if (new URLSearchParams(location.search).get("scope") === "baseline") return;

    close().click(); await pause();
    setView(workspaceLayoutView("many")); await pause();
    const many = geometry("many-cards"); const board = pick<HTMLElement>("[data-team-board]");
    check("many cards scroll vertically inside Board", board.scrollHeight > board.clientHeight + 500);
    board.scrollTop = 240; board.scrollLeft = 100; await pause();
    check("real Board scroll offsets move on both axes", board.scrollTop > 0 && board.scrollLeft > 0);
    const before = [board.scrollLeft, board.scrollTop]; const sameBoard = board;
    clickCard(); await pause(); close().click(); await pause();
    check("selection and Close preserve Board node and offsets", pick("[data-team-board]") === sameBoard && before[0] === board.scrollLeft && before[1] === board.scrollTop);
    geometry("many-after-close"); await snapshot("many-scrolled");
    check("long columns keep the same available viewport", near(many.board.height, box(board).height));

    setView(workspaceLayoutView("long-detail")); await pause(); clickCard(); await pause();
    const body = pick<HTMLElement>(".team-task-detail-body"); body.scrollTop = body.scrollHeight; await pause();
    check("long task detail has its own real scrolling region", body.scrollHeight > body.clientHeight && body.scrollTop > 0);
    check("detail scroll does not move its Close offscreen", box(close()).bottom <= box(pick("[data-team-inspector]")).bottom);
    geometry("long-detail"); await snapshot("long-detail"); close().click(); await pause();

    setView(workspaceLayoutView()); await size(650, 560);
    const narrow = geometry("narrow-unselected");
    check("narrow workspace is tested within a wide outer window", window.innerWidth >= 1200 && box(host).width <= 650);
    const trigger = clickCard(); await pause();
    const pane = pick<HTMLElement>("[data-team-inspector]"); const n = geometry("narrow-selected");
    check("container-narrow task inspector is an overlay", pane.getAttribute("role") === "dialog" && pane.hasAttribute("data-team-inspector-overlay"));
    check("overlay does not resize Board", near(narrow.board.width, n.board.width) && near(narrow.board.height, n.board.height));
    check("overlay receives keyboard focus", document.activeElement === close());
    await snapshot("narrow-overlay"); close().click(); await pause();
    check("Close returns focus without scrolling", document.activeElement === trigger);
    const members = pick<HTMLButtonElement>("[data-team-members-toggle]"); members.focus(); members.click(); await pause();
    check("Members remain accessible with no selection", pick<HTMLElement>("[data-team-members]").hidden === false && pane.hasAttribute("data-team-inspector-overlay"));
    const memberClose = pick<HTMLButtonElement>('[data-team-members] button[aria-label="Close"]'); key(memberClose); await pause();
    check("Members overlay Escape returns focus", document.activeElement === members && !pane.hasAttribute("data-team-inspector-overlay"));

    clickCard(); await pause(); pick<HTMLButtonElement>('[data-task-action="return"]').click();
    const input = pick<HTMLInputElement>(".team-reason input"); input.value = "Unsent synthetic reason"; input.dispatchEvent(new Event("input", { bubbles: true })); input.focus();
    key(input); check("editable input owns Escape", !!detail());
    key(close(), { isComposing: true }); check("composing Escape is not stolen", !!detail());
    const menu = document.createElement("div"); menu.setAttribute("role", "menu"); document.body.append(menu); key(close());
    check("external menu owns Escape", !!detail()); menu.remove();
    const modal = document.createElement("dialog"); document.body.append(modal); modal.showModal(); key(close());
    check("external native modal owns Escape", !!detail()); modal.close(); modal.remove(); input.focus();
    await size(1100, 560); check("resize to split inspector preserves edited focus", document.activeElement === input);
    await size(650, 560); check("resize to overlay preserves edited focus", document.activeElement === input && input.value === "Unsent synthetic reason");
    close().focus(); key(close()); await pause(); check("overlay Escape dismisses and returns focus", !detail() && document.activeElement === trigger);

    await size(650, 310); geometry("short-height"); clickCard(); await pause();
    check("short-height overlay keeps Close in bounds", box(close()).bottom <= box(host).bottom && box(close()).y >= box(host).y);
    await snapshot("short-height"); close().click(); await pause();
    setView(workspaceLayoutView("empty")); await pause(); geometry("empty-queue"); await snapshot("empty-queue");

    setView(workspaceLayoutView()); await size(650, 560); button("Graph").click(); await pause();
    pick<HTMLButtonElement>('[data-plan-task="1"] .team-plan-card').click(); await pause();
    const graph = pick<HTMLElement>("[data-team-graph]"); graph.querySelector<HTMLButtonElement>(".team-plan-port.output")!.click(); key(graph);
    check("graph gesture owns first Escape", !!detail() && !graph.querySelector('[role="status"] button'));
    key(graph); await pause(); check("second graph Escape dismisses inspector", !detail());
    button("Board").click(); await pause(); clickCard(); await pause(); pick("[data-team-board]").dispatchEvent(new MouseEvent("click", { bubbles: true, button: 0 }));
    check("blank Board dismissal stays local", !detail());
    const base = view(); const stage = base.groups.flatMap((group) => group.cards).find((card) => card.node === 1)!.stages[0];
    setOpenTeamNode({ team: base.bar.id, node: stage.node }); await pause();
    check("programmatic stage Open task retains its parent and focus marker", !!detail() && !!host.querySelector(`[data-stage-focus][aria-current="true"]`));
    close().click(); await pause(); setView(workspaceLayoutView()); await pause(); check("same-team refresh does not reopen dismissed inspector", !detail());
    check("all layout/dismissal controls cause zero task mutations or reads", operations.length === 0 && pages.length === 0);
    clickCard(); await pause(); pick<HTMLButtonElement>('[data-task-action="accept"]').click();
    check("real Accept button keeps its exact callback", JSON.stringify(operations) === JSON.stringify([["accept", { node: 1 }]]));
    pick<HTMLButtonElement>('[data-task-action="return"]').click();
    const reason = pick<HTMLInputElement>(".team-reason input"); reason.value = "Synthetic follow-up"; reason.dispatchEvent(new Event("input", { bubbles: true })); button("Send").click();
    check("real Return flow keeps its exact callback", JSON.stringify(operations.at(-1)) === JSON.stringify(["return", { node: 1, reason: "Synthetic follow-up" }]));
  } catch (error) { failures.push(String(error)); }
  finally { emit({ type: "result", ok: failures.length === 0, label: "readonly production TeamWorkspace with product-derived bounded synthetic views", engine: navigator.userAgent, checks, failures, measurements, operations, pages }); }
}
void run();
