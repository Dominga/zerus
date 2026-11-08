# Qt to Zerus Next parity checklist

This checklist lists the user-facing capabilities of the retired Qt GUI and their
status in Zerus Next, the Tauri client in `client/`. It supports the owner's
decision to make Zerus Next the default GUI on every platform. It switches
nothing and changes no Qt code. Gaps are tracked as child beads of
`zerus-5wmh.13`, or by an existing bead where one already covers them.

The [Qt retirement inventory](superpowers/specs/2026-10-29-qt-retirement-inventory.md)
maps Qt files to their owners and beads. This checklist instead records what a
person can do in each GUI today. Where they disagree, this checklist is the
newer reading: for example, the upgrade no longer imports Qt drafts or
attention records (owner scope of 2026-11-05, PR #1469).

## Baseline and reading guide

- Zerus Next: `main` at `359514526` (2026-11-08).
- Qt: the `tray/src` tree at `f5a822649^`, the last commit before the Qt sources
  were removed. Read a file with `git show f5a822649^:tray/src/<file>`.
- Evidence paths are shortened: `ui/` is `client/src/ui/`, `store/` is
  `client/src/store/`, `core/` is `client/core/src/`, `tauri/` is
  `client/src-tauri/src/` and `App.tsx` is `client/src/App.tsx`. Other paths are
  relative to the repository root. Qt file names in the last column are in
  `tray/src` at `f5a822649^`.
- Rows describe Zerus Next in its default native alert mode unless a row says
  otherwise.

Statuses:

| Status | Meaning |
| --- | --- |
| done | Works in Zerus Next; the evidence column points at the code. A note in brackets names an addition Qt did not have. |
| partial | Part of the Qt behaviour is present; the evidence says what is missing. |
| **missing** | No Zerus Next equivalent. |
| unverified | The code path is not obvious and needs a live check. |
| removed by owner decision | Deliberately not carried over; the row names the decision. |

Every partial, missing and unverified row names the bead that tracks it.

## Summary

| Area | done | partial | missing | other |
| --- | --- | --- | --- | --- |
| Sessions list, row actions and header | 22 | 2 | 5 | 2 owner decisions |
| Archive and search | 10 | 4 | 1 | |
| Composer and sending | 29 | 4 | 2 | |
| Activity | 10 | 5 | 2 | |
| Questions and approvals | 12 | | | |
| Notifications | 6 | 6 | | |
| Tray, menu bar and Dock | 6 | 2 | 1 | 2 owner decisions |
| Shortcuts and navigation | 6 | | | |
| Lifecycle and updates | 6 | 2 | 1 | |
| Settings | 17 | | 1 | |
| Machines, accounts, projects, new session, tasks and worktrees | 18 | | | |
| Terminal, processes and platform session support | 6 | | 1 | 1 unverified |
| Total (192 rows) | 148 | 25 | 14 | 5 |

The gaps a person is likely to meet first in daily use are the interrupted
prompt that does not come back after Stop (`zerus-5wmh.13.4`), short tray
messages that never appear on Linux and Windows (`zerus-5wmh.13.13`) and the
missing Terminal Copy and Paste menu (`zerus-5wmh.13.12`).

## Owner decisions

| Qt capability | Zerus Next | Source |
| --- | --- | --- |
| Footer summary (N shown, N working, N attention) | not shown; the filters above the list say what is listed | owner feedback of 2026-11-07, PR #1685 (`7efb78f2b`) |
| Per-session rows in the tray menu | one count line per machine | owner, 0.48.0 (`core/menubar.rs:639-678`) |
| Open in terminal-mode submenu and chooser | a saved or `tray.conf` mode is still read | owner, 0.48.0 (`tauri/desk.rs:279-280`) |
| Upgrade import of Qt state other than projects and organisation (drafts, attention records, pending operations) | not imported; Qt's files stay untouched and a one-time notice explains what to set up again | owner scope of 2026-11-05, PR #1469 (`9164d784a`) |
| Row menu: Pause, Resume, Restore, Fork, Move to archive, folder and shell, running commands, native DeepSeek UI, copy name, reference and folder | in the detail header's Session actions menu; the row menu keeps the owner's ten actions | owner request of 2026-11-07, `zerus-423fh` (`00c72fa7f`) |

## Gap beads

| Bead | Priority | Gap |
| --- | --- | --- |
| `zerus-5wmh.13.4` | P2 | After Stop, return the interrupted prompt to an empty message field |
| `zerus-5wmh.13.5` | P2 | Reference an existing file from the composer |
| `zerus-5wmh.13.6` | P2 | OpenCode queued-message stamps, waiting hint and Remove |
| `zerus-5wmh.13.12` | P2 | Built-in Terminal Copy and Paste menu; verify keyboard copy |
| `zerus-5wmh.13.13` | P2 | Short tray messages call `osascript` on Linux and Windows |
| `zerus-5wmh.13.14` | P2 | Windows: focus-aware banners, update alert, tray theme, watchdog, startup note |
| `zerus-5wmh.13.7` | P3 | Composer and voice small controls |
| `zerus-5wmh.13.8` | P3 | Activity rendering: full tool output, View process, Markdown, inline-code links, empty state |
| `zerus-5wmh.13.10` | P3 | Session list keyboard navigation |
| `zerus-5wmh.13.11` | P3 | Compact-row tooltip, global Compact session rows, saved-session terminal, batch folder removal |
| `zerus-5wmh.13.15` | P3 | Tray Refresh entry, Linux without a tray host, Linux launch during service start |
| `zerus-5wmh.13.16` | P3 | Alert text language and the in-window 99+ cap |
| `zerus-5wmh.13.17` | P3 | Search result refinements after the search port (`zerus-5wmh.66.2.2`) |
| `zerus-5wmh.18` | existing | Selection Move to project, with project organisation |
| `zerus-5wmh.66.10` | existing | Desktop presence for phone alerts |

Keyboard commands for every area are listed once, under Shortcuts and navigation.

## Sessions list, session actions, archive and search

### Sessions list and rows

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| List heading switches between Sessions and Archive; New session in the header | done | `ui/sessions/SessionList.tsx:650,652` | `SessionsWindow.cpp:338,378` |
| Collapse the list to a strip; hover opens it; keep it open; width splitter | done | `ui/sessions/SessionList.tsx:586-602,639-653,721-739,944` | `SessionsWindow.cpp:335,380` |
| Session and machine actions menu: Pause all and Resume all per machine, Expand all collapsed, Mark all as read, Clear archive | done | `ui/sessions/SessionList.tsx:294-301`; `ui/sessions/machine-batch.ts:57-70` | `SessionsWindow.cpp:2844-2892` |
| List background menu and Sessions rail menu | done | `ui/sessions/background-menu.ts:37-52`; `ui/components/Rail.tsx:47` | `SessionsWindow.cpp:340-373` |
| State filters with counts; machine, project and account pickers; removable chips; folder chip | done | `ui/sessions/SessionList.tsx:72-78,347-397,667-682` | `SessionsWindow.cpp:400-423`; `MachineFilter.cpp`; `ProjectFilter.cpp`; `AccountFilter.cpp` |
| Project headers: collapse, counters, tooltip; header menu (New session, Filter, Rename, Settings, Move up and down, Sort by name, Delete) | done | `ui/sessions/SessionList.tsx:281-324`; `ui/sessions/organization-ui.ts:58-80` | `SessionsWindowRows.cpp:411`; `SessionsWindow.cpp:2568-2616` |
| Drag to reorder projects and sessions and move sessions between projects; drop a task on a session | done | `ui/sessions/SessionList.tsx:742-805,864-867` | `SessionList.cpp:540-583`; `SessionsWindow.cpp:555-561` |
| Session card: status with working clock, unread mark, question chip, folder and branch, Git marks, machine, model, account, mute and review-later marks | done | `ui/sessions/SessionCard.tsx:372-425` | `SessionCardDelegate.h` |
| Subagent toggle; child rows open subagent Activity | done | `ui/sessions/SessionCard.tsx:319-330`; `ui/sessions/SessionList.tsx:850-856` | `SessionList.cpp:415-423` |
| Compact and expanded rows; double-click toggles | done | `ui/sessions/SessionList.tsx:870-871`; `ui/sessions/SessionCard.tsx:557` | `SessionList.cpp:184-196,371-393` |
| Compact-row tooltip with the hidden state, detail, folder, machine and Git lines | partial (`zerus-5wmh.13.11`) | the title tooltip shows the name only: `ui/sessions/SessionCard.tsx:553` | `SessionList.cpp:245-249` |
| Strip tiles: tooltip and context menu | done | `ui/sessions/SessionList.tsx:1023-1024` | `SessionList.cpp:267-286` |
| Ctrl or Cmd-click and Shift-click selection; Esc clears it | done | `ui/sessions/SessionList.tsx:326-337,603-608` | `SessionList.cpp:296-312,588` |
| Keyboard range selection with Shift+Up, Down, Home, End | **missing** (`zerus-5wmh.13.10`) | `ui/sessions/SessionList.tsx:614-637` handles plain arrows only | `SessionList.cpp:589-602` |
| Up and Down move; Right and Left expand or collapse a row and go to the parent | done | `ui/sessions/SessionList.tsx:614-633` | `SessionList.cpp:604-622` |
| Left and Right on a plain row collapse or expand its project; Home and End | **missing** (`zerus-5wmh.13.10`) | no project branch in the list key handler (`ui/sessions/SessionList.tsx:624-633`) | `SessionList.cpp:623-627` |
| Enter on a row jumps to the latest Activity | **missing** (`zerus-5wmh.13.10`) | Enter is handled in search results only | `SessionsWindow.cpp:565,5499-5504` |
| Selection bar and menu: Mark read, Mark needs attention, Compact, Expand, Pause, Resume or Restore, Archive, Forget and Terminate with confirmation | done | `ui/sessions/selection.ts:11-31,113-136`; `store/batch.ts:134-145`; `ui/sessions/ActionDialogs.tsx:326-345` | `SessionSelection.cpp:52-218,258-286` |
| Selection Move to project | **missing** (`zerus-5wmh.18`) | disabled with "Arrives with project organisation." (`ui/sessions/selection.ts:19`; `ui/sessions/selection-ui.ts:59-66`); a single row can move | `SessionSelection.cpp:184-195` |
| Selection Terminate: remove working folders after stopping | **missing** (`zerus-5wmh.13.11`) | the batch dialog has no such option (`ui/sessions/selection.ts:111-112`) | `SessionSelection.cpp:274-283` |
| Footer: creation status, Mark all as read, Saved drafts | done | `ui/sessions/SessionList.tsx:702-712` | `SessionsWindow.cpp:459-489` |
| Footer summary (N shown, N working, N attention) | removed by owner decision | dropped on owner feedback of 2026-11-07 (PR #1685) | `SessionsWindowRows.cpp:473-479` |
| Creation rows with Hide launch status | done | `ui/sessions/CreationCard.tsx:26-31,35` | `SessionsWindow.cpp:502-511` |

### Session row actions and the detail header

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Row menu: Compact or Expand, Copy connect or restore command, Mute, Mark read or needs attention, Start fresh, Clear, Rename, Move to project, Terminate and Forget | done | `ui/sessions/SessionList.tsx:211-253`; `ui/sessions/ActionDialogs.tsx:77-130`; `ui/sessions/mark-entries.ts:45-51`; `ui/sessions/OrganizationDialogs.tsx:25-42` | `SessionsWindow.cpp:5291-5380` |
| Row menu: Pause, Resume, Restore; Fork; Move to archive; Open folder and shell; Show running commands; native DeepSeek UI; Copy name, reference and folder | removed by owner decision | in the header menu (`ui/sessions/SessionDetail.tsx:106-126`); the row menu keeps the owner's ten actions (`zerus-423fh`, 2026-11-07) | `SessionsWindow.cpp:5295-5376` |
| Open terminal for a saved session (resume in a terminal after confirmation) | partial (`zerus-5wmh.13.11`) | running sessions only: `store/sessionTerminal.ts:14-17` | `SessionsWindow.cpp:5295,5466-5476` |
| Header: title, loading indicator, status, provider and machine, folder, branch, worktree, model | done | `ui/sessions/SessionDetail.tsx:130-157` | `SessionsWindow.cpp:575-662` |
| Header icons and Session actions menu (folder, shell, copy IDs, Rename, Fork, connect command, Pause or Resume, Clear, Start fresh, archive, Terminate) | done | `ui/sessions/SessionDetail.tsx:106-142` | `SessionsWindow.cpp:608-654` |
| Goal button; account usage for the selected session | done | `ui/sessions/SessionDetail.tsx:299`; `ui/components/SessionUsageStrip.tsx:7-11` | `SessionsWindow.cpp:663-676` |
| Rename, Terminate (with Remove working folder), Start fresh and Fork dialogs | done | `ui/sessions/ActionDialogs.tsx:95-262`; `ui/sessions/actions-ui.ts:392-398` | `SessionsWindow.cpp:4672-4704,5698-5828` |
| Worktree cleanup offer and review after archive or terminate | done | `store/cleanupNotices.ts:29-36`; `tauri/actions.rs:1181` | `SessionsWindow.cpp:1840,1916-1995` |

### Archive

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Archive filter with count; date picker with presets and a checked custom range; Refresh | done | `ui/sessions/ArchiveBar.tsx:35,66-159`; `ui/sessions/archive-dates.ts:18,114` | `ArchiveDatePicker.cpp`; `ArchiveBrowser.cpp:17-19` |
| Status, Newer and Older paging, per-machine errors | done | `ui/sessions/ArchiveBar.tsx:39-55`; `store/archive.ts:104-131` | `ArchiveBrowser.cpp:23-80` |
| Archived rows grouped by project and filtered; archive counts in pickers | done | `store/archive.ts:182-196,446-449`; `ui/sessions/archive-filters.ts:5-27` | `SessionsWindowRows.cpp:266-300` |
| Search archive names and IDs with paging | partial (`zerus-5wmh.13.17`) | the query reaches `hgs` (`store/archive.ts:373,440`), but while searching the status and Newer and Older are hidden (`ui/sessions/ArchiveBar.tsx:39,53`), so matches past the loaded page are unreachable | `SessionsWindow.cpp:386,410`; `ArchiveBrowser.cpp:58` |
| Read-only recorded Activity for an archived session | done | `ui/sessions/SessionDetail.tsx:166-173`; `store/archiveActivity.ts:206` | `SessionsWindow.cpp:3434-3716` |
| Restore, blocked when the name is live; copy restore command; rename; Forget; Clear archive | done | `ui/sessions/SessionDetail.tsx:141`; `store/restores.ts:195`; `ui/sessions/actions-ui.ts:154-247,355`; `ui/sessions/archive-entries.ts:14-45` | `SessionsWindow.cpp:371-373,618,1958-1966,3148-3154` |

### Search

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Search box for sessions and archive; Esc clears; strip search button | done | `ui/sessions/SessionList.tsx:656-666,956` | `SessionsWindow.cpp:382-395` |
| Matching on name, machine, model, effort, folder, branch, worktree, session, run and archive IDs | done | `store/content-search-model.ts:85-90` | `SessionSearch.cpp:239,274-275` |
| Child conversation IDs match and the result says which ID matched | partial (`zerus-5wmh.13.17`) | only subagent IDs match; the result shows no matched ID (`store/content-search-model.ts:88`; `ui/sessions/SearchResults.tsx:49-51`) | `SessionSearch.cpp:239-268` |
| Message content search across machines, honoring the filters, with status and Search again | done | `store/contentSearch.ts:22-71`; `store/content-search-model.ts:27-79` | `SessionSearch.cpp:75-213,309-314` |
| One result row per matching message | partial (`zerus-5wmh.13.17`) | one row per session with the newest excerpt only (`store/content-search-model.ts:94-104`) | `SessionSearch.cpp:276-282` |
| Result rows with highlights, badges, author and time; keyboard navigation; opening a hit shows the saved message | done | `ui/sessions/SearchResults.tsx:26-55`; `store/contentSearch.ts:72-84` | `SessionSearch.cpp:132,253-262`; `SessionsWindow.cpp:434-456` |
| Back to latest activity while a search result is shown | partial (`zerus-5wmh.13.17`) | the button appears only when not following (`ui/sessions/Composer.tsx:288`); otherwise the preview ends when the search is cleared | `ActivityView.cpp:1631-1636` |
| Pending session creations in search results | **missing** (`zerus-5wmh.13.17`) | `store/contentSearch.ts:40-57` builds session items only | `SessionSearch.cpp:119,287-305` |
| Go to popup (Ctrl+K) | done | `App.tsx:121`; `ui/sessions/GoToDialog.tsx:37` | `SessionsWindowGoTo.cpp` |

## Composer, Activity, questions and approvals

### Composer and sending

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Enter sends, Shift+Enter adds a line, Ctrl+Enter, nothing acted on while an input method composes | done | `ui/sessions/Composer.tsx:215-229,250`; `store/shortcuts.ts:45,63` | `MessageComposer.cpp:218-229,399-406` |
| Escape stops a working turn, otherwise leaves the field | done | `ui/sessions/Composer.tsx:208-214,228` | `MessageComposer.cpp:219-222` |
| Draft per session with cursor and selection, restored on return and after restart | done | `store/composer.ts:279-309,428-461`; `ui/sessions/Composer.tsx:195-206`; `tauri/exit_flush.rs` | `MessageComposer.cpp:366-388,480-548`; `ComposerDraftStore.cpp` |
| Draft status on the session row | done | `ui/sessions/SessionCard.tsx:49,74,86` | `SessionsWindow.cpp:1222-1243` |
| Saved drafts: list, preview, Copy text, Restore with replace confirmation, Go to session, uncertain-send warning | done (adds Recover) | `ui/sessions/SavedDrafts.tsx:279-288` | `MessageComposer.cpp:1230-1387` |
| Editor resize grip: drag, double-click or Home resets, Up/Down by 20 px, height remembered | done | `ui/sessions/Composer.tsx:40-110` | `MessageComposer.cpp:71-122,283-290,449-468` |
| Send while the agent works: queue panel, Send now (button, Ctrl+Shift+Enter) | done | `ui/sessions/QueuePanel.tsx:12-39`; `ui/sessions/SessionDetail.tsx:276`; `core/input_queue.rs` | `ActivityView.cpp:1281-1305,1895`; `SessionsWindow.cpp:1859,1868` |
| Send and push now; with an empty field it sends the queue now | done | `ui/sessions/Composer.tsx:255-260`; `store/composer.ts:381-421` | `MessageComposer.cpp:830-854` |
| Stop (interrupt) with its result notices | done | `ui/sessions/Composer.tsx:337-340`; `store/actions.ts:77-80` | `MessageComposer.cpp:315-316,550-554`; `SessionsWindow.cpp:1187-1215` |
| After Stop, the interrupted turn's prompt returns to an empty message field | **missing** (`zerus-5wmh.13.4`) | `tauri/composer/send.rs:3252` only cancels a waiting send; no UI offers the interrupted prompt | `SessionsWindow.cpp:1194-1197,2279-2307` |
| Status line: block reasons, disabled-Send hint, Refresh this session | done | `ui/sessions/Composer.tsx:315,324-331`; `store/composer.ts:124-134,390-395` | `MessageComposer.cpp:308-311,811-819,1141-1227` |
| 64 KiB message limit | done | `store/composer.ts:35-38,193-198` | `MessageComposer.cpp:1146,1204` |
| Duplicate safeguard: an unconfirmed delivery locks Send until Allow retry | done | `ui/sessions/Composer.tsx:332-335`; `ui/sessions/composer-ui.ts:42-43` | `MessageComposer.cpp:306-307,412-417,1185,1193` |
| Failed message in Activity: Retry, Delete with confirmation, Check Terminal or native UI | done | `ui/sessions/Activity.tsx:499-560`; `store/composer.ts:562-591` | `ActivityView.cpp:1053-1068`; `SessionsWindow.cpp:4469-4526` |
| Attach from a file picker (8 files, 10 MiB each, 20 MiB total), `[File #n]` and `[Image #n]` at the cursor | done | `ui/sessions/Composer.tsx:318-321`; `core/attachments.rs:143-149` | `MessageComposer.cpp:856-863,969-1000` |
| Attachment chips: thumbnails, size, preview, guarded open, remove with its reference, keyboard use | done (adds Undo) | `ui/sessions/Attachments.tsx:26-131` | `MessageComposer.cpp:1008-1124` |
| Paste an image (32-megapixel limit) | done | `ui/sessions/Composer.tsx:230-237`; `store/composer.ts:471-485` | `MessageComposer.cpp:197-202,393-398` |
| Paste files copied in a file manager; drop raw image data on the editor | partial (`zerus-5wmh.13.7`) | paste accepts image blobs only (`ui/sessions/Composer.tsx:230-237`); drops carry file paths only (`store/drops.ts:158-164`) | `MessageComposer.cpp:186-196,230-246` |
| Drag and drop on the session panel: attach, or paste paths into Terminal, with an overlay hint | done | `store/drops.ts:9-49,158-164`; `ui/sessions/SessionDetail.tsx:282` | `SessionFileDrop.h`; `SessionsWindow.cpp:1595-1610,2618-2660` |
| Reference an existing file (path only; browse locally or check a remote path) | **missing** (`zerus-5wmh.13.5`) | the button is always disabled: `ui/sessions/Composer.tsx:322` ("Arrives with file references.") | `MessageComposer.cpp:297-303,865-959` |
| The agent's suggested next message as placeholder; Tab inserts it | done | `ui/sessions/Composer.tsx:183-190,217-225,311`; `store/suggestions.ts` | `MessageComposer.cpp:209-215,556-570,822-825` |
| Model and effort chip: Apply, Save for resume, provider notice, Open Terminal, OpenCode picker with favorites | done | `ui/sessions/ModelSettingsDialog.tsx:30-66`; `ui/sessions/ModelPicker.tsx:82-159` | `MessageComposer.cpp:317-363,1389-1667`; `OpenCodeModelPicker.cpp` |
| Voice message: record, Space or Esc stops, preview, provider choice, Discard with confirmation, Send voice, permission and cold-cache dialogs | done | `ui/sessions/VoiceControls.tsx:15-158`; `core/voice_composer.rs:2063-2141` | `speech/SpeechComposer.cpp:60-99,426-533` |
| Stop and preview button while recording in send-immediately mode | partial (`zerus-5wmh.13.7`) | keyboard only (`ui/sessions/Composer.tsx:240-245`); `core/voice_composer.rs:2022` `stop_preview` has no button | `speech/SpeechComposer.cpp:90-92` |
| Voice notice chip opens a dialog with the full notice | partial (`zerus-5wmh.13.7`) | plain chip with a tooltip: `ui/sessions/VoiceControls.tsx:43-45` | `speech/SpeechComposer.cpp:492-499` |
| Round video: mode switch, mirrored circular preview, playback | done | `ui/sessions/VoiceControls.tsx:33-39,102-108` | `speech/RoundVideo.cpp`; `speech/SpeechComposer.cpp:93-97` |
| Compaction indicator; Cancel send while waiting on compaction or startup | done | `ui/sessions/Composer.tsx:176-182,292-299`; `ui/sessions/HeldMessage.tsx:13-31` | `SessionsWindow.cpp:958-968`; `ActivityView.cpp:1270-1280` |
| Cold-cache dialog: Send with full context, Compact and continue, Clear context | done | `ui/sessions/ComposerDialogs.tsx:15-33`; `core/cache.rs:196-230` | `SessionsWindow.cpp:4816-4851` |
| Stopped session: Start to send (Resume, Compact and resume, Start fresh), then sends | done | `ui/sessions/ComposerDialogs.tsx:36-54`; `core/send.rs:286-334` | `SessionsWindow.cpp:4704-4803` |
| Clear context with confirmation (Dismiss and clear), draft kept | done | `ui/sessions/ComposerDialogs.tsx:57-69`; `ui/sessions/SessionDetail.tsx:101-104,138-139` | `SessionsWindow.cpp:606-617,994-1029,4638-4670` |
| Start fresh with confirmation | done | `ui/sessions/ActionDialogs.tsx:133-142` | `SessionsWindow.cpp:648,4681-4702` |
| Prompt-cache chip: countdown, warning, Usage details, Clear context | partial (`zerus-5wmh.13.7`) | no Usage details button (`ui/sessions/Composer.tsx:113-158`); the Context chip still opens the inspector (`ui/sessions/UsageChips.tsx:101`) | `SessionsWindow.cpp:987-1002` |
| Usage-limit chip with Refresh usage; Context chip opens the inspector | done | `ui/sessions/UsageChips.tsx:44-101` | `SessionsWindow.cpp:916-927,1063` |
| Narrow toolbar shortens chips | done | `ui/sessions/Composer.tsx:56-57,251` | `ComposerToolbar.cpp` |
| Subagent composer: sends to the child, own drafts, no voice or push | done | `ui/sessions/Composer.tsx:165,220`; `tauri/composer/send.rs:277-297` | `SessionsWindow.cpp:1043-1070` |

### Activity

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| GitHub-style Markdown (headings, lists, task lists, tables, quotes, code, strikethrough); raw HTML as text; no remote images | done | `ui/sessions/markdown.ts:1-169`; `ui/sessions/Activity.tsx:187-249` | `MarkdownHtml.cpp`; `MarkdownObjects.cpp` |
| Bare URLs become links; a URL written as inline code becomes a link | partial (`zerus-5wmh.13.8`) | inline code never links: `ui/sessions/markdown.ts:72` | `MarkdownHtml.cpp:299-311,375-408` |
| Links: web pages open the browser; file references open a dialog (open, folder, download) | done | `ui/sessions/Activity.tsx:196-206`; `ui/sessions/SessionFileDialog.tsx:36-68` | `ActivityView.cpp:1737-1765` |
| Copy a reply as Markdown; select, copy, Select All, context menu | done | `ui/sessions/Activity.tsx:144-157,669,1128-1137` | `ActivityView.cpp:798-800,1542-1547,1851-1866` |
| Activity width: drag, double-click reset, Full width and Reset width | done | `ui/sessions/Activity.tsx:84-141` | `ActivityView.cpp:1253-1255,1498-1540` |
| Follow latest; Jump to latest (N new) | done | `ui/sessions/Activity.tsx:685-719,1097-1105`; `ui/sessions/Composer.tsx:267,288-291` | `ActivityView.cpp:1607-1636` |
| Tool calls grouped and expandable to each event's full output | partial (`zerus-5wmh.13.8`) | expanded rows show a 110-character preview per event (`ui/sessions/transcript.ts:197-201,568`) | `ActivityView.cpp:1105-1140` |
| View process link from a tool event to its recorded process | **missing** (`zerus-5wmh.13.8`) | no such link in the client UI | `ActivityView.cpp:1129-1134` |
| Thinking and subagent reports as Markdown | partial (`zerus-5wmh.13.8`) | plain text: `ui/sessions/Activity.tsx:636-642` (peer and background cards do render Markdown) | `ActivityView.cpp:942-975` |
| Session cleared marker; Question and Your answer blocks; queued and sent stamps | done | `ui/sessions/transcript.ts:512,543-549`; `ui/sessions/Activity.tsx:575-583,667` | `ActivityView.cpp:347-363,934-940,1029-1046` |
| Local message stamps: Sending, Not sent, Submitted to terminal, Delivery not confirmed | done | `ui/sessions/transcript.ts:226-251` | `ActivityView.cpp:1053-1055` |
| OpenCode queue: Queued or Not delivered stamp, waiting hint, Remove queued message with confirmation | **missing** (`zerus-5wmh.13.6`) | `core/input_queue.rs:546-558` (`inbox_removable`) has no caller; no UI reads `delivery_state` | `ActivityView.cpp:1036-1038,1072-1087`; `SessionsWindow.cpp:4433-4467` |
| Attachment, voice and video cards: play, seek, transcript; playback stops when hidden | done | `ui/sessions/Activity.tsx:254-490`; `store/mediaPlayback.ts` | `ActivityView.cpp:759-782,979-1028,2153-2187` |
| Recorded-context fallback; Older events have expired | done | `ui/sessions/transcript.ts:212-216`; `ui/sessions/Activity.tsx:1176-1178` | `ActivityView.cpp:753-757,905-916` |
| A message search hit opens in Activity with the query highlighted | partial (`zerus-5wmh.13.17`) | shown as plain highlighted text, without Markdown, links or Copy as Markdown; the jump caption does not become Back to latest activity (`ui/sessions/Activity.tsx:1158-1162`) | `SessionsWindow.cpp:382-392,436-458`; `ActivityView.cpp:1592-1605` |
| Empty-state guidance (ready for the first message, sign in first, open the terminal) | partial (`zerus-5wmh.13.8`) | one generic line: `ui/sessions/Activity.tsx:1181` | `ActivityView.cpp:1145-1150` |
| Subagent activity with a way back to the main activity | done | `ui/sessions/Activity.tsx:1151-1157` | `SessionsWindow.cpp:1041-1062` |

### Questions and approvals

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| A required question card replaces the editor | done | `ui/sessions/Composer.tsx:269-271`; `ui/sessions/SessionDetail.tsx:307` | `MessageComposer.cpp:1126-1131` |
| Single and multiple choice with Markdown descriptions, Other with free text, bounds | done | `ui/sessions/QuestionCard.tsx:145-217` | `QuestionCard.cpp:578-614` |
| Free-text checks (under 4 KiB, one line, no control characters, no leading `/` or `!`) | done | `core/questions.rs:1036-1055` | `QuestionCard.cpp:106-111` |
| Several questions: tabs, Back and Next, answered count, Enter submits | done | `ui/sessions/QuestionCard.tsx:90-95,119-137` | `QuestionCard.cpp:216-217,626,795-797` |
| Question queue with pending count and kept drafts | done | `ui/sessions/QuestionCard.tsx:104-118` | `QuestionCard.cpp:193-197,382-404` |
| Approvals: Approve once after a 3 s review while visible and focused, Deny; widening options need Confirm | done | `ui/sessions/QuestionCard.tsx:69-85,138-139`; `core/questions.rs:21,108,1587` | `QuestionCard.cpp:218-246,590,817,847` |
| Optional questions: Skip, response recorded; row badge without a status change; notification setting | done | `ui/sessions/QuestionCard.tsx:133`; `ui/sessions/SessionCard.tsx:363-367`; `ui/screens/settings/Sessions.tsx:21,45` | `QuestionCard.cpp:220-221,465`; `OptionalQuestionBadge.h` |
| Trust prompts and plan review; Codex hooks trust and update answers continue in Terminal | done | `core/questions.rs:155,232-233,1454-1455,1794-1812` | `QuestionCard.cpp:805,849`; `SessionsWindow.cpp:1156-1180` |
| Saved answer drafts, also listed in Saved drafts | done | `tauri/composer/questions.rs:1454-1463`; `core/drafts.rs:244` | `QuestionCard.cpp:271-479` |
| Submit states, Allow retry, Open Terminal or native UI; cold-cache check before answering | done | `ui/sessions/QuestionCard.tsx:130-134,219-247` | `QuestionCard.cpp:211-213,434-475`; `SessionsWindow.cpp:5122` |
| Recovery chip: Retry now, Cancel retry, Attempts, Open Terminal, Refresh usage, Settings | done | `ui/sessions/RecoveryChip.tsx:18-83`; `store/recovery.ts:20-47` | `RecoveryWidgets.h`; `SessionsWindow.cpp:928-956` |
| Mark as read chip; a reply that stays in view is marked read; read marks reach `hgs` with retry | done | `ui/sessions/Composer.tsx:278-287`; `store/viewedReply.ts`; `tauri/readsync.rs` | `ActivityView.cpp:1549-1574`; `ReadMarkSync.cpp` |

## Notifications, tray and menu bar, shortcuts, lifecycle and updates

In the default native alert mode the Zerus Next app posts alerts itself while it runs, also with its window closed. In service mode the `hgs` desktop service posts them (`src/desktop/notifications/`) and the app only opens the clicked session; service mode starts on its own only on Linux and macOS once the service is registered (`core/desktop/service.rs:20-26`). Rows describe native mode.

### Notifications

| Capability | Platform | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- | --- |
| Needs attention alert with its reason; New reply alert once the session is idle; subagent requests; optional-question alerts off by default | all | done | `core/attention.rs:20-22,442-700`; `ui/screens/settings/Sessions.tsx:45-47` | `AttentionTracker.cpp:120-242` |
| No banners while Zerus is the active app | Windows | partial (`zerus-5wmh.13.14`) | Linux and macOS report focus (`tauri/platform/linux_notify.rs:640-658`; `tauri/platform/macos_notify.rs:163-196`); `tauri/desk.rs:383` `set_focused` is Linux and macOS only, so Windows always alerts | `AttentionTracker.cpp:45-49,122-125` |
| Alert withdrawn when attention clears or is read; Mute withdraws and never replays | all | done | `tauri/desk.rs:1025-1042`; `core/attention.rs:228-237,393`; `ui/sessions/actions-ui.ts:40-78` | `TrayAgent.cpp:558,706-714`; `AttentionTracker.cpp:99-105` |
| Click opens the session's Activity, waiting for fresh data | all | done | `tauri/desk.rs:684-770,954-1022`; `store/desk.ts:105-125` | `TrayAgent.cpp:166-194,593-608` |
| Delivery failure notice and retry; macOS permission hint | all | done | `tauri/desk.rs:753-766`; `core/attention.rs:863-871` | `TrayAgent.cpp:195-201`; `MacAttentionNotifier.mm:84-92` |
| macOS sound and native notification center; Linux actions, hints, daemon restarts and Wayland activation; Windows balloon | per platform | done | `tauri/platform/macos_notify.rs:52-151`; `tauri/platform/linux_notify.rs:427-610`; `tauri/platform/windows/tray.rs:118-128,407-440` | `MacAttentionNotifier.mm`; `LinuxAttentionNotifier.cpp`; `WindowsAttentionNotifier.cpp` |
| Alerts continue with the window closed | all | done | `tauri/lib.rs:1196,1221,1249` | `TrayAgent.cpp:137-160` |
| Update available alert, once per release | Windows | partial (`zerus-5wmh.13.14`) | macOS and Linux post it (`tauri/updates.rs:739-761`); the Windows hook does nothing (`tauri/updates.rs:924-925`) | `TrayAgent.cpp:164-175` |
| Short tray messages (a new session's terminal advisory, the terminal did not open) | Linux, Windows | partial (`zerus-5wmh.13.13`) | `tauri/desk.rs:1180-1196` always runs `osascript` (callers `tauri/creations.rs:1782,1790`); on Linux and Windows the message reaches stderr at most | `TrayAgent.cpp:655-830` |
| Phone alerts wait while the owner is at the desktop | Linux, macOS | partial (`zerus-5wmh.66.10`) | the app reports no presence; only the registered `hgs` service does (`src/desktop/presence_worker.rs`) | `TrayAgent.cpp:279-290`; `DesktopPresence.cpp` |
| Alert text in the interface language | all | partial (`zerus-5wmh.13.16`) | English constants: `core/attention.rs:20-22,444-448`; `crates/freedesktop-notify/src/lib.rs:34` | `AttentionTracker.cpp:125-134`; `LinuxAttentionNotifier.cpp:52` |
| In-window attention badge capped at 99+ | all | partial (`zerus-5wmh.13.16`) | raw count: `ui/components/Rail.tsx:49-51` | `SessionsWindow.cpp:304-306,3374-3381` |

### Tray, menu bar and Dock

| Capability | Platform | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- | --- |
| Tray icon: count tile, amber on attention, stale dot, error mark, 99+; tooltip; `icon-color` override | all | done | `core/menubar.rs:247-286,583-637`; `tauri/platform/macos_menubar.rs:155,322-374`; `tauri/platform/linux_tray.rs:1019`; `tauri/platform/windows/tray.rs:234-279` | `IconFactory.cpp`; `TrayAgent.cpp:161,537-549` |
| Icon follows a light or dark theme change | Windows | partial (`zerus-5wmh.13.14`) | macOS redraws (`tauri/platform/macos_menubar.rs:235-300`); Windows reads the theme once (`tauri/platform/windows/tray.rs:260-279`) | `TrayAgent.cpp:537-549` |
| Left click opens Zerus, right click the menu; Open Zerus; Update available; Restart to finish update (macOS); machine state lines; Quit saving drafts | all | done | `core/menubar.rs:644-676`; `tauri/updates.rs:905-916,1305-1311`; `tauri/platform/linux_tray.rs:256-264,968-973`; `tauri/platform/windows/tray.rs:114-117,183,189` | `TrayMenu.cpp:162-245`; `QtTrayIcon.cpp`; `SniTrayIcon.cpp` |
| Per-session rows in the tray menu | all | removed by owner decision | one count line per machine (`core/menubar.rs:639-678`, owner, 0.48.0) | `TrayMenu.cpp:23-31,168-234` |
| Open in terminal-mode submenu | all | removed by owner decision | only read from files (`tauri/desk.rs:224-288`); see Terminal preferences | `TrayMenu.cpp:62-148`; `TrayAgent.cpp:832-873` |
| Tray Refresh | all | **missing** (`zerus-5wmh.13.15`) | the entry type exists (`core/menubar.rs:378`) but the menu never adds it; the macOS handler does nothing (`tauri/platform/macos_menubar.rs:209`) | `TrayMenu.cpp:241-242` |
| Menu not rebuilt while open | all | done | `core/menubar.rs:696-731`; `tauri/platform/windows/tray.rs:136-177` | `QuietTrayIcon.cpp`; `TrayMenu.h:40-43` |
| Dock badge, Dock icon only with a window, Dock click restores; app menu | macOS | done | `tauri/platform/macos_menubar.rs:100-131`; `tauri/lifecycle.rs:19-53,196-243`; `tauri/exit_flush.rs:528-570` | `MacApplicationBadge.mm`; `MacAppVisibility.mm` |
| StatusNotifierItem tray re-registering after panel restarts; launcher badge | Linux | done | `tauri/platform/linux_tray.rs:221-440,715-745`; `tauri/platform/linux_tray/badge.rs` | `SniTrayIcon.cpp`; `LinuxApplicationBadge.cpp` |
| Desktop without a tray host | Linux | partial (`zerus-5wmh.13.15`) | the app runs without an icon (`tauri/platform/linux_tray.rs:740`); Qt refused to start so the service retried | `main.cpp:364-367` |
| Notification-area icon re-added after Explorer restarts; taskbar overlay badge; taskbar grouping | Windows | done | `tauri/platform/windows/tray.rs:107-110,260-406`; `tauri/platform/windows/startup.rs:105-109` | `QtTrayIcon.cpp`; `WindowsApplicationBadge.cpp`; `main.cpp:284` |

### Shortcuts and navigation

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Commands with per-platform default keys; Settings > Keyboard; key routing that keeps Terminal and text fields safe | done | `store/shortcuts.ts:22-81,392-514`; `ui/screens/settings/Keyboard.tsx:27-91` | `Shortcuts.cpp`; `KeyboardSettings.cpp` |
| Go to screens (Ctrl+1 to 5, Ctrl+,); Go to popup (Ctrl+K); shortcut help (F1, Ctrl+/) | done | `App.tsx:114-121`; `store/goTo.ts:19-129`; `ui/sessions/GoToDialog.tsx`; `ui/sessions/ShortcutHelpDialog.tsx:8-42` | `SessionsWindowShortcuts.cpp:209-215`; `SessionsWindowGoTo.cpp`; `ShortcutsDialog.cpp` |
| New session, content scale, side panel, Send, Send queued now, Send and push | done | `App.tsx:108-127`; `ui/sessions/SessionDetail.tsx:271,276`; `ui/sessions/Composer.tsx:250,258` | `SessionsWindowShortcuts.cpp:222-245` |
| Window commands: next and previous session, next and previous needing attention, rename (F2), search (Ctrl+F), refresh (Ctrl+R, F5), next and previous tab, Activity and Terminal (Ctrl+\`), next and previous area (F6) | done | `ui/sessions/window-commands.ts:88-245`, installed at `App.tsx:131`, so Go to runs them too (`store/goTo.ts:126-127`); the Projects list keeps Alt+Up and Alt+Down (`store/shortcuts.ts:410-413`) | `SessionsWindowShortcuts.cpp:218-241,346-435` |
| Shortcut help: Terminal rule text and its ✓ marks | done (✓ marks as in Settings > Keyboard) | `ui/sessions/shortcutHelp.ts:30-35`; `ui/sessions/ShortcutHelpDialog.tsx:38` | `ShortcutsDialog.cpp:15-110` |
| Key hints in tooltips | done | `ui/components/Rail.tsx:34-46` | `Shortcuts.cpp:261-271` |

### Lifecycle and updates

| Capability | Platform | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- | --- |
| One instance per user handing over open or stay hidden | all | done | `tauri/instance.rs:1-45`; `tauri/instance_windows.rs:34-80,222-260` | `main.cpp:296-362`; `SingleInstance.cpp` |
| A launch waits for the login service that is still starting | Linux | partial (`zerus-5wmh.13.15`) | macOS waits (`tauri/instance.rs:447-520`); on Linux the check reports no service (`tauri/instance.rs:681-687`) | `SingleInstance.cpp:107-140` |
| `--sessions` and `--background`; closing hides the window; show, raise and unminimise; window geometry remembered; keep above | all | done | `tauri/cli.rs:23-31`; `tauri/lifecycle.rs:13-168`; `tauri/window_geometry.rs`; `tauri/window_layer.rs` | `AppConfig.cpp:461-463`; `main.cpp:279`; `SessionsWindow.cpp:5831-5908` |
| Start at login; macOS Fix login item | all | done | `packaging/linux/hgs-tray.service`; `tauri/login_item.rs:106-207`; `ui/screens/settings/Updates.tsx:18-70` | `tray/packaging`; `SessionsWindow.cpp:1775-1823` |
| Quit saves drafts and clears badges | all | done | `tauri/exit_flush.rs`; `tauri/signals.rs`; `tauri/lifecycle.rs:253-283` | `main.cpp:380-382`; `TerminationSignals.cpp` |
| Hang watchdog | Windows | partial (`zerus-5wmh.13.14`) | Linux and macOS only: `tauri/lib.rs:1307-1309`; `tauri/watchdog.rs` | `main.cpp:410-423`; `MainThreadWatchdog.cpp` |
| Restart after a crash | Windows | done | `tauri/platform/windows/startup.rs:104-122` | `main.cpp:392-409` |
| Startup note while the hgs command line is updated | Windows | **missing** (`zerus-5wmh.13.14`) | `tauri/platform/windows/startup.rs:18-32,76-101` runs silently | `StartupNotice.cpp`; `main.cpp:102-127` |
| Update checks, Settings > Updates, status-bar New version and Restart to finish update, alert once per release, Windows links and winget only | all | done | `tauri/updates.rs:751-866,1313-1323`; `ui/screens/settings/Updates.tsx:73-140`; `App.tsx:76-85`; `core/updates.rs:604-607,720-727` | `UpdatesWidget.h`; `UpdateController.cpp`; `WindowsUpdateTrust.cpp` |

## Settings, machines, accounts, projects, new session, tasks, worktrees, terminal and processes

### Settings

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Settings page and its sections (Worktrees added later) | done | `ui/screens/settings/sections.ts:36-50`; `ui/screens/Settings.tsx:11-53` | `SettingsPage.h:43,140-145` |
| Theme (system, dark, light), interface language with restart hint, usage indicator style, content scale with reset, Activity width and full width | done (adds skins and skin import) | `ui/screens/settings/Appearance.tsx:148-194`; `core/settings.rs:157` | `SettingsPage.h:46-96`; `UiLanguage.h:33-34` |
| Keep Zerus above other windows (KWin on Linux) | done | `ui/screens/settings/Appearance.tsx:202-205`; `tauri/window_layer.rs`; `tauri/platform/linux_window_layer.rs` | `SettingsPage.h:92-94`; `KWinWindowLayer.cpp` |
| Compact session rows: one switch for the whole list | **missing** (`zerus-5wmh.13.11`) | the key is stored (`core/settings.rs:181`) but has no control and nothing reads it; only per-row compacting exists (`store/ui.ts:58-68`) | `SettingsPage.h:98`; `SessionsWindow.cpp:428,1692` |
| Expand the collapsed list on hover; hide projects without active sessions; animate working sessions; show completed subagents; notify about optional questions | done | `ui/screens/settings/Sessions.tsx:30-54` | `SettingsPage.h:98-106` |
| Automatic recovery rules, session-limit mode, delays, sync status | done | `ui/screens/settings/Recovery.tsx:156-175` | `RecoverySettings.h:99-132` |
| Processes tab and timings | done | `ui/screens/settings/Processes.tsx:10-36` | `SettingsPage.h:109-121` |
| Updates: versions, release, automatic checks, channel, Check now, installation details | done | `ui/screens/settings/Updates.tsx:82-138` | `UpdatesWidget.h:59-198` |
| Mobile workspace, computer codes, legacy relay and Windows connector panels | done | `ui/screens/settings/WorkspacePanel.tsx:34-53`; `ui/screens/settings/LegacyRelay.tsx:12-25` | `WorkspaceSettings.h`; `RelaySettings.h`; `WindowsMobileSettings.h` |
| Phones: pair (QR, code), revoke with confirmation; phone notification delivery | done | `ui/screens/settings/Mobile.tsx:19-70`; `ui/screens/settings/PhoneDialogs.tsx:13-28` | `PhonePairing.cpp`; `MobileDeliverySettings.h` |
| Downloaded files: usage and size limit | done (adds Open folder) | `ui/screens/settings/Files.tsx:69-117` | `DownloadCacheSettings.cpp` |
| Worktree cleanup rules per computer | done | `ui/screens/settings/Worktrees.tsx:62-86` | `WorktreeSettings.h` |
| Automatic approvals with danger confirmation and journal | done | `ui/screens/settings/Approvals.tsx:67-120` | `AutomaticApprovalSettings.cpp` |
| Voice input: providers, processing computer, microphone, statistics | done | `ui/screens/settings/Voice.tsx:110-324` | `speech/SpeechSettings.cpp` |
| Keyboard shortcut editor: change, clear, reset, conflicts | done | `ui/screens/settings/Keyboard.tsx:58-225` | `KeyboardSettings.cpp` |
| Private analytics | done | `ui/screens/settings/Analytics.tsx:142-283` | `AnalyticsSettings.cpp` |
| About: versions and links | done (adds service status, migration report, login item) | `ui/screens/settings/About.tsx:24-63` | `SettingsPage.h:146-155` |
| Terminal preferences: custom terminal command and where terminals open | done; chooser removed by owner decision | `tauri/desk.rs:222-240` reads `terminal=` and `open-mode` from `tray.conf` and an old saved mode; the Open in chooser was removed by the owner in 0.48.0 (`tauri/desk.rs:279-280`); the `--terminal` and `--open-mode` options are accepted and ignored by design (`tauri/cli.rs:84-91`) | `AppConfig.h:15-54`; `TrayMenu.cpp:68-147` |

### Machines, accounts and projects

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Machines: list, refresh, add, SSH settings and effective values, label colour, test connection, open SSH terminal, install or update hgs, enable local sessions, reset overrides, remove with confirmation | done | `ui/screens/Machines.tsx:95-243`; `ui/screens/MachineSetupDialog.tsx:15-23`; `store/machineEditor.ts:71-79` | `MachinesPage.cpp:61-454` |
| Windows session-host details; config or `HGS_PEERS` note | done | `ui/screens/machines-ui.ts:41-45`; `ui/screens/Machines.tsx:95-96` | `MachinesPage.cpp:337-338,413-420` |
| Pause all and Resume all per machine | done | `ui/sessions/machine-batch.ts:62-69` | `SessionsWindow.cpp:2844-2885` |
| Accounts across machines with filters and refresh; usage panel | done | `ui/screens/Accounts.tsx:86-120,351-363`; `tauri/screens.rs:2247` | `AccountsPage.cpp:196-212`; `AccountUsage.h` |
| Sign in, install agent, Start OpenCode, add, copy to another machine, rename, remove, make default, permissions, default model and effort, DeepSeek key | done | `ui/screens/Accounts.tsx:174-301,358`; `ui/screens/AccountDistributionDialog.tsx:36-64`; `ui/screens/AccountDefaultsDialog.tsx:21-40` | `AccountsPage.cpp:223-1102` |
| Projects: list and reorder, new, rename, colour, default project, folders (add, edit, remove, open, copy, move), new session from a folder, clone, delete with moving sessions | done | `ui/screens/Projects.tsx:183-340`; `ui/screens/ProjectCloneDialog.tsx` | `ProjectsDialog.cpp:137-629` |
| Swarm: computers, Sync now, join, match projects, conflicts | done | `ui/screens/ProjectSwarmDialog.tsx:82-118`; `ui/screens/SwarmConflicts.tsx` | `SwarmDialog.cpp`; `SwarmConflictReview.cpp` |
| One-time import of old hgs project folders | done | `tauri/organization/legacy_projects.rs`; `store/legacyProjects.ts:6-10` | `ProjectsDialog.cpp:357` |

### New session

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Project and computer; working folder with local and remote browsing; inline new project | done | `ui/sessions/NewSessionDialog.tsx:336-398`; `ui/components/FolderBrowser.tsx` | `NewSessionDialog.cpp:142-169,518,680-683` |
| Repository URL and clone destination; unconfirmed clone recovery | done (recovery moved to the creation card) | `ui/sessions/NewSessionDialog.tsx:370-389`; `ui/sessions/creations-ui.ts:31` | `NewSessionDialog.cpp:163,188-196,268,670` |
| Worktree choice: Auto, None, existing, create | done | `ui/sessions/NewSessionDialog.tsx:414-441`; `ui/sessions/WorktreePicker.tsx` | `NewSessionDialog.cpp:200-216` |
| Agent and account with usage; shell and PowerShell; OpenCode model and effort; session name; terminal window; remembered choices | done | `ui/sessions/NewSessionDialog.tsx:453-525`; `tauri/newsession.rs:487-492` | `NewSessionDialog.cpp:225-475,718,937` |
| No-machine guidance | done | `ui/sessions/NewSessionDialog.tsx:534` | `NewSessionDialog.cpp:279,727-728` |

### Tasks and worktrees

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Task board: stage tiles, empty states, Show older, only this session; create by title | done | `ui/screens/Tasks.tsx:29-189`; `ui/screens/TaskCreate.tsx` | `TasksView.cpp:320-379`; `TaskInspector.cpp:87-90` |
| Card actions and drag to reorder, restage or assign | done | `ui/screens/Tasks.tsx:99-162`; `ui/screens/TaskStartButton.tsx` | `TaskRow.cpp:142-211`; `TaskDrag.h` |
| Task view, description editor with attachments, comment and send modes, assignment, cancel with reason, project task settings, recovery and start review | done | `ui/screens/TaskInspector.tsx:57-111`; `ui/screens/TaskInput.tsx`; `ui/screens/TaskAssignmentPopover.tsx`; `ui/screens/TaskSettingsDialog.tsx:19-29` | `TaskInspector.cpp`; `TaskDialogs.cpp` |
| Folder and worktrees panel with cleanup reminder | done | `ui/sessions/Inspector.tsx:154-208`; `ui/screens/ProjectWorktrees.tsx` | `WorktreePanel.h` |
| Worktree overview, inspection, review and batch removal, background cleanup results | done | `ui/screens/WorktreeOverviewDialog.tsx`; `ui/screens/WorktreeReviewDialog.tsx`; `ui/screens/WorktreeCleanup.tsx` | `WorktreesDialog.cpp`; `WorktreeCleanupDialog.h`; `WorktreeBatchCleanupDialog.h`; `WorktreeCleanupStatus.h` |

### Terminal and processes

| Capability | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- |
| Built-in Terminal: Connect, Disconnect, Reconnect; 5,000-line scrollback; theme and scale; paste (images go to the message field); dropped files insert paths | done | `ui/sessions/Terminal.tsx:15-123`; `store/drops.ts:34` | `TerminalView.cpp`; `TerminalScreen.cpp:696-698` |
| Terminal right-click menu: Copy selection, Paste | **missing** (`zerus-5wmh.13.12`) | `ui/sessions/Terminal.tsx` has no context-menu handler | `TerminalScreen.cpp:699-708` |
| Terminal keyboard copy (Ctrl+Shift+C; Cmd+C on macOS) | unverified (`zerus-5wmh.13.12`) | no copy handler in `ui/sessions/Terminal.tsx`; the shortcut help lists it (`ui/sessions/shortcutHelp.ts:10,21`); needs a live check on each platform | `TerminalScreen.cpp:28-34,476-479` |
| External terminal, native DeepSeek UI, shell in the session folder, connect and restore commands | done | `store/sessionTerminal.ts:12-24`; `store/sessionFolders.ts:39`; `ui/sessions/actions-ui.ts:344-355` | `SessionsWindow.cpp:599,635,5295,5304` |
| Processes tab: count in the label, live and recorded tree, output, Copy command, Stop and Terminate selected | done | `ui/sessions/Processes.tsx:25-123`; `store/processes.ts:74-75,255-264` | `ProcessView.cpp`; `SessionsWindow.cpp:1330,3787` |

### Platform-specific session and terminal support

| Capability | Platform | Status | Zerus Next evidence | Qt |
| --- | --- | --- | --- | --- |
| Windows Terminal tab or window, console fallback; console host keeping failed output; ConPTY; PowerShell quoting; other Windows session warning | Windows | done | `core/terminal_launch.rs:136-140,475-488,1020-1034`; `core/terminal_launch/console_host.rs`; `core/conpty.rs`; `ui/sessions/LaunchWarning.tsx` | `WindowsTerminalBackend.cpp`; `ConsoleHost.cpp`; `PtyProcess_win.cpp`; `LocalLaunchWarning.h` |
| Konsole launch over D-Bus; KWin keep-above; clipboard through Klipper and wl-copy | Linux | done | `core/terminal_launch/konsole_bus.rs`; `tauri/platform/linux_window_layer.rs`; `tauri/clipboard/linux.rs` | `KonsoleBackend.cpp`; `KWinWindowLayer.cpp`; `ClipboardBackend.cpp` |
| Terminal.app launch; Keychain-locked sign-in messages | macOS | done | `core/terminal_launch.rs:169,363`; `core/views/accounts.rs:451` | `MacBackend.cpp`; `AccountsPage.cpp:649-651` |
