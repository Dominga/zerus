# Tauri client phase 3b: Settings (zerus-5wmh.24)

Status: design ruled on 2026-10-28 (storage, switch set, Automatic
approvals). The touch points in section 2 are with webui.

Qt is the reference: `tray/src/SettingsPage.h` builds the screen (section list
at `:43`, pages `:46-155`, entry points `:159-172`, refresh on open `:157`).
The mockup `design/webui/src/screens/Settings.tsx` and `settings/*.tsx`
already lays out all 13 sections in Qt's order; the views port it onto live
data. Texts, controls, order, ranges and defaults are Qt's. No dot separators
in the UI.

## 1. Storage (ruled)

Coordinator ruling, 2026-10-28: "the read-only seed. Use the client's own
<data_dir>/settings.json, keyed by Qt's QSettings names. Seed it once from
Qt's values with Qt's types and defaults, and never write Qt's store."

- The client keeps its local preferences in `<data_dir>/settings.json`.
  It is written atomically on a core worker thread, and its keys are Qt's
  QSettings names (`workspace/theme`, `processes/activeSeconds`, ...).
- At the first start under the switch identity (`com.hgdev.hgs-tray`), the
  store is seeded once from Qt's values for the keys the client has. The read
  is read-only and uses Qt's types and defaults. The sources are:
  - macOS: the preference plist;
  - Linux: `~/.config/hgdev/hgs-tray.conf`;
  - Windows: `%APPDATA%/hgdev/hgs-tray.ini`.
  On macOS the domain is read through cfprefsd (`/usr/bin/defaults export
  com.hgdev.hgs-tray -`), which may hold values the plist file does not
  have yet, and the file only when that fails.
- Settings Qt does not have use a `client/` prefix (`client/darkSkin`,
  `client/lightSkin`, `client/overviewWidth`, `client/overviewFullWidth`), so a key Qt adds
  later never collides with them; they are never seeded. Webview-only
  layout such as panel widths starts at its defaults.
- `settings.json` records that it was seeded, when, and from which source,
  so a second seed never happens. A Qt edit after the seed is not imported.
  The dev identity never seeds.
- Qt's store is never written, so a rollback finds Qt exactly as it was.

Settings that live on a computer (recovery policy, mobile delivery, worktree
policy, speech provider, analytics) are written through `hgs`, as Qt does.

## 2. Page shell and touch points (webui)

- Shell: `client/src/ui/screens/Settings.tsx`, from the mockup. It holds:
  - `SECTIONS` (id, label, icon), in Qt order: appearance, sessions,
    recovery, processes, updates, mobile, files, worktrees, approvals,
    voice, keyboard, analytics, about;
  - `section()` / `openSection(id)`;
  - a `refresh` hook per section, called when the section opens
    (SettingsPage.h:157);
  - entry points `openUpdates`, `openKeyboard`, `openRecovery`,
    `openAbout` (`:167-170`).
- Section API: a section is `export function XSection(): JSX.Element` in
  `client/src/ui/screens/settings/<File>.tsx`, registered in `SECTIONS`. It
  reads and writes local preferences only through the settings store below.
- Settings store: `client/src/store/settings.ts` (webui's file) becomes a view
  of the core store. It offers:
  - `settings` (reactive);
  - `setSetting(key, value)`, which invokes `settings_set`;
  - the `settings-changed` event.

  Its existing `workspace/animateActivity` and `workspace/reduceMotion`
  values move over from local storage once.
- webui's slot: `processes`, implemented as `ProcessesSection` (phase 4
  Task 11). It uses the keys `processes/enabled` (default off, which hides the
  tab), `processes/activeSeconds` 2.5 [1, 60], `processes/backgroundSeconds`
  30 [5, 600], `processes/unknownSeconds` 10 [1, 600] and
  `processes/loadingSeconds` 0.5 [0, 5] (`ProcessSettings.h`). The core
  table holds the clamps.
- Optional questions: Sessions owns `notifications/optionalQuestions`
  (default off, `QuestionNotificationSettings.h`). The notifier (Mac Task 6)
  reads it from the core store; the seed covers the Qt fallback.
- "Open in" and icon color are tray settings (`tray.conf`), not part of
  SettingsPage, so Settings does not show them. menubar.json stays the only
  store for Open in.
- Updates: the page drives the client's updater (Mac Task 7), not Qt's.
  webui provides the updater state and actions (installed and latest
  version, channel, automatic checks, view release); this plan supplies the
  page. Qt's keys `updates/channel` and `updates/automatic` are seeded
  only.

## 3. Pages, sources and data

| Section | Qt source | Data | Owner |
|---|---|---|---|
| Appearance | SettingsPage.h:46-95, UiLanguage.h, ContentScale.h, ActivityWidth.h, WindowLayer.cpp | local: `workspace/theme` (system), `workspace/language`, `workspace/usageIndicator` (rails), `workspace/contentScale` (75-200 %, 5 % steps), `workspace/activityWidth`, `workspace/activityFullWidth`, `workspace/alwaysOnTop` (not on Wayland yet, zerus-5wmh.24.2); the client's skins (import later, zerus-5wmh.24.1) and Overview width | events |
| Sessions | :97-107, QuestionNotificationSettings.h | local: `workspace/compact` (deferred: the client has no compact rows yet, zerus-5wmh.24.3; the seeded value is kept), `workspace/expandSessionsOnHover`, `workspace/hideEmptyProjects`, `workspace/animateActivity`, `workspace/showCompletedSubagents`, `notifications/optionalQuestions` | events |
| Automatic recovery | RecoverySettings.h, RecoveryWidgets.h | `hgs` recovery per computer (`requestRecovery`), 15 s refresh; local `recovery/sharedPolicy` | events |
| Processes | :109-121, ProcessSettings.h | local `processes/*` | webui (Task 11) |
| Updates | UpdatesWidget.h, UpdateController.cpp | client updater (webui); seeded `updates/*` | events page, webui state |
| Mobile connection | :123-138, WorkspaceSettings.h, RelaySettings.h / WindowsMobileSettings.h, PhonePairing.cpp, MobileDeliverySettings.h | `hgs` workspace connection, legacy connector config, phones (`requestMobile`, `requestMachines`), `swarm preference get/set mobile_delivery` | events |
| Files | DownloadCacheSettings.cpp | local `downloads/folder`, `downloads/cacheBytes` (core cache, .9.17) | events |
| Worktrees | WorktreeSettings.h | `hgs worktrees policy`, gated on `worktree-maintenance-v1` (W2 core) | events |
| Automatic approvals | AutomaticApprovalSettings.cpp | local `automaticApprovals/enabled`; `hgs` approval log list/detail | see 4 |
| Voice input | speech/SpeechSettings.cpp | local `speech/microphone`, `speech/preview_before_send`, `speech/manage_host`, `speech/host`; provider `speech/machine_id`, `speech/profile`, `speech/profile_revision` (already the voice composer's preferences); `hgs speech` stats and profiles | events |
| Keyboard | KeyboardSettings.cpp, Shortcuts.cpp (QSettings group `shortcuts`) | local overrides per action; needs the client's shortcut registry | events with webui |
| Private analytics | AnalyticsSettings.cpp, AnalyticsClient.cpp:29-38 | `hgs` analytics summary, stats, configure, purge per computer | events |
| About | :146-155 | installed version, `hgs --version`, repository link | events |

Help markers: among these pages, only Worktrees gates on one
(`worktree-maintenance-v1`, WorktreeSettings.h:69). The other `hgs` pages
show Qt's error text when an older CLI refuses (e.g.
speech/SpeechSettings.cpp:293). The client keeps exactly that, and adds no
gates of its own.

## 4. Order and the switch

By how often the owner changes them, most often first:

1. Appearance
2. Sessions
3. Voice input
4. Processes
5. Keyboard
6. Worktrees
7. Updates
8. Mobile connection
9. Automatic recovery
10. Files
11. Private analytics
12. Automatic approvals
13. About

Coordinator ruling: the seed carries the owner's current values over, so at
the switch a page is needed only to change a value, not to keep it.

- Required for the switch: Appearance, Sessions, Voice input, Keyboard and
  About, plus Processes (webui's Task 11).
- After the switch: Files, Updates, Worktrees (with W2), Automatic recovery,
  Mobile connection with phone pairing, and Private analytics. The
  `hgs`-backed ones keep working through the CLI meanwhile.

Automatic approvals:

- The page shows Qt's texts and the journal, with the switch disabled under
  Qt's unavailable wording.
- At the switch, the client checks whether Qt had the opt-in enabled
  (`automaticApprovals/enabled`) and says so on the page, so the feature
  never goes quiet without notice.
- The delivery worker is its own task, zerus-5wmh.54 (P2).

## 5. Slices

Each slice is reviewed by webui and merged into `integrate/client-core-2` after
the local gates (arch, Windows-target clippy, Mac). The switch set comes
first:

1. The core settings store: key table, clamps, atomic writes, the one-time
   seed from Qt's formats, plus the shell, Appearance and Sessions.
2. Voice input, Keyboard (with webui's shortcut registry) and About.
3. After the switch:
   - Files, Updates (on webui's updater state);
   - Worktrees (W2 view), Automatic recovery, Mobile connection with phones;
   - Private analytics, and Automatic approvals (page and notice).

Tests:

- Core: the key table against Qt's defaults and ranges.
- The seed, against fixture files in each Qt format (INI with Qt's escaping
  and `@Variant`; a binary plist), with each source left untouched.
- Every `hgs` request and parser, against recorded Qt fixtures.
- Views: each label and hint equals the `tr()` text at the cited line.
