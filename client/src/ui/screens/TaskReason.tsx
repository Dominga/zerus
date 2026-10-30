import { Show, createSignal, onCleanup } from "solid-js";

/** The mockup's five-second explanation beside a blocked task command. */
export function createTaskReason() {
  const [tip, setTip] = createSignal<{ x: number; y: number; text: string }>();
  let timer: number | undefined;
  onCleanup(() => clearTimeout(timer));
  const show = (el: Element, text: string) => {
    const r = el.getBoundingClientRect(); clearTimeout(timer);
    setTip({ x: r.left + r.width / 2, y: r.bottom + 6, text });
    timer = window.setTimeout(() => setTip(undefined), 5000);
  };
  const clear = () => { clearTimeout(timer); setTip(undefined); };
  function Tip() {
    return <Show when={tip()}>{(t) => <div class="tk-tip" role="status" style={{ left: `${t().x}px`, top: `${t().y}px` }} ref={(el) => queueMicrotask(() => {
      const w = el.offsetWidth; el.style.left = `${Math.max(8 + w / 2, Math.min(t().x, innerWidth - 8 - w / 2))}px`;
    })}>{t().text}</div>}</Show>;
  }
  return { show, clear, Tip };
}
