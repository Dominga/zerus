import { For, createMemo, createSignal, onCleanup, onMount, type JSX } from "solid-js";
import { tr } from "../../i18n";
import { setPage } from "../../store/ui";
import { Icon } from "../lib/icons";
import { progress, stateLabel, stateTone } from "./team-model";
import { TeamBarActions } from "./TeamActions";
import type { TeamView } from "./types";

// The fork's strip, adapted to queue teams. Narrow strips fold the action row
// below the counts; every operation remains reachable.

/** The strip's fold thresholds, on the strip's own width. */
export const compactFor = (width: number): "none" | "low" | "high" => (width < 900 ? "high" : width < 1100 ? "low" : "none");

export function TeamBar(props: {
  view: TeamView;
  busy: boolean;
  onAction: (kind: string) => void;
  /** The strip's width (tests); the strip's own measured width otherwise. */
  width?: number;
  drawers?: JSX.Element;
}) {
  const counts = createMemo(() => progress(props.view));
  const [measured, setMeasured] = createSignal(0);
  const [viewport, setViewport] = createSignal(typeof window === "undefined" ? 1200 : window.innerWidth);
  let bar: HTMLElement | undefined;
  onMount(() => {
    const resize = () => setViewport(window.innerWidth);
    window.addEventListener("resize", resize);
    onCleanup(() => window.removeEventListener("resize", resize));
    if (!bar) return;
    const observe = () => setMeasured(Math.round(bar!.getBoundingClientRect().width));
    observe();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => observe());
    observer.observe(bar);
    onCleanup(() => observer.disconnect());
  });
  const width = () => props.width ?? (measured() || viewport());
  const compact = (): "none" | "low" | "high" => compactFor(width());
  const segments = () => [
    { key: "accepted", n: counts().accepted },
    { key: "reported", n: counts().reported },
    { key: "active", n: counts().active },
    { key: "waiting", n: counts().waiting },
    { key: "rest", n: counts().total - counts().accepted - counts().reported - counts().active - counts().waiting },
  ].filter((segment) => segment.n > 0);
  const countsTip = () => [tr("Accepted %1", counts().accepted), tr("In review %1", counts().reported),
    tr("In progress %1", counts().active), tr("Waiting %1", counts().waiting)].join(", ");
  return (
    <header class="team-bar" ref={bar} classList={{ "team-bar-compact": compact() !== "none" }} data-compact={compact()} data-strip data-team-bar>
      <Icon name="users" size={16} />
      <b class="team-name ell" title={props.view.bar.goal || props.view.bar.name}>{props.view.bar.name}</b>
      <span class={`tm-state-badge tone-${stateTone(props.view.bar.status)}`} data-state={props.view.bar.status}>{stateLabel(props.view.bar.status)}</span>
      <div class="team-progress" role="img" aria-label={countsTip()} title={countsTip()}>
        <For each={segments()}>{(segment) => <i class={`seg ${segment.key}`} style={{ "flex-grow": String(segment.n) }} />}</For>
      </div>
      <span class="mono team-count">{counts().accepted}/{counts().total}</span>
      <span class="team-chips" data-strip-chips>
        <span class="chip team-chip" title={tr("Active members")}><Icon name="users" size={12} />{tr("%1 active", props.view.bar.counts.members.active)}</span>
        <span class="chip team-chip" title={tr("Open inbox items")}><Icon name="inbox" size={12} />{props.view.bar.counts.inbox === 1 ? tr("%n inbox item", 1) : tr("%n inbox items", props.view.bar.counts.inbox)}</span>
        <span class="chip team-chip" title={tr("Open cards")}><Icon name="layers" size={12} />{props.view.bar.counts.cards === 1 ? tr("%n card", 1) : tr("%n cards", props.view.bar.counts.cards)}</span>
      </span>
      <button type="button" class="btn ghost small" onClick={() => setPage("team-catalog")}>{tr("Catalog")}</button>
      {props.drawers}
      <TeamBarActions view={props.view} busy={props.busy} onAction={props.onAction} />
    </header>
  );
}
