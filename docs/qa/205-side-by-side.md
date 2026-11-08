# Task 205: Qt and Tauri side by side on the parity audit

Checked on 2026-11-08 at the integration line 177e18e03, the same commit in both
GUIs, both reading the owner's real hgs 1.57.2 state (67 sessions, the team
"Zerus Next for the fork") and changing nothing in it.

## How the pairs were taken

- **Tauri:** `python scripts/tauri_template.py build`, then `launch` with
  `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333`, both
  through `hgs run`. The build identity is
  `dev.zerus.next.d593d7a40f84`, buildKind development (the launch step checks
  it). `client/scripts/cdp-drive.mjs` attaches to that port and runs a JSON
  plan:
  - DOM clicks and value setters only;
  - `Page.captureScreenshot`, with the viewport sized to the Qt window
    (2560x1530; the first pairs at 1200x790).
- **Qt:** the `qt-build` template's `hgs-tray` from `.ci-build/gui`, launched
  through `hgs run` with:
  - `HGS_TRAY_HOME` and `HGS_TRAY_LOCK_DIR`, both in `.zerus-team/qa205`, so its
    settings are an INI there (`Settings::useHome`) and its single-instance pipe
    and lock are its own;
  - `--sessions`;
  - the window maximized (2576x1568).
- **Qt control and capture:**
  - `scripts/qt-uia.ps1` presses controls through UI Automation patterns
    (Invoke, SelectionItem, Value, Window), never mouse or keyboard input;
  - `scripts/qt-printwindow.ps1` saves only that process's own windows with
    PrintWindow.
- **Both apps:**
  - start inside the hgs run job and show "Zerus is in a sandbox"; Continue
    anyway was pressed (an expected notice for this launch, not a finding);
  - used only navigation: tabs, pages and drawers. No action, Send, Allow or
    Deny was pressed. The lead's composer holds the owner's draft, which was
    left untouched.
- **Screenshots:** `.zerus-team/qa205/shots`, `t-*` for Tauri and `q-*` for
  Qt.

## Results

| Item | Pair | Result |
| --- | --- | --- |
| 194 Sessions list: team strip, members under the lead | `q-196-team`/`q-208-activity` (list), `t-194-lead-row` | **Fail**: member rows under the lead overflow the pane in Tauri, cutting their chips (F1) |
| 195 Sessions list: states, chips, project actions | `q-194-sessions`, `t-194-sessions`, `t-194-lead-row` | **Fail**: the attention counts and a reply state differ (F2); the lead's header says Ready while its row says Waiting (F3) |
| 196 Teams plan: phase header and editing | `q-196-team`, `t-196-team` | **Fail**: phase titles shrink to a few letters and each summary is shown twice (F4); two different Healthy counts (F5) |
| 197 Plan graph, Board, Layers | `q-196-team`, `t-196-team` | **Fail**: accepted tasks are not folded into one row, so the graph draws all 102 cards (F6) |
| 198 Inbox and Feed, coordinator, cards | `q-198-inbox-feed`, `t-198-inbox-feed`, `q-208-activity`, `t-208-activity` | **Pass** for Inbox and Feed (same 7 Inbox entries and Feed events, same order). **Fail** for the question cards in the lead's Activity (F7) and the wait strip at 1200 px (F8) |
| 199 Side column: member card | `q-196-team`, `t-196-team` | **Fail**: no Permissions line on member cards, and a member Qt shows as No report is Working on in Tauri (F9). Member menu, Tests tab and team settings were not captured |
| 200 New team | `t-200-newteam` (Tauri only) | **Fail** (Tauri alone): the page keeps about 1380 px of a 2560 px window and its footer note is clipped (F10). Qt's New team tab is gone once a team is open in that session, so there is no Qt pair here |
| 202 Settings | `q-202-settings`, `t-202-settings` | **Fail** (minor): the same 17 sections in another order, and Build folders has no icon (F11) |
| 202 Machines | `q-202-machines`, `t-202-machines` | **Fail**: no Phones section and no session count per machine (F12) |
| 206 Compact side panel | none | Not captured: both apps were left on their default skin |
| 208 Activity: system and team notices | `q-208-activity`, `t-208-activity` | **Pass** for the notices: collapsed one-line team notices and the lead's replies in the same order and wording. **Fail** for the lead's header (F13) |

## Failures and repro

Repro starts for every failure: launch both apps as above, at the same window
size, on the owner's data.

- **F1 (194).** Sessions list, scroll to `claude/Zerus/work-6926cceb`. In
  Tauri the team members under the lead are indented but keep their full row
  width: the provider chip ("Claud", "DeepSee") and the Resume button are cut
  at the pane edge. Qt fits them.
- **F2 (195).** Sessions list.
  - The rail badge and the attention filter show 6 in Tauri and 3 in Qt; the
    General group's yellow count is 5 in Tauri and 2 in Qt.
  - `work-9b8fda6a` shows "Stopped" with a mail icon in Tauri and "New reply"
    in Qt.
  - Qt has its own fresh INI and Tauri its own data, so the difference is in
    how each counts, not in saved read marks.
- **F3 (195).** Open `work-6926cceb` (the lead). Its list row in Tauri says
  "Waiting · Waiting for reports: Frontend (SolidJS) #205", but the detail
  header badge says "Ready". Qt says Waiting in both.
- **F4 (196).** Lead > Team tab, Graph.
  - Tauri cuts the phase titles to "P.", "Live c…", "Universa…", "Working …",
    and repeats each phase's task summary twice on the row ("#1 Route Claude's
    … #1 Route Claude's 'Read out…").
  - Qt shows "Phase 2 · Live check of Zerus Next on Windows" and one summary.
- **F5 (196).** Same view, phase 9. Tauri shows "Healthy 96/102" beside the
  title and "Healthy 101/102" at the right. Qt shows "No build yet, 96
  accepted, Healthy 101/102".
- **F6 (197).** Same view, phase 9 open.
  - Qt folds the accepted tasks into "96 accepted #194 … and 84 more · Show" and
    draws only the 6 open tasks (#268, #205, #236, #238, #313, #293).
  - Tauri draws all 102 cards across three layers, scrolling sideways.
- **F7 (198).** Lead > Activity.
  - Tauri puts the two question cards (Setting "Turn on the build broker",
    rights "Give qa …") in a short inner scroll and leaves an empty band of
    about 170 px under it, above the composer.
  - Qt shows both cards in full with their Deny/Allow rows.
- **F8 (198).** Window 1200x790, lead > Activity or Team. Tauri's wait strip
  breaks "7 wait for you" one word per line; Qt keeps it on one line.
- **F9 (199).** Lead > Team, Members.
  - Tauri has no "Permissions: N allowed · N denied · last …" line on member
    cards.
  - Tauri shows Frontend (DeepSeek) as "Working on #268", where Qt shows "No
    report". Tauri's own strip above says that member "got no report".
- **F10 (200).** Rail > New team at 2560x1530. Tauri's form and lead pane end
  at about x=1380; the rest of the window is empty and the footer note is
  clipped under a second scrollbar.
- **F11 (202).** Settings.
  - Qt orders the sections Automatic approvals, Teams, Model servers, Build
    folders, Voice input, …, About.
  - Tauri puts Teams, Model servers and Build folders after About, and Build
    folders has no icon.
- **F12 (202).** Machines.
  - Qt shows the Phones section: pair, revoke, refresh, and the relay errors.
  - Qt shows "This machine · 67 sessions".
  - Tauri has neither.
  - The unreachable msi-old is "Not connected" in Qt and "Reconnecting" in
    Tauri.
- **F13 (208).** Lead > Activity.
  - Qt's lead header carries the team: Running, Free, No build yet, the
    phase/task/phase-count bars, 1 problem, Integrate & build, Close phase,
    Switch to standard, Pause and Stop. It also shows a "Team Zerus Next for
    the fork · Lead / Open team panel" strip.
  - Tauri's lead header has none of these outside the Team tab.

## Not covered

- 199's member menu, Tests tab and team settings dialog.
- 206's compact skin (it needs a skin switch in both apps).
- 195's project actions.
- Qt's New team form with no team open.
- Light skins.
