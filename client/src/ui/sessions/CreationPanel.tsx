import { Show, createEffect, createSignal } from "solid-js";
import { tr } from "../../i18n";
import { BUTTON, PROGRESS, createDetailsView, fixFolderRights, panelButtons, panelStatus, sandboxFolderOf } from "./creations-ui";
import { cardVersion, creationDetailsText, editChoices, hideCreation, returnToFilters, runCreationAction, selectedCard } from "../../store/creations";
import { now, toast } from "../../store/ui";
import { sandboxFolderGrant } from "../../bridge";
import { teamConfirm } from "../../store/teamConfirm";
import type { CreationCard } from "../../types";
import "../styles/creations.css";

// The creation progress view in the detail pane while a card is selected
// (SessionsWindow.cpp: the panel 1509-1537, renderCreation 3729-3756). The
// title is the tag, with "{caption} ({m}:{ss})" under it (D6); the details
// come from the core (`creation_details`), read again when the card
// changes and every second (the controller's clock, 65), keeping their
// scroll; the buttons follow Qt's grid (1522-1524). Each button acts on the
// operation shown. Nothing here opens a composer or touches drafts.

export function CreationPanel() {
  let text: HTMLTextAreaElement | undefined;
  let details: { element: HTMLTextAreaElement; view: ReturnType<typeof createDetailsView> } | undefined;
  const card = selectedCard;
  const id = () => card()?.id;
  const [fixing, setFixing] = createSignal<"idle" | "fixing" | "elevated" | "fixed">("idle");

  // Task #247: Qt's Fix folder rights (SessionsWindow.cpp:2007-2035): grant
  // the Windows user full control of the folder a DeepSeek sandbox error
  // names; a folder another account owns asks Windows for an administrator
  // (the second call's --elevated), and either way the session starts again.
  const grant = async (shown: CreationCard, elevated: boolean): Promise<void> => {
    const folder = sandboxFolderOf(shown.error);
    if (!folder) return;
    const confirmed = elevated
      ? await teamConfirm.ask({
          name: "creationGrantAdmin", title: tr("Grant as administrator"),
          text: tr("Another account owns\n%1\nso your user cannot change its rights.\n\nWindows will ask an administrator to approve adding full control for your user to this folder. Nothing else changes and no ownership is taken.", folder),
          action: tr("Yes"),
        })
      : await teamConfirm.ask({
          name: "creationFixAccess", title: tr("Fix folder rights"),
          text: tr("Give your Windows user full control of\n%1\n\nThe DeepSeek sandbox needs it to start a session there. Only your own access to this folder changes.", folder),
          action: tr("Yes"),
        });
    if (!confirmed) return;
    setFixing(elevated ? "elevated" : "fixing");
    try {
      const result = await sandboxFolderGrant(folder, elevated);
      if (!result.ok) {
        const error = result.error || tr("unknown error");
        if (!elevated && (error.includes("os error 5") || error.includes("another account owns it"))) {
          setFixing("idle");
          await grant(shown, true);
          return;
        }
        setFixing("idle");
        toast(tr("The folder rights were not changed: %1", error), "", "error", true);
        return;
      }
      setFixing("fixed");
      toast(tr("Folder rights fixed; starting the session again."), "");
      await runCreationAction(shown.id, shown.can_resume ? "resume" : "check");
    } catch (error) {
      setFixing("idle");
      toast(tr("The folder rights were not changed: %1", String(error)), "", "error", true);
    }
  };

  createEffect(() => {
    if (!text) return;
    if (details?.element !== text) details = { element: text, view: createDetailsView(text) };
    const current = id();
    if (current !== undefined) {
      cardVersion(current);
      now();
    }
    const answer = details.view.read(current);
    if (current === undefined) return;
    creationDetailsText(current).then(answer, () => {});
  });

  return (
    <section class="detail cr-detail">
      <Show when={card()}>{(c) => {
        const buttons = () => panelButtons(c());
        return (
          <>
            <header class="cr-head">
              <h1 class="cr-title">{c().tag}</h1>
              <p class="cr-status tabular">{panelStatus(c(), now())}</p>
            </header>
            <textarea class="field cr-text" readonly aria-label={tr(PROGRESS)} ref={text} />
            <div class="cr-actions">
              <Show when={buttons().resume}><button class="btn c1" onClick={() => void runCreationAction(c().id, "resume")}>{tr(BUTTON.resume)}</button></Show>
              <Show when={buttons().check}><button class="btn c2" onClick={() => void runCreationAction(c().id, "check")}>{tr(BUTTON.check)}</button></Show>
              <Show when={buttons().stop}>
                <button class="btn wide" disabled={!buttons().stopEnabled} onClick={() => void runCreationAction(c().id, "stop")}>{buttons().stopText}</button>
              </Show>
              <Show when={buttons().edit}><button class="btn c1" onClick={() => void editChoices(c().id)}>{tr(c().worktree_fenced ? BUTTON.recreate : BUTTON.edit)}</button></Show>
              <button class="btn c2" onClick={() => void hideCreation(c().id)}>{tr(BUTTON.hide)}</button>
              <button class="btn wide" onClick={returnToFilters}>{tr(BUTTON.back)}</button>
              <Show when={fixFolderRights(c())}>
                <button class="btn wide" data-action="creation-fix-folder-rights" disabled={fixing() !== "idle" && fixing() !== "fixed"}
                  title={tr("Give your Windows user full control of this folder, which the DeepSeek sandbox needs")}
                  onClick={() => void grant(c(), false)}>
                  {fixing() === "fixing" ? tr(BUTTON.fixing) : fixing() === "elevated" ? tr(BUTTON.elevated) : fixing() === "fixed" ? tr(BUTTON.fixed) : tr(BUTTON.fix)}
                </button>
              </Show>
            </div>
          </>
        );
      }}</Show>
    </section>
  );
}
