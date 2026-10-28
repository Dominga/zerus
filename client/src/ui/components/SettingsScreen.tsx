import { Show, type JSX } from "solid-js";
import { setSetting, settings } from "../../store/settings";
import { setPage } from "../../store/ui";
import { motionReduced } from "../motion";

// Settings, as far as the client has them: the motion settings, where the
// mockup puts them (Sessions: Animate working sessions, SettingsPage.h; the
// motion choice beside Appearance). The full screen with its sections moves in
// with zerus-5wmh.8; it uses the same rows (design/webui settings/ui.tsx).

function ToggleRow(props: { id: string; label: string; hint?: JSX.Element; checked: boolean; onChange: (on: boolean) => void }) {
  return (
    <div class="set-row">
      <div class="set-row-text">
        <label class="set-label" for={props.id}>{props.label}</label>
        <Show when={props.hint}><div class="set-hint">{props.hint}</div></Show>
      </div>
      <div class="set-row-control">
        <button id={props.id} type="button" role="switch" class="set-switch switch" classList={{ on: props.checked }} aria-checked={props.checked}
          aria-label={props.label} onClick={() => props.onChange(!props.checked)} />
      </div>
    </div>
  );
}

export function SettingsScreen() {
  const systemOnly = () => motionReduced() && !settings.reduceMotion;
  return (
    <div class="set-scroll">
      <div class="set-content">
        <header class="set-head">
          <h2>Settings</h2>
          <p class="set-intro">The other settings move to the new client in a later phase; until then the Zerus app has them.</p>
        </header>
        <section class="set-group" aria-labelledby="set-sessions">
          <div class="set-group-head"><h3 id="set-sessions">Sessions</h3></div>
          <ToggleRow id="set-animate-activity" label="Animate working sessions" checked={settings.animateActivity} onChange={(on) => setSetting("animateActivity", on)}
            hint={motionReduced() ? "Reduce motion is on, so working sessions hold still." : undefined} />
        </section>
        <section class="set-group" aria-labelledby="set-motion">
          <div class="set-group-head"><h3 id="set-motion">Motion</h3></div>
          <ToggleRow id="set-reduce-motion" label="Reduce motion" checked={settings.reduceMotion} onChange={(on) => setSetting("reduceMotion", on)}
            hint={<>
              Stops endless motion: the Zerus mark, working sessions, loading indicators and the playing video note. Panels and menus appear without animating.
              <Show when={systemOnly()}><br />The system asks for reduced motion, so Zerus reduces it whatever this setting says.</Show>
            </>} />
        </section>
        <p class="set-foot">Appearance and session list preferences apply to this Zerus.</p>
        <div><button class="btn small" onClick={() => setPage("sessions")}>Back to Sessions</button></div>
      </div>
    </div>
  );
}
