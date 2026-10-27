import { For, Show, createMemo } from "solid-js";
import { Icon } from "../../lib/icons";
import { setDialog, toast, contentScale, setContentScale, demoState } from "../../state";
import { skin } from "../../skin/runtime";
import type { ResolvedSkin } from "../../skin/skin";
import { prefs, setPrefs, chooseTheme, chooseSkin, skinList, importedIds, systemLight, platform, type Theme } from "./store";
import { SectionHead, Group, Row, Select, Slider, Check, Actions } from "./ui";

/** Resolve a token of a skin, following {group.token} references. */
export function tok(s: ResolvedSkin, group: string, key: string, depth = 0): string {
  const v = s.tokens[group]?.[key];
  if (typeof v !== "string") return "transparent";
  const ref = /^\{([a-z]+)\.([a-z0-9-]+)\}$/.exec(v);
  return ref && depth < 4 ? tok(s, ref[1], ref[2], depth + 1) : v;
}
const num = (s: ResolvedSkin, group: string, key: string) => Number(s.tokens[group]?.[key] ?? 0);

const VARIANT_TEXT: Record<string, Record<string, string>> = {
  "status-badge": { pill: "Pill status badges", dot: "Dot status badges" },
  "agent-mark": { chip: "Agent chips", glyph: "Agent glyphs" },
  "tree-lines": { lines: "Tree lines", indent: "Indented subagents" },
  "attention-edge": { bar: "Attention bar", tint: "Attention tint" },
  rail: { icons: "Icon rail", labeled: "Labeled rail" },
  "sessions-pane": { left: "Sessions on the left", right: "Sessions on the right" },
  inspector: { docked: "Side panel docked", collapsed: "Side panel collapsed" },
  "activity-column": { centered: "Centered Activity", full: "Full-width Activity" },
};

/** Variants that differ from the base skin, as a short readable list. */
export function variantSummary(s: ResolvedSkin): string {
  const base = skinList()[0];
  const parts: string[] = [];
  for (const [k, v] of Object.entries(s.components)) if ((base.components as Record<string, string>)[k] !== v) parts.push(VARIANT_TEXT[k]?.[v] ?? `${k}: ${v}`);
  for (const [k, v] of Object.entries(s.layout)) if ((base.layout as Record<string, string>)[k] !== v) parts.push(VARIANT_TEXT[k]?.[v] ?? `${k}: ${v}`);
  return parts.length ? parts.join(", ") : "Standard components and layout";
}

/** A miniature of the workspace painted only with the skin's own tokens. */
export function SkinPreview(props: { skin: ResolvedSkin }) {
  const s = () => props.skin;
  const c = (k: string) => tok(s(), "color", k);
  const r = () => `${Math.max(2, num(s(), "radius", "card") * 0.6)}px`;
  return (
    <div class="skin-prev" aria-hidden="true" style={{ background: c("page"), "border-color": c("divider") }}>
      <div class="skp-rail" style={{ background: c("pane"), "border-color": c("divider") }}>
        <i style={{ background: c("accent") }} /><i style={{ background: c("muted") }} /><i style={{ background: c("muted") }} />
      </div>
      <div class="skp-list" style={{ background: c("pane"), "border-color": c("divider") }}>
        <For each={["status-working", "status-attention", "status-error"] as const}>
          {(st, i) => (
            <div class="skp-card" style={{ background: i() === 0 ? c("card-selected") : "transparent", "border-color": i() === 0 ? c("card-selected-border") : "transparent", "border-radius": r() }}>
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
    </div>
  );
}

function SkinCard(props: { skin: ResolvedSkin }) {
  const s = () => props.skin;
  const chosen = () => (s().appearance === "dark" ? prefs.darkSkin : prefs.lightSkin) === s().id;
  const active = () => skin().id === s().id;
  const swatches = ["accent", "status-background", "violet", "warn", "danger", "info"];
  const pick = () => {
    const applied = chooseSkin(s().id);
    if (!applied) toast(s().name, `Used when your system appearance is ${s().appearance}.`);
  };
  return (
    <button class="skin-card" classList={{ chosen: chosen(), active: active() }} aria-pressed={chosen()} onClick={pick}
      title={`${s().name}: ${variantSummary(s())}`}>
      <SkinPreview skin={s()} />
      <span class="skin-meta">
        <span class="skin-name ell">{s().name}</span>
        <Show when={active()} fallback={<Show when={chosen()}><span class="chip">For {s().appearance} appearance</span></Show>}>
          <span class="chip skin-inuse"><Icon name="check" size={11} />In use</span>
        </Show>
      </span>
      <span class="skin-swatches">
        <For each={swatches}>{(k) => <i style={{ background: tok(s(), "color", k) }} />}</For>
        <span class="skin-origin">{importedIds().includes(s().id) ? "Imported" : "Built-in"}</span>
      </span>
      <span class="skin-variants ell">{variantSummary(s())}</span>
    </button>
  );
}

export function AppearanceSection() {
  const dark = createMemo(() => skinList().filter((s) => s.appearance === "dark"));
  const light = createMemo(() => skinList().filter((s) => s.appearance === "light"));
  const scale = () => Math.round(contentScale() * 100);
  const onTopSupported = () => demoState() !== "error";
  const appearanceHint = (a: "dark" | "light") =>
    prefs.theme === "system" ? (systemLight() === (a === "light") ? "In use with your system appearance" : `Used when your system appearance is ${a}`)
      : prefs.theme === a ? "In use" : `Used with the ${a === "dark" ? "Dark" : "Light"} theme`;
  return (
    <>
      <SectionHead title="Appearance" />
      <Group>
        <Row label="Theme" for="set-theme" hint="Each theme uses one skin. Follow system appearance switches between your dark and light skin.">
          <Select id="set-theme" value={prefs.theme} onChange={(v) => chooseTheme(v as Theme)}
            options={[{ value: "system", label: "Follow system appearance" }, { value: "dark", label: "Dark" }, { value: "light", label: "Light" }]} />
        </Row>
        <div class="set-row stack">
          <div class="set-row-text">
            <span class="set-label">Skin</span>
            <div class="set-hint">A skin sets colors, fonts, radii, density and motion, and chooses variants of existing components and layouts. It never contains CSS or code.</div>
          </div>
          <div class="skin-groups">
            <For each={[["dark", dark()], ["light", light()]] as const}>
              {([a, list]) => (
                <div class="skin-group" classList={{ idle: skin().appearance !== a }}>
                  <div class="skin-group-head"><span class="eyebrow">{a === "dark" ? "Dark" : "Light"}</span><span class="dim">{appearanceHint(a)}</span></div>
                  <div class="skin-grid"><For each={list}>{(s) => <SkinCard skin={s} />}</For></div>
                </div>
              )}
            </For>
          </div>
          <Actions>
            <button class="btn" onClick={() => setDialog("settings-skin-import")}><Icon name="download" size={14} />Import skin…</button>
            <button class="btn" onClick={() => toast("Skins folder", platform === "windows" ? "Opened %APPDATA%\\hgs\\skins in Explorer." : "Opened ~/.config/hgs/skins in the file manager.")}><Icon name="folder-open" size={14} />Open skins folder</button>
          </Actions>
        </div>
      </Group>

      <Group>
        <Row label="Interface language" for="set-lang" hint={<>Restart Zerus to apply the interface language. Agent sessions keep running.<br />Russian is a preview. Untranslated text stays in English.</>}>
          <Select id="set-lang" value={prefs.language} onChange={(v) => setPrefs("language", v)} options={[{ value: "en", label: "English" }, { value: "ru", label: "Russian (preview)" }]} />
        </Row>
        <Row label="Usage limit indicator" for="set-usage" hint="Compares used quota with the elapsed part of each limit window. The bars show usage above elapsed time; the marker shows elapsed time on the usage bar. Colors warn when the current pace would use up the limit before it resets.">
          <Select id="set-usage" value={prefs.usageIndicator} onChange={(v) => setPrefs("usageIndicator", v)}
            options={[{ value: "rails", label: "Usage and elapsed time bars" }, { value: "marker", label: "Pace marker" }, { value: "marker-countdown", label: "Pace marker and reset countdown" }]} />
        </Row>
      </Group>

      <Group>
        <Row label="Content scale" wide hint="Adjusts Activity, the message field and Terminal from 75% to 200%. The session list and the side panel keep their size.">
          <Slider label="Content scale" resetLabel="Reset to 100%" value={scale()} min={75} max={200} step={5} defaultValue={100} format={(v) => `${v}%`}
            onChange={(v) => setContentScale(v / 100)} />
        </Row>
        <Row label="Activity width" wide hint={`In a wide window, Activity, questions and the message field stay centered at this width, which grows with content scale. Drag the column edges in Activity to adjust it, or double-click an edge to restore 900 px. Full-width Activity fills the whole pane.`}>
          <div class="col" style={{ gap: "8px", "align-items": "stretch" }}>
            <Slider label="Maximum Activity width" resetLabel="Reset to 900 px" value={prefs.activityWidth} min={480} max={2400} step={10} defaultValue={900} format={(v) => `${v} px`}
              disabled={prefs.fullWidth} onChange={(v) => setPrefs("activityWidth", v)} />
            <Check label="Full-width Activity" checked={prefs.fullWidth} onChange={(v) => setPrefs("fullWidth", v)} />
          </div>
        </Row>
        <Row label="Overview width" wide hint="In a wide window, Overview stays centered at this width. Drag its edges to adjust it; double-click an edge to return to this width. Full-width Overview fills the whole page.">
          <div class="col" style={{ gap: "8px", "align-items": "stretch" }}>
            <Slider label="Default Overview width" resetLabel="Reset to 1200 px" value={prefs.overviewWidth} min={720} max={2400} step={20} defaultValue={1200} format={(v) => `${v} px`}
              disabled={prefs.overviewFullWidth} onChange={(v) => setPrefs("overviewWidth", v)} />
            <Check label="Full-width Overview" checked={prefs.overviewFullWidth} onChange={(v) => setPrefs("overviewFullWidth", v)} />
          </div>
        </Row>
        <Row label="Window" hint={onTopSupported()
          ? "Files and terminals opened from Zerus may appear behind it while this is on."
          : "Keep Above is unavailable in this Zerus session. On Wayland, KDE Plasma 6 with KWin scripting is required. You can use your desktop's window menu or rules."}>
          <Check label="Keep Zerus above other windows" checked={prefs.onTop} disabled={!onTopSupported()} onChange={(v) => setPrefs("onTop", v)} />
        </Row>
      </Group>
      <p class="set-foot">Appearance and session list preferences apply to this Zerus.</p>
    </>
  );
}
