// A synthetic heavy screen for the pacing scenarios: a virtualized session list
// (94 px cards, 40 px project headers) and a long Activity, next to the
// Diagnostics panel. It runs the same measurement code as the client without the
// Tauri side, in any WebKit (client/bench/run-webkitgtk.py) or browser.
// Synthetic content only.

import { createMemo, createSignal, For } from "solid-js";
import { render } from "solid-js/web";
import Diagnostics from "../../src/perf/Diagnostics";

const params = new URLSearchParams(location.search);
const ROWS = Number(params.get("rows") ?? 2000);
const EVENTS = Number(params.get("events") ?? 1000);
const CARD = 94;
const HEADER = 40;
const OVERSCAN = 400;

interface Item {
  key: string;
  header: boolean;
  top: number;
  height: number;
  title: string;
  detail: string;
  meta: string;
  status: string;
}

const items: Item[] = [];
let offset = 0;
for (let i = 0, project = 0; i < ROWS; project++) {
  items.push({ key: `h${project}`, header: true, top: offset, height: HEADER, title: `sample-project-${project}`, detail: "", meta: "20 sessions", status: "" });
  offset += HEADER;
  for (let k = 0; k < 20 && i < ROWS; k++, i++) {
    items.push({
      key: `s${i}`,
      header: false,
      top: offset,
      height: CARD,
      title: `Synthetic session ${i} with a longer title`,
      detail: `Reading client/src/ui/SessionCard${i}.tsx`,
      meta: `project-${project} / feature-branch-${i}`,
      status: ["Ready", "Working", "Needs approval", "Done", "Paused"][i % 5],
    });
    offset += CARD;
  }
}
const totalHeight = offset;

interface Event {
  id: string;
  kind: "prompt" | "tool" | "reply";
  text: string;
  bullets: string[];
}

function session(salt: string): Event[] {
  return Array.from({ length: EVENTS }, (_, i): Event => {
    if (i % 4 === 0) return { id: `${salt}${i}`, kind: "prompt", text: `Request ${i} (${salt}): review client/src/store/catalog.ts and keep the list stable.`, bullets: [] };
    if (i % 4 !== 3) return { id: `${salt}${i}`, kind: "tool", text: `rg -n "applyBatch" /home/example/project/client/src --stats ${i}`, bullets: [] };
    return {
      id: `${salt}${i}`,
      kind: "reply",
      text: `The change keeps object identity of unchanged rows and reconciles by key (result ${i}).`,
      bullets: [0, 1, 2, 3].map((k) => `field_${i}_${k} beside example/cache/catalog.json and wrapped prose that continues across lines`),
    };
  });
}

const sessions = [session("a"), session("b")];

function App() {
  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewport, setViewport] = createSignal(800);
  const [current, setCurrent] = createSignal(0);
  let listRef: HTMLDivElement | undefined;
  let activityRef: HTMLDivElement | undefined;

  const visible = createMemo(() => {
    const top = scrollTop() - OVERSCAN;
    const bottom = scrollTop() + viewport() + OVERSCAN;
    let lo = 0;
    let hi = items.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (items[mid].top + items[mid].height < top) lo = mid + 1;
      else hi = mid;
    }
    const result: Item[] = [];
    for (let i = lo; i < items.length && items[i].top < bottom; i++) result.push(items[i]);
    return result;
  });

  const openSession = async (index: number) => {
    setCurrent(index % sessions.length);
    if (activityRef) activityRef.scrollTop = activityRef.scrollHeight;
    await Promise.resolve();
  };

  return (
    <div style={{ display: "grid", "grid-template-columns": "310px 1fr", "grid-template-rows": "1fr 280px", height: "100%" }}>
      <div
        ref={(el) => {
          listRef = el;
          queueMicrotask(() => setViewport(el.clientHeight));
        }}
        onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
        style={{ overflow: "auto", "scrollbar-gutter": "stable", "border-right": "1px solid var(--z-color-border)" }}
      >
        <div style={{ position: "relative", height: `${totalHeight}px` }}>
          <For each={visible()}>
            {(item) => (
              <div
                style={{
                  position: "absolute",
                  left: 0,
                  right: 0,
                  top: 0,
                  height: `${item.height}px`,
                  transform: `translateY(${item.top}px)`,
                  padding: item.header ? "10px 12px" : "10px 12px",
                  "box-sizing": "border-box",
                  background: item.header ? "transparent" : "var(--z-color-raised)",
                  "border-bottom": "1px solid var(--z-color-border)",
                }}
              >
                <div style={{ "font-weight": item.header ? "600" : "500", "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis" }}>{item.title}</div>
                {!item.header && (
                  <>
                    <div style={{ color: "var(--z-color-dim)", "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis" }}>{item.detail}</div>
                    <div style={{ display: "flex", gap: "8px", color: "var(--z-color-dim)" }}>
                      <span>{item.status}</span>
                      <span style={{ "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis" }}>{item.meta}</span>
                    </div>
                  </>
                )}
              </div>
            )}
          </For>
        </div>
      </div>
      <div ref={activityRef} style={{ overflow: "auto", "scrollbar-gutter": "stable", padding: "0 24px" }}>
        <div style={{ "max-width": "900px", margin: "0 auto" }}>
          <For each={sessions[current()]}>
            {(event) => (
              <article style={{ margin: "12px 0", padding: event.kind === "prompt" ? "8px 12px" : "0", background: event.kind === "prompt" ? "var(--z-color-raised)" : "transparent", "border-radius": "6px" }}>
                {event.kind === "tool" ? (
                  <code style={{ color: "var(--z-color-dim)" }}>{event.text}</code>
                ) : (
                  <p style={{ margin: "4px 0" }}>{event.text}</p>
                )}
                {event.bullets.length > 0 && (
                  <ul>
                    <For each={event.bullets}>
                      {(bullet) => (
                        <li>
                          Preserve <code style={{ background: "var(--z-color-raised)", padding: "1px 4px", "border-radius": "4px" }}>{bullet.split(" ")[0]}</code> {bullet.slice(bullet.indexOf(" "))}
                        </li>
                      )}
                    </For>
                  </ul>
                )}
              </article>
            )}
          </For>
        </div>
      </div>
      <div style={{ "grid-column": "1 / span 2", "border-top": "1px solid var(--z-color-border)", overflow: "auto" }}>
        <Diagnostics
          scrollList={() => listRef ?? null}
          scrollActivity={() => activityRef ?? null}
          openSession={openSession}
          label={params.get("label") ?? "harness"}
          representative={params.get("representative") !== "0"}
          displayHz={async () => (params.get("hz") ? Number(params.get("hz")) : null)}
        />
      </div>
    </div>
  );
}

render(() => <App />, document.getElementById("root")!);
