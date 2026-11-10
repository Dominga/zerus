# Slicing the documentation into CodeLore

Status: the rules for tasks 26 and 27 of team ede724c4 (task #25, owner,
2026-11-10). Zerus's documentation moves into the ZERUS project's CodeLore
store, where agents find it with `hgs team lore find` and `lore find`; the
code itself is searched with `lore atlas search` in a checkout
(`hgs guide code`). This document says what is sliced, how one record looks,
how it cites its source, what is never sliced, the exact commands, and the two
halves of the corpus.

## 1. The store and its areas

The store is the ZERUS project's memory store under Zerus data on the home
computer, never a folder of the repository. Its path:

    # the team's project id, then its store folder ($STORE below)
    echo '{"team":"<team id>"}' | hgs team status --json      # -> "project_id", and "memory"."store"
    echo '{"project":"<project_id>"}' | hgs project config get --json   # -> "store"

Its areas, one per documentation family (`lore -C "$STORE" areas` lists them):

| Area | What goes there |
|---|---|
| `general` | docs/ files of no other family |
| `agents` | CLAUDE.md, CONTRIBUTING.md, README.md, .agents/, .beads/README.md, src/agent_guide/ |
| `reference` | docs/reference.md |
| `teams` | the Teams guides and contracts (docs/teams-guide.md, docs/agent-coordination.md, docs/ru/teams-guide.md) |
| `memory` | docs/lore.md and every lore document |
| `desktop` | docs/desktop-*.md, docs/approval-ui-*.md, the notification documents |
| `client` | client/ READMEs and documents |
| `cli` | crates/, tray/ and src/ READMEs |
| `mobile` | docs/mobile-*.md, docs/relay-*.md, docs/ios-*.md, services/, mobile/ |
| `distribution` | packaging, installation, CI, release and distribution documents |
| `testing` | test providers, host testing, local CI, qualification, client/bench/, docs/qa/ |
| `qt` | docs/qt-*.md |
| `specs` | docs/superpowers/specs/ |
| `impl-plans` | docs/superpowers/plans/ |

Each file of the lists in sections 7 and 8 names its area.

## 2. What one record is

One record per section, or per decision a section takes. A section is a
heading with its text down to the next heading of the same or a higher level;
sections of a long document are its `##` headings, and a `###` heading becomes
its own record when its own text is longer than about 1500 characters.

- **topic** (the default): how something works now. Fields: `area`, `title`
  (the heading, at most 120 characters; add the document's short name when the
  heading alone is ambiguous, for example "Merge queue: the lock"), `body` (at
  most 3000 characters), `status` `current`, `refs`.
- **decision**: a section that records a choice (an owner's answer, a "we
  decided", a table row of decisions). Fields: `area`, `title`, `question`,
  `choice`, `reason`, `decided_by` (`owner`, `lead` or the name the text
  gives), `status` `active` (or `superseded` when the text says so), `refs`.
- **recipe**: a numbered procedure done more than once (an install, a release
  step). Fields: `area`, `title`, `when`, `steps` (a list), `check`, `status`
  `current`, `refs`.

The body is the section's facts in plain present-tense English: the text
itself when it fits, else its summary that keeps every rule, number, name,
path and command. Never history ("first we tried"), never an opinion of yours.
A section in another language is summarised in English. A table becomes
sentences or one record per row when the rows are decisions.

An implementation plan (docs/superpowers/plans/) is history once built: one
topic per plan file, `title` "Plan: <its goal>", `body` what it built, where
(paths) and what it decided; its decisions as decision records. Never one
record per step.

## 3. How a record cites its source

Every record's `refs.links` starts with its source, then the code paths the
section names:

    "refs": {"links": ["zerus:docs/teams-guide.md#plan-modes@<commit>",
                       "zerus:src/state/teams/ops.rs"]}

- `zerus:<path>` is the path in the Zerus repository, with forward slashes.
- `#<heading>` is the section's heading as a GitHub anchor: lower case,
  spaces as `-`, everything but letters, digits, `-` and `_` dropped. A nested
  section names its heading only.
- `@<commit>` is the short commit of the text you read:
  `git log -1 --format=%h -- <path>` in your checkout.
- A code path is cited only when the section names it and it exists:
  `git ls-files <path>` prints it.

The body ends with one line `Source: <path>, <heading>.` so a reader sees the
source without the links. Anyone checks a record with
`git show <commit>:<path>` and the heading. `refs.files` and `refs.commits` are
never used: lore checks them against the store's own git, where Zerus's files
and commits do not exist, and refuses the record.

## 4. What is never sliced

- Secrets: keys, tokens, passwords, certificates, signing material, team IDs,
  device identifiers, private URLs with credentials. lore refuses what looks
  like one; never work around a refusal, leave the part out and say so.
- Personal data: people's e-mail addresses, home paths that name a person,
  phone numbers, account names; write the role ("the owner", "a developer").
- Transcripts: pasted conversations, agent replies, terminal logs, test
  output longer than a line. Keep the fact they show, never the transcript.
- Licence texts and third-party notices, generated files, and validation
  evidence (images, JSON results): not in the lists at all.
- AGENTS.md: it mirrors CLAUDE.md, which is sliced instead.

A skipped section is reported (section 6), never silently dropped.

## 5. The commands

Set who you are once, and the store:

    export LORE_BY=agent:<your model>          # for example agent:deepseek-v4.1-flash
    export STORE="<the store path from section 1>"

Before writing, check that nobody wrote the section already (the other half
never has your files, but a rerun has):

    lore -C "$STORE" find "<the heading>" --no-working -n 3

Write a file's records in one batch, one JSON object per line, all of one kind
per call (run once for topics, once for decisions, once for recipes):

    lore -C "$STORE" --json add topic --stdin --batch < topics.jsonl
    lore -C "$STORE" --json add decision --stdin --batch < decisions.jsonl

Example line of topics.jsonl:

    {"area":"teams","title":"Plan modes: free planning","body":"In free planning one open phase takes tasks as they come and runs them as they are added, until the owner closes it. A phase's own mode, else the team's plan_mode, decides it. Source: docs/teams-guide.md, Plan modes.","status":"current","refs":{"links":["zerus:docs/teams-guide.md#plan-modes@c48ceebd4","zerus:src/state/teams/ops.rs"]}}

lore checks every field and commits the batch to the store's own git; a
refusal writes nothing and names each refused line (`#n`): fix those lines
and send the batch again. A duplicate refusal names the existing record:
improve it with `lore -C "$STORE" update <id> -s body=...` instead, or add
`--new` when yours really is another section. Never edit the store's files by
hand and never commit anything to the Zerus repository.

When your sandbox refuses to run lore against the store (a folder outside your
own), write each batch to `.lore-slices/<n>-<kind>.jsonl` in your own folder
(git ignores that folder) and ask the lead once, with the files and this
command, which the lead runs for each file:

    lore -C "$STORE" --json add <kind> --stdin --batch < <your folder>/.lore-slices/<file>

## 6. The report

Per file of your list: the records written (their ids, which the batch's
answer lists) and every section skipped with its reason (secret, personal
data, transcript, history only, an empty heading). Check your half when done:

    lore -C "$STORE" --json find "<a heading of each file>" -n 1

answers with that file's record and its `zerus:` source.

## 7. Task 26: the first half: 187 files, 4361 KiB


| File | Area |
|---|---|
| `.agents/skills/beads/SKILL.md` | `agents` |
| `.beads/README.md` | `agents` |
| `CLAUDE.md` | `agents` |
| `CONTRIBUTING.md` | `agents` |
| `README.md` | `agents` |
| `SECURITY.md` | `general` |
| `client/README.md` | `client` |
| `client/bench/README.md` | `client` |
| `client/bench/results/spike-29-webkitgtk-idle-2026-10-28.md` | `client` |
| `client/bench/terminal-keys/README.md` | `client` |
| `client/bench/text-edit/README.md` | `client` |
| `client/i18n/shared/README.md` | `client` |
| `client/media/README.md` | `client` |
| `docs/analytics.md` | `general` |
| `docs/approval-ui-qualification.md` | `desktop` |
| `docs/ci-and-aur.md` | `distribution` |
| `docs/desktop-attention-discovery.md` | `desktop` |
| `docs/desktop-background-profile.md` | `desktop` |
| `docs/desktop-catalog-changes.md` | `desktop` |
| `docs/desktop-catalog-read.md` | `desktop` |
| `docs/desktop-catalog.md` | `desktop` |
| `docs/desktop-consumer-activation.md` | `desktop` |
| `docs/desktop-creation-clone.md` | `desktop` |
| `docs/desktop-creation-launch-status.md` | `desktop` |
| `docs/desktop-creation-launch.md` | `desktop` |
| `docs/desktop-creation-worktree.md` | `desktop` |
| `docs/desktop-delegation-hello.md` | `desktop` |
| `docs/desktop-details.md` | `desktop` |
| `docs/desktop-executable-upgrades.md` | `desktop` |
| `docs/desktop-import-transport.md` | `desktop` |
| `docs/desktop-legacy-listing.md` | `desktop` |
| `docs/desktop-linux-notification-class-m-probe-plan.md` | `desktop` |
| `docs/desktop-linux-notification-class-m-supervisor.md` | `desktop` |
| `docs/desktop-linux-notification-class-m.md` | `desktop` |
| `docs/desktop-linux-notifications.md` | `desktop` |
| `docs/desktop-mac-installation.md` | `desktop` |
| `docs/desktop-macos-notifications.md` | `desktop` |
| `docs/desktop-macos-original-child.md` | `desktop` |
| `docs/desktop-migration.md` | `desktop` |
| `docs/desktop-native-reservations.md` | `desktop` |
| `docs/desktop-notification-controls.md` | `desktop` |
| `docs/desktop-operation-store.md` | `desktop` |
| `docs/desktop-operations.md` | `desktop` |
| `docs/desktop-owned-runner.md` | `desktop` |
| `docs/desktop-passive-feed.md` | `desktop` |
| `docs/desktop-producer-binding.md` | `desktop` |
| `docs/desktop-producer-drain.md` | `desktop` |
| `docs/desktop-producer-io.md` | `desktop` |
| `docs/desktop-producer-prepared.md` | `desktop` |
| `docs/desktop-producer-session.md` | `desktop` |
| `docs/desktop-producer-timing.md` | `desktop` |
| `docs/desktop-quit-cli-origin.md` | `desktop` |
| `docs/desktop-quit-journal.md` | `desktop` |
| `docs/desktop-quit-preanchor-readset.md` | `desktop` |
| `docs/desktop-quit-store-design.md` | `desktop` |
| `docs/desktop-read-queue.md` | `desktop` |
| `docs/desktop-revoke-hold.md` | `desktop` |
| `docs/desktop-schema-35-rollback.md` | `desktop` |
| `docs/desktop-source.md` | `desktop` |
| `docs/desktop-startup-consumer.md` | `desktop` |
| `docs/desktop-startup-exclusion.md` | `desktop` |
| `docs/desktop-task-message.md` | `desktop` |
| `docs/desktop-task-start-reconcile.md` | `desktop` |
| `docs/desktop-termination-outcomes.md` | `desktop` |
| `docs/desktop-universal-prepared-policy.md` | `desktop` |
| `docs/desktop-windows-mode.md` | `desktop` |
| `docs/encrypted-workspaces.md` | `general` |
| `docs/external-workstreams.md` | `general` |
| `docs/fork-activity.md` | `general` |
| `docs/fork-mobile-tailscale.md` | `general` |
| `docs/fork-tauri-parity.md` | `general` |
| `docs/fork-updates.md` | `distribution` |
| `docs/fork-windows-peers.md` | `general` |
| `docs/installation.md` | `distribution` |
| `docs/ios-distribution.md` | `mobile` |
| `docs/linux-distribution.md` | `distribution` |
| `docs/local-ci.md` | `testing` |
| `docs/local-models.md` | `general` |
| `docs/macos-distribution.md` | `distribution` |
| `docs/mobile-architecture.md` | `mobile` |
| `docs/mobile-deployment.md` | `mobile` |
| `docs/mobile-session-projects.md` | `mobile` |
| `docs/opencode-existing.md` | `general` |
| `docs/opencode.md` | `general` |
| `docs/path-identity.md` | `general` |
| `docs/project-swarm-recovery.md` | `general` |
| `docs/public-distribution.md` | `distribution` |
| `docs/qa/205-side-by-side.md` | `testing` |
| `docs/qa/319-side-by-side-2.md` | `testing` |
| `docs/qt-parity-checklist.md` | `qt` |
| `docs/qt-release-acceptance.md` | `qt` |
| `docs/reference.md` | `reference` |
| `docs/relay-architecture.md` | `mobile` |
| `docs/release-candidate.md` | `distribution` |
| `docs/retired-features/session-graph.md` | `general` |
| `docs/shared-session-notifications.md` | `desktop` |
| `docs/source-migration.md` | `general` |
| `docs/speech-providers.md` | `general` |
| `docs/speech-server.md` | `general` |
| `docs/speech-setup.md` | `general` |
| `docs/superpowers/specs/2026-10-08-github-markdown-activity-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-09-composer-toolbar-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-10-homebrew-cask-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-10-mobile-presence-notifications-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-10-windows-support-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-10-worktree-cleanup-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-11-scheduled-session-state-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-13-local-agents-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-13-native-ui-plugin-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-13-project-task-queue-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-13-windows-native-host-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-15-keyboard-shortcuts-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-15-rust-mobile-connector-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-15-session-agent-graph-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-16-ios-foundation-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-17-desktop-phone-management-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-17-native-answer-channels-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-17-opencode-2-adapter-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-17-opencode-providers-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-18-task-replication-loro-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-18-windows-installer-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-19-android-cached-history-retirement-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-19-history-projection-transport.md` | `specs` |
| `docs/superpowers/specs/2026-10-19-history-projection-upgrade-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-20-task-tracker-ios-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-20-windows-mobile-gateway-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-21-compatible-read-mark-hot-path-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-24-agent-teams-core-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-25-team-phases.md` | `specs` |
| `docs/superpowers/specs/2026-10-26-activity-task-execution-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-26-session-creation-progress-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-26-team-creation-limits-reports-hierarchy.md` | `specs` |
| `docs/superpowers/specs/2026-10-26-team-delivery.md` | `specs` |
| `docs/superpowers/specs/2026-10-26-team-permissions-and-lead-load.md` | `specs` |
| `docs/superpowers/specs/2026-10-27-event-data-plane-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-28-team-customization.md` | `specs` |
| `docs/superpowers/specs/2026-10-28-webkitgtk-display-pacing.md` | `specs` |
| `docs/superpowers/specs/2026-10-29-hgs-desktop-service-design.md` | `specs` |
| `docs/superpowers/specs/2026-10-29-qt-retirement-inventory.md` | `specs` |
| `docs/superpowers/specs/2026-10-31-local-macos-paired-bundle.md` | `specs` |
| `docs/superpowers/specs/2026-11-04-session-names-as-labels.md` | `specs` |
| `docs/superpowers/specs/2026-11-07-account-labels-child-sessions-design.md` | `specs` |
| `docs/superpowers/specs/2026-11-07-approval-categories-design.md` | `specs` |
| `docs/superpowers/specs/2026-11-07-keep-cache-warm-design.md` | `specs` |
| `docs/superpowers/specs/2026-11-08-task-controls-design.md` | `specs` |
| `docs/test-providers.md` | `testing` |
| `docs/validation.md` | `general` |
| `docs/voice-messages.md` | `general` |
| `docs/windows-host-testing.md` | `testing` |
| `docs/windows-membership-qualification.md` | `testing` |
| `docs/windows-project-folders.md` | `general` |
| `docs/windows-wsl.md` | `general` |
| `mobile/android/README.md` | `mobile` |
| `mobile/android/camera-video-fixed/README.md` | `mobile` |
| `mobile/ios/README.md` | `mobile` |
| `services/mobile/README.md` | `mobile` |
| `services/mobile/connector/README.md` | `mobile` |
| `services/mobile/crypto/README.md` | `mobile` |
| `services/mobile/relay/README.md` | `mobile` |
| `services/mobile/relay/tests/ORACLE.md` | `mobile` |
| `services/mobile/tests/BLACKBOX.md` | `mobile` |
| `services/mobile/vectors/README.md` | `mobile` |
| `services/speech/README.md` | `mobile` |
| `src/agent_guide/designer.md` | `agents` |
| `src/agent_guide/index.md` | `agents` |
| `src/agent_guide/launch.md` | `agents` |
| `src/agent_guide/run.md` | `agents` |
| `src/agent_guide/send.md` | `agents` |
| `src/agent_guide/stop.md` | `agents` |
| `src/agent_guide/tasks.md` | `agents` |
| `src/agent_guide/team-broker.md` | `agents` |
| `src/agent_guide/team-cpp.md` | `agents` |
| `src/agent_guide/team-custom.md` | `agents` |
| `src/agent_guide/team-draft.md` | `agents` |
| `src/agent_guide/team-input.md` | `agents` |
| `src/agent_guide/team-lead.md` | `agents` |
| `src/agent_guide/team-lore.md` | `agents` |
| `src/agent_guide/team-park.md` | `agents` |
| `src/agent_guide/team-propose.md` | `agents` |
| `src/agent_guide/team-run.md` | `agents` |
| `src/agent_guide/team-safe.md` | `agents` |
| `src/agent_guide/team-tests.md` | `agents` |
| `src/agent_guide/team-tree.md` | `agents` |
| `src/agent_guide/team.md` | `agents` |
| `src/agent_guide/tests.md` | `agents` |
| `src/agent_guide/wait.md` | `agents` |

## 8. Task 27: the second half: 46 files, 4191 KiB


| File | Area |
|---|---|
| `crates/content-search/README.md` | `cli` |
| `crates/presence-sampler/README.md` | `cli` |
| `docs/agent-coordination.md` | `teams` |
| `docs/lore.md` | `memory` |
| `docs/ru/teams-guide.md` | `teams` |
| `docs/superpowers/plans/2026-10-08-github-markdown-activity.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-09-composer-toolbar.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-10-desktop-presence-and-delivery-setting.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-10-homebrew-cask.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-10-relay-presence-alerts.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-10-shared-read-state.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-15-keyboard-shortcuts.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-16-ios-foundation.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-17-native-answer-channels.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-18-opencode-2-adapter-phase-1.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-18-opencode-providers-phase-1.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-18-task-replica-phase1.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-18-task-tracker-phase3a-desktop.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-18-task-tracker-phase3b-drag-drop.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-19-task-agent-commands.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-19-task-exchange-relay.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-19-task-replica-android-bindings.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-19-task-replication-phase2-computers.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-20-task-actions-relay.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-20-task-replica-phone-sync.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-20-task-screen-android.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-20-task-tracker-ios.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-21-kimi-approvals-native-qualification.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-25-teams-next-phase.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-archived-activity.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-mac-switch.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-new-session-dialog.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-organization.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-phase1.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-phase2-actions.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-phase3a-screens.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-phase3b-settings.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-phase4-core-section.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-phase4.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-phase5-core-section.md` | `impl-plans` |
| `docs/superpowers/plans/2026-10-28-tauri-client-session-menu-parity.md` | `impl-plans` |
| `docs/superpowers/plans/2026-11-07-keep-cache-warm.md` | `impl-plans` |
| `docs/superpowers/plans/upstream-pr-42-commands.md` | `impl-plans` |
| `docs/superpowers/plans/upstream-pr-42.md` | `impl-plans` |
| `docs/teams-guide.md` | `teams` |
| `tray/translations/README.md` | `cli` |
