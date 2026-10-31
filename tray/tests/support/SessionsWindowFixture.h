#pragma once
// U62 (#712): what every test_sessionswindow_* suite shares: the includes,
// helpers and the fixture with its fake hgs. Each area suite derives its
// TestSessionsWindow from SessionsWindowFixture and keeps its tests' bodies.
#include "Wait.h"
#include "Settings.h"
#include <QtTest>
#include <QAction>
#include <QApplication>
#include <QFrame>
#include <QToolButton>
#include <QAbstractButton>
#include <tuple>
#include <QComboBox>
#include <QCheckBox>
#include <QDateTime>
#include <QContextMenuEvent>
#include <QDir>
#include <QClipboard>
#include <QDesktopServices>
#include <QDragEnterEvent>
#include <QDropEvent>
#include <QMimeData>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFile>
#include <QFileDialog>
#include <QProgressBar>
#include <QSaveFile>
#include <QSemaphore>
#include <atomic>
#include <QJsonDocument>
#include <QLineEdit>
#include <QLabel>
#include <QMessageBox>
#include <QMenu>
#include <QStackedWidget>
#include <QSplitter>
#include <QSplitterHandle>
#include <QMouseEvent>
#include <QScrollBar>
#include <QScrollArea>
#include <QListWidget>
#include <QRadioButton>
#include "WorkingLine.h"
#include "SessionCardDelegate.h"
#include "JsonSettings.h"
#include "SessionLeadPanel.h"
#include "GoToDialog.h"
#include "ShortcutsDialog.h"
#include <QPushButton>
#include <QPlainTextEdit>
#include <QSettings>
#include <QScopeGuard>
#include <QStandardItemModel>
#include <QUuid>
#include <algorithm>
#include <memory>
#include <QTemporaryDir>
#include <QLockFile>
#include <QProcess>
#include <QProcessEnvironment>
#include <QTextBrowser>
#include <QTextDocument>
#include <QTextTable>
#include <QTabWidget>
#include <QTabBar>
#include <QSlider>
#include <QFontInfo>
#include <QVBoxLayout>
#include <QProxyStyle>
#include <QStyleFactory>
#include <QTableWidget>
#include <QWindow>
#include <utility>
#include "SessionsWindow.h"
#include "CacheKeepalive.h"
#include "Extensions.h"
#include "InspectorCards.h"
#include "JsonParse.h"
#include "OffGuiThread.h"
#include "QuestionReply.h"
#include "EventStream.h"
#include "DownloadCache.h"
#include "ProcessView.h"
#include "TasksView.h"
#include "TaskInspector.h"
#include "ActivityView.h"
#include "AttachmentViewer.h"
#include "ExternalFile.h"
#include "QuestionCard.h"
#include "RecoveryWidgets.h"
#include "SettingsPage.h"
#include "WindowLayer.h"
#include "SwarmDialog.h"
#include "SwarmController.h"
#include "TerminalView.h"
#include "TerminalScreen.h"
#include "NewSessionDialog.h"
#include "DirectoryDialog.h"
#include "ProjectsDialog.h"
#include "MessageComposer.h"
#include "AccountSwitch.h"
#include "OpenCodeModelPicker.h"
#include "OpenCodeModels.h"
#include "NativeBrowser.h"
#include "NativeBrowserPlugin.h"
#include "MessageBoxIcons.h"
#include "ComposerToolbar.h"
#include "SessionList.h"
#include "SessionStates.h"
#include "TeamController.h"
#include <QTextDocumentFragment>
#include <QToolTip>
#include "TeamPanel.h"    // N7: the leftover Stop lives in the Team tab
#include "TeamNewTab.h"   // WhiteWh fork: agent teams
#include "TeamWidgets.h"
#include "TeamConversation.h"   // U55: the banner above the lead's conversation
#include "TeamWaitingBanner.h"
#include <QToolBar>
#include <QTimer>
#include <QFileInfo>
#include <functional>
#include "SessionLeads.h"
#include "SessionOrganization.h"
#include "MachineFilter.h"
#include "ProjectFilter.h"
#include "AccountFilter.h"
#include "SessionSearch.h"
#include "DashboardPage.h"
#include "AccountUsage.h"
#include "WorktreePanel.h"
#include "AccountUsageStore.h"
#include "MachinesPage.h"
#include "WorkspaceColorDialog.h"
#include "PhonePairing.h"
#include "PageCards.h"
#include <QTreeWidget>
#include "PopupMouse.h"
#include "AccountsPage.h"
#include "FakeCli.h"
#include "FakeTasksCli.h"
#include "TaskDrag.h"
#include "LaunchWarnings.h"
#include "LocalSessionHost.h"
#include "ProcessRunner.h"
#include "SettingsJson.h"
#include "SessionCreationController.h"
#include "ProcessSettings.h"
#include "QuestionNotificationSettings.h"
#include "Shortcuts.h"
#include "ReadmeShots.h"

// U25 part 6: the inspector is a column of cards; a card by its key and
// whether it is unfolded.
inline QFrame *inspectorCard(QWidget &root, const char *key)
{
    for (auto *card : root.findChildren<QFrame *>("inspectorCard"))
        if (card->property("card").toString() == QLatin1String(key)) return card;
    return nullptr;
}
inline bool inspectorCardOpen(QWidget &root, const char *key)
{
    auto *card = inspectorCard(root, key);
    auto *header = card ? card->findChild<QPushButton *>("collapsibleHeader") : nullptr;
    return header && header->property("expanded").toBool();
}
inline void openInspectorCard(QWidget &root, const char *key)
{
    if (inspectorCardOpen(root, key)) return;
    if (auto *card = inspectorCard(root, key)) card->findChild<QPushButton *>("collapsibleHeader")->click();
}

class FolderUrlRecorder : public QObject {
    Q_OBJECT
public:
    // Files that run programs are revealed instead; no test starts a file manager.
    FolderUrlRecorder() {
        QDesktopServices::setUrlHandler("file",this,"record");
        ExternalFile::setRevealHandler([this](const QString &path) { revealed<<path; return true; });
    }
    ~FolderUrlRecorder() override { QDesktopServices::unsetUrlHandler("file"); ExternalFile::setRevealHandler({}); }
    QList<QUrl> urls;
    QStringList revealed;
public slots:
    void record(const QUrl &url) { urls<<url; }
};

// Calls sample right after each step of the session panel animation, before
// layout requests deferred during that step run: what a repaint then would show.
class ExpansionSteps : public QObject {
public:
    explicit ExpansionSteps(std::function<void()> sample) : m_sample(std::move(sample)) {}
protected:
    bool eventFilter(QObject *, QEvent *event) override {
        if (event->type() == QEvent::DynamicPropertyChange && static_cast<QDynamicPropertyChangeEvent *>(event)->propertyName() == "expansion")
            QMetaObject::invokeMethod(this, m_sample, Qt::QueuedConnection);
        return false;
    }
private:
    std::function<void()> m_sample;
};
// Question prompts as shown: the agent's Markdown, or plain text for terminal panels.
inline QStringList questionPrompts(const QuestionCard *card)
{
    QStringList prompts;
    for (const auto *prompt : card->findChildren<QWidget *>("questionPrompt")) {
        if (const auto *markdown = qobject_cast<const QTextBrowser *>(prompt)) prompts << markdown->toPlainText();
        else if (const auto *label = qobject_cast<const QLabel *>(prompt)) prompts << label->text();
    }
    return prompts;
}

// Presentation and persistence now complete asynchronously. Wait only for the
// exact selected draft before tests enter new text or inspect restored text.
inline void setComposerText(MessageComposer *composer, const QString &text)
{
    WAIT_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    composer->editor()->setPlainText(text);
}

// Wait for a read accepted through the window's ordinary Refresh path. Earlier
// fake-CLI replies cannot then overwrite the next manually injected transition;
// the window's polling age is refreshed too, as it is for a real user refresh.
inline void settleSelectedInspection(MessageComposer *composer, HgsClient *client,
                                    const QString &host, const QString &name, const QJsonObject &details)
{
    QSignalSpy loading(client, &HgsClient::inspectionLoadingChanged);
    QSignalSpy ready(client, &HgsClient::inspectionReady);
    qsizetype accepted = -1;
    WAIT_VERIFY(([&] {
        if (accepted >= 0) return true;
        const auto before = loading.size();
        composer->sessionRefreshRequested(composer->sessionKey());
        for (qsizetype i = before; i < loading.size(); ++i) {
            const auto &row = loading[i];
            if (row[0].toString() == host && row[1].toString() == name && row[2].toString().isEmpty()
                && row[3].toString().isEmpty() && row[4].toBool()) {
                accepted = ready.size(); return true;
            }
        }
        return false;
    })());
    WAIT_VERIFY(([&] {
        for (qsizetype i = accepted; i < ready.size(); ++i) {
            const auto &row = ready[i];
            if (row[0].toString() == host && row[1].toString() == name && row[3].toString().isEmpty()
                && row[2].toJsonObject() == details) return true;
        }
        return false;
    })());
}

// The window also polls the selected session. A fixture inspection replies with
// what the fixture said when it started, so one still running when a test injects
// details lands afterwards and replaces them. On Windows it can even arrive before a
// confirmation accepted from a short timer returns: Qt posts such timer events, and
// they share a batch with queued process replies (zerus-a0ak, zerus-a857). Call
// this before show(): the window then inspects only when something asks it to, such
// as show, selection, Refresh, a tab change or a changed conversation in a reply.
// HgsClient runs one inspection per session, so once the reply to the test's own
// inspection is in (settleSelectedInspection), none is pending and no poll follows.
inline void keepDetailsPollsOut(SessionsWindow &window)
{
    window.setDetailsPollForTests(500, 24 * 60 * 60 * 1000);
}

namespace {
// QMacStyle's layout rect for a push button starts 4 px lower and ends 8 px higher.
class NativeLayoutMargins : public QProxyStyle {
public:
    using QProxyStyle::QProxyStyle;
    QRect subElementRect(SubElement element, const QStyleOption *option, const QWidget *widget) const override
    {
        return element == SE_PushButtonLayoutItem ? option->rect.adjusted(0, 4, 0, -8)
                                                  : QProxyStyle::subElementRect(element, option, widget);
    }
};
}
namespace {
// Stands in for the optional embedded native UI plugin (NativeBrowserPlugin.h).
struct FakeNativeBrowsers final : NativeBrowserFactory {
    struct Browser final : NativeBrowser {
        QWidget *widget() override { return &view; }
        void load(const QUrl &) override {}
        QWidget view;
    };
    NativeBrowser *create(const QString &, const QColor &, QWidget *) override { ++created; return new Browser; }
    int created = 0;
};
}
namespace {
// A Windows control console whose CLI reports a session host, alone: Machines
// offers Enable local sessions for it (spec D16).
inline FleetState enablableConsole()
{
    BoxState local;local.host="windows-pc";local.ok=true;local.localSessions=false;local.metrics={{"platform","windows"}};
    local.localHost={{"state","not-installed"},{"pid",QJsonValue::Null},{"protocol",QJsonValue::Null},{"conpty",QJsonValue::Null},
        {"integrity",QJsonValue::Null},{"windows_session",QJsonValue::Null}};
    FleetState fleet;fleet.setLocal(local,QDateTime::currentMSecsSinceEpoch());return fleet;
}
}
namespace {
// Presses a chord on the platform input path: the window's QWindow first, then
// its focus widget and that widget's ancestors, as with a real keyboard.
inline void pressChord(QWidget *window, Qt::Key key, Qt::KeyboardModifiers modifiers = Qt::NoModifier)
{
    QTest::keyClick(window->windowHandle(), key, modifiers);
}
// The physical Control key: Qt reports it as Meta on macOS, where Ctrl is Command.
inline Qt::KeyboardModifier physicalControl()
{
#ifdef Q_OS_MACOS
    return Qt::MetaModifier;
#else
    return Qt::ControlModifier;
#endif
}
// Presses a command's current chord (index: which one) key by key, sent to the
// window or, with a widget, to that widget. Shift+Tab arrives as Backtab, as
// from a real keyboard.
inline void pressSequence(QWidget *target, const QKeySequence &sequence, bool window = true)
{
    for (int i = 0; i < sequence.count(); ++i) {
        const auto modifiers = sequence[i].keyboardModifiers(); auto key = sequence[i].key();
        if (key == Qt::Key_Tab && (modifiers & Qt::ShiftModifier)) key = Qt::Key_Backtab;
        if (window) pressChord(target, key, modifiers); else QTest::keyClick(target, key, modifiers);
    }
}
inline void pressCommand(QWidget *window, const QString &id, int index = 0)
{
    const auto sequence = ShortcutRegistry::instance().keys(id).value(index);
    QVERIFY2(!sequence.isEmpty(), qPrintable(id));
    pressSequence(window, sequence);
}
inline QStackedWidget *workspacePages(QWidget &window)
{
    auto *dashboard = window.findChild<DashboardPage *>();
    return dashboard ? qobject_cast<QStackedWidget *>(dashboard->parentWidget()) : nullptr;
}
inline QPushButton *railButton(QWidget &window, const QString &glyph)
{
    for (auto *button : window.findChildren<QPushButton *>("railButton")) if (button->property("glyph") == glyph) return button;
    return nullptr;
}
inline QString currentSessionKey(const QListWidget *list)
{
    return list->currentItem() ? list->currentItem()->data(SessionRoles::Key).toString() : QString();
}
inline bool focusedIn(const QWidget *area)
{
    const auto *focus = QApplication::focusWidget();
    return focus && (focus == area || area->isAncestorOf(focus));
}

// A copy of the base fixture for Send and push now: inspect answers from
// <script>.details, send answers each message as the mode says, resume starts
// nothing, and send-now records each queue id in <script>.send-now.
struct PushFixture {
    enum class Send { Confirm, Slow, Refuse, Uncertain };
    QTemporaryDir dir; QString script = dir.filePath("hgs");
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
        {"process_state", "running"}, {"activity", "busy"}, {"phase", "working"}, {"events", QJsonArray{}}, {"cursor", 0}};
    // Set before install(): send also appends its synthetic phases, never the message, to <script>.send-trace.
    bool traceSend = false;
    // gateSendNow: send-now waits until release() before it answers.
    bool install(const QString &base, Send mode = Send::Confirm, bool gateSendNow = false)
    {
        QFile original(base); if (!original.open(QIODevice::ReadOnly)) return false;
        const QByteArray traceEntry = traceSend ? QByteArray(" import os,time\n def trace(*v):\n  try:\n"
            "   with open(__file__+'.send-trace','a') as f:f.write(' '.join(str(x)[:80] for x in v)+'\\n')\n  except OSError:pass\n"
            " trace('send-entry','pid='+str(os.getpid()),'ms='+str(int(time.time()*1000)))\n") : QByteArray();
        const QByteArray traceRequest = traceSend ? QByteArray(" trace('send-request','request='+str(payload.get('request_id')),"
            "'run='+str(payload.get('expected_run_id')),'conversation='+str(payload.get('expected_conversation_id')),"
            "'ms='+str(int(time.time()*1000)))\n") : QByteArray();
        const QByteArray traceResponse = traceSend ? QByteArray(" trace('send-response','printed','ms='+str(int(time.time()*1000)))\n") : QByteArray();
        const QByteArray confirm = " print(json.dumps({'status':'submitted','request_id':payload['request_id'],'name':a[1],'run_id':payload['expected_run_id'],"
            "'conversation_id':payload['expected_conversation_id'],'first_message':not payload['expected_conversation_id']}))\n";
        // Every branch is a QByteArray: MSVC rejects a conditional that mixes literals with one.
        const QByteArray send = mode == Send::Refuse ? QByteArray(" print('refused by the fixture',file=sys.stderr);sys.exit(1)\n")
            : mode == Send::Uncertain ? QByteArray(" print('delivery uncertain: the fixture lost the reply',file=sys.stderr);sys.exit(1)\n")
            : mode == Send::Slow ? QByteArray(" __import__('time').sleep(3)\n") + confirm : confirm;
        return FakeCli::install(script, original.readAll().replace("if False:pass\n", "if False:pass\n"
            "elif c=='inspect':print(open(__file__+'.details').read())\n"
            "elif c=='resume':pass\n"
            "elif c=='send':\n" + traceEntry + " payload=json.loads(sys.stdin.read())\n" + traceRequest + send + traceResponse +
            "elif c=='send-now':\n payload=json.loads(sys.stdin.read())\n"
            " open(__file__+'.send-now','a').write(payload['queue_id']+'\\n')\n"
            + QByteArray(gateSendNow ? " t=__import__('time').time()\n"
                " while not __import__('os').path.exists(__file__+'.release') and __import__('time').time()-t<30:__import__('time').sleep(0.05)\n" : "") +
            " print(json.dumps({'status':'submitted','request_id':payload['request_id'],'name':a[1],'run_id':payload['expected_run_id'],"
            "'conversation_id':payload['expected_conversation_id'],'queue_id':payload['queue_id']}))\n")) && save();
    }
    bool release() const { QFile file(script + ".release"); return file.open(QIODevice::WriteOnly); }
    void queue(const QString &id, const QString &text, bool canSendNow = true)
    {
        details["input_queue"] = QJsonObject{{"id", id}, {"text", text}, {"can_send_now", canSendNow}};
    }
    bool save() const { return FakeCli::replace(script + ".details", QJsonDocument(details).toJson(QJsonDocument::Compact)); }
    QStringList sentNow() const
    {
        QFile log(script + ".send-now");
        return log.open(QIODevice::ReadOnly) ? QString::fromUtf8(log.readAll()).split('\n', Qt::SkipEmptyParts) : QStringList();
    }
};

// Opens the dashboard session of a push fixture with its details settled.
inline void openPushSession(SessionsWindow &window, const FleetState &state, const PushFixture &fixture)
{
    keepDetailsPollsOut(window); window.setFleet(state); window.show(); window.activateWindow();
    QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    settleSelectedInspection(window.findChild<MessageComposer *>("messageComposer"), window.findChild<HgsClient *>(), {}, "codex/hgs/dashboard", fixture.details);
}

// Saves the fixture's details and waits until the window has read them.
inline void inspectPush(SessionsWindow &window, const PushFixture &fixture)
{
    QVERIFY(fixture.save());
    settleSelectedInspection(window.findChild<MessageComposer *>("messageComposer"), window.findChild<HgsClient *>(), {}, "codex/hgs/dashboard", fixture.details);
}

// Presses Send and push now in the message field.
inline void pressPush(SessionsWindow &window)
{
    auto *editor = window.findChild<MessageComposer *>("messageComposer")->editor();
    editor->setFocus(); QTRY_VERIFY(editor->hasFocus());
    const auto push = ShortcutRegistry::instance().keys("message.push").value(0); QCOMPARE(push.count(), 2);
    for (int i = 0; i < push.count(); ++i) pressChord(&window, push[i].key(), push[i].keyboardModifiers());
}

// Types text into the message field and presses Send and push now.
inline void pushMessage(SessionsWindow &window, const QString &text)
{
    setComposerText(window.findChild<MessageComposer *>("messageComposer"), text);
    pressPush(window);
}

inline bool pushes(const MessageComposer *composer)
{
    for (const auto *action : composer->editor()->actions()) if (action->objectName() == "shortcut:message.push") return true;
    return false;
}

inline QString notice(SessionsWindow &window) { return window.findChild<QLabel *>("notice")->text(); }
inline bool noticeIsError(SessionsWindow &window) { return window.findChild<QLabel *>("notice")->property("error").toBool(); }
}
// Push tests wait for deliveries through the fake CLI, which is slow on Windows runners.
constexpr int PushTimeout = 20000;
// Sessions rows prepared off the GUI thread (SessionsWindow::prepareRows).
namespace {
using RowPreparation = SessionsWindow::RowPreparation;
// The application default, for one test.
struct AsyncRows {
    AsyncRows() { SessionsWindow::setRowPreparationForTests(RowPreparation::Async); }
    ~AsyncRows() { SessionsWindow::setRowPreparationForTests(RowPreparation::Blocking); }
};
inline void settleRows(SessionsWindow &window)
{
    QTRY_VERIFY_WITH_TIMEOUT(!window.rowsPendingForTests(), 20000);
}
inline SessionsWindow::PreparedRows prepared(const SessionsWindow::RowInputs &inputs)
{
    return offGuiThread([&inputs] { return SessionsWindow::prepareRows(inputs); });
}
inline QStringList preparedKeys(const SessionsWindow::PreparedRows &rows)
{
    QStringList keys;
    for (const auto &row : rows.rows) keys << row.key;
    return keys;
}
inline const SessionsWindow::PreparedRow *preparedRow(const SessionsWindow::PreparedRows &rows, const QString &key)
{
    for (const auto &row : rows.rows) if (row.key == key) return &row;
    return nullptr;
}
// Every row role an item holds: what a prepared row lists.
inline QMap<int, QVariant> itemRoles(const QListWidgetItem *item)
{
    QMap<int, QVariant> roles;
    QList<int> candidates{Qt::ToolTipRole, Qt::AccessibleTextRole, Qt::AccessibleDescriptionRole};
    for (int role = Qt::UserRole; role < SessionRoles::Digest; ++role) candidates << role;
    for (int role : candidates) if (item->data(role).isValid()) roles.insert(role, item->data(role));
    return roles;
}
inline SessionInfo rowSession(const QString &name, const QString &cmd)
{
    SessionInfo s; s.name = name; s.cmd = cmd; s.project = name.section('/', 1, 1); s.tag = name.section('/', 2);
    s.tracked = true; s.resumable = true; s.state = "running"; s.activity = "idle"; s.phase = "idle"; s.processState = "running";
    return s;
}
// This computer with working, unread, stopped, paused, scheduled and subagent
// sessions; a reachable peer asking for approval; and a peer last polled long ago.
inline FleetState rowFleet(qint64 now)
{
    auto working = rowSession("codex/hgs/dashboard", "codex"); working.activity = "busy"; working.phase = "tool";
    working.currentTool = "Bash"; working.toolDetail = "ctest"; working.turnStarted = 1700000000;
    auto unread = rowSession("kimi/docs/research", "kimi"); unread.unreadReply = true; unread.replyId = "reply-1"; unread.conversationId = "conv-kimi";
    auto stopped = rowSession("codex/hgs/stopped", "codex"); stopped.state = "stopped";
    auto paused = rowSession("codex/website/navigation", "codex"); paused.state = "paused";
    auto subs = rowSession("claude/hgs/subs", "claude"); subs.activity = "busy"; subs.phase = "thinking"; subs.conversationId = "conv-subs";
    subs.subagents = QJsonObject{{"a", QJsonObject{{"name", "Tests"}, {"state", "working"}}},
                                 {"b", QJsonObject{{"name", "Docs"}, {"state", "finished"}}},
                                 {"c", QJsonObject{{"name", "Deploy"}, {"state", "working"}, {"display_state", "approval"}}}};
    subs.subagentActiveCount = 2;
    auto scheduledRow = rowSession("codex/hgs/scheduled", "codex"); scheduledRow.scheduledJobs = 2;
    BoxState arch; arch.host = "arch"; arch.ok = true; arch.metrics = QJsonObject{{"platform", "linux"}};
    arch.sessions = {working, unread, stopped, paused, subs, scheduledRow};
    auto approval = rowSession("claude/infra/review", "claude"); approval.activity = "busy"; approval.phase = "approval";
    BoxState mac; mac.host = "mac"; mac.ok = true; mac.sessions = {approval};
    auto stale = rowSession("codex/old/stale", "codex"); stale.activity = "busy"; stale.phase = "tool";
    BoxState old; old.host = "old"; old.ok = true; old.sessions = {stale};
    FleetState state; state.setLocal(arch, now); state.setPeer(mac, now); state.setPeer(old, now - FleetState::kPeerStaleMs - 60000);
    return state;
}
// Two projects: the paused session and the peer's approval are in "Second".
inline QString storeRowProjects()
{
    SessionOrganization projects;
    const auto second = projects.createGroup("Second", "6f1d2c3b-0000-4000-8000-000000000002");
    projects.moveSession(projects.observe("arch", "codex/website/navigation", {}), second);
    projects.moveSession(projects.observe("mac", "claude/infra/review", {}), second);
    for (const auto *machine : {"arch", "mac", "old"}) projects.markImported(machine);
    setJsonSetting("workspace/organization", QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    return second;
}
}

class SessionsWindowFixture : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    // Each test starts from the settings initTestCase left (none): in
    // one suite a test could lean on what an earlier one left behind.
    void init();
protected:
    FleetState fleet() const;
    // Two running OpenCode 2 sessions, each with its own run and conversation,
    // and a stopped Codex session, for Forget of a multi-selection.
    FleetState openCodeSelectionFleet() const;
    // A copy of the base fixture that records each session-action and kill
    // call. A <fixture>.outcome file "<name>\n<status>" refuses that session.
    QString openCodeSelectionFixture(const QString &name) const;
    // The base fixture's script; tests start program(), see FakeCli.h.
    QString script() const { return m_dir->filePath("hgs"); }
    QString program() const { return FakeCli::program(script()); }
    // A copy of the base fixture whose inspect answers with these details, so
    // polls that arrive while a test waits cannot replace them.
    QString programInspecting(const QString &name, const QJsonObject &details, const QJsonObject &child = {}, const QJsonObject &usage = {}) const;
    // A copy of the base fixture, named `name`, whose `hgs task …` calls answer with
    // this listing of project "Work"; each call's argv is appended to <fixture>.tasks.
    QString tasksFixture(const QString &name, const QJsonArray &tasks) const;
    // Created by initTestCase(): listing the data rows (-datatags, as
    // RunQtShard.cmake does) exits without destroying the test object. That
    // listing calls every *_data() function first, so none may use m_dir.
    std::unique_ptr<QTemporaryDir> m_dir;
    std::unique_ptr<TestWait::ModalGuard> m_modals;
};
