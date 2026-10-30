#pragma once
namespace AccountUsage {class Button;class RefreshButton;}
namespace SessionUsage {class ContextButton;}
namespace CacheStatus {class Chip;}
namespace RecoveryUi {class Panel;}
namespace ActivityWidth {class ColumnLayout;}
class ArchiveBrowser;
class ToolbarChip;
class SettingsPage;
class SwarmController;
class WorktreeCleanupStatus;
class SessionCreationController;
class QPlainTextEdit;

#include <QPointer>
#include <QJsonArray>
#include <QJsonObject>
#include <QTimer>
#include "SessionStates.h"
#include <QDeadlineTimer>
#include <QElapsedTimer>
#include <QWidget>
#include <memory>
#include <optional>
#include "FleetState.h"
#include "HgsClient.h"
#include "SessionOrganization.h"
#include "SessionRowPreferences.h"
#include "TeamModel.h"   // WhiteWh fork: agent teams
#include "TeamCatalogEditor.h"
#include "SessionAccounts.h"
#include "HeldInputBar.h"
#include "SessionRows.h"
#include <QFont>

class QAction;
class QFileSystemWatcher;
class QLabel;
class QLineEdit;
class QListWidget;
class QListWidgetItem;
class QPushButton;
class QTextBrowser;
class QStackedWidget;
class QSplitter;
class QMenu;
class ProjectsDialog;
class WorktreePanel;
class ActivityView;
class MessageComposer;
class QuestionCard;
namespace ModelServers { class Controller; }
class TerminalView;
class NativeUiView;
class QTabWidget;
class QFrame;
class SessionList;
class SessionPanelDock;
class QHBoxLayout;
class MachinesPage;
class MachineFilter;
class ProjectFilter;
class AccountFilter;
class DashboardPage;
class AccountsPage;
class GoToDialog;
class ShortcutsDialog;
struct GoToItem;
class SessionSearch;
class IdentityBadge;
class SwarmButton;
class QToolBar;
class BusyIndicator;
class ShortcutRouter;

// A view over hgs snapshots and its durable event journal. Closing this window
// never owns or terminates an agent. All operations still go through the CLI.
class SessionsWindow : public QWidget {
    Q_OBJECT
public:
    explicit SessionsWindow(const QString &hgsPath, QWidget *parent = nullptr);
    void setFleet(const FleetState &fleet);
    // The same listings as the last setFleet, refreshed by FleetState::refreshListing:
    // pages take the new poll times and metrics, while rows, read state and the
    // organization stay unless time alone changed what they show.
    void refreshFleet(const FleetState &fleet);
    void showSessionList();
    void showUpdates();
    void showSession(const QString &host, const QString &name);
    void showAttentionSession(const QString &host, const QString &name);
    void showNotificationNotice(const QString &message) { showNotice(message, false); }
    // N7: a click on a team attention notification opens that team.
    void showTeamAttention(const QString &token);
    void showProjects(const QString &host = {});
    // WhiteWh fork: name, a suggested session name, comes last.
    void showNewSession(const QString &agent = {}, const QString &project = {}, const QString &host = {}, const QString &folder = {}, const QString &path = {}, const QString &account = {}, const QJsonObject &recipe = {}, const QString &taskUuid = {}, const QString &taskParentId = {}, const QString &name = {});
    void newSessionLaunchFailed(const QString &launchId, const QString &error = {});
    void setClipboardMode(bool clipboard);
    void setConnectionError(const QString &error);
    void setPollingHosts(const QSet<QString> &hosts);
    static QString status(const SessionInfo &session, bool reachable = true);
    // Tests only, so suites need not wait out fixed delays: timed notices
    // (5 s), compaction polls (2.5 s), the details poll tick (500 ms) with its
    // Activity interval (2.5 s), and the wait of Send and push now (20 s, which
    // also restarts an armed push's wait). The application keeps the defaults.
    void setNoticeDurationForTests(int ms) { m_noticeMs = ms; }
    void setCompactPollIntervalForTests(int ms) { m_compactTimer.setInterval(ms); }
    void setDetailsPollForTests(int tickMs, int activityMs) { m_timer.setInterval(tickMs); m_activityPollMs = activityMs; }
    void setPushTimeoutForTests(int ms) { m_pushTimeout = ms; if (m_push) m_push->deadline.setRemainingTime(ms); }

    // Sessions rows are prepared off the GUI thread (spec "Model preparation off
    // the GUI thread"). rebuild() reconciles the window's own state on the GUI
    // thread, copies what the rows read into RowInputs, and prepareRows() turns
    // that copy into PreparedRows on the shared preparation thread
    // (ProcessRunner::post). Only the newest preparation is applied, and only
    // rows whose digest changed are written to the list.
    using Entry = SessionRows::Entry;
    struct RowInputs {
        quint64 generation = 0;
        QList<Entry> entries;   // listed sessions, sorted, with organization identities
        FleetState fleet;
        SessionOrganization organization;
        SessionRowPreferences rowPreferences;
        SessionRows::Selection selection;
        QString readSelectionKey, creationSessionToSelect, revealCreation;
        // Keys with an unsent draft in the message and subagent composers.
        QSet<QString> drafts, childDrafts, expanded;
        QString filter;
        QSet<QString> hostFilters, projectFilters, accountFilters;
        QString folderHost, folderPath;
        bool folderCheckout = false;
        double archiveFrom = 0, archiveUntil = 0;   // the archive window shown
        QJsonObject pendingLaunches;
        QSet<QString> ownedCreations;
        bool includeCompleted = false, hideEmptyProjects = true;
        QHash<QString, Entry> terminating;
        QHash<QString, QJsonObject> gitStatus;      // host + '\n' + path
        QHash<QString, QColor> machineColors;
        QJsonArray accountProfiles, accountDetailsProfiles;
        QFont tooltipFont;
        int accessibleWidth = 420;
        qint64 nowSecs = 0;
        // WhiteWh fork: agent teams and session leads, copied on the GUI thread.
        QList<Teams::Team> teams;
        QSet<QString> teamProblems;                 // team id, newline, member id
        QHash<QString, QVariantMap> teamStrips;     // a lead card's strip, by team id
        QSet<QString> collapsedTeams;
        QJsonObject sessionLeads;                   // SessionLeads::all()
        QHash<QString, int> keepWarmHours;          // Keep cache warm, by entry key
    };
    struct PreparedRow {
        enum Kind { Header, Session, Child, Launch };
        Kind kind = Session;
        QString key, parentKey, child, launch;
        Qt::ItemFlags flags;
        bool hidden = false;
        // Every role the row's item holds; any other row role is cleared.
        QMap<int, QVariant> roles;
        QByteArray digest;   // of key, flags, hidden and roles
    };
    struct PreparedRows {
        quint64 generation = 0;
        QList<PreparedRow> rows;
        bool uniqueKeys = true;
        int firstSession = -1, firstVisible = -1;
        QMap<QString, int> counts;   // by state filter
        QMap<QString, int> machineCounts, projectCounts, accountCounts;
        QSet<QString> projectKeys, drafts;
        QJsonObject creationScope;
        QList<SessionAccounts::Account> accounts;
        QList<std::pair<QString, QString>> gitReads;
        int unreadReplies = 0;
        QString countText, countShort;
    };
    // Reads only its argument: callable on any thread.
    static PreparedRows prepareRows(const RowInputs &inputs);
    // Tests: Blocking prepares on the preparation thread while the GUI thread
    // waits, so rows are current when rebuild() returns, as suites expect.
    enum class RowPreparation { Async, Blocking };
    static void setRowPreparationForTests(RowPreparation mode);
    RowInputs rowInputsForTests() const { return rowInputs(); }
    quint64 rowGenerationForTests() const { return m_rowGeneration; }
    bool rowsPendingForTests() const { return m_rowsPending || m_rowRequestQueued; }
    int droppedRowPreparationsForTests() const { return m_droppedRowPreparations; }
    int postedRowPreparationsForTests() const { return m_postedRowPreparations; }

signals:
    // Prepared rows of this generation were written to the list.
    void rowsApplied(quint64 generation);

    void replyViewed(const QString &host, const QString &name, const QString &conversation, const QString &reply);
    void repliesMarkedRead(const QJsonObject &replies);
    void attentionMarksChanged(const QJsonObject &marks, const QJsonObject &readReplies);
    void notificationPreferencesChanged(const QString &host, const QString &name, const QString &run,
                                        const QString &conversation, const QJsonObject &preferences);
    void refreshRequested();
    // One computer's listing now: a created session waits to be confirmed there.
    void hostRefreshRequested(const QString &host);
    void sessionActivated(const QString &host, const QString &name, const QString &tty);
    void terminalOpenRequested(const QString &host, const QString &name, const QString &tty);
    void folderShellRequested(const QString &host, const QString &directory);
    void copySessionCommandRequested(const QString &host, const QString &name, const QString &archiveId);
    void copyTextRequested(const QString &text);
    void newSessionRequested(const QString &host, const QString &cmd, const QString &target, const QString &name, const QString &account, const QString &launchId, const QString &permissions = {}, const QString &localModel = {}, const QString &expectedMachine = {});
    void projectsChanged(const QString &host);
    void accountLoginRequested(const QString &host, const QString &id);
    void accountInstallRequested(const QString &host, const QString &provider);
    void machineSshRequested(const QString &alias);
    // N7 (U18): a team's attention item, raised once and withdrawn when gone.
    void teamAttentionRaised(const QString &token, const QString &title, const QString &body);
    void teamAttentionCleared(const QString &token);
    // U25 (spec 2.3): the sessions of team members other than the lead
    // (host + '\n' + name), muted until they carry their own choice.
    void teamMembersChanged(const QSet<QString> &members);

public:
    // WhiteWh fork: apply the selected skin again, as after a change in Settings.
    void applySkin() { applyTheme(); }
protected:
    bool eventFilter(QObject *watched, QEvent *event) override;
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
    void closeEvent(QCloseEvent *event) override;
    void changeEvent(QEvent *event) override;
    bool nativeEvent(const QByteArray &eventType, void *message, qintptr *result) override;

private:
    ArchiveBrowser *m_archiveBrowser = nullptr;
    QMap<QString,QList<SessionInfo>> m_archiveSearchRows;
    void applyTheme();
    void updateWindowPinAppearance();
    void applyContentScale();
    void applyActivityWidth();
    void saveOrganization();
    void applyFleet(const FleetState &fleet, bool refresh);
    bool refreshEntries();
    void setSessionRowCollapsed(const Entry &entry, bool collapsed);
    void saveSessionRowPreferences();
    void updateSessionRowPresentation();
    void savePendingLaunches();
    void dispatchTaskLaunches();
    bool saveTaskLaunches();
    QJsonObject m_taskLaunches;
    bool m_taskLaunchRecoveryBlocked = false;
    QHash<quint64, QString> m_taskLaunchRequests;
    void renderCreation();
    void selectCreation(const QString &id);
    void placeLaunchedSessions();
    bool takeEndedLaunch(const QString &launchId, QJsonObject *launch = nullptr);
    void createGroup(const QString &session = {});
    void showGroupMenu(const QString &group, const QPoint &position);
    void showWorkspaceSettings();
    void revealSession(const QString &key);
    // The listed session a saved draft belongs to, or null.
    const Entry *draftOwner(const QString &draftKey) const;
    void openDraftSession(const QString &draftKey);
    void updateHeaderText();
    void updateSessionLoading();
    void updateSessionFeedback();
    void rebuild();
    // The row preparation (SessionsWindowRows.cpp): the copy rebuild() takes, its
    // request to the preparation thread, and writing the newest result.
    RowInputs rowInputs() const;
    void requestRows();
    void postRows();
    void applyRows(const PreparedRows &prepared);
    void applyListRows(const PreparedRows &prepared);
    static QMap<int, QVariant> sessionCardRoles(const Entry &entry, const RowInputs &inputs);
    static QMap<int, QVariant> childCardRoles(const Entry &entry, const QString &id, const QJsonObject &child, const RowInputs &inputs);
    // Selects (or only scrolls to) the row with this key now, and again when
    // rows still being prepared are applied.
    void revealRow(const QString &key, bool select);
    void revealPendingRow();
    SessionRows::Selection rowSelection() const { return {m_selectedKey, m_details, m_subagentId, m_subagentConversation}; }
    static inline RowPreparation s_rowPreparation = RowPreparation::Async;
    quint64 m_rowGeneration = 0;
    bool m_rowsPending = false, m_rowRequestQueued = false;
    // One row preparation in flight; requests meanwhile fold into one more.
    bool m_rowPreparing = false, m_rowsAgain = false;
    QTimer *m_confirmPoll = nullptr; QElapsedTimer m_confirmPollAge;
    void pollConfirmingCreations();
    int m_droppedRowPreparations = 0, m_postedRowPreparations = 0;
    // One-time choices rebuild() made for the next applied rows: the row to
    // select instead of the selected session, a placed creation, a fork whose
    // terminal opens once it is selected, and a row to reveal.
    std::optional<QString> m_rowSelection;
    bool m_rowCreationPlaced = false;
    QString m_forkTerminalKey;
    struct PendingReveal { QString key; bool select = false; };
    std::optional<PendingReveal> m_reveal;
    void selectSession();
    void inspect();
    // The DeepSeek native UI: its tab when the embedded plugin is available,
    // otherwise the system browser (NativeBrowserPlugin.h).
    void showNativeUi(const Entry &entry);
    void openSubagent(const QString &id);
    void closeSubagent();
    QJsonObject childRoster(const Entry &entry) const;
    bool showChild(const Entry &entry, const QString &id, const QJsonObject &child, bool countsComplete, bool includeCompleted) const;
    QString childKey(const Entry &entry, const QString &id) const;
    bool childNeedsAction(const Entry &entry) const;
    bool entryNeedsAttention(const Entry &entry) const;
    bool entryHasDraft(const Entry &entry) const;
    void renderDetails();
    void updateWorkingLine(const Entry *entry);
    void loadMedia(class ActivityView *view, const QString &key, const QString &href, bool open);
    struct MediaDownload { QPointer<class ActivityView> view; QString key; bool open = false; };
    QHash<quint64, MediaDownload> m_mediaDownloads;   // WhiteWh fork
    // The Tasks tab follows the open session's project and its sessions.
    void updateTasks(const Entry *entry);
    void updateWorktrees();
    void filterFolder(const QString &host, const QString &path, bool checkout);
    void openRelatedSession(const QString &host, const QString &name, const QString &archive);
    void checkViewedReply();
    void markAllRepliesRead();
    void updateAttentionIndicator();
    void updateSessionsToggle();
    QString interruptedPrompt(const QString &key) const;
    void restoreInterruptedPrompt();
    struct InterruptedPrompt { QString run, conversation, text; };
    QHash<QString, InterruptedPrompt> m_interruptPrompts;
    void applySessionStrip();
    SessionPanelDock *m_sessionDock = nullptr;
    QPushButton *m_sessionsToggle = nullptr, *m_stripSearch = nullptr, *m_batchButton = nullptr, *m_panelNewSession = nullptr;
    // WhiteWh fork: the rail's New session and its session list toggle (cards view).
    QPushButton *m_railNewSession = nullptr, *m_railSessionsToggle = nullptr;
    void applyWorkspaceView();
    QString foldInspectorSections(const QString &html) const;
    void toggleInspectorSection(const QString &key);
    QHBoxLayout *m_sessionHeader = nullptr, *m_filterRow = nullptr;
    QString m_countFull, m_countShort;
    bool m_focusSearch = false;
    // WhiteWh fork: the owner opened the finished subagents in the Tasks card.
    bool m_showFinishedChildren = false;
    // WhiteWh fork: the selected session's background jobs, at most 64.
    QJsonArray m_backgroundJobs;
    int m_stripLayout = 0, m_stripReserve = 0;
    QWidget *m_stripSpacer = nullptr;
    QPushButton *m_savedDrafts = nullptr;
    QPushButton *m_markAllReadButton = nullptr;
    QLabel *m_attentionBadge = nullptr;
    int m_attentionCount = 0;
    QTimer m_readTimer;
    QElapsedTimer m_readDwell;
    QString m_readCandidate, m_readSelectionKey;
    void acceptInspection(const QString &host, const QString &name, const QJsonObject &data, const QString &archiveId);
    void showNotice(const QString &message, bool error = false, bool transient = false);
    void clearNotice();
    // Opens a session file like a browser download; see ExternalFile.h.
    void openExternalFile(const QString &path, bool otherMachine);
    void jumpToLatestActivity(QListWidgetItem *item);
    void copySessionIdentity();
    void openSession();
    void changeSession();
    // Spec D5: asks before resuming or restoring this selected session starts its
    // agent in a session host of another Windows session. False after Cancel, or
    // when the selection changed while the warning was open.
    bool confirmLocalStart(const Entry original);
    void terminateSession();
    // F7: a member session a Done/Stopped team left running. The Team tab has
    // already confirmed; this selects it and runs the terminate flow without
    // asking a second time.
    void stopTeamSession(const QString &host, const QString &name);
    void archiveSession();
    // Native lifecycle cleanup reports its result after Archive or Terminate.
    // Keep a review action for remaining work. Host, session, root, common dir.
    QStringList m_archivedWorktree;
    QString m_archivedWorktreeRun, m_archivedWorktreeOwnerId, m_worktreeCleanupReason;
    bool m_worktreeCleanupOffered = false, m_worktreeCleanupUnconfirmed = false;
    QPushButton *m_worktreeCleanup = nullptr;
    WorktreeCleanupStatus *m_terminationCleanup = nullptr;
    QHash<QString,QJsonObject> m_terminationCleanupResults;
    int m_worktreeCleanupRevision = 0;
    void offerWorktreeCleanup();
    void updateWorktreeCleanupOffer();
    void cleanUpArchivedWorktree();
    void reviewTerminatedWorktrees(const QList<QStringList> &offers);
    bool m_removeTerminatedWorktree = false;
    void renameSession();
    void forkSession();
    void populateBatchActions(QMenu *menu);
    enum class SelectionAction { MarkRead, ReviewLater, CompactRows, ExpandRows, Pause, Resume, Archive, Forget, Terminate, Move };
    QList<Entry> selectionEntries() const;
    bool selectionActionApplies(const Entry &entry, SelectionAction action) const;
    static QString selectionActionText(SelectionAction action);
    static QString selectionActionIcon(SelectionAction action);
    void setupSelectionActions();
    void updateSelectionActions();
    void showSelectionMenu(const QPoint &position);
    void populateSelectionMenu(QMenu *menu, const QList<Entry> &entries);
    void populateSelectionProjects(QMenu *menu, const QList<Entry> &entries);
    void runSelectionAction(SelectionAction action, const QList<Entry> &entries, bool clearArchive = false);
    void runNextSelectionAction();
    bool finishSelectionAction(const QString &operation, bool ok, const QString &detail);
    void clearArchive();
    QFrame *m_selectionBar = nullptr;
    QLabel *m_selectionCount = nullptr;
    QToolBar *m_selectionTools = nullptr;
    QMap<SelectionAction, QAction *> m_selectionActions;
    QAction *m_clearArchiveAction = nullptr;
    QList<Entry> m_selectionQueue;
    SelectionAction m_selectionOperation = SelectionAction::Pause;
    QString m_selectionCommand, m_selectionRestoreKey;
    // The scoped request of the selected OpenCode binding being forgotten; only
    // its own result advances the selection.
    quint64 m_selectionForgetRequest = 0;
    QStringList m_selectionErrors;
    int m_selectionTotal = 0, m_selectionDone = 0, m_selectionFailed = 0;
    bool m_selectionRunning = false;
    bool m_selectionRemoveWorktrees = false;
    QStringList m_selectionCleanupTarget;
    QList<QStringList> m_selectionCleanupOffers;
    int m_selectionCleanupOverflow = 0;
    enum class SessionMenuAction { OpenTerminal, OpenProcesses, OpenFolder, FolderShell, CopyCommand, ChangeState, Rename, Fork, Archive, CopyName, CopyReference, CopyFolder, Terminate, MarkRead, ReviewLater, StartFresh, Clear, ToggleMute, ToggleKeepWarm, CollapseRow, ExpandRow };
    void copySessionReference(const Entry &entry, const QString &agent = {});
    void showSessionMenu(const QString &key, const QPoint &position);
    void clearContext();
    // A stopped session's draft waits for an explicit resume or fresh start.
    bool canStartStopped(const Entry &entry) const;
    void startFresh();
    void startToSend(const QString &key, const QString &text, const QList<MessageAttachment> &attachments);
    struct StartContinuation {
        QString key, host, name, run, conversation, text;
        QList<MessageAttachment> attachments;
        bool fresh = false, compact = false;
        qint64 started = 0;
    } m_startSend;
    // U3: the resume guard refused a stale transcript; ask how to continue.
    void resumeBehind(const QString &name, const QString &detail);
    void cancelStartContinuation(const QString &reason);
    void continueAfterStart(const QString &host, const QString &name, const QJsonObject &data);
    struct CompactContinuation {
        quint64 request = 0;
        QString key, host, name, run, conversation, text, retryId, commandId;
        QList<MessageAttachment> attachments;
        qint64 started = 0;
    };
    QHash<QString, CompactContinuation> m_compacts;
    QTimer m_compactTimer;
    QPushButton *m_compactCancel = nullptr;
    void cancelCompactContinuation(const QString &key, const QString &reason);
    void pollCompactContinuations();
    void continueAfterCompact(const QString &host, const QString &name, const QJsonObject &data);
    void compactToSend(const QString &key, const QString &host, const QString &name, const QString &run, const QString &conversation,
                       const QString &text, const QList<MessageAttachment> &attachments, const QString &retryId = {});
    quint64 m_clearRequest = 0;
    QString m_clearKey;
    quint64 m_interruptRequest = 0;
    QString m_interruptKey;
    QString m_interruptName, m_interruptMachine;
    quint64 m_queueSendRequest = 0;
    QString m_queueSendKey;
    // One confirmed OpenCode inbox removal at a time.
    quint64 m_removeQueuedRequest = 0;
    QString m_removeQueuedKey, m_removeQueuedMessage, m_removeQueuedName;
    void removeQueuedMessage(const QString &messageId);
    // A confirmed OpenCode Forget that removes only the Zerus binding.
    quint64 m_forgetBindingRequest = 0;
    QString m_forgetBindingName;
    bool queuedMessageRemovable(const QString &messageId) const;
    void runSessionMenuAction(const Entry &original, SessionMenuAction action);
    bool selectSessionEntry(const Entry &original);
    QString selectedDirectory() const;
    void openFolderShell();
    void openSessionFolder();
    void openLocalFolder(const QString &directory);
    // #744: a session folder's Open folder state, read from PathProbe (never a
    // stat on this thread): enabled, checking, and its tooltip.
    struct FolderState { bool open = false; bool checking = false; QString hint; };
    FolderState folderState(bool local, const QString &directory) const;
    void showFolderState(QPushButton *button, QAction *action, bool local, const QString &directory);
    QString m_folderButtonPath, m_menuFolderPath;
    QPointer<QAction> m_menuFolderAction;
    const Entry *selected() const;
    bool awaitingRestoredInspection(const Entry &entry) const;
    bool isTerminating(const Entry &entry) const;
    void reconcileTerminations();
    bool archiveNameOccupied(const Entry &archive) const;
    QString messageBlockReason(const Entry &entry) const;
    QString modelSettingsBlockReason(const Entry &entry) const;
    // U53: the account chip beside the model, and why it cannot change now.
    void updateAccountChip(const Entry &entry, bool unavailable);
    QString accountSwitchBlockReason(const Entry &entry) const;
    void changeModelSettings(const QString &key, const QString &model, const QString &effort);
    // OpenCode's grouped model picker in the composer: catalog and favorite requests.
    void connectOpenCodeModels();
    QString openCodeCatalogSession(quint64 request, const QString &host, const QString &location);
    void applyPendingModelSettings();
    void applyQueuedModelSettings();
    void sendMessage(const QString &key, const QString &text, const QList<MessageAttachment> &attachments, const QString &retryId = {}, const QString &compactionId = {}, bool contextChosen = false);
    void deliverMessage(const QString &key, const QString &host, const QString &name, const QString &run, const QString &conversation,
                        const QString &text, const QList<MessageAttachment> &attachments, const QString &retryId, const QString &compactionId, bool fullContextConsent = false, const QJsonObject &evidence = {});
    void messageAction(const QString &id, const QString &action);
    void finishMessage(quint64 request, bool ok, const QJsonObject &receipt, const QString &error = {}, bool uncertain = false);
    void reconcileMessages();
    void answerQuestion(const QString &key, const QString &questionId, const QJsonArray &answers);
    void finishQuestion(quint64 request, bool ok, const QString &error = {}, bool uncertain = false, bool submitted = false);
    void renderQuestion(const Entry &entry, int navigation = 0);
    void setInspectorVisible(bool visible);
    void updateInspectorMinimum();
    void applyInspectorWidth();
    // Keyboard commands of the Window scope (Shortcuts.h), implemented in
    // SessionsWindowShortcuts.cpp. Every command's action stays enabled; a
    // command with nothing to do does nothing or shows a notice.
    void setupShortcuts();
    void applyShortcuts();
    void runShortcut(const QString &id);
    QAction *shortcutAction(const QString &id) const;
    // Empty queueId: the selected session's current snapshot. False, with a
    // notice when nothing can be sent, unless a request is already running.
    bool sendQueuedNow(const QString &queueId = {});
    // Go to and the cheat sheet, in SessionsWindowGoTo.cpp.
    void openGoTo();
    void openShortcutHelp();
    QList<GoToItem> goToItems() const;
    void activateGoTo(const GoToItem &item);
    void rememberRecentSession(const QString &key);
    GoToDialog *m_goTo = nullptr;
    ShortcutsDialog *m_shortcutHelp = nullptr;
    QStringList m_recentSessions;
    void stepSession(int direction, bool attention);
    void stepTab(int direction);
    void stepFocus(int direction);
    void stepContentScale(int direction);
    QWidget *viewFocusTarget() const;
    QHash<QString, QAction *> m_shortcutActions;
    ShortcutRouter *m_shortcutRouter = nullptr;
    // Send and push now (message.push): after its send is confirmed, the first
    // snapshot of this session's native queue that holds the message is sent now.
    // Run and conversation are those its send request expects.
    struct PendingPush { QString key, run, conversation, text; quint64 request = 0; bool confirmed = false; QDeadlineTimer deadline; };
    std::optional<PendingPush> m_push;
    QTimer m_pushTimer;
    int m_pushTimeout = 20000;
    void armPush(const QString &key, const QString &text);
    void pushSendFinished(quint64 request, bool ok, bool uncertain);
    void pushInspected(const QString &key, const QJsonObject &details);
    // Before another session (or none) becomes the selected one.
    void pushSelectionChanged(const QString &key);
    void checkPush();
    void endPush(const QString &notice = {}, bool error = false);

    IdentityBadge *m_providerBadge, *m_machineBadge;
    HgsClient m_client;
    QString m_nativeLaunchToOpen;
    FleetState m_fleet;
    QList<Entry> m_entries;
    QJsonObject m_pendingLaunches;
    SessionCreationController *m_creations=nullptr;
    QString m_selectedCreation,m_revealCreation,m_creationSessionToSelect;
    QWidget *m_creationPanel=nullptr;
    QLabel *m_creationTitle=nullptr;
    QPlainTextEdit *m_creationText=nullptr;
    QPushButton *m_creationContinue=nullptr,*m_creationCheck=nullptr,*m_creationStop=nullptr,*m_creationEdit=nullptr;
    bool m_admittingCreation=false;
    QHash<QString,QString> m_creationNotices;
    // Launches a listing reported ended, oldest first, with their launch intent.
    QList<QPair<QString, QJsonObject>> m_endedLaunches;
    QHash<QString, Entry> m_terminating;
    QSet<QString> m_terminationFinished;
    QString m_terminationCommand;
    QString m_restoreKey, m_renameKey, m_forkKey, m_forkGroup, m_forkBefore;
    struct PendingRestore { Entry source; QString conversation; bool succeeded = false; };
    // Durable draft handoffs outlive list navigation. Only m_restoreKey follows
    // the user's selected conversation, and only one legacy write is in flight.
    QMap<QString, PendingRestore> m_restores;
    QString m_restoreWriteKey;
    QMap<QString, QPair<QString, QString>> m_restoredInspections;
    bool m_renameArchived = false;
    QString m_selectedKey, m_filter = "all";
    bool m_initialSelectionPending = true;
    QString m_folderFilterHost,m_folderFilterPath;
    bool m_folderFilterCheckout=false;
    QPushButton *m_folderFilterClear=nullptr;
    WorktreePanel *m_worktrees=nullptr;
    class SocratiCodePanel *m_socratiCode=nullptr;
    // WhiteWh fork: agent teams on this computer and the inspector's Team tab.
    class TeamController *m_teams=nullptr;
    std::shared_ptr<class TeamAttentionNotices> m_teamNotices;
    QString m_pendingTeamAttention;   // a clicked notice whose team is not read yet
    class TeamPanel *m_teamPanel=nullptr;
    // The team header the user selected in the list; a selected session shows its own team.
    QString m_selectedTeam;
    // A member session's team, task and a link to the Team tab, above Activity.
    class QFrame *m_teamStrip=nullptr;
    class QLabel *m_teamStripText=nullptr;
    void openTeamTask(int task);
    // F7: a session named here is taken into the new team as its existing lead.
    void showNewTeam(const QString &host = {}, const QString &name = {});
    void showTeamDesigner();
    void showTeamWaiting();
    void showModelServers();
    void openWaitingItem(const QString &team,const QString &kind,int phase,int task);
    void startTeamDesigner(const QString &folder,const QString &goal,const QJsonObject &agent);
    QString m_teamDesignerFolder,m_teamDesignerProject;
    QString teamDesignerSession() const;
    class TeamCatalogEditor *m_teamCatalog=nullptr;
    class TeamWorkspace *m_teamWorkspace=nullptr;
    // U25 (spec 3): the header rows that carry a lead session's team.
    QHBoxLayout *m_titleRow=nullptr, *m_chipsRow=nullptr; bool m_teamHeader=false;
    // U30 (#508): the header pill's state, its glyph and the clock that keeps
    // Cooling counting down between polls.
    SessionStates::Shown m_badgeState;
    QLabel *m_badgeGlyph=nullptr;
    QTimer m_stateClock;
    // The Team designer mode on the rail, and New team next to Activity: the
    // selected session leads the team built there; once created, Team follows.
    class TeamDesignerPage *m_teamDesignerPage=nullptr;
    class QFrame *m_branchChip=nullptr;
    class QLabel *m_branchIcon=nullptr,*m_branchText=nullptr;
    class QPushButton *m_teamDesignerNav=nullptr;
    int m_teamDesignerPageIndex=-1;
    // U50: the rail's Waiting for you page, made on first use.
    class TeamWaitingPage *m_teamWaitingPage=nullptr;
    // U60 (#656): the model servers, their tab and their settings section.
    ModelServers::Controller *m_modelServers=nullptr;
    class ModelServersPage *m_modelServersPage=nullptr;
    class ModelServerSettings *m_modelServerSettings=nullptr;
    class QPushButton *m_modelServersNav=nullptr;
    int m_modelServersPageIndex=-1,m_modelServerSettingsSection=-1;
    class QPushButton *m_teamWaitingNav=nullptr;
    int m_teamWaitingPageIndex=-1;
    // U55: the same list as banners above the session's chat and Activity and
    // above the lead's conversation in the Team tab, read while the window shows.
    class TeamWaitingFeed *m_waitingFeed=nullptr;
    class TeamWaitingBanner *m_waitingBanner=nullptr,*m_teamWaitingBanner=nullptr;
    QWidget *m_waitingHost=nullptr;
    QWidget *m_teamNewTab=nullptr;
    class TeamNewTab *m_teamNewForm=nullptr;
    bool m_teamNewWaiting=false;
    QString m_teamOpenAfterCreate;
    void ensureTeamNewForm();
    void setNewTeamTab(bool shown);
    class TeamNewDialog *teamNewForm(QWidget *parent);
    int m_teamSettingsSection=-1;
    void updateTeamPanel();
    void updateProposalCards();
    void showTeam(const QString &team);
    Teams::Live teamLive(const Teams::Member &member) const;
    // The strip a team lead's card carries (SessionRoles::TeamLead).
    QVariantMap teamStrip(const Teams::Team &team) const;
    QSet<QString> m_collapsedTeams;
    class IronRulesPanel *m_ironRules=nullptr;
    class SessionLeadPanel *m_leadPanel=nullptr;
    // WhiteWh fork: the skin's own title bar in place of the system one.
    class WindowTitleBar *m_titleBar=nullptr;
    bool m_framelessMain=false, m_railInTitle=false;
    void applyWindowFrame();
    // Moves the rail's controls into the title bar (and the version into the
    // status bar) or back to the rail.
    void placeRailControls(bool inTitle);
    QWidget *m_sidebar=nullptr; class QVBoxLayout *m_railLayout=nullptr; class QHBoxLayout *m_statusLayout=nullptr; QPushButton *m_versionButton=nullptr;
    void updateLeadPanel();
    void sendReportToLead();
    // WhiteWh fork: the project (organization group) a session belongs to.
    QString projectOfKey(const QString &key) const;
    QString teamProjectRules() const;
    QSet<QString> m_hostFilters, m_projectFilters, m_accountFilters;
    QJsonArray m_sessionAccountProfiles;
    QJsonArray m_sessionAccountDetailsProfiles;
    QString selectedHostTarget() const;
    void updateDashboard();
    // Gives each computer's live event stream the checkout folders of its
    // sessions (selected, then shown rows, then the rest); those need no
    // git-status poll while it follows them.
    void updateGitWatch();
    QJsonObject m_details;
    QHash<QString, QString> m_questionSelections;
    QJsonArray m_events;
    qint64 m_cursor = 0;
    QString m_inspectError, m_connectionError, m_childrenHtml;
    quint64 m_settingsRequest = 0;
    // `account models` requests by the session, computer and folder they were made for,
    // and the newest one issued per session; favorite changes by their session. Both keep
    // the composer's picker context (openCodeGeneration) they were made in.
    struct OpenCodeCatalogRequest { QString key, host, cwd; quint64 context = 0; };
    QHash<quint64, OpenCodeCatalogRequest> m_openCodeCatalogRequests;
    QHash<QString, quint64> m_openCodeCatalogLatest;
    struct OpenCodeFavoriteRequest { QString key, host, value; bool favorite = false; quint64 context = 0; };
    QHash<quint64, OpenCodeFavoriteRequest> m_openCodeFavoriteRequests;
    QString m_settingsKey, m_settingsRun, m_settingsConversation;
    QString m_settingsHost, m_settingsName;
    bool m_settingsAutomatic = false;
    QSet<QByteArray> m_settingsAttempts;
    struct PendingSettings { QString host, name, run, conversation, model, effort, id; };
    QHash<QString, PendingSettings> m_queuedSettings;
    static QByteArray settingsIdentity(const QString &key, const PendingSettings &pending);
    const SessionInfo *listedSettingsSession(const PendingSettings &pending) const;
    bool settingsAttemptAllowed(const QString &key, const PendingSettings &pending, const QByteArray &identity) const;
    void requestAutomaticSettings(const QString &key, const PendingSettings &pending, const QByteArray &identity);
    // An automatic apply that dispatch scheduled instead, as when a chooser
    // appeared after the idle listing, by session key. It runs once more after
    // the listing or inspect shows the session leave Ready (`rearmed`).
    struct ScheduledSettings { QByteArray identity; bool rearmed = false; };
    QHash<QString, ScheduledSettings> m_scheduledSettings;
    // The selected session's inspection was already running when a settings
    // receipt arrived: its reply shows the settings from before the change.
    QSet<QString> m_inspectionBehindSettings;
    QByteArray m_settingsIdentity; // The automatic apply in flight.
    bool m_pending = false, m_clipboard = false, m_rebuilding = false;
    // N7: the Team tab's leftover Stop names a session; its entry key waits
    // here until the inspection that decides the real action has loaded.
    QString m_terminateAfterInspect;
    // Polls keep hidden pages' data current; these pages render when shown.
    bool m_sessionsStale = false, m_dashboardStale = false;
    // What the session rows were rendered with: peer reachability by poll age
    // (FleetState::freshness), and when a row's text next changes with the clock.
    QByteArray m_renderedFreshness;
    qint64 m_rowsChangeAt = 0;
    QSet<QString> m_retryReads;
    QHash<QString, quint64> m_waitingPreparations;
    quint64 m_waitingPreparationSerial = 0;
    bool m_draftRefreshPending = false;
    int m_tick = 0, m_activityPollMs = 2500, m_noticeMs = 5000;
    QTimer m_timer;
    QTimer m_sessionLoadingTimer;
    // WhiteWh fork: a read of the selected session is pending.
    bool m_selectedLoading = false;
    struct SessionRead { QString host, name, archive, agent; QElapsedTimer age; };
    QHash<QString, SessionRead> m_sessionReads;
    QHash<QString, QElapsedTimer> m_remotePollAges;
    BusyIndicator *m_sessionLoading = nullptr;
    QElapsedTimer m_processPollAge,m_inspectionAge;
    // Runs whose event-stream subscription ended; they are polled (EventStream.h).
    QSet<QString> m_streamEnded;
    SessionList *m_sessions;
    SessionSearch *m_searchResults;
    QStackedWidget *m_listStack = nullptr;
    MachinesPage *m_machinesPage;
    MachineFilter *m_machineFilter;
    ProjectFilter *m_projectFilter;
    AccountFilter *m_accountFilter;
    DashboardPage *m_dashboard;
    AccountsPage *m_accountsPage;
    SessionOrganization m_organization;
    SessionRowPreferences m_sessionRowPreferences;
    SwarmController *m_swarm = nullptr;
    QByteArray m_savedOrganization;
    // SessionOrganization::revision() of the organization last saved and sent
    // to the swarm; 0 when the next save must serialize it.
    quint64 m_savedOrganizationRevision = 0;
    QPushButton *m_sessionsNav, *m_machinesNav, *m_accountsNav;
    SwarmButton *m_brand = nullptr;
    QString m_fullTitle, m_fullMeta;
    QList<QPushButton *> m_filters;
    QLineEdit *m_search;
    QLabel *m_heading, *m_count, *m_title, *m_meta, *m_badge, *m_model, *m_hint, *m_notice;
    QString m_modelElision;
    QLabel *m_connectionStatus;
    QLabel *m_sessionFeedback = nullptr;
    QPushButton *m_noticeDismiss;
    QPushButton *m_connectionRetry;
    QSet<QString> m_pollingHosts;
    void updateConnectionStatus();
    QPushButton *m_goal;
    ToolbarChip *m_markRead;
    class SessionFileDrop *m_fileDrop;
    QString terminalDropTarget() const;
    void dropTerminalFiles(const QStringList &paths);
    void pasteTerminalPaths(const QStringList &paths);
    quint64 m_terminalDropRequest = 0;
    QString m_terminalDropKey;
    // The machine of an untracked session's transfer, empty for a tracked one.
    QString m_terminalDropUntracked;
    int m_terminalDropCount = 0;
    SessionUsage::ContextButton *m_contextUsage, *m_subagentContextUsage;
    AccountUsage::Button *m_accountUsage;
    QWidget *m_usageStrip;
    AccountUsage::RefreshButton *m_usageRefresh;
    class AccountUsageStore *m_usageStore;
    QJsonObject m_accountUsageData;
    QLabel *m_usageWarning;
    ToolbarChip *m_usageLimit;
    QLabel *m_cacheWarning;
    QLabel *m_cacheDetail;
    QPushButton *m_cacheClear;
    CacheStatus::Chip *m_cacheStatus;
    void refreshAccountUsage(bool force = false);
    void renderAccountUsage();
    void updateDashboardAccounts(bool request = false, bool force = false);
    QPushButton *m_shell, *m_fileManager, *m_copyHeaderId, *m_clear, *m_restore, *m_more;
    QAction *m_renameAction, *m_forkAction, *m_openAction, *m_changeAction, *m_freshAction, *m_archiveAction, *m_forgetAction;
    QAction *m_markAllReadAction = nullptr;
    QAction *m_expandAllRowsAction = nullptr;
    QMenu *m_sessionMenu;
    QTextBrowser *m_children = nullptr, *m_info = nullptr;
    ActivityView *m_activityView = nullptr;
    class ProcessView *m_processes = nullptr;
    class TasksView *m_tasks = nullptr;
    QWidget *m_taskView = nullptr;
    class TaskInspector *m_taskInspector = nullptr;
    QWidget *m_nativeSignIn = nullptr;
    // A stopped OpenCode service: started only by this explicit action.
    QWidget *m_openCodeService = nullptr;
    QLabel *m_openCodeServiceText = nullptr;
    QPushButton *m_openCodeServiceStart = nullptr;
    quint64 m_openCodeStartRequest = 0;
    QString m_openCodeStartMachine, m_openCodeStartHost;
    void startOpenCodeService();
    ActivityView *m_subagentView = nullptr;
    QStackedWidget *m_activityStack = nullptr;
    QLabel *m_subagentTitle = nullptr, *m_subagentHint = nullptr;
    QString m_subagentId, m_subagentConversation, m_subagentCwd;
    QSet<QString> m_expandedSessions;
    QJsonObject m_subagentDetails;
    MessageComposer *m_composer = nullptr, *m_subagentComposer = nullptr;
    class WorkingLine *m_workingLine = nullptr;   // WhiteWh fork
    // U35: the messages hgs holds for the selected session, with Cancel; the
    // ones just queued per session until a read lists them; cancels in flight.
    class HeldInputBar *m_heldBar = nullptr;
    class TerminalDraftBar *m_terminalDraft = nullptr;
    QHash<QString, QJsonArray> m_justHeld;
    // #511 N5: the held messages each session's composer queued, until hgs settles them.
    QHash<QString, HeldSent> m_sentHeld;
    QHash<quint64, QPair<QString, QString>> m_heldCancels;
    void renderHeld();
    void openSessionTerminal(const QString &host, const QString &name);
    QHash<quint64, QString> m_childMessages;
    QuestionCard *m_question = nullptr;
    class TeamProposalCards *m_proposalCards = nullptr;
    QList<ActivityWidth::ColumnLayout *> m_activityColumns;   // content sharing Activity's reading column
    RecoveryUi::Panel *m_recovery = nullptr;
    ToolbarChip *m_recoveryChip = nullptr;
    quint64 m_recoveryAction = 0;
    QString m_recoveryActionKey;
    TerminalView *m_terminal = nullptr;
    NativeUiView *m_nativeUi = nullptr;
    QTabWidget *m_detailTabs = nullptr;
    // U25 part 6 (spec 6): the inspector's column of cards.
    class InspectorCards *m_inspector = nullptr;
    QMap<QString, class InspectorValue *> m_environment;
    QLabel *m_contextUsed = nullptr, *m_contextTokens = nullptr, *m_contextLeft = nullptr;
    class ContextBar *m_contextBar = nullptr;
    QWidget *m_contextRows = nullptr, *m_changesList = nullptr;
    QLabel *m_changesEmpty = nullptr;
    QPushButton *m_commitChanges = nullptr, *m_openDiff = nullptr;
    quint64 m_diffRequest = 0;
    void openInspectorCard(int card);
    void updateEnvironmentCard();
    void updateContextCard();
    void updateChangesCard();
    void showDiff(const QJsonObject &data, const QString &error);
    QLabel *m_detailsContext = nullptr;
    QLabel *m_detailsIdTitle = nullptr, *m_detailsIdValue = nullptr;
    QPushButton *m_copySessionId = nullptr;
    QPushButton *m_detailsClear = nullptr;
    // WhiteWh fork: Keep cache warm and its limit in hours (CacheKeepalive).
    class QCheckBox *m_keepWarm = nullptr;
    class QSlider *m_keepWarmHours = nullptr;
    QLabel *m_keepWarmLimit = nullptr;
    void setKeepWarm(int hours);
    QList<TeamCatalogEditor::Computer> teamComputers() const;
    QFrame *m_inspectorPanel = nullptr;
    QSplitter *m_workSplitter = nullptr;
    QPushButton *m_inspectorToggle = nullptr;
    QWidget *m_inspectorCorner = nullptr;
    static constexpr int DefaultInspectorWidth = 400;
    int m_inspectorWidth = DefaultInspectorWidth;
    bool m_inspectorWidthPending = false;
    struct PendingMessage { QString key, runId, conversationId; };
    QHash<quint64, PendingMessage> m_pendingMessages;
    QHash<QString, QJsonArray> m_localMessages;
    QHash<QString, QJsonObject> m_pendingSendEvidence;
    QHash<QString, int> m_pendingEvidenceUsers;
    void beginSendEvidence(const QString &key, const QJsonObject &data);
    void finishSendEvidence(const QString &key);
    struct PendingAnswer { QString key, questionId, questionHash; };
    QHash<quint64, PendingAnswer> m_pendingAnswers;
    // U25: unmuting a member muted by its team writes mute, then unmute: the
    // host records only a change, and revision 0 means the session never chose.
    struct PendingNotifications { Entry entry; bool thenUnmute = false; };
    QSet<QString> m_teamMembers;
    QHash<quint64, PendingNotifications> m_pendingNotifications;
    bool m_confirmingQuestionCache = false;
    QStackedWidget *m_detailStack = nullptr;
    QSplitter *m_splitter;
    // Null until the pages exist: restoring a collapsed list runs earlier.
    QStackedWidget *m_pages = nullptr;
    SettingsPage *m_settingsPage=nullptr;
    QPushButton *m_settingsNav;
    QPushButton *m_windowPin = nullptr;
    ProjectsDialog *m_projectsPage;
    QPushButton *m_projectsNav;
    bool m_dark = false;
    // U53: the one account switch in flight: its request and session.
    quint64 m_accountSwitchRequest = 0;
    QString m_accountSwitchKey;
    QFileSystemWatcher *m_skinWatcher = nullptr;
    QString m_fg, m_muted, m_surface, m_border, m_accent;
};
