# Task 319: Qt and Tauri side by side, part 2

This part covers the items 205 (`docs/qa/205-side-by-side.md`) did not reach, and
rechecks F1-F13 as their fixes are accepted. Checked on 2026-11-08 against the
owner's real hgs 1.57.2 state; nothing in it was changed.

## How the pairs were taken

- **Tauri:**
  - `scripts/tauri_template.py build` and `launch`, through `hgs run`, in this
    folder at 16e5dfc4e (the integration line 177e18e03 plus 205 and 317);
  - `--build-info` reports `dev.zerus.next.d593d7a40f84`, development, clean;
  - driven by `client/scripts/cdp-drive.mjs` over WebView2's debugging port,
    with DOM clicks only; the viewport is 2560x1530.
- **Qt:**
  - the `qt-build` template's `hgs-tray`, with `HGS_TRAY_HOME` and
    `HGS_TRAY_LOCK_DIR` in `.zerus-team/qa319` (its own INI, pipe and lock);
  - maximized (2576x1568);
  - driven by `scripts/qt-uia.ps1` (UI Automation patterns only) and captured
    by `scripts/qt-printwindow.ps1`.
- **Skins:**
  - Tauri: Settings > Appearance.
  - Qt: UI Automation cannot commit a choice in its skin combo, so the skin and
    theme went into the isolated INI (`workspace/skin`, `workspace/theme`) and
    the Qt GUI was restarted.
- **No actions:** no action, Send, Allow, Deny or Save was pressed. The Team
  settings dialogs were closed with Cancel. The lead's composer holds the
  owner's draft, which was left untouched.
- **Screenshots:** `.zerus-team/qa319/shots`, `t-*` for Tauri and `q-*` for Qt.
- **Cleanup:** both apps were stopped afterwards.

## Results

| Item | Pair | Result |
| --- | --- | --- |
| 199 Member menu | `t-199-member-menu`; Qt's menu opens on a mouse press that UI Automation cannot send, so the Qt side is its source (`TeamWorkspace.cpp` 4136-4235) | **Pass** for the lead's menu: the same actions with the same enabled state |
| 199 Tests tab | `q-199-tests`, `t-199-tests` | **Fail** (N3): both show no run, but Tauri has no queue section and no running/waiting counters |
| 199 Team settings | `q-199-team-settings`, `t-199-team-settings` | **Fail** (N4): Tauri's team iron rules field is collapsed to a thin bar, and its rule fields have no placeholder |
| 206 Compact skin | `q-206-compact`, `t-206-compact` | **Pass**: the same unified title bar and side-panel sections. Minor (N5): number format |
| 195 Project actions | `q-195-projects`, `t-195-projects` | **Fail** (N6): project-level New team/New session, the Sessions section and the counts are missing, and folder management is "unavailable for this computer" |
| 200 New team, no team open | `q-200-newteam`, `t-200-newteam-xp` | **Fail** (N2): no Working mode row and no session details in the lead pane; F10's clipped footer is still there (314 not merged) |
| Light skins | `q-light-sessions`, `q-light-team`, `t-light-lead`, `t-light-settings` | **Pass** for the skin. **Fail** (N7): toasts stack over the side panel |

## New differences and repro

Repro starts for every difference: launch both apps as above, at 2560x1530,
on the owner's data.

- **N1 (lead header, 317 follow-up).** Window 1200x790, lead session. The team
  bar, now also in the lead's header, overlaps itself: the progress bar covers
  "Switch to standard…" and "Team settings" is cut. It is the same bar as the
  Team tab, which overlaps the same way at that width.
- **N2 (200).** Select a session with no team (XP_Start), then New team.
  - Qt's form has a "Working mode: Auto: follow the project" row.
  - Qt's lead pane names "claude/XP/XP_Start, Claude Code, model, account".
  - Tauri has neither.
  - Qt offers New team as a "+ New team" tab on the session; Tauri only from
    the rail.
- **N3 (199).** Team tab > Tests.
  - Qt shows "Test requests wait in a queue…", a Queue section ("Nothing
    waits.") and "0 running / 0 waiting".
  - Tauri shows only "Recent runs: No test runs recorded".
- **N4 (199).** Team tab > Team settings.
  - Tauri's "Team iron rules" field is a thin bar, not a text box.
  - Tauri's member rule fields have no placeholder; Qt shows "One additional
    rule per line (20 maximum)".
  - Tauri's dialog also leads with Branches, Landing and Members, before the
    member rules; Qt leads with the rules.
- **N5 (206, minor).** Side panel > Context/usage. Tauri prints
  "1,863,139,812"; Qt prints "3 695 480" (the owner's locale).
- **N6 (195).** Projects page > XP.
  - Qt has New team… and New session… in the page header.
  - Qt has a Sessions section ("2 sessions, 0 in teams, 0 teams", each with its
    state) and session counts in the project list.
  - Tauri has none of these. It also says "Folder management is unavailable for
    this computer" and disables Add folder, Edit folder and New session for the
    local machine, where Qt's folder actions work.
- **N7.** A fresh Tauri profile. "Waiting for you" and "Low disk space" toasts
  (five) stack inside the window over the side panel, as in 205, and again after
  a skin change. Qt shows none in the window.

## F1-F13 recheck

| F | Fixed by | Recheck |
| --- | --- | --- |
| F7 cards in full | 317 (accepted) | **Pass**: at 2560 px the cards box is 338/338 px with Deny/Allow on both; at 1200 px it stops at 60% and scrolls once |
| F8 strip on one line | 317 | **Pass**: one line at 1200 px |
| F13 lead header | 317 | **Pass**: team bar and the "Team … Lead / Open team panel" strip; the Team tab keeps one bar. See N1 for 1200 px |
| F1, F10 | 314 (accepted) | Pending: 00761659e is not on the integration line yet |
| F2, F3, F9 | 315 (in progress) | Pending |
| F4, F5, F6 | 316 (in progress) | Pending |
| F11, F12 | 318 (submitted) | Pending |
