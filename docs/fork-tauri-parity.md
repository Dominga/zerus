# Fork-only Qt features in Zerus Next

Inventory of the WhiteWh fork's Qt desktop (`tray/src`) against `client/src`, made
for the owner's switch to the Tauri app (task 64). **Ported** means the Tauri
client has the feature with its own tests; **partial** means the main path exists
but some controls do not; **named** means it is not ported and the reason follows.

| Feature | Qt location | Tauri location | Status |
| --- | --- | --- | --- |
| Teams (workspace, waiting, plan modes, New team, settings, catalog, designer) | `Team*.cpp`, `Team*.h` | `client/src/ui/teams`, `client/src/store/teams*`, `client/core/src/teams` | Ported |
| Model servers tab and Settings > Model servers | `ModelServersPage.*`, `ModelServerSettings.*`, `ModelServers.*` | None | Named: the client core has no model-server read/action layer; the root `src/state/model_servers.rs` is not exposed as a Tauri command and its `up`/`down` controls run native processes. Needs a core projection and owner-approved control semantics before porting. |
| Permission catalog with native controls | `PermissionCatalogDialog.*`, `PermissionCatalog.*`, `AccountsPage.cpp` | `Accounts.tsx`, `AccountDefaultsDialog.tsx` | Partial: Accounts shows the catalog summary, permission source and provider/bypass modes; the per-control native editor is missing because `client/core` account actions only accept `provider` or `bypass`. |
| Project iron rules panel and editor | `IronRulesPanel.h`, `IronRulesEditor.h`, `IronRules.h` | `client/src/ui/teams/TeamSettingsDialog.tsx` for team/member rules | Partial: team and member rules are ported; the project/session iron-rules panel has no client core read/action layer. |
| DeepSeek/dsh and Kimi accounts, model lists, effort | `AccountsPage.cpp`, `AccountSwitch.*`, `ModelServers.*` | `Accounts.tsx`, `AccountSetupDialog.tsx`, `AccountDefaultsDialog.tsx`, `store/accounts*` | Ported: the account provider list, models, effort and permissions mode come from the core account views; tests cover dsh and Kimi provider rows. |
| Per-session account switch chip | `AccountSwitch.*` | `Accounts.tsx` account management | Partial: accounts can be added and made default; the session toolbar chip that switches one running session's account is not present. |
| Fork update feed and replacement | `ForkUpdater.*` | `store/updates.ts`, `settings/Updates.tsx` | Named: the Tauri updater follows the upstream release channels; the fork feed, replacement and source build are a release/signing decision, not a UI port. |
| Mobile relay and legacy relay settings | `WindowsMobileServices.h`, `WindowsMobileSettings.h`, `SettingsPage` | `settings/Mobile.tsx`, `settings/LegacyRelay.tsx`, `store/*relay*` | Ported |
| Task board additions and task actions | `Task*.cpp`, `Task*.h` | `screens/Tasks.tsx`, `store/taskBoard.ts`, `TaskInput.tsx` | Ported |
| In-app image, video and file viewer | `MediaViewer.h`, `ActivityView.cpp` | `sessions/Activity.tsx` (`MediaCard`, `AttachmentCard`), `store/attachments` | Ported |
| Workspace skins, icons and the custom title bar | `WorkspaceSkin.h`, `WorkspaceIcons.h`, `WindowTitleBar.*` | `client/src/skin`, `settings/Appearance.tsx`, native Tauri frame | Partial: skins and icons are ported; the fork's custom Qt title bar is replaced by the native Tauri window frame. |
| WSL path mapping and WSL file open | `WslPathMappingDialog.*`, `WslFileOpenDialog.*`, `DirectoryDialog.cpp` | None | Named: these dialogs translate Windows paths to a WSL distro inside the Qt host. The Tauri host would need the same mapping and file-open commands; no core layer exists yet. |
| Claude app session import | `ClaudeAppSessionsDialog.h` | None | Named: it reads Claude Desktop's session store and imports a conversation; needs a host-side importer and a privacy review of that store. |
| SocratiCode panel and Extensions | `SocratiCodePanel.h`, `Extensions.h` | None | Named: the fork's extension host and its native panel are not part of the Tauri client; porting needs an extension API, not a screen. |
| Session leads and attention watch | `SessionLeadPanel.h`, `SessionLeads.h`, `TeamAttentionWatch.*` | Teams lead panels and attention store | Partial: team lead/attention is ported for Teams; the standalone session-lead panel is not. |
| New session's dense form and folder choices | `NewSessionDense.h`, `SessionLaunch` paths | `sessions/NewSessionDialog.tsx` | Ported |
| Working line, agent working, held input and terminal draft bar | `WorkingLine.h`, `AgentWorking.h`, `HeldInputBar.h`, `TerminalDraftBar.h` | `sessions/Activity.tsx`, `Composer.tsx`, `store/composer.ts`, `store/terminal.ts` | Partial: the working/activity and draft paths are ported; the Qt working-line and terminal-draft widgets have no separate Tauri equivalent. |
| Private-store and sandbox notices | `PrivateStoreNotices.h`, `SandboxCheck.h` | Core private-store errors surface in account and team actions | Partial: core checks exist; the dedicated notices are not shown by a Qt-equivalent panel. |
| Cache keepalive, slow-event log, hang trace, fork log, path probe, mapped file, PowerShell 7 helper | `CacheKeepalive.*`, `SlowEventLog.*`, `HangTrace.*`, `ForkLog.h`, `PathProbe.*`, `MappedLocalFile.*`, `PowerShell7.h` | Diagnostics and launch code in `client/src-tauri`, `client/src/perf` | N/A: internal instrumentation and host helpers, not owner-facing screens. |
| Teams member/task account move | `TeamAccountMove.*` | `client/src/ui/teams` member actions | Ported |

## Named for the owner

The switch keeps these owner-visible gaps unless they are scheduled: **Model
servers**, the **native permission catalog editor**, the **project iron-rules
panel**, the **fork updater feed**, **WSL path mapping**, **Claude app session
import**, **SocratiCode/Extensions**, and the **standalone session-leads panel**.
Each needs a new core/host layer or a release-channel decision, not only UI.

The remaining forks' screens (Teams, accounts and providers, mobile relay, task
board, media viewer, skins, New session) are already in the Tauri client.
