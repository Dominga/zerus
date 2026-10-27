// Board motion, compositor-only: FLIP glides for reordered cards (translate),
// a ghost that flies from a card to its new stage tile (translate + scale +
// opacity) and a tile bump (scale). Everything is skipped for reduced motion.

export const reduced = () =>
  matchMedia("(prefers-reduced-motion: reduce)").matches || document.documentElement.dataset.motion === "off";

const EASE = "cubic-bezier(.2,.8,.2,1)";

/** Records card positions now; the returned function animates them to where they land. */
export function capture(): () => void {
  if (reduced()) return () => {};
  const before = new Map<string, number>();
  for (const el of document.querySelectorAll<HTMLElement>(".tk-card[data-task]")) before.set(el.dataset.task!, el.getBoundingClientRect().top);
  return () => queueMicrotask(() => {
    for (const el of document.querySelectorAll<HTMLElement>(".tk-card[data-task]")) {
      const top = before.get(el.dataset.task!);
      if (top === undefined) continue;
      const dy = top - el.getBoundingClientRect().top;
      if (Math.abs(dy) < 1) continue;
      el.animate([{ translate: `0 ${dy}px` }, { translate: "0 0" }], { duration: 260, easing: EASE });
    }
  });
}

/** A small copy of the card travels to the stage tile it now belongs to. */
export function flyToTile(id: string, stage: string) {
  if (reduced()) return;
  const card = document.querySelector<HTMLElement>(`.tk-card[data-task="${CSS.escape(id)}"]`);
  const tile = document.querySelector<HTMLElement>(`.tk-tile[data-tile="${stage}"]`);
  if (!card || !tile) return;
  const from = card.getBoundingClientRect(), to = tile.getBoundingClientRect();
  const ghost = card.cloneNode(true) as HTMLElement;
  ghost.removeAttribute("data-task");
  ghost.classList.add("tk-ghost");
  ghost.setAttribute("aria-hidden", "true");
  Object.assign(ghost.style, { left: `${from.left}px`, top: `${from.top}px`, width: `${from.width}px`, height: `${from.height}px` });
  document.body.append(ghost);
  const dx = to.left + to.width / 2 - (from.left + from.width / 2);
  const dy = to.top + to.height / 2 - (from.top + from.height / 2);
  ghost.animate([
    { translate: "0 0", scale: "1", opacity: 1 },
    { translate: `${dx * 0.35}px ${dy * 0.35 - 8}px`, scale: ".7", opacity: .9, offset: .35 },
    { translate: `${dx}px ${dy}px`, scale: ".12", opacity: 0 },
  ], { duration: 560, easing: EASE }).finished.finally(() => ghost.remove());
  tile.animate([{ scale: "1" }, { scale: "1.1" }, { scale: "1" }], { duration: 320, delay: 430, easing: EASE });
}

/** A tile acknowledges a drop. */
export function bumpTile(stage: string) {
  if (reduced()) return;
  document.querySelector<HTMLElement>(`.tk-tile[data-tile="${stage}"]`)?.animate([{ scale: "1" }, { scale: "1.1" }, { scale: "1" }], { duration: 320, easing: EASE });
}
