import { createSignal } from "solid-js";
import { reviewTaskCopy } from "../bridge";
import { tasks } from "./tasks";

export interface CopyReviewReply {
  abi: string; project: { id: string }; uuid: string;
  state: "review" | "moved" | "refused" | "unknown";
  fingerprint?: string; backup?: string; reason?: string;
}
interface Panel { live: boolean; generation: number }
export interface ReviewingCopy { project: string; uuid: string; fingerprint: string; origin: HTMLElement; panel: Panel; generation: number }
export const [reviewingCopy, setReviewingCopy] = createSignal<ReviewingCopy>();
export const [copyReviewBusy, setCopyReviewBusy] = createSignal(false);
export const [copyReviewOutcomes, setCopyReviewOutcomes] = createSignal<Record<string, { state: "moved" | "refused" | "unknown"; reason: string }>>({});
let preparing: { panel: Panel; generation: number; origin: HTMLElement } | undefined;
let confirming: ReviewingCopy | undefined;
const syncBusy = () => setCopyReviewBusy(!!preparing || !!confirming);
function retire(panel: Panel) {
  panel.generation++;
  if (preparing?.panel === panel) preparing = undefined;
  const current = reviewingCopy();
  if (current?.panel === panel && confirming !== current) setReviewingCopy(undefined);
  syncBusy();
}
/** Reads belong to this live panel; confirmed writes settle independently. */
export function createCopyReviewPanel() {
  const panel: Panel = { live: true, generation: 0 };
  return {
    open: (project: string, uuid: string, origin: HTMLElement) => openCopyReview(project, uuid, origin, panel),
    owns: (current: ReviewingCopy | undefined) => panel.live && current?.panel === panel && current.generation === panel.generation,
    retireOrigin: (origin: HTMLElement | undefined) => {
      if (!origin) return;
      if (preparing?.panel === panel && preparing.origin === origin) preparing = undefined;
      const current = reviewingCopy();
      if (current?.panel === panel && current.origin === origin && confirming !== current) setReviewingCopy(undefined);
      syncBusy();
    },
    retire: () => retire(panel),
    dispose: () => { panel.live = false; retire(panel); },
  };
}
const keyOf = (project: string, uuid: string) => `${project}\n${uuid}`;
function outcome(project: string, uuid: string, state: "moved" | "refused" | "unknown", reason = "") {
  const key = keyOf(project, uuid), entries = Object.entries(copyReviewOutcomes()).filter(([k]) => k !== key).slice(-15);
  setCopyReviewOutcomes({ ...Object.fromEntries(entries), [key]: { state, reason } });
}
export const copyReviewOutcome = (project: string, uuid: string) => copyReviewOutcomes()[keyOf(project, uuid)];
export function dismissCopyReview(project: string, uuid: string) {
  const key = keyOf(project, uuid);
  if (copyReviewOutcomes()[key]?.state !== "moved") return;
  const next = { ...copyReviewOutcomes() }; delete next[key]; setCopyReviewOutcomes(next);
}
export function copyReviewCards(project: string, live: { uuid: string; error: string }[]) {
  const finished = Object.keys(copyReviewOutcomes()).filter((key) => key.startsWith(`${project}\n`))
    .map((key) => ({ uuid: key.slice(project.length + 1), error: "" }));
  return [...finished, ...live.filter((row) => !finished.some((done) => done.uuid === row.uuid))].slice(0, 5);
}
function matches(reply: CopyReviewReply, project: string, uuid: string) {
  return reply.abi === "task-copy-review-v1" && reply.project?.id === project && reply.uuid === uuid;
}
async function openCopyReview(project: string, uuid: string, origin: HTMLElement, panel: Panel) {
  if (!panel.live || !origin.isConnected || copyReviewBusy() || reviewingCopy() || ["unknown", "moved"].includes(copyReviewOutcome(project, uuid)?.state ?? "")) return;
  const request = { panel, generation: panel.generation, origin };
  preparing = request; syncBusy();
  const current = () => preparing === request && panel.live && panel.generation === request.generation && origin.isConnected && tasks.project() === project;
  try {
    const reply = await reviewTaskCopy(project, uuid);
    if (!current()) return;
    if (!matches(reply, project, uuid) || reply.state !== "review" || !/^[0-9a-f]{64}$/.test(reply.fingerprint ?? "")) throw new Error(reply.reason || "Invalid local task-copy review.");
    setReviewingCopy({ project, uuid, fingerprint: reply.fingerprint!, origin, panel, generation: request.generation });
  } catch (error) { if (current()) outcome(project, uuid, "refused", String(error)); }
  finally { if (preparing === request) { preparing = undefined; syncBusy(); } }
}
export async function confirmCopyReview() {
  const current = reviewingCopy();
  if (!current || copyReviewBusy()) return;
  if (!current.panel.live || current.generation !== current.panel.generation || !current.origin.isConnected) { setReviewingCopy(undefined); return; }
  confirming = current; syncBusy();
  try {
    const reply = await reviewTaskCopy(current.project, current.uuid, current.fingerprint);
    if (!matches(reply, current.project, current.uuid) || !["moved", "refused", "unknown"].includes(reply.state)) throw new Error("Invalid local task-copy review outcome.");
    outcome(current.project, current.uuid, reply.state as "moved" | "refused" | "unknown", reply.reason || "");
  } catch (error) { outcome(current.project, current.uuid, "unknown", String(error)); }
  finally { if (confirming === current) confirming = undefined; if (reviewingCopy() === current) setReviewingCopy(undefined); syncBusy(); }
}
