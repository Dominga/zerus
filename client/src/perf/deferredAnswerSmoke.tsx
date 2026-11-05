// The real Composer and question card in a deferred answer's states
// (zerus-5wmh.66.4.7), with synthetic views only. The base view is the card
// the app's own Rust built for state 2 (`composer::questions::handed`); each
// other state changes only what its Rust view changes. State 2: the desktop
// service still sends the answer (a normal status, locked, no Allow retry).
// States 4 and 5: it was not sent (never started, refused). State 6: it was
// not confirmed (uncertain, Allow retry). No CLI, service, session or file is
// used. Served by Vite, outside the app entry:
//   npx vite --port <port> --strictPort, then load
//   http://127.0.0.1:<port>/scripts/deferred-answer-smoke.html?state=2&skin=zerus-light
// (add &lang=ru for the Russian interface) in WebKitGTK; the page posts
// "ready" (or its failures) to `result`.
import { render } from "solid-js/web";
import { applyBatch, toSession } from "../store/catalog";
import { setSelectedKey } from "../store/ui";
import { emptyBatch, type ComposerView, type QuestionView } from "../types";
import { applySkin, skin } from "../skin/runtime";
import { installUiLanguage, tr, uiLanguage } from "../i18n";
import { setSetting, settingsReady } from "../store/settings";
import "../ui/styles/base.css";
import "../ui/styles/app.css";
import "../ui/styles/detail.css";
import "../ui/styles/client.css";

const STILL = "The desktop service is still sending this answer. Its result appears here when it finishes.";
const NOT_STARTED = "The answer was not sent: the desktop service could not start it. The question may have changed. Your answer was kept, so you can submit it again.";
const REFUSED = "The answer was not sent: it was refused before anything reached the agent. Your answer was kept, so you can submit it again.";
const UNCONFIRMED = "Delivery is not confirmed by the desktop service. Check Terminal before retrying.";
const params = new URLSearchParams(location.search);
const russian = params.get("lang") === "ru";
const requested = Number(params.get("state"));
const state = [2, 4, 5, 6].includes(requested) ? requested : 2;
const report = (value: unknown) => (window as unknown as { webkit?: { messageHandlers?: { result?: { postMessage(text: string): void } } } })
  .webkit?.messageHandlers?.result?.postMessage(JSON.stringify(value));
const row = { name: "claude/sample-project/review", agent: "claude", state: "running", status: "input", run_id: "run-1", conversation_id: "conv-1", cwd: "/example/review" };
const key = "run:run-1";
// State 2's view, as Rust built it: its status is the catalog source, which
// the card translates.
const base: QuestionView = {
 "answering": true,
 "card": "question\n[\"\\nclaude/sample-project/review\",\"tool:t1\",\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"]\nrun-1\nconv-1",
 "revision": 1,
 "epoch": 2,
 "asked": "Asked 10:33",
 "asked_tip": "",
 "terminal_label": {
  "source": "Open Terminal"
 },
 "native_ui": false,
 "view": {
  "heading": {
   "source": "Agent needs your answer"
  },
  "progress": {
   "source": "%1 of %2 answered",
   "args": [
    {
     "kind": "number",
     "value": 1
    },
    {
     "kind": "number",
     "value": 1
    }
   ]
  },
  "pending": {
   "source": "%1 pending",
   "args": [
    {
     "kind": "number",
     "value": 1
    }
   ]
  },
  "pending_tip": {
   "source": "Questions waiting for an answer, including the questions shown in this card."
  },
  "nav": null,
  "tabs_visible": false,
  "page": 0,
  "forms": [
   {
    "id": "q_0",
    "tab": {
     "source": "✓ %1",
     "args": [
      {
       "kind": "text",
       "value": {
        "source": "%1: %2",
        "args": [
         {
          "kind": "number",
          "value": 1
         },
         {
          "kind": "verbatim",
          "value": "Checks"
         }
        ]
       }
      }
     ]
    },
    "prompt": "Which checks?",
    "body": "",
    "markdown": true,
    "monospace_body": false,
    "multi": false,
    "mode": {
     "source": "Select one"
    },
    "mode_visible": true,
    "options": [
     {
      "id": "opt_0_0",
      "label": {
       "source": "%1",
       "args": [
        {
         "kind": "verbatim",
         "value": "Unit"
        }
       ]
      },
      "description": {
       "source": "%1",
       "args": [
        {
         "kind": "verbatim",
         "value": ""
        }
       ]
      },
      "markdown": false,
      "checked": true,
      "enabled": false,
      "visible": true
     },
     {
      "id": "opt_0_1",
      "label": {
       "source": "%1",
       "args": [
        {
         "kind": "verbatim",
         "value": "UI"
        }
       ]
      },
      "description": {
       "source": "%1",
       "args": [
        {
         "kind": "verbatim",
         "value": ""
        }
       ]
      },
      "markdown": false,
      "checked": false,
      "enabled": false,
      "visible": true
     }
    ],
    "other": {
     "id": "",
     "label": {
      "source": "Other"
     },
     "description": {
      "source": "Write your own answer"
     },
     "markdown": false,
     "checked": false,
     "enabled": false,
     "visible": true
    },
    "text": {
     "visible": false,
     "enabled": false,
     "value": "",
     "placeholder": {
      "source": "Write your answer…"
     },
     "accessible_name": {
      "source": "Your answer to %1",
      "args": [
       {
        "kind": "verbatim",
        "value": "Which checks?"
       }
      ]
     },
     "max_units": 4096,
     "max_chars": 0
    }
   }
  ],
  "status": {
  "source": "The desktop service is still sending this answer. Its result appears here when it finishes."
 },
  "status_error": false,
  "available": true,
  "locked": true,
  "review": false,
  "created_at": 1793000000.0,
  "submit": {
   "visible": true,
   "enabled": false,
   "label": {
    "source": "Submitting…"
   },
   "review_label": null
  },
  "approve": {
   "visible": false,
   "enabled": false,
   "label": {
    "source": "Approve once"
   },
   "review_label": {
    "source": "Approve once"
   }
  },
  "deny": {
   "visible": false,
   "enabled": false,
   "label": {
    "source": "Deny"
   },
   "review_label": null
  },
  "skip": {
   "visible": false,
   "enabled": false,
   "label": {
    "source": "Skip"
   },
   "review_label": null
  },
  "previous": {
   "visible": false,
   "enabled": false,
   "label": {
    "source": "← Back"
   },
   "review_label": null
  },
  "next": {
   "visible": false,
   "enabled": false,
   "label": {
    "source": "Next →"
   },
   "review_label": null
  },
  "retry": false,
  "skip_tip": {
   "source": "Dismiss this optional question without sending an answer. The agent can continue."
  },
  "retry_tip": {
   "source": "Check the agent in Terminal before retrying to avoid submitting twice."
  }
 }
} as unknown as QuestionView;
function question(): QuestionView {
  const q: QuestionView = JSON.parse(JSON.stringify(base));
  if (state === 2) return q;
  const status = state === 4 ? NOT_STARTED : state === 5 ? REFUSED : UNCONFIRMED;
  q.answering = false;
  q.view.status = { source: status };
  q.view.status_error = true;
  q.view.locked = false;
  for (const option of q.view.forms[0].options) option.enabled = true;
  if (q.view.forms[0].other) q.view.forms[0].other.enabled = true;
  q.view.next.enabled = true;
  q.view.submit.label = { source: "Submit answer" };
  q.view.submit.enabled = state !== 6;
  q.view.retry = state === 6;
  return q;
}
const view = {
  handle: 1, key: "\nclaude/sample-project/review", loaded: true, rev: 1, text_epoch: 1,
  text: "", position: 0, anchor: 0, question: question(), input_visible: false, queue: null, attachments: [], removed: null,
  attaching: false, sending: false, waiting: null, uncertain: false, delivery_pending: false, status: "", status_error: false,
  can_send: false, send_label: { source: "Send" }, available: true, unavailable_reason: "", block_reason: null,
  attachments_available: true, attachments_reason: "", session_status: "", session_status_error: false, session_blocks_input: false,
  inspect_error: false, cache: { visible: false, tone: "quiet", label: "", short_label: "" }, cache_warning: "", cache_details: "",
  context: { visible: false, text: "", tooltip: "", warning: false, tone: "quiet", percent: null, accessible_name: "" }, local: [],
  cancel_send: false, compacting: false,
  clear: { enabled: true, dismiss: false, title: "Clear session?", text: "", confirm: "Clear session", run: "run-1", conversation: "conv-1",
    cache_enabled: true, running: false, reason: "", cache_reason: "" },
} as unknown as ComposerView;
// Only reads answer; nothing is written anywhere.
Object.assign(window, { __TAURI_INTERNALS__: { invoke: async (command: string) => {
  if (command === "composer_open") return view;
  return null;
} } });

async function run() {
  if (russian) {
    // The interface language through the settings, as the app applies it.
    await settingsReady;
    setSetting("workspace/language", "ru", { persist: false });
    installUiLanguage();
    for (let i = 0; i < 100 && uiLanguage() !== "ru"; i++) await new Promise((resolve) => setTimeout(resolve, 20));
  }
  applySkin(skin());
  applyBatch({ ...emptyBatch(), computers: [{ computer: "", status: "live", attempt: 0 }], upserts: [{ computer: "", key, row }] });
  setSelectedKey(toSession("", key, row).key);
  const { composer } = await import("../store/composer");
  composer.apply(view);
  const { Composer } = await import("../ui/sessions/Composer");
  const { QuestionCard } = await import("../ui/sessions/QuestionCard");
  const root = document.getElementById("root")!;
  root.style.cssText = "width:880px;padding:16px;box-sizing:border-box";
  // As the session detail places them: the card above the composer.
  render(() => <div class="cmp-col"><QuestionCard /><Composer /></div>, root);
  await new Promise((resolve) => setTimeout(resolve, 600));
  const buttons = [...document.querySelectorAll("button")];
  const retry = buttons.some((b) => b.textContent?.includes(tr("Allow retry")));
  const submit = buttons.find((b) => b.classList.contains("primary"));
  const status = document.querySelector(".qcard-status");
  const failures: string[] = [];
  if (russian && uiLanguage() !== "ru") failures.push("language");
  // In Russian the status is the catalog's translation, never its source.
  if (russian && status?.textContent === question().view.status.source) failures.push("status untranslated");
  if (retry !== (state === 6)) failures.push(`Allow retry shown=${retry}`);
  if (!status || status.classList.contains("error") !== (state !== 2)) failures.push("status style");
  if (!submit || submit.disabled !== (state === 2 || state === 6)) failures.push("submit");
  report({ ready: true, state, language: uiLanguage(), failures, status: status?.textContent ?? null, submit: submit?.textContent ?? null });
}
run().catch((error) => report({ ready: false, failures: [String(error)] }));
