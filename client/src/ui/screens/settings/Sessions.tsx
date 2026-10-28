import { Show } from "solid-js";
import { flag, setSetting } from "../../../store/settings";
import { motionReduced } from "../../motion";
import { Group, SectionHead, ToggleRow } from "./ui";

// Settings > Sessions (tray/src/SettingsPage.h:97-107,
// QuestionNotificationSettings.h) as the approved mockup lays it out
// (design/webui/src/screens/settings/General.tsx), plus the client's Reduce
// motion: Qt reads workspace/reduceMotion but offers no control for it.

const toggle = (key: string) => (on: boolean) => void setSetting(key, on);

export function SessionsSection() {
  const systemOnly = () => motionReduced() && !flag("workspace/reduceMotion");
  return (
    <>
      <SectionHead title="Sessions" />
      <Group>
        <ToggleRow label="Expand the collapsed session list on hover" checked={flag("workspace/expandSessionsOnHover")} onChange={toggle("workspace/expandSessionsOnHover")} />
        <ToggleRow label="Hide projects without active sessions" checked={flag("workspace/hideEmptyProjects")} onChange={toggle("workspace/hideEmptyProjects")} />
        <ToggleRow label="Animate working sessions" checked={flag("workspace/animateActivity")} onChange={toggle("workspace/animateActivity")}
          hint={motionReduced() ? "Reduce motion is on, so working sessions hold still." : undefined} />
        <ToggleRow label="Show completed subagents" checked={flag("workspace/showCompletedSubagents")} onChange={toggle("workspace/showCompletedSubagents")}
          hint="Include agents whose latest turn has finished in the session list and its counts. Plan & agents always shows all reported agents. Agents with unknown status remain visible." />
        <ToggleRow label="Notify about optional questions" checked={flag("notifications/optionalQuestions")} onChange={toggle("notifications/optionalQuestions")}
          hint="Off by default. Shows a system notification once for each new optional question, which the agent keeps working without. It is withdrawn when the questions are answered or skipped, or the session is marked as read. Questions that need an answer always notify." />
      </Group>
      <Group title="Motion">
        <ToggleRow label="Reduce motion" checked={flag("workspace/reduceMotion")} onChange={toggle("workspace/reduceMotion")}
          hint={<>
            Stops endless motion: the Zerus mark, working sessions, loading indicators and the playing video note. Panels and menus appear without animating.
            <Show when={systemOnly()}><br />The system asks for reduced motion, so Zerus reduces it whatever this setting says.</Show>
          </>} />
      </Group>
      <p class="set-foot">Closing Zerus keeps agents running. Projects and the arrangement of your workspace are saved on this device.</p>
    </>
  );
}
