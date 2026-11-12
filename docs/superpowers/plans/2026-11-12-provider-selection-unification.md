# Provider account, model and effort selection unification

Date: 2026-11-12
Status: phases 1–4 implemented; non-OpenCode launch changes remain a separate follow-up

## Summary

The desktop client already has most of the backend data needed for a unified
provider account, model and effort selection:

- Accounts and New session use the same grouped account projection from
  `accounts::group` (`client/src-tauri/src/screens.rs:1329`).
- `hgs account model-settings` exposes an account's native model catalog,
  launch defaults and effort support (`src/accounts/model_defaults.rs:108`).
- OpenCode has a separate short/full catalog flow with model metadata,
  favorites and effort labels.

The inconsistency comes from independent frontend projections and selection
rules built on top of those sources. Account names, default/native semantics,
model types, effort fallbacks and account-change behavior differ by screen.

The recommended first step is a shared, side-effect-free selection model and
account presentation functions. Launch, session settings, account defaults and
team drafts should keep their own apply operations. A single dropdown or a
single controller owning every workflow would couple behaviors that are not
interchangeable.

## Scope

This audit covers the released desktop/Tauri client in `client/`, its pure Rust
core and the related `hgs` account/model catalog code in `src/`.

The prototypes in `design/` and the Android/iOS clients are outside this
revision. They can later consume the same account labels and catalog shape, but
their presentation and transport contracts need a separate compatibility pass.

## Existing sources of truth

### Account identity and presentation

The raw account catalog stores a profile ID, provider and label. Native profile
labels can also be renamed; `src/accounts.rs:813` exposes those labels to the
CLI.

The desktop core groups profiles by provider identity in
`client/core/src/views/accounts.rs:982`. An `Account` group's label is chosen as
follows:

1. start with a member profile's label (`:1012`);
2. replace an unnamed/default label with a named member's label (`:1036`);
3. if no named label remains, use the best usage identity's email (`:1042`).

The Accounts screen displays this group label at
`client/src/ui/screens/Accounts.tsx:82` and edits it through Rename at `:193`.
This is the display name the other selectors should reuse.

New session receives the same `Account` groups as Accounts through
`LaunchAccounts.groups`; `client/src-tauri/src/screens.rs:1329` refreshes or
rebuilds them with the same `accounts::group` functions. The exact `Profile.id`
must nevertheless remain the launch binding: a grouped account may contain
aliases or profiles on several computers.

### Model catalogs

For Claude, Codex, Kimi and DeepSeek, `hgs account model-settings` is the
existing read-side catalog. The underlying account model entries already carry
`default_effort` (`src/state/effort.rs:839`).

For a live session, inspection additionally owns the current and pending model,
effort, readiness and session-only apply rules
(`src/state/effort.rs:977`).

OpenCode remains a separate source adapter. Its launch and session pickers use
account short lists and folder/service catalogs, with provider metadata,
favorites and native effort labels
(`client/src/store/newSessionOpencode.ts:45`,
`client/src/ui/sessions/opencode-models.ts:153`).

## Current account-selection inventory

| Surface | Current display rule | Main inconsistency |
| --- | --- | --- |
| Accounts | Grouped `Account.label` | This is the desired canonical presentation |
| New session, classic | Usage email, cached email, then `profile.label` (`client/src/ui/sessions/newsession-ui.ts:202`) | Ignores grouped `Account.label`; configured names lose to email |
| New session, dense | Email or row name in `DenseAccount`/`DenseChip` (`client/src/ui/sessions/NewSessionDialog.tsx:137`, `:423`) | Repeats the email-first rule in another component |
| Composer account chip | Selected `Profile.label`, session account ID or `Native account` (`client/src/ui/sessions/AccountSwitchChip.tsx:31`) | A native profile grouped with a named alias can differ from Accounts |
| Inspector | Grouped `Account.label`, then recorded ID/home (`client/src/ui/sessions/Inspector.tsx:273`) | Closest to the desired rule; copy action intentionally prefers email |
| Session account filter | Group label plus email/organization, with computer/profile disambiguation (`client/src/ui/sessions/scope-filters.ts:53`) | Useful disambiguation logic, but a separate presentation implementation |
| New team and Swap member | `profile.label || account.label || profile.id`; native profiles omitted (`client/src/store/newTeamSources.ts:43`) | Profile-first order differs from Accounts; default and native are conflated |
| Team member card | ID or `Default account` when no labels map is supplied (`client/src/ui/teams/members.ts:60`, `client/src/ui/teams/MemberCard.tsx:186`) | Configured account names are usually absent |
| Add/Move team member | Free-form account ID (`client/src/ui/teams/MemberCard.tsx:393`, `:416`) | No account catalog or shared selection behavior |
| Usage popover | Usage ID, session account ID or `the native account` (`client/src/ui/sessions/UsageChips.tsx:68`) | Does not resolve the Accounts display label |

### Naming problems to fix

One account can currently appear as its configured label, raw profile ID,
email, `Default account`, `Native account` or `Account n/a`.

The default-account concept is also overloaded:

- an empty team account means the provider's effective default on that
  computer;
- `native-<provider>` identifies a concrete native profile;
- `Default account` is the built-in label of several native profiles;
- the account chosen with **Make default** may be a managed, non-native profile.

A unified model must represent these as different states rather than encode
them in label strings or infer every rule from an ID prefix.

## Current model/effort inventory

### Accounts → Model and effort

The read model has catalog models, Zerus defaults, provider defaults and effort
support (`client/core/src/account_actions/models.rs:20`). The UI offers Provider
settings and Provider default (`client/src/ui/screens/AccountDefaultsDialog.tsx:34`).

The projection drops `default_effort` and has no effort-label field, even
though the underlying catalog has a default effort. The dialog therefore cannot
present the selected model's native default as precisely as the catalog allows.

### Session → Model and effort

The session has its own `ModelChoice`, `modelChoices` and `effortChoices`
(`client/src/ui/sessions/session-model-settings.ts:5`, `:17`, `:63`). This path
correctly owns pending changes, readiness and session-only application; those
parts should not be replaced by the shared catalog model.

OpenCode effort labels are supported here. Non-OpenCode inspection currently
projects model IDs, labels and effort IDs without the same optional metadata
(`src/state/effort.rs:1000`).

### New session

The visible model/effort implementation is OpenCode-specific
(`client/src/store/newSessionOpencode.ts:108`, `:166`). Although the common
`LaunchChoices` type contains model and effort, the core keeps an explicit model
only for OpenCode and clears it for other agents
(`client/core/src/launch.rs:1347`).

Consequently, adding model selection for other providers to New session is a
separate launch-behavior change. It must not be slipped into a naming or type
refactor.

### Teams

Team controls reuse `team_role_models`, but their frontend rules differ from
Accounts and sessions:

- separate `ModelChoice` with only ID, label and effort IDs
  (`client/src/ui/teams/catalogControls.ts:133`);
- static `KNOWN_MODELS` and `EFFORTS` (`:135`, `:142`);
- model options fall back to static lists and retain the current custom model
  (`:148`);
- effort options fall back to the static list even when the selected model has
  no such variants (`:158`);
- New team uses its own fixed effort list instead of the loaded model's
  variants (`client/src/ui/teams/NewTeam.tsx:60`, `:264`);
- Team settings repeats the fixed efforts and free-form model input
  (`client/src/ui/teams/TeamSettingsDialog.tsx:295`);
- Add member uses free-form model and account fields
  (`client/src/ui/teams/MemberCard.tsx:416`).

Templates and roles may intentionally name a model that is absent from the
currently selected account. A closed dropdown is therefore not a universal
replacement for team text input; custom-model support must be an explicit
capability.

## Proposed shared model

Place pure types, normalization and transition functions in
`client/src/ui/provider-selection.ts`. Put asynchronous loading and the bounded
catalog cache in `client/src/store/providerSelection.ts`. Visual components can
be added later without making them owners of these rules.

A possible TypeScript shape is:

```ts
type AccountRef = {
  host: ComputerId;
  provider: string;
  profileId: string; // exact launch/session binding
};

type AccountSelection =
  | { kind: "default"; host: ComputerId; provider: string }
  | { kind: "profile"; ref: AccountRef };

interface AccountOption {
  ref: AccountRef;
  groupKey: string;
  label: string;         // Account.label, as shown in Accounts
  profileLabel: string;  // raw label of this exact profile
  email?: string;
  organization?: string;
  native: boolean;
  isDefault: boolean;
}

interface EffortOption {
  id: string;
  label: string;
}

interface ModelOption {
  id: string;
  label: string;
  efforts: EffortOption[];
  defaultEffort?: string;
}

interface ModelCatalog {
  models: ModelOption[];
  effortSupported: boolean;
  defaults?: { model: string; effort: string };
  providerDefaults?: { model: string; effort: string };
  status: "idle" | "loading" | "ready" | "error";
  error?: string;
  truncated: boolean;
}
```

OpenCode-only fields such as provider grouping, favorite/free/default marks and
full-catalog availability should remain in an OpenCode extension or adapter,
not become nullable noise in every provider's base model.

### Capabilities are separate from catalog availability

A readable model catalog does not imply that every workflow can apply every
field. The selection state should be driven by explicit capabilities, for
example:

```ts
interface SelectionCapabilities {
  chooseAccount: boolean;
  chooseModel: boolean;
  chooseEffort: boolean;
  allowCustomModel: boolean;
}
```

Apply behavior remains with the caller:

- New session builds a launch plan;
- Composer applies exact-session settings and handles pending identities;
- Accounts edits defaults with revision checks and per-computer results;
- team screens edit drafts, roles or members.

This keeps Kimi's native effort management, DeepSeek's distinct launch path,
OpenCode's folder catalogs and session pending rules out of a false common
apply abstraction.

## Shared function contracts

### Account presentation and options

- `accountOptions(view, criteria)` returns exact `AccountRef` bindings with the
  grouped display label and availability metadata.
- `accountDisplayName(group, profile?)` uses `Account.label` first, then
  `profile.label`, email and finally profile ID as fallbacks.
- `accountSecondaryText(group)` exposes email/organization as secondary text,
  never as a replacement for the configured name.
- `accountOptionLabel(option, siblings)` adds computer/profile disambiguation
  only when labels collide, following `scope-filters.ts:79`.
- Default selection remains `{ kind: "default" }`; its tooltip may show the
  resolved default profile, but its identity is not rewritten to that profile.
  A native selection remains `{ kind: "profile" }` and displays its Accounts
  label.

Selection and launch code must continue using IDs, never labels, as identity.

### Model and effort options

- `normalizeModelCatalog(raw, adapter)` validates and bounds source data,
  deduplicates model IDs and normalizes `id/value`, `effort_options/efforts`
  and optional labels/default effort.
- `modelOptions(catalog, current, capabilities)` retains an unavailable current
  model as unavailable/current, and permits a new custom ID only where the
  caller explicitly allows it.
- `effortOptions(catalog, modelId, current)` uses only the selected model's
  variants when a ready catalog names them. A current unavailable variant stays
  visible without becoming valid for a new selection.
- `selectAccount(state, account)` changes the account/catalog key and clears
  model and effort until the appropriate catalog is available.
- `selectModel(catalog, modelId, currentEffort)` preserves effort only when the
  new model offers it; otherwise it selects the source default or the empty
  provider/model default according to the workflow.
- `selectionCatalogKey(scope)` includes computer, provider and profile, plus
  folder for OpenCode. Cached answers must retain the existing request/context
  identity checks so a late answer cannot populate another account or folder.

Static known-model fallback may remain as an explicitly marked loading/error
fallback for team editors. It must not be presented as the selected account's
verified catalog, and a generic static effort list must not override an empty
variant list reported by a ready model catalog.

## Implementation plan

### Phase 1: unify account names without changing bindings

1. Add the shared account presentation/option functions and their tests.
2. Convert New session classic and dense rows to use grouped labels, keeping
   exact profile selection and ranking unchanged.
3. Convert AccountSwitchChip, team member labels, New team, Swap member,
   UsageChips and the session filter to the same resolver.
4. Keep email/organization as secondary text or tooltip content. Keep unknown
   recorded IDs/homes visible as diagnostics instead of inventing a native
   account identity.

Expected result: renaming an account in Accounts updates every selector to the
same primary name without changing launch, switch or filter bindings.

### Phase 2: align the catalog projections

1. Preserve `default_effort` in the Accounts and team-role client-core models;
   carry optional effort labels where a source provides them.
2. Reuse one bounded Rust catalog projection for
   `client/core/src/account_actions/models.rs` and
   `client/core/src/teams/role_models.rs` instead of maintaining two parsers.
3. Update `client/src/bridge.ts` to the shared wire shape.
4. Add the TypeScript normalizer and OpenCode adapter, preserving OpenCode's
   favorites, provider grouping and short/full catalog behavior.

Expected result: Accounts, teams, sessions and New session interpret the same
model entry and effort variant identically.

### Phase 3: migrate selection consumers

1. Replace the independent account/model/effort transitions in Account defaults,
   team catalog controls, New team and Swap member with the shared functions.
2. Adapt sessionModels while retaining its pending request, readiness, receipt
   and exact-session behavior.
3. Adapt New session's OpenCode selection without changing its plan.
4. Replace Add/Move member's free-form account input with catalog-backed
   choices; retain custom model input only under `allowCustomModel`.

Expected result: account changes always reset dependent choices, and model
changes preserve effort only when valid for the new model.

### Phase 4: compose the UI where it helps

Implemented `ProviderSelectionButton` and `ProviderSelectionDialog` in Composer.
The dialog combines the existing AccountSwitchChip with the extracted
ModelSettingsContent, so account switch/move and model pending/apply behavior
stay unchanged. Cancel restores focus to the button and Apply closes the
combined dialog.

New session's account/usage list, OpenCode's rich model picker and compact team
rows keep their own presentation. The shared contract is the model and behavior,
not necessarily identical presentation.

### Separate behavioral follow-up

Enabling explicit non-OpenCode model/effort choices in New session requires a
launch contract change: `client/core/src/launch.rs:1351` currently discards
those choices. Specify and test provider-specific launch behavior separately,
including precedence over saved account defaults and native flags.

## Concrete defects found during the audit

1. **Fixed.** `SwapMemberDialog` previously read models for `props.computer`
   but built account choices without passing that computer, so it used local
   accounts. It now passes `props.computer`; `SwapMemberDialog.render.test.ts`
   proves that the dialog offers the remote computer's accounts.
2. New session's common choice type suggests general model/effort support, but
   the plan keeps it only for OpenCode (`client/core/src/launch.rs:1351-1380`).
   Document this boundary until the separate launch follow-up changes it.

## Test plan

### Account naming

Cover renamed native profiles, a native profile grouped with a named alias,
missing identity, shared email, identical labels on different computers,
unknown recorded bindings and default-versus-native selection.

Update the characterization tests in:

- `client/src/ui/sessions/newsession-ui.test.ts`;
- `client/src/ui/sessions/NewSessionDialog.dense.test.ts`;
- `client/src/store/accountSwitch.test.ts`;
- `client/src/store/newTeamSources.test.ts`;
- `client/src/ui/sessions/scope-filters.test.ts`;
- team member and render tests that currently expect IDs or Default account.

### Model/effort behavior

Cover optional labels/default effort, duplicate and unavailable models, a model
with no effort variants, preservation of a current unavailable effort, custom
team models, account changes, model changes and stale catalog answers.

Update or extend:

- `client/src/ui/screens/account-defaults.test.ts`;
- `client/src/store/sessionModels.test.ts`;
- `client/src/store/newSessionOpencode.test.ts`;
- `client/src/ui/sessions/opencode-models.test.ts`;
- `client/src/ui/teams/catalogControls.test.ts`;
- `client/src/ui/teams/swapMember.test.ts`;
- New team and Team settings tests.

Add core parser tests for the complete catalog shape and a Swap member test
proving that accounts and models come from the same selected computer.

### Regression boundaries

The existing account grouping, launch binding, OpenCode favorites, session
pending identities and launch-plan tests must remain green unless a test's old
expectation intentionally encoded the inconsistent presentation being replaced.

Run the focused frontend and client-core suites for each phase, followed by the
project's standard client build and test commands for the final deliverable.

## Decisions to confirm before implementation

1. For an empty default selection, show `Default account` with the resolved
   account as secondary text; for an explicit native profile, show its Accounts
   label. This preserves the semantic distinction without hiding the default.
2. Keep custom model IDs in team templates/roles through an explicit capability;
   do not silently reject an existing role because its account catalog is
   unavailable.
3. Treat visual merging of Composer's account and model buttons as a later UI
   step, not a prerequisite for consistent data and names.
4. Treat explicit non-OpenCode launch model/effort selection as a separate
   behavior change rather than part of the initial refactor.


## Implementation outcome

Phases 1–4 are implemented:

- `client/src/ui/provider-selection.ts` owns account presentation, exact account
  references, model catalog normalization and account/model transitions.
- `client/core/src/model_catalog.rs` is the shared bounded Rust projection used
  by account defaults and team-role model reads. It preserves aligned effort
  labels and the model's default effort.
- New session classic/dense, Composer, Inspector support code, session filters,
  Go To, Accounts defaults, live session settings, New team, Swap member, Team
  designer, Team catalog editor, Team settings and Add/Move member now use the
  shared account or model rules as appropriate.
- Exact profile IDs still carry launch/switch/filter identity. Session pending
  identities, account revision checks, OpenCode favorites and all apply
  operations remain with their original workflows.
- Editable team model fields still accept custom IDs. A ready model catalog no
  longer invents generic effort variants for a model that reports none.
- The unused `Native account` fallback and its Next-only translation were
  removed; the generated Russian catalog was refreshed.
- Composer now opens one ProviderSelectionDialog for account, model and effort,
  with render coverage for Cancel/focus restoration and Apply/close behavior.

Verification completed:

- `npm run check` and `npm run build` pass.
- Focused frontend suites pass, including the new provider-selection tests and
  the remote Swap member regression test.
- Full frontend suite: 4,332 tests pass. The only failure is the pre-existing
  scrollbar coverage audit in `src/ui/styles/popups.test.ts` for
  `.nt-intro-lead`, `.tg-nav` and `.tg-article`, unrelated to this change.
- Focused client-core model tests pass. The full client-core library suite has
  1,820 passing tests and two unrelated machine-dependent failures:
  `image::linux::tests::same_inode_native_rewrite_after_the_last_check_executes_the_immutable_original`
  and
  `task_start::tests::native_capture_reads_the_actual_lost_reply_checkpoint_without_another_delivery`.
  Both also fail when rerun individually.
- `python3 scripts/ci/check-source.py`, `python3 scripts/check-ui-separators.py`
  and `python3 client/scripts/generate-i18n-catalog.py --check` pass.
- `git diff --check` passes. The installed Rust toolchain lacks the `rustfmt`
  component, so `cargo fmt --check` could not be run.
