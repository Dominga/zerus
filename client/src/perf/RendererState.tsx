import { createSignal, For, onMount, Show } from "solid-js";
import { tr } from "../i18n";

export interface RendererDiagnostics {
  requested: { renderer: string; reason: string } | null;
  observed: {
    hardware_acceleration_policy: "always" | "never" | "on-demand" | "unavailable";
    composited_layers: "unavailable";
    buffer_transport: "unavailable";
  };
  overrides: { name: string; value: string | null }[];
}

/** Startup evidence only: a requested mode or policy is not a layer/transport observation. */
export function RendererState(props: { read: () => Promise<RendererDiagnostics | null> }) {
  const [state, setState] = createSignal<RendererDiagnostics | null>(null);
  onMount(() => { void props.read().then(setState).catch(() => setState(null)); });
  const value = (policy: string) => policy === "unavailable" ? tr("Unavailable") : policy;
  return <Show when={state()} fallback={<span>{tr("Native renderer diagnostics unavailable")}</span>}>{snapshot =>
    <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
      <div><strong>{tr("Requested renderer")}</strong>: <code>{snapshot().requested?.renderer}  {snapshot().requested?.reason}</code></div>
      <section aria-label={tr("Observed WebKit state")}>
        <strong>{tr("Observed WebKit state")}</strong>
        <dl style={{ display: "grid", "grid-template-columns": "max-content minmax(0, 1fr)", gap: "3px 10px", margin: "4px 0" }}>
          <dt>{tr("Hardware acceleration policy")}</dt><dd style={{ margin: 0 }}>{value(snapshot().observed.hardware_acceleration_policy)}</dd>
          <dt>{tr("Composited layers")}</dt><dd style={{ margin: 0 }}>{value(snapshot().observed.composited_layers)}</dd>
          <dt>{tr("Backing buffer transport")}</dt><dd style={{ margin: 0 }}>{value(snapshot().observed.buffer_transport)}</dd>
        </dl>
        <span class="hint">{tr("The public WebKitGTK API does not expose the layer tree or selected buffer transport.")}</span>
      </section>
      <details>
        <summary>{tr("Renderer overrides after startup selection")}</summary>
        <For each={snapshot().overrides}>{entry => <div><code>{entry.name}={entry.value === null ? tr("Not set") : entry.value === "other" ? tr("Other value") : entry.value}</code></div>}</For>
      </details>
    </div>
  }</Show>;
}
