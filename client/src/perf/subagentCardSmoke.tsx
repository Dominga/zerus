// Task 198 (W5): the subagent confirmation card beside Qt's u19-subagents
// shots (tray/tests/test_questioncard.cpp subagentCardListsEachSubagentAndAnswersTheCard):
// one row per subagent, Deny, "Allow all, don't ask again" and Allow all.
// The card view is the core's shape (core questions.rs card for a
// subagent_confirmation request); `?confirm=1` shows the second step.
// Synthetic: no CLI, no session; only the settings read is answered.
//   http://127.0.0.1:<port>/scripts/subagent-card-smoke.html?skin=whitewh-dark[&confirm=1]
const native = window as unknown as { __TAURI_INTERNALS__?: unknown };
native.__TAURI_INTERNALS__ = {
  transformCallback: () => 1,
  invoke: async (command: string) => {
    if (command === "settings_get") return { values: {}, revision: 0 };
    if (command.startsWith("plugin:event|")) return 1;
    throw new Error(`synthetic smoke refuses ${command}`);
  },
};

import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";
import { render } from "solid-js/web";
import { QuestionCard } from "../ui/sessions/QuestionCard";
import { questions } from "../store/questions";
import { applySkin, skin } from "../skin/runtime";

const text = (source: string, ...values: (string | number)[]) => ({ source, args: values.map((value) => typeof value === "number" ? { kind: "number", value } : { kind: "verbatim", value }) });
const button = (visible: boolean, enabled: boolean, label: string) => ({ visible, enabled, label: text(label), review_label: null });
const confirm = new URLSearchParams(location.search).get("confirm") === "1";
const view = {
  card: "sub-1", revision: 1, epoch: 1, asked: "", asked_tip: "", terminal_label: text("Open Terminal"), native_ui: false,
  view: {
    heading: text("Start %1 subagents", 3), progress: null, pending: null, pending_tip: text(""), nav: null, tabs_visible: false, page: 0,
    forms: [{ id: "q_0", tab: text("Question %1", 1), prompt: "Start 3 subagents", body: "", markdown: false, monospace_body: false, multi: false, mode: text("Select one"), mode_visible: false,
      options: [], other: null, text: null }],
    status: text("Allow all starts every subagent listed; Deny stops them all."), status_error: false, available: true, locked: false, read_only: false,
    review: false, created_at: 0,
    submit: button(false, false, "Submit answer"), approve: button(true, true, "Allow all"), deny: button(true, true, "Deny"),
    always: button(true, true, confirm ? "Confirm" : "Allow all, don't ask again"),
    subagents: [
      { kind: "Explore", model: "haiku", effort: "", task: "Read the payments service and list where cards are stored." },
      { kind: "code-reviewer", model: "opus", effort: "high", task: "Review the cursor pagination diff for missing tests." },
      { kind: "general-purpose", model: "sonnet", effort: "", task: "Draft the release notes for saved cards." },
    ],
    skip: button(false, false, "Skip"), previous: button(false, false, "← Back"), next: button(false, true, "Next →"), retry: false,
    skip_tip: text(""), retry_tip: text(""),
  },
};

window.addEventListener("error", (event) => { document.title = `failed: ${event.message}`; });
applySkin(skin());
const root = document.getElementById("root")!;
root.style.cssText = "display:flex;flex-direction:column;justify-content:flex-end;width:760px;height:520px;padding:12px;box-sizing:border-box";
render(() => <QuestionCard />, root);
// The composer's own effect applies its (empty) question while it settles; apply the card until it stays.
let tries = 0;
const show = () => {
  if (!document.querySelector(".qcard")) questions.apply(view as never);
  if (document.querySelector(".qcard") || ++tries > 40) setTimeout(() => { document.title = document.querySelector(".qcard") ? `${document.title}: ready` : "failed: no card"; }, 300);
  else setTimeout(show, 100);
};
setTimeout(show, 100);
