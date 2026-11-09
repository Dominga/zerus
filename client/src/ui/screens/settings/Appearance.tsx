import { For, Show, createMemo, createSignal, onCleanup, lazy, Suspense } from "solid-js";
import { windowLayer, setWindowLayer } from "../../../store/windowLayer";
import { Icon } from "../../lib/icons";
import { setContentScale, toast } from "../../../store/ui";
import { previewColumn } from "../../sessions/Activity";
import { flag, num, setSetting, settingsLoaded, text } from "../../../store/settings";
import { appearanceFor, chooseSkin, chooseTheme, chosenSkin, skin, skinList, skins, systemLight, theme, type Theme } from "../../../skin/runtime";
import type { ResolvedSkin } from "../../../skin/skin";
import { previewLayout } from "../../../skin/choice";
import { Group, Row, SectionHead, Select, Slider, Check, Actions } from "./ui";
import { tr, trNoop } from "../../../i18n";

// Settings > Appearance (tray/src/SettingsPage.h:46-95, UiLanguage.h,
// ContentScale.h, ActivityWidth.h, WindowLayer.cpp), with the client's skins
// and Overview width from the approved mockup
// (design/webui/src/screens/settings/Appearance.tsx).

/** A token of a skin, following {group.token} references. */
export function tok(s: ResolvedSkin, group: string, key: string, depth = 0): string {
  const v = s.tokens[group]?.[key];
  if (typeof v !== "string") return "transparent";
  const ref = /^\{([a-z]+)\.([a-z0-9-]+)\}$/.exec(v);
  return ref && depth < 4 ? tok(s, ref[1], ref[2], depth + 1) : v;
}
const num0 = (s: ResolvedSkin, group: string, key: string) => Number(s.tokens[group]?.[key] ?? 0);

const VARIANT_TEXT: Record<string, Record<string, string>> = {
  "status-badge": { pill: trNoop("Pill status badges"), dot: trNoop("Dot status badges") },
  "agent-mark": { chip: trNoop("Agent chips"), glyph: trNoop("Agent glyphs") },
  "tree-lines": { lines: trNoop("Tree lines"), indent: trNoop("Indented subagents") },
  "attention-edge": { bar: trNoop("Attention bar"), tint: trNoop("Attention tint") },
  rail: { icons: trNoop("Icon rail"), labeled: trNoop("Labeled rail") },
  "sessions-pane": { left: trNoop("Sessions on the left"), right: trNoop("Sessions on the right") },
  inspector: { docked: trNoop("Side panel docked"), collapsed: trNoop("Side panel collapsed") },
  "activity-column": { centered: trNoop("Centered Activity"), full: trNoop("Full-width Activity") },
  // Task 348: Compact's layout, named as the others are.
  "side-panel": { tabs: trNoop("Side panel tabs"), sections: trNoop("Side panel sections") },
  chrome: { system: trNoop("System title bar"), unified: trNoop("One title bar with the controls") },
};

/** Variants that differ from the base skin, as a short readable list. */
export function variantSummary(s: ResolvedSkin): string {
  const base = skins[0];
  const parts: string[] = [];
  for (const [k, v] of Object.entries(s.components)) if ((base.components as Record<string, string>)[k] !== v) parts.push(VARIANT_TEXT[k]?.[v] ? tr(VARIANT_TEXT[k][v]) : `${k}: ${v}`);
  for (const [k, v] of Object.entries(s.layout)) if ((base.layout as Record<string, string>)[k] !== v) parts.push(VARIANT_TEXT[k]?.[v] ? tr(VARIANT_TEXT[k][v]) : `${k}: ${v}`);
  return parts.length ? parts.join(", ") : tr("Standard components and layout");
}

/**
 * A miniature of the workspace painted only with the skin's own tokens, in the
 * skin's own layout (task 348): Compact's one title bar with the window
 * controls and its side panel of sections, a labeled rail, sessions on the
 * right, a collapsed side panel, a full-width Activity and indented subagents
 * each show, so two skins with other layouts never look alike.
 */
export function SkinPreview(props: { skin: ResolvedSkin }) {
  const s = () => props.skin;
  const c = (k: string) => tok(s(), "color", k);
  const r = () => `${Math.max(2, num0(s(), "radius", "card") * 0.6)}px`;
  const l = () => previewLayout(s());
  return (
    <div class="skin-prev" aria-hidden="true" style={{ background: c("page"), "border-color": c("divider") }}
      data-chrome={l().chrome} data-rail={l().rail} data-sessions={l().sessions} data-inspector={l().inspector}
      data-side-panel={l().sidePanel} data-activity={l().activity} data-tree={l().tree}>
      <Show when={l().chrome === "unified"}>
        <div class="skp-title" style={{ background: c("pane"), "border-color": c("divider") }}>
          <span class="skp-line" style={{ background: c("accent"), width: "14%" }} />
          <span class="skp-tab" style={{ background: c("card-selected") }} />
          <span class="skp-tab" style={{ background: c("tile") }} />
          <span class="grow" />
          <For each={[0, 1, 2]}>{() => <b style={{ background: c("muted") }} />}</For>
        </div>
      </Show>
      <div class="skp-body">
        <div class="skp-rail" style={{ background: c("pane"), "border-color": c("divider") }}>
          <For each={["accent", "muted", "muted"]}>{(k) => (
            <span class="skp-rail-item"><i style={{ background: c(k) }} /><Show when={l().rail === "labeled"}><span class="skp-line" style={{ background: c("text-3") }} /></Show></span>
          )}</For>
        </div>
        <div class="skp-list" style={{ background: c("pane"), "border-color": c("divider") }}>
          <For each={["status-working", "status-attention", "status-error"] as const}>
            {(st, i) => (
              <div class="skp-card" classList={{ "skp-child": i() > 0 }}
                style={{ background: i() === 0 ? c("card-selected") : "transparent", "border-color": i() === 0 ? c("card-selected-border") : "transparent", "border-radius": r(),
                  ...(i() > 0 && l().tree === "lines" ? { "border-left-color": c("divider") } : {}) }}>
                <span class="skp-dot" style={{ background: c(st) }} />
                <span class="skp-line" style={{ background: c("text-2"), width: `${64 - i() * 12}%` }} />
              </div>
            )}
          </For>
        </div>
        <div class="skp-detail" style={{ background: c("surface"), "border-color": c("divider"), "border-radius": r() }}>
          <span class="skp-bubble" style={{ background: c("bubble"), "border-radius": r() }} />
          <span class="skp-msg" style={{ background: c("tile"), "border-color": c("accent"), "border-radius": r() }}>
            <span class="skp-line" style={{ background: c("text-3"), width: "80%" }} />
            <span class="skp-line" style={{ background: c("text-3"), width: "55%" }} />
          </span>
          <span class="skp-btn" style={{ background: c("accent"), "border-radius": r() }} />
          <span class="skp-font" style={{ color: c("text"), "font-family": (s().tokens.font?.ui as string[] | undefined)?.map((f) => `"${f}"`).join(", ") }}>Aa</span>
        </div>
        <Show when={l().inspector === "docked"}>
          <div class="skp-side" style={{ background: c("pane"), "border-color": c("divider") }}>
            <Show when={l().sidePanel === "sections"} fallback={
              <>
                <span class="skp-side-tabs"><For each={[0, 1, 2]}>{(i) => <i style={{ background: i === 0 ? c("accent") : c("muted") }} />}</For></span>
                <For each={[70, 50, 60]}>{(w) => <span class="skp-line" style={{ background: c("text-3"), width: `${w}%` }} />}</For>
              </>
            }>
              <For each={[60, 45, 55]}>{(w) => (
                <span class="skp-section" style={{ "border-color": c("divider") }}>
                  <span class="skp-line" style={{ background: c("text-2"), width: `${w}%` }} />
                  <span class="skp-line" style={{ background: c("text-3"), width: `${w - 15}%` }} />
                </span>
              )}</For>
            </Show>
          </div>
        </Show>
      </div>
    </div>
  );
}

function SkinCard(props: { skin: ResolvedSkin }) {
  const s = () => props.skin;
  const chosen = () => chosenSkin(s().appearance) === s().id;
  const active = () => skin().id === s().id;
  const builtIn = () => skins.some((b) => b.id === s().id);
  const swatches = ["accent", "status-background", "violet", "warn", "danger", "info"];
  const pick = () => {
    if (!chooseSkin(s().id)) toast(s().name, s().appearance === "dark" ? tr("Used when your system appearance is dark.") : tr("Used when your system appearance is light."));
  };
  return (
    <button class="skin-card" classList={{ chosen: chosen(), active: active() }} aria-pressed={chosen()} onClick={pick}
      title={`${s().name}: ${variantSummary(s())}`}>
      <SkinPreview skin={s()} />
      <span class="skin-meta">
        <span class="skin-name ell">{s().name}</span>
        <Show when={active()} fallback={<Show when={chosen()}><span class="chip">{s().appearance === "dark" ? tr("For dark appearance") : tr("For light appearance")}</span></Show>}>
          <span class="chip skin-inuse"><Icon name="check" size={11} />{tr("In use")}</span>
        </Show>
      </span>
      <span class="skin-swatches">
        <For each={swatches}>{(k) => <i style={{ background: tok(s(), "color", k) }} />}</For>
        <span class="skin-origin">{builtIn() ? "Built-in" : "Imported"}</span>
      </span>
      <span class="skin-variants ell">{variantSummary(s())}</span>
    </button>
  );
}

/**
 * A slider's setting: live while dragging, previewed after 150 ms as Qt does
 * for content scale and Activity width, stored once it settles. A drag that
 * ends where it started stores nothing and returns to the stored value.
 */
function liveSetting(key: string, preview?: (value: number | undefined) => void) {
  const [live, setLive] = createSignal<number | undefined>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const stop = () => { if (timer !== undefined) clearTimeout(timer); timer = undefined; };
  const clear = () => { stop(); setLive(undefined); preview?.(undefined); };
  onCleanup(clear);
  return {
    value: () => live() ?? num(key),
    input: (v: number) => {
      setLive(v);
      stop();
      if (preview) timer = setTimeout(() => preview(v), 150);
    },
    commit: (v: number) => { stop(); setLive(undefined); void setSetting(key, v); preview?.(undefined); },
    // After release a change event may still come: let it store first.
    end: () => setTimeout(() => { if (live() !== undefined) clear(); }, 0),
  };
}

const SkinImportDialog=lazy(()=>import("./SkinImportDialog"));

export function AppearanceSection() {
  const [importOrigin,setImportOrigin]=createSignal<HTMLElement>();
  const openFolder=async()=>{try{await (await import("../../../skin/user")).openSkinsFolder();}catch(error){(await import("../../../store/actions")).notify("error",String(error),true);}};
  const dark = createMemo(() => skinList().filter((s) => s.appearance === "dark"));
  const light = createMemo(() => skinList().filter((s) => s.appearance === "light"));
  const scale = liveSetting("workspace/contentScale", (v) => setContentScale(v ?? num("workspace/contentScale")));
  const overview = liveSetting("client/overviewWidth");
  const activity = liveSetting("workspace/activityWidth", previewColumn);
  const appearanceHint = (a: "dark" | "light") =>
    theme() === "system" ? (systemLight() === (a === "light") ? tr("In use with your system appearance") : a === "dark" ? tr("Used when your system appearance is dark") : tr("Used when your system appearance is light"))
      : theme() === a ? tr("In use") : a === "dark" ? tr("Used with the Dark theme") : tr("Used with the Light theme");
  return (
    <>
      <SectionHead title={trNoop("Appearance")} />
      <Group>
        <Row label={trNoop("Theme")} for="set-theme" hint={trNoop("Each theme uses one skin. Follow system appearance switches between your dark and light skin.")}>
          <Select id="set-theme" value={theme()} onChange={(v) => chooseTheme(v as Theme)}
            options={[{ value: "system", label: trNoop("Follow system appearance") }, { value: "dark", label: trNoop("Dark") }, { value: "light", label: trNoop("Light") }]} />
        </Row>
        <div class="set-row stack">
          <div class="set-row-text">
            <span class="set-label">{tr("Skin")}</span>
            <div class="set-hint">{tr("A skin sets colors, fonts, radii, density and motion, and chooses variants of existing components and layouts. It never contains CSS or code.")}</div>
          </div>
          <div class="skin-groups">
            <For each={[["dark", dark()], ["light", light()]] as const}>
              {([a, list]) => (
                <div class="skin-group" role="group" aria-labelledby={`skin-group-${a}`} classList={{ idle: appearanceFor(theme(), systemLight()) !== a }}>
                  <div class="skin-group-head"><span class="eyebrow" id={`skin-group-${a}`}>{a === "dark" ? tr("Dark") : tr("Light")}</span><span class="dim">{appearanceHint(a)}</span></div>
                  <div class="skin-grid"><For each={list}>{(s) => <SkinCard skin={s} />}</For></div>
                </div>
              )}
            </For>
          </div>
          <Actions><button class="btn" onClick={event=>setImportOrigin(event.currentTarget)}><Icon name="download" size={14}/>{tr("Import skin…")}</button><button class="btn" onClick={()=>void openFolder()}><Icon name="folder-open" size={14}/>{tr("Open skins folder")}</button></Actions>
        </div>
      </Group>
      <Show when={importOrigin()}>{origin=><Suspense><SkinImportDialog origin={origin()} close={()=>setImportOrigin(undefined)}/></Suspense>}</Show>

      <Group>
        <Row label={trNoop("Interface language")} for="set-lang" hint={<>{tr("Restart Zerus to apply the interface language. Agent sessions keep running.")}<br />{tr("Russian is a preview. Untranslated text stays in English.")}</>}>
          <Select id="set-lang" value={text("workspace/language")} disabled={!settingsLoaded()} onChange={(v) => void setSetting("workspace/language", v)}
            options={[{ value: "en", label: trNoop("English") }, { value: "ru", label: trNoop("Russian (preview)") }]} />
        </Row>
        <Row label={trNoop("Usage limit indicator")} for="set-usage" hint={trNoop("Compares used quota with the elapsed part of each limit window. The bars show usage above elapsed time; the marker shows elapsed time on the usage bar. Colors warn when the current pace would use up the limit before it resets.")}>
          <Select id="set-usage" value={text("workspace/usageIndicator")} onChange={(v) => void setSetting("workspace/usageIndicator", v)}
            options={[{ value: "rails", label: trNoop("Usage and elapsed time bars") }, { value: "marker", label: trNoop("Pace marker") }, { value: "marker-countdown", label: trNoop("Pace marker and reset countdown") }]} />
        </Row>
      </Group>

      <Group>
        <Row label={trNoop("Content scale")} wide hint={trNoop("Adjusts Activity, the message field and Terminal from 75% to 200%. The session list and the side panel keep their size.")}>
          <Slider label={trNoop("Content scale")} resetLabel={trNoop("Reset to 100%")} value={Math.round(scale.value() * 100)} min={75} max={200} step={5} defaultValue={100} format={(v) => `${v}%`}
            onInput={(v) => scale.input(v / 100)} onCommit={(v) => scale.commit(v / 100)} onEnd={scale.end} />
        </Row>
        <Row label={trNoop("Activity width")} wide hint={trNoop("In a wide window, Activity, questions and the message field stay centered at this width, which grows with content scale. Drag the column edges in Activity to adjust it, or double-click an edge to restore 900 px. Full-width Activity fills the whole pane.")}>
          <div class="col" style={{ gap: "8px", "align-items": "stretch" }}>
            <Slider label={trNoop("Maximum Activity width")} resetLabel={trNoop("Reset to 900 px")} value={activity.value()} min={480} max={2400} step={10} defaultValue={900} format={(v) => `${v} px`}
              disabled={flag("workspace/activityFullWidth")} onInput={activity.input} onCommit={activity.commit} onEnd={activity.end} />
            <Check label={trNoop("Full-width Activity")} checked={flag("workspace/activityFullWidth")} onChange={(v) => void setSetting("workspace/activityFullWidth", v)} />
          </div>
        </Row>
        <Row label={trNoop("Overview width")} wide hint={trNoop("In a wide window, Overview stays centered at this width. Drag its edges to adjust it; double-click an edge to return to this width. Full-width Overview fills the whole page.")}>
          <div class="col" style={{ gap: "8px", "align-items": "stretch" }}>
            <Slider label={trNoop("Default Overview width")} resetLabel={trNoop("Reset to 1200 px")} value={overview.value()} min={720} max={2400} step={20} defaultValue={1200} format={(v) => `${v} px`}
              disabled={flag("client/overviewFullWidth")} onInput={overview.input} onCommit={overview.commit} onEnd={overview.end} />
            <Check label={trNoop("Full-width Overview")} checked={flag("client/overviewFullWidth")} onChange={(v) => void setSetting("client/overviewFullWidth", v)} />
          </div>
        </Row>
        <Row label={trNoop("Window")} hint={windowLayer().hint}>
          <Check label={trNoop("Keep Zerus above other windows")} checked={windowLayer().on} disabled={!windowLayer().supported}
            onChange={(v) => void setWindowLayer(v)} />
        </Row>
      </Group>
      <p class="set-foot">{tr("Appearance and session list preferences apply to this Zerus.")}</p>
    </>
  );
}
