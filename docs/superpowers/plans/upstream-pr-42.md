# Agent teams and the WhiteWh fork's desktop work, through phase 42

This pull request brings the WhiteWh fork's own work to upstream Zerus, up to
the fork's phase 42 build. Its branch is `WhiteWh:agent-teams-v2`. It continues
the earlier branch tip `a15a7ff6` by fast-forward, with these additions:

- upstream `main` merged twice, at `2badcfaf` and at `0aaac554` (#431);
- the fork's commits up to its phase 42 integration;
- one commit that takes the fork-only parts out (see "Not in this pull
  request").

It merges into `main` (`0aaac554`) without conflicts.

## What the fork adds

- **Agent teams** (`hgs team`, `src/state/teams/`, `src/agent_guide/team*.md`,
  the desktop Teams workspace).
  - **Team setup and planning:**
    - A lead plans a team of agent sessions on the owner's word.
    - A dispatcher hands out tasks and tracks each attempt.
    - Plans run in phases, with planning rules and plan health (U28).
  - **Delivery and reports:**
    - Each member sends its report on one route.
    - A question, background work or a build waits instead of being captured
      as the report.
    - The lead accepts or rejects reports. A rejection can hold the task.
    - A report that Zerus captured can be replaced.
    - Each member has its own queue.
  - **Task pipelines (U36):** a task passes through review and test stages
    after the implementer's report, before the lead accepts it.
    - The stages are delivered to their members.
    - A test stage holds the task's folder.
    - The rounds are bounded, and the lead decides after them.
    - The catalog has a Pipelines tab. The Board has stage columns and chips.
  - **Customization (U45-U52):** team templates, roles, routing, models per
    role and work-ahead rules, all opt-in.
  - **Working time (U37):** each task's node shows its working time, computed
    on the server.
  - **Owner waiting (U50):** one list of what waits for the owner across
    every team (`hgs team waiting`).
  - Specs: `docs/superpowers/specs/2026-10-24-agent-teams-core-design.md`,
    `2026-10-25-team-phases.md`, `2026-10-26-team-delivery.md`,
    `2026-10-26-team-creation-limits-reports-hierarchy.md`,
    `2026-10-26-team-permissions-and-lead-load.md` and
    `2026-10-28-team-customization.md`.
  - Plan: `docs/superpowers/plans/2026-10-25-teams-next-phase.md`.
- **Session states (U30):** Building, Waiting for dependencies, Cooling and
  Cold, with Offline kept. They are the same on the server, the desktop and
  Android, through the shared `session-state-contract.json`.
  - The session's own build and test jobs count as work.
  - Long-lived services show as services: they never make the session wait.
- **Desktop:**
  - switchable workspace skins, including the WhiteWh cards view;
  - the inspector as a column of cards;
  - the Activity additions (`docs/fork-activity.md`);
  - held input and the attention strip;
  - Claude desktop app sessions (`hgs claude-app`);
  - local models through Ollama (`docs/local-models.md`).
- **Windows:**
  - the portable build;
  - another Windows PC as an SSH peer (`src/windows_peer.rs`);
  - the mobile connector and a relay for this PC, for phone access over
    Tailscale (`docs/fork-mobile-tailscale.md`);
  - the DeepSeek harness on Windows.
- **Smaller fixes** across the CLI, the desktop and the tests, each in its own
  commit.

## Not in this pull request

- **The external pull requests merged into the fork.** Their authors bring
  them upstream themselves:

  | Fork PR | Author | Merge commit |
  |---|---|---|
  | #5 | romanilyin | `e61b0b90` |
  | #6 | n3td0g1 | `18601298` |
  | #7 | Dominga | `187e9b25` |
  | #8 | Dominga | `4af05552` |
  | #9 | Dominga | `1b72f661` |
  | #10 | n3td0g1 | `16a8d837` |
  | #11 | n3td0g | `b6f5a937` |
  | #12 | n3td0g | `b423d050` |
  | #13 | Stinger (Spb) | `a70bf503` |
  | #14 | Dominga | `ddac76b6` |

  Neither those merges nor their commits are ancestors of this branch.

- **Fork commits that build on those pull requests.** They follow their PRs
  later:
  - `bd793f0e`: without PowerShell 7, shells and installers fall back to 5.1.
    It changes #5's PowerShell 7 requirement (`PowerShell7.h`,
    `docs/windows-wsl.md`).
  - `9a86c4d8`: a batch answer's rule reaches its hook before the session
    allowance. It fixes #6's `hand_over`.
  - `b9653f46`: Answer keeps the permission's Allow focused through rebuilds.
    It builds on #5's focus restore in `TeamWorkspace::render` (`318c395b`).
  - In the phases 40-44 commit (`5162f50c`, fork `8770884a`):
    - #12's captured-report hold (A7) is left out: the member status
      `captured_hold`, the busy rule, the inbox note and the lead guide's line.
    - #6's subagent-group test of #511 N4 is left out.
    - The Android palette test keeps upstream's list form, extended to all 13
      badge kinds and the Build tone, instead of #5's rewrite with named keys.
  - In the phase 42 commit (`1b7e1782`, fork `39c11e57`): #9's role model
    picker in the catalog editor is left out (`modelKey`, `fillModels`,
    `requestModels`).
- **Fork-only parts.** The earlier tip `a15a7ff6` carried them by mistake.
  Commit `6712bb0f` "Fork-only parts stay in the fork" takes them out:
  - SocratiCode and the optional extensions framework: `src/extensions*`, the
    `socraticode` cargo feature and CLI command, `Extensions.h`,
    `SocratiCodePanel`, its inspector card, Settings > Extensions and
    `HgsClient::requestSocratiCode`. They stay fork-only until upstream defines
    an extension model.
  - The fork's Windows workflow, `.github/workflows/fork-windows.yml`.
  - The fork updater: `ForkUpdater`, its CMake build number and feed, the
    About page's fork rows, and its hooks in `UpdateController`, `TrayAgent`
    and `main.cpp`.
  - Its docs, `docs/fork-updates.md` and `docs/fork-windows-peers.md`.

  `UpdateController` and the About page are exactly upstream's again.
- **The account switch of a running session** (U53, fork #617). It is not
  part of this pull request.

## How it was tested

Windows 11, MSVC and Qt 6.10.3, with jobs capped at 8.

- The Rust checks ran on the final tip.
- The privacy guard, the Qt build and the Qt suites ran on `6712bb0f`, just
  before the second upstream merge. That merge touches no Qt code.

| Check | Result |
|---|---|
| Rust suite (`cargo test --locked`, every test of the hgs package) | 1807 passed, 0 failed, 13 ignored |
| `cargo clippy --locked --all-targets -- -D warnings` | clean |
| `scripts/check-source-privacy.py` | 0 findings in the source |
| Qt build (GUI and every suite) | passes (324 of 324) |
| `test_teams` | 38 passed, 0 failed, 0 skipped |
| `test_teamsui` | 127 passed, 0 failed, 3 skipped (see below) |
| `test_updates` | 38 passed, 0 failed, 3 skipped |

- In `test_teamsui`, `memberEditorPicksAnExistingSession` failed once in its
  first full run. It passed three runs on its own and the second full run.
  The fork stabilises this test inside #5 (`0e9fbb0a`), which this branch
  leaves out.
- On Windows, the upstream Python tests touched by the `0aaac554` merge
  (`test_agent_compatibility`, `test_agent_hint_controls` and
  `test_pause_signal_trace_source`) fail the same 10 tests and error on the
  same 2 on this branch as on `0aaac554` itself.
- The fork's own lead tip passes 1874 Rust tests. The difference is the tests
  of the external pull requests left out above.
- Earlier fork integrations ran every Qt suite, plus Android unit tests. On
  Windows, 27 Android tests fail on POSIX file permissions, which Windows does
  not support.

## Open issues

- **Review fixes of phase 49:** the fork's review of this work is still
  open. Its fixes follow in a later pull request.
- **Fork #617** (U53, the account switch of a running session) is not
  included.
- **The captured-report hold (A7)** arrives with #12. Until then, a member
  whose report Zerus captured may get its next task before the lead decides
  that report.
- **The open pull requests #322, #323 and #326** (agent teams core, agent
  teams desktop, workspace skins) carry earlier parts of this work. This pull
  request contains their content, so they can be closed in its favour.

## Migration notes

- **New state:**
  - Teams live under the hgs state directory in `sessions/teams/`.
  - A build without teams never reads them, and nothing existing is rewritten.
- **One shape for a task's waits and steps** (spec sections 0 and 1):
  - `Task.waits` is `{for, on, reason, at}`, with an optional `state` and
    request fields.
  - `Attempt.prepare` is `[{step, from, to, at, result, reason}]`, ordered
    branch, model, context.
  - Fork-only fields of earlier fork builds (`prepare_wait`, separate wait
    lists) are no longer written.
- **CLI:**
  - `hgs team ...` and `hgs claude-app ...` are new.
  - `hgs socraticode` is gone, as fork-only.
  - Every existing command, the `HGS_*` variables and the paths stay
    compatible.
- **Desktop settings:**
  - The Settings list has no Extensions page.
  - The QSettings keys `extensions/socraticode` and `socraticode/nativeQdrant`
    that fork builds wrote are no longer read.
  - New keys: `teams/*`, `workspace/skin` and `ui/collapsed/inspector/*`.
- **Session states:** `hgs ls`, `inspect` and the mobile API add
  `session_state`, `state_reason`, `build`, `last_active` and `cache_ttl`. The
  existing fields stay as they were.

## Review order

1. `0cded315` and `e8e5e3dd`, the upstream merges. The only conflict was in
   `src/state/dsh.rs`, in the second merge.
2. `6712bb0f`, the fork-only removal (deletions only).
3. The specs and the plan, in `docs/superpowers/`.
4. The teams core in `src/state/teams/`:
   1. `model.rs`, `store.rs` and `ops.rs`;
   2. `commands.rs` and `owner_word.rs`;
   3. `dispatcher.rs`, `pipeline.rs`, `stages.rs` and `timing.rs`;
   4. `inbox.rs`, `plan.rs` and `models.rs`;
   5. the guides in `src/agent_guide/`.
5. The session states: `src/state/session_state.rs`, `builds.rs`,
   `services.rs` and the shared contract.
6. The desktop teams code in `tray/src/Team*`, with `tray/tests/test_teams.cpp`
   and `test_teamsui.cpp`.
7. The shared fixture `tests/fixtures/team-stage-delivery.json`. It is written
   by a Rust test and read by a Qt test.
8. The remaining desktop, Windows and Android commits, one by one.

## Plans

- **U28:** planning rules and plan health, delivered here.
- **U36:** task pipelines with review and test stages, delivered here.
- **U37:** each task's working time on its node, delivered here.
- **U38:** the pipeline profile, to come. It measures where time and tokens
  went per task, phase and plan, using the timing of U37 and the planning
  rules of U28.
