import { TextInput } from "../lib/TextField";
import { For, Show, createSignal, onCleanup, onMount } from "solid-js";
import { Icon } from "../lib/icons";
import { accountMark, accountAppearance, saveAccountAppearance, validEmoji, MARK_STYLE } from "../../store/accountMarks";
import { settings } from "../../store/settings";
import { toast } from "../../store/notices";
import { AccountMarkCircle } from "../sessions/SessionCard";
import type { Account } from "../../types";
import { tr } from "../../i18n";

// The same swatches as machine label colors (MachineColorDialog).
const SWATCHES = ["#ffcf33", "#ff9234", "#f85c63", "#e45dbf", "#a96bf5", "#6677ff", "#2b97ff", "#23c5dd", "#00aa7f", "#41c86b", "#a6cc38", "#8796aa"];

/** Color and emoji mark of one account on this desktop (spec section 3.2). */
export default function AccountMarkDialog(props: { account: Account; origin?: HTMLElement; close: () => void }) {
  let dialog!: HTMLDialogElement;
  const origin = props.origin ?? document.activeElement;
  const initial = accountAppearance(settings, props.account.members);
  const [hex, setHex] = createSignal(initial.color);
  const [emoji, setEmoji] = createSignal(initial.emoji);
  const [busy, setBusy] = createSignal(false);
  const colorValid = () => hex() === "" || /^#[\da-f]{6}$/iu.test(hex());
  const emojiValid = () => emoji().trim() === "" || validEmoji(emoji());
  const valid = () => colorValid() && emojiValid();
  const tabColor = () => SWATCHES.find((color) => color === hex().toLowerCase()) ?? SWATCHES[0];
  // The preview applies the dialog's values over the stored ones.
  const preview = () => {
    const token = (m: { host: string; id: string }) => encodeURIComponent(`${m.host}${String.fromCharCode(31)}${m.id}`);
    // What session cards will show: the emoji only with emoji account marks.
    const values: Record<string, string | boolean | number | string[]> = { [MARK_STYLE]: settings[MARK_STYLE] ?? "letter" };
    const first = props.account.members[0];
    if (first) {
      if (colorValid() && hex()) values[`accounts/color/${token(first)}`] = hex().toLowerCase();
      if (emojiValid() && emoji().trim()) values[`accounts/mark/${token(first)}`] = emoji().trim();
    }
    return accountMark({ ...props.account, members: first ? [first] : [] }, values);
  };
  const stepSwatch = (event: KeyboardEvent & { currentTarget: HTMLDivElement }) => {
    if (busy() || event.altKey || event.ctrlKey || event.metaKey) return;
    const rows = [...event.currentTarget.querySelectorAll<HTMLButtonElement>(".cd-swatch:not(:disabled)")];
    const at = rows.findIndex((row) => row === event.target);
    if (at < 0) return;
    const next = event.key === "ArrowDown" || event.key === "ArrowRight" ? (at + 1) % rows.length
      : event.key === "ArrowUp" || event.key === "ArrowLeft" ? (at + rows.length - 1) % rows.length
      : event.key === "Home" ? 0 : event.key === "End" ? rows.length - 1 : undefined;
    if (next === undefined) return;
    event.preventDefault(); rows[next].click(); rows[next].focus();
  };
  const save = async (color: string | null, mark: string | null) => {
    if (busy()) return;
    setBusy(true);
    try {
      if (await saveAccountAppearance(props.account, color, mark)) props.close();
      else toast(tr("Color and mark"), tr("The account appearance could not be saved. Try again."), "warn", true);
    } finally { setBusy(false); }
  };
  const apply = () => valid() && void save(hex() ? hex().toLowerCase() : null, emoji().trim() ? emoji().trim() : null);
  onMount(() => { dialog.showModal(); queueMicrotask(() => dialog.querySelector<HTMLButtonElement>(".cd-swatch[tabindex='0']")?.focus()); });
  onCleanup(() => { dialog.close(); queueMicrotask(() => { if (origin instanceof HTMLElement && origin.isConnected) origin.focus(); }); });
  return <dialog class="dlg" ref={dialog} aria-labelledby="account-mark-title" style={{ width: "400px" }} onCancel={(event) => { if (busy()) event.preventDefault(); else props.close(); }} onClose={props.close}>
    <div class="dlg-head"><Icon name="palette" size={18} class="accent" /><h2 id="account-mark-title">{tr("Color and mark for %1", props.account.label)}</h2><span class="grow" /><button class="icon-btn" aria-label={tr("Close")} disabled={busy()} onClick={props.close}><Icon name="x" size={16} /></button></div>
    <div class="dlg-body">
      <div class="cd-preview am-preview"><AccountMarkCircle mark={preview()} size={28} /><span class="am-label">{preview().label}</span></div>
      <div class="cd-swatches" role="radiogroup" aria-label={tr("Color")} onKeyDown={stepSwatch}>
        <For each={SWATCHES}>{(color) => <button class="cd-swatch" role="radio" aria-checked={hex().toLowerCase() === color} tabIndex={!busy() && color === tabColor() ? 0 : -1} aria-label={color} classList={{ on: hex().toLowerCase() === color }} style={{ "--sw": color }} disabled={busy()} onClick={() => setHex(color)} />}</For>
      </div>
      <div class="form-row"><label for="am-hex">{tr("Hex")}</label><TextInput id="am-hex" class="field mono" maxLength={7} placeholder={tr("Automatic")} value={hex()} aria-invalid={!colorValid()} disabled={busy()} onInput={(event) => setHex(event.currentTarget.value.trim())} /></div>
      <div class="form-row"><label for="am-emoji">{tr("Emoji")}</label><TextInput id="am-emoji" class="field am-emoji" maxLength={16} placeholder={tr("None")} value={emoji()} aria-invalid={!emojiValid()} aria-describedby="am-emoji-hint" disabled={busy()} onInput={(event) => setEmoji(event.currentTarget.value)} /></div>
      <p id="am-emoji-hint" class="dim am-hint">
        <Show when={emojiValid()} fallback={tr("Enter one emoji, or leave the field empty.")}>
          {settings[MARK_STYLE] === "emoji" ? tr("Session cards show the emoji instead of the letter.") : tr("Session cards show the emoji when Settings > Sessions uses emoji account marks.")}
        </Show>
      </p>
      <label class="btn ghost cd-more">{tr("More colors…")}<input type="color" value={/^#[\da-f]{6}$/iu.test(hex()) ? hex() : "#2b97ff"} disabled={busy()} onInput={(event) => setHex(event.currentTarget.value)} /></label>
    </div>
    <div class="dlg-foot"><button class="btn ghost" disabled={busy()} onClick={() => void save(null, null)}>{tr("Reset to default")}</button><span class="spacer" /><button class="btn" disabled={busy()} onClick={props.close}>{tr("Cancel")}</button><button class="btn primary" disabled={busy() || !valid()} onClick={apply}>{tr("Apply")}</button></div>
  </dialog>;
}
