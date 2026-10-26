#include "FakeTasksCli.h"
#include "ActivityView.h"
#include "TaskDialogs.h"
#include <QSplitter>
#include <QScrollBar>
#include <QSpinBox>
#include "HgsClient.h"
#include "MessageComposer.h"
#include "ComposerDraftStore.h"
#include "AttachmentViewer.h"
#include <QBuffer>
#include "TaskBoard.h"
#include "SessionStatusBadge.h"
#include "TasksView.h"
#include "TaskInspector.h"
#include "TaskDialogs.h"
#include <QComboBox>
#include <QElapsedTimer>
#include <QLineEdit>
#include <QPlainTextEdit>
#include "TaskRow.h"
#include <QCheckBox>
#include <QScrollArea>
#include <QVBoxLayout>
#include <QTextBrowser>
#include <QTabBar>
#include <QMenu>
#include <QAbstractButton>
#include <QDateTime>
#include <QLabel>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QToolTip>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTest>
#include <QToolButton>

namespace {
const qint64 Hour = 60LL * 60 * 1000;
qint64 now() { return QDateTime::currentMSecsSinceEpoch(); }
QList<TaskSession> sessions() {
    return {{"", "codex/api", "Ready", ""}, {"", "codex/web", "Working", "", true, true, SessionStatusBadge::Working},
            {"orchard-mac", "codex/ios", "Ready", ""}, {"", "codex/stopped", "Stopped", "The agent is not running. Resume it before sending."}};
}
QAbstractButton *tile(TasksView &view, const QString &stage) { return view.findChild<QAbstractButton *>("taskTile:" + stage); }
QWidget *row(TasksView &view, int n) { return view.findChild<QWidget *>(QString("taskRow:T-%1").arg(n, 6, 10, QLatin1Char('0'))); }
QToolButton *action(QWidget *row, const QString &name) { return row ? row->findChild<QToolButton *>("taskAction:" + name) : nullptr; }
QStringList visibleRows(TasksView &view) {
    QStringList out;
    for (auto *w : view.findChildren<QWidget *>()) if (w->objectName().startsWith("taskRow:") && w->isVisibleTo(&view)) out << w->objectName().mid(8);
    std::sort(out.begin(), out.end(), [&](const QString &a, const QString &b) {
        return view.findChild<QWidget *>("taskRow:" + a)->y() < view.findChild<QWidget *>("taskRow:" + b)->y(); });
    return out;
}
}

class TestTasksView : public QObject {
    Q_OBJECT
    QTemporaryDir m_settings;
private slots:
    void initTestCase();
    void init() { ComposerDraftStore::flushWrites(); QDir(ComposerDraftStore::directory()).removeRecursively(); QSettings().remove("tasks"); }
    void tilesCountAndChooseTheFirstStage();
    void rowsShowTheQueueAndTheOpenSessionsTasks();
    void actionsRunHgsAndRefresh();
    void sendNowNamesWhyASessionCannotTakeIt();
    void theInputFilesATaskForTheOpenSession();
    void onlyThisSessionFiltersAndIsRemembered();
    void doneShowsTheLastWeekAndOlderOnRequest();
    void aFailedListingIsShownInsteadOfTasks();
    void aSessionOutsideProjectsExplainsWhy();
    void theTabBadgeCountsReviewAndTodo();
    void rowsWorkFromTheKeyboard();
    void leavingAProjectClearsItsCounts();
    void aFailedListingBacksOffAndClearsItsCounts();
    void unreadableTaskFilesAreNamed();
    void aRowOpensOnlyOnAClickOnItself();
    void renamedTaskKeepsItsEmphasisAndRejectsAReusedName();
    void taskImagesAreSavedWithTheDescription();
    void inspectorFollowsExecutorIdentityAcrossRename();
    void inlineTaskEditingRetainsFailuresAndCommandsStayInTheFooter();
    void selectingAnotherTaskNeverPublishesAnEmptyDetailsFrame();
    void assignmentOnlyStoresAPlanUntilStart();
    void commentImagesUseActivityAndDraftsFollowTheTask();
    void inspectorShowsReportsAndBoundsItsTaskList();
    void inspectorDiscardsHistoryAfterLeavingTheProject();
    void inspectorKeepsSelectionInAFixedLowerPane();
    void sixLongReviewTasksStayCompactAndRevealOneDetailSection();
    void inspectorHistoryShowsEarlierEntriesWithoutMovingTheBoard();
    void inspectorPagesOlderHistoryWithoutGrowingItsWindow();
};

void TestTasksView::selectingAnotherTaskNeverPublishesAnEmptyDetailsFrame() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); TaskInspector inspector(&client, &board);
    inspector.resize(380, 760); inspector.show(); board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions()); QTRY_COMPARE(inspector.displayedCount(), 2); inspector.selectTask("uuid-1");
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail"); QTRY_VERIFY(detail->toPlainText().contains("Task 1"));
    auto *start = inspector.findChild<QPushButton *>("taskInspectorAction:sendNow"); QVERIFY(start && start->isEnabled());
    auto *split = inspector.findChild<QSplitter *>("taskInspectorSplit"); const auto sizes = split->sizes();
    const auto icon = start->icon().cacheKey();
    QFile delay(dir.filePath("delay-get")); QVERIFY(delay.open(QIODevice::WriteOnly)); delay.write("0.3"); delay.close();
    inspector.selectTask("uuid-2");
    QVERIFY(detail->toPlainText().contains("Task 1")); QVERIFY(start->isEnabled()); start->click(); QCOMPARE(split->sizes(), sizes);
    for (const auto &call : FakeTasksCli::calls(dir.path())) QVERIFY(call.value(1) != "send");
    QElapsedTimer elapsed; elapsed.start();
    while (!detail->toPlainText().contains("Task 2") && elapsed.elapsed() < 3000) {
        QTest::qWait(5); QVERIFY(!detail->toPlainText().isEmpty()); QVERIFY(start->isEnabled()); QCOMPARE(start->icon().cacheKey(), icon);
    }
    QVERIFY(detail->toPlainText().contains("Task 2"));
    QCOMPARE(inspector.findChild<QPushButton *>("taskInspectorAction:sendNow"), start);
    QVERIFY(start->isEnabled()); QCOMPARE(split->sizes(), sizes); QCOMPARE(detail->verticalScrollBar()->value(), 0);
    // Context Edit waits for the requested task, never editing the previous document.
    board.setOpenInInspector(true); board.actOnTask(TaskBoard::item(FakeTasksCli::task(1, "todo")), TaskAction::Edit);
    QTRY_VERIFY(inspector.findChild<TaskForm *>());
    QTRY_COMPARE(inspector.findChild<TaskForm *>()->findChild<QPlainTextEdit *>("taskPrompt")->toPlainText(), QString("Task 1"));
}

void TestTasksView::assignmentOnlyStoresAPlanUntilStart() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); TaskInspector inspector(&client, &board);
    auto known = sessions(); known[0].id = "launch_id:main";
    for (int i = 0; i < 70; ++i) { auto extra = known[0]; extra.name = QString("codex/session-%1").arg(i, 3, 10, QLatin1Char('0')); extra.id += QString::number(i); known.append(extra); }
    inspector.resize(500, 900); inspector.show(); board.setContext("project-1", "orchard", "", "codex/api", "key", known);
    inspector.setContext("project-1", "", "codex/api", known); QTRY_COMPARE(inspector.displayedCount(), 1); inspector.selectTask("uuid-1");
    auto *assignment = inspector.findChild<QPushButton *>("taskInspectorAssignment"); QTRY_VERIFY(assignment->isEnabled());
    assignment->click(); auto *menu = inspector.findChild<QMenu *>("taskAssignmentMenu"); QVERIFY(menu);
    QVERIFY(!inspector.findChild<TaskForm *>());
    QAction *newSession = nullptr, *specific = nullptr; int childIndex = -1, newIndex = -1, anyIndex = -1, i = 0;
    for (auto *entry : menu->actions()) {
        if (entry->text() == "Subagent") childIndex = i;
        if (entry->text() == "New session") { newSession = entry; newIndex = i; }
        if (entry->text().startsWith("Any session")) anyIndex = i;
        if (entry->text() == "Specific session") specific = entry;
        ++i;
    }
    QVERIFY(childIndex > 0 && newIndex > childIndex && anyIndex > newIndex); QVERIFY(newSession && specific);
    auto *search = menu->findChild<QLineEdit *>("taskAssignmentSearch"); QVERIFY(search); search->setText("session-069");
    QVERIFY(menu->findChildren<QPushButton *>().size() < 55);
    bool foundLast = false; for (auto *entry : menu->findChildren<QPushButton *>()) if (entry->text() == "codex/session-069") foundLast = true;
    QVERIFY(foundLast); newSession->trigger(); menu->close();
    QTRY_VERIFY(assignment->text().contains("New session"));
    QVERIFY(FakeTasksCli::calls(dir.path()).contains({"task", "target", "uuid-1", "new-session", "--parent", "codex/api", "--session-id", "launch_id:main", "--project", "project-1"}));
    for (const auto &call : FakeTasksCli::calls(dir.path())) QVERIFY(call.value(1) != "send");
    QSignalSpy starts(&board, &TasksView::newExecutorRequested);
    inspector.findChild<QPushButton *>("taskInspectorAction:sendNow")->click(); QCOMPARE(starts.size(), 1);
    QCOMPARE(board.board().tasks.first().stage, QString("todo"));
}

void TestTasksView::commentImagesUseActivityAndDraftsFollowTheTask() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); TaskInspector inspector(&client, &board);
    inspector.resize(500, 900); inspector.show(); board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions()); QTRY_COMPARE(inspector.displayedCount(), 2); inspector.selectTask("uuid-1");
    auto *composer = inspector.findChild<MessageComposer *>("taskCommentComposer"); QTRY_VERIFY(composer->sessionKey().endsWith("uuid-1"));
    QImage image(20, 16, QImage::Format_RGB32); image.fill(Qt::green); QByteArray bytes; QBuffer buffer(&bytes); QVERIFY(buffer.open(QIODevice::WriteOnly)); QVERIFY(image.save(&buffer, "PNG"));
    composer->editor()->setPlainText("Review this screenshot"); QVERIFY(composer->addAttachment("screenshot.png", "image/png", bytes));
    inspector.selectTask("uuid-2"); QTRY_VERIFY(composer->sessionKey().endsWith("uuid-2")); QVERIFY(composer->editor()->toPlainText().isEmpty());
    composer->editor()->setPlainText("Other task"); inspector.selectTask("uuid-1"); QTRY_VERIFY(composer->sessionKey().endsWith("uuid-1"));
    QVERIFY(composer->editor()->toPlainText().contains("Review this screenshot")); QCOMPARE(composer->draftSnapshot().attachments.size(), 1);
    composer->findChild<QPushButton *>("sendMessage")->click(); QTRY_VERIFY(composer->editor()->toPlainText().isEmpty());
    inspector.findChild<QTabBar *>("taskDetailTabs")->setCurrentIndex(2);
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail"); QTRY_VERIFY(detail->toPlainText().contains("Review this screenshot"));
    QTRY_VERIFY(detail->toPlainText().contains("screenshot.png"));
    bool media = false; for (const auto &call : FakeTasksCli::calls(dir.path())) if (call.value(1) == "comment" && call.contains("--content-json")) media = true;
    QVERIFY(media);
}

void TestTasksView::inlineTaskEditingRetainsFailuresAndCommandsStayInTheFooter() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); TaskInspector inspector(&client, &board);
    inspector.resize(380, 760); inspector.show(); board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions()); QTRY_COMPARE(inspector.displayedCount(), 1);
    auto *card = inspector.findChild<TaskRow *>("taskInspectorRow:T-000001");
    auto *primary = card->findChild<QToolButton *>("taskAction:sendNow"); QVERIFY(primary && primary->isVisibleTo(card));
    inspector.selectTask("uuid-1"); auto *details = inspector.findChild<QPushButton *>("taskInspectorEdit");
    QTRY_VERIFY(details->isEnabled()); details->click();
    auto *form = inspector.findChild<TaskForm *>(); QVERIFY(form && form->isVisibleTo(&inspector));
    QVERIFY(inspector.findChildren<TaskDialog *>().isEmpty()); QVERIFY(!form->findChild<QTextBrowser *>("taskHistory"));
    QVERIFY(!form->findChild<QComboBox *>("taskAssignedTo"));
    auto *composer = form->findChild<MessageComposer *>("taskDescriptionComposer"); QVERIFY(composer);
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    auto *prompt = form->findChild<QPlainTextEdit *>("taskPrompt"); prompt->setPlainText("Revised requirement");
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("edit"); fail.close();
    composer->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(form->findChild<QLabel *>("taskDialogError")->isVisibleTo(form)); QCOMPARE(prompt->toPlainText(), QString("Revised requirement"));
    QVERIFY(fail.remove()); composer->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(!inspector.findChild<QScrollArea *>("taskInlineEditor"));
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail"); QTRY_VERIFY(detail->toPlainText().contains("Revised requirement"));
    for (const auto &call : FakeTasksCli::calls(dir.path())) QVERIFY(call.value(1) != "target");
    auto *tabs = inspector.findChild<QTabBar *>("taskDetailTabs"); auto *footer = inspector.findChild<QWidget *>("taskInspectorActions");
    const auto footerTop = footer->mapTo(&inspector, QPoint());
    for (const int tab : {1, 2, 0}) { tabs->setCurrentIndex(tab); QTest::qWait(20); QCOMPARE(footer->mapTo(&inspector, QPoint()), footerTop); }
    auto *notes = inspector.findChild<MessageComposer *>("taskCommentComposer"); QVERIFY(notes);
    auto *note = notes->editor(); note->setPlainText("Keep this note");
    QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("comment"); fail.close();
    notes->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(inspector.findChild<QLabel *>("taskInspectorNoteError")->isVisibleTo(&inspector)); QCOMPARE(note->toPlainText(), QString("Keep this note"));
    QVERIFY(fail.remove()); notes->findChild<QPushButton *>("sendMessage")->click(); QTRY_VERIFY(note->toPlainText().isEmpty());
    tabs->setCurrentIndex(2); QTRY_VERIFY(detail->toPlainText().contains("Keep this note"));
    inspector.findChild<QPushButton *>("taskInspectorAction:sendNow")->click();
    QTRY_COMPARE(inspector.selectedStage(), QString("active"));
    auto *pause = inspector.findChild<QPushButton *>("taskInspectorAction:pause"); QVERIFY(pause && !pause->isEnabled());
    QVERIFY(pause->toolTip().contains("individual task"));
    for (const auto &call : FakeTasksCli::calls(dir.path())) QVERIFY(call.value(1) != "cancel");
    const auto preview = qEnvironmentVariable("HGS_TASK_INLINE_PREVIEW");
    if (!preview.isEmpty()) { inspector.selectTask("uuid-1"); QTest::qWait(50); QVERIFY(inspector.grab().save(preview)); }
}

void TestTasksView::inspectorFollowsExecutorIdentityAcrossRename() {
    auto retained = FakeTasksCli::pinned("codex/old"); retained["session_id"] = "launch_id:original";
    const QJsonArray history{QJsonObject{{"kind", "started"}, {"actor", QJsonObject{{"kind", "agent"},
        {"host", ""}, {"session", "codex/old"}, {"session_id", "launch_id:original"}}}, {"at", 1}}};
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "active",
        {{"target", retained}, {"session", retained}, {"log", history}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(400, 700); inspector.show();
    auto known = sessions(); known[0].name = "codex/new"; known[0].id = "launch_id:original";
    board.setContext("project-1", "orchard", "", "codex/new", "key", known);
    inspector.setContext("project-1", "", "codex/new", known);
    QTRY_COMPARE(inspector.displayedCount(), 1); inspector.selectTask("uuid-1");
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail");
    auto *context = inspector.findChild<QLabel *>("taskInspectorState");
    QTRY_VERIFY(context->text().contains("Executor"));
    QVERIFY(context->text().contains("new")); QVERIFY(!context->text().contains("old"));
    QSignalSpy opened(&inspector, &TaskInspector::executorRequested);
    inspector.findChild<QPushButton *>("taskOpenActivity")->click(); QCOMPARE(opened.size(), 1);
    QCOMPARE(opened.first().at(1).toString(), QString("codex/new"));
    QCOMPARE(opened.first().at(2).toString(), QString("launch_id:original"));
    // A different session reusing the label must never become the executor.
    known[0].id = "launch_id:replacement";
    inspector.setContext("project-1", "", "codex/new", known);
    inspector.findChild<QToolButton *>("taskOnlySession")->setChecked(false);
    QTRY_VERIFY(!inspector.findChild<QPushButton *>("taskOpenActivity")->isEnabled());
    const auto preview = qEnvironmentVariable("HGS_TASK_INSPECTOR_PREVIEW");
    if (!preview.isEmpty()) { inspector.resize(430, 760); QTest::qWait(50); QVERIFY(inspector.grab().save(preview)); }
}

void TestTasksView::inspectorShowsReportsAndBoundsItsTaskList() {
    QJsonArray tasks{FakeTasksCli::task(1, "review", {{"session", FakeTasksCli::pinned("codex/api")},
        {"summary", "Committed <abc123>; installation pending."}, {"text", "Keep <script> literal"},
        {"log", QJsonArray{QJsonObject{{"kind", "comment"}, {"actor", QJsonObject{{"kind", "person"}}}, {"at", 1}, {"text", "Please keep the history."}}}}})};
    tasks.append(FakeTasksCli::task(2, "active", {{"session", FakeTasksCli::pinned("codex/api")}}));
    for (int n = 3; n < 125; ++n) tasks.append(FakeTasksCli::task(n, "todo"));
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), tasks));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(400, 700); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions());
    QTRY_COMPARE(inspector.displayedCount(), 1);
    inspector.selectTask("uuid-1");
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail");
    QTRY_VERIFY(detail->toPlainText().contains("Keep <script> literal"));
    QVERIFY(!detail->toPlainText().contains("Committed <abc123>"));
    auto *sections = inspector.findChild<QTabBar *>("taskDetailTabs"); sections->setCurrentIndex(1);
    QTRY_VERIFY(detail->toPlainText().contains("Committed <abc123>; installation pending."));
    QVERIFY(!detail->toPlainText().contains("Keep <script> literal"));
    sections->setCurrentIndex(2); QTRY_VERIFY(detail->toPlainText().contains("Please keep the history."));
    inspector.selectTask("uuid-2");
    QTRY_VERIFY(inspector.findChild<QLabel *>("taskInspectorState")->text().contains("start unconfirmed"));
    inspector.selectTask("uuid-3");
    QTRY_VERIFY(inspector.findChild<QLabel *>("taskInspectorState")->text().contains("#1 in queue"));
    QCOMPARE(inspector.displayedCount(), 100);
    QVERIFY(inspector.findChild<QLabel *>("taskInspectorScope")->text().contains("100 of 122"));
    inspector.selectTask("uuid-124");
    QTRY_VERIFY(detail->toPlainText().contains("Task 124")); QCOMPARE(inspector.displayedCount(), 100);
    for (const auto &call : FakeTasksCli::calls(dir.path())) QVERIFY(call.at(1) == "list" || call.at(1) == "get");
}

void TestTasksView::sixLongReviewTasksStayCompactAndRevealOneDetailSection() {
    QJsonArray live;
    if (const auto path = qEnvironmentVariable("HGS_TASK_DENSITY_DATA"); !path.isEmpty()) {
        QFile file(path); QVERIFY(file.open(QIODevice::ReadOnly)); live = QJsonDocument::fromJson(file.readAll()).object().value("tasks").toArray();
    }
    QJsonArray tasks;
    for (int n = 1; n <= 6; ++n) {
        const QString prompt = QString("Task %1: make task ownership clear while working in Activity. ").arg(n).repeated(8);
        const QString report = QString("REPORT_BODY_%1: implemented the change, checked the behavior, and recorded the remaining work. ").arg(n).repeated(30);
        auto task = FakeTasksCli::task(n, "review", {{"text", prompt}, {"summary", report}, {"session", FakeTasksCli::pinned("codex/api")},
            {"log", QJsonArray{QJsonObject{{"kind", "comment"}, {"actor", QJsonObject{{"kind", "person"}}}, {"at", 1}, {"text", "HISTORY_BODY: please verify the result."}}}}});
        if (n <= live.size()) {
            const auto actual = live.at(n - 1).toObject(); task["text"] = actual.value("text"); task["summary"] = actual.value("summary");
        }
        tasks.append(task);
    }
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), tasks));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(560, 900); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions());
    QTRY_COMPARE(inspector.displayedCount(), 6);
    for (const int width : {560, 380}) {
        inspector.resize(width, 760); QTest::qWait(30);
        for (auto *row : inspector.findChildren<TaskRow *>()) {
            QVERIFY2(row->height() <= 88, qPrintable(QString("Collapsed task height %1 at width %2").arg(row->height()).arg(width)));
            QVERIFY(!row->findChild<QLabel *>("taskSummary")->isVisibleTo(row));
            QVERIFY(row->findChild<QLabel *>("taskSummary")->text().isEmpty());
        }
    }
    const auto preview = qEnvironmentVariable("HGS_TASK_DENSITY_PREVIEW");
    if (!preview.isEmpty()) for (const bool dark : {true, false}) {
        inspector.setTheme(dark); inspector.resize(560, 900); QTest::qWait(50);
        QVERIFY(inspector.grab().save(preview + (dark ? "-dark.png" : "-light.png")));
    }
    auto *split = inspector.findChild<QSplitter *>("taskInspectorSplit");
    auto *list = inspector.findChild<QScrollArea *>("taskInspectorList");
    QTest::qWait(30); const auto splitSizes = split->sizes(); const auto listRect = list->geometry();
    inspector.selectTask("uuid-1");
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail");
    auto *sections = inspector.findChild<QTabBar *>("taskDetailTabs");
    QTRY_VERIFY(!detail->toPlainText().isEmpty());
    QCOMPARE(split->sizes(), splitSizes); QCOMPARE(list->geometry(), listRect);
    QCOMPARE(sections->currentIndex(), 0); QVERIFY(!detail->toPlainText().contains("REPORT_BODY_1"));
    QVERIFY(!detail->toPlainText().contains("HISTORY_BODY"));
    sections->setCurrentIndex(1); QTRY_VERIFY(detail->toPlainText().simplified().contains(tasks.first().toObject().value("summary").toString().simplified()));
    QTRY_COMPARE(detail->verticalScrollBar()->value(), 0);
    QCOMPARE(split->sizes(), splitSizes); QCOMPARE(list->geometry(), listRect);
    sections->setCurrentIndex(2); QTRY_VERIFY(detail->toPlainText().contains("HISTORY_BODY"));
    if (!preview.isEmpty()) {
        inspector.setTheme(true); sections->setCurrentIndex(1); QTest::qWait(100);
        QCOMPARE(split->sizes(), splitSizes); QCOMPARE(list->geometry(), listRect);
        QVERIFY(inspector.grab().save(preview + "-result.png"));
    }
    auto *first = inspector.findChild<TaskRow *>("taskInspectorRow:T-000001");
    auto *more = first->findChild<QToolButton *>("taskMoreActions"); QVERIFY(more->isVisibleTo(first));
    auto *accept = more->menu()->findChild<QAction *>("taskMenuAction:accept"); QVERIFY(accept && accept->isEnabled());
    accept->trigger();
    QTRY_COMPARE(inspector.findChild<QAbstractButton *>("taskTile:done")->property("count").toInt(), 1);
}

void TestTasksView::inspectorHistoryShowsEarlierEntriesWithoutMovingTheBoard() {
    QJsonArray history;
    for (int n = 1; n <= 18; ++n) history.append(QJsonObject{{"kind", "comment"}, {"actor", QJsonObject{{"kind", "person"}}},
        {"at", 1791018000000.0 + n * 1000}, {"text", QString("COMMENT_%1_END with **Markdown** and `code`.").arg(n)}});
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "review", {{"log", history}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(340, 760); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions());
    QTRY_COMPARE(inspector.displayedCount(), 1); inspector.selectTask("uuid-1");
    auto *tabs = inspector.findChild<QTabBar *>("taskDetailTabs"); QTRY_VERIFY(tabs->isEnabled()); tabs->setCurrentIndex(2);
    auto *activity = inspector.findChild<ActivityView *>("taskDetailActivity");
    QTRY_VERIFY(activity->plainText().contains("COMMENT_18_END")); QVERIFY(!activity->plainText().contains("COMMENT_1_END"));
    auto *split = inspector.findChild<QSplitter *>("taskInspectorSplit"); const auto sizes = split->sizes();
    auto *more = inspector.findChild<QPushButton *>("taskDetailMore"); QVERIFY(more->isVisible()); more->click();
    QTRY_VERIFY(activity->plainText().contains("COMMENT_3_END")); QVERIFY(!activity->plainText().contains("COMMENT_1_END"));
    QTRY_COMPARE(activity->browser()->verticalScrollBar()->value(), 0); QCOMPARE(split->sizes(), sizes);
    more->click(); QTRY_VERIFY(activity->plainText().contains("COMMENT_1_END")); QVERIFY(more->isHidden());
    QTRY_COMPARE(activity->browser()->verticalScrollBar()->value(), 0); QCOMPARE(split->sizes(), sizes);
}

void TestTasksView::inspectorPagesOlderHistoryWithoutGrowingItsWindow() {
    QJsonArray history;
    for (int n = 1; n <= 95; ++n) history.append(QJsonObject{{"kind", "comment"}, {"actor", QJsonObject{{"kind", "person"}}},
        {"at", double(n)}, {"text", QString("EVENT_%1_END").arg(n)}});
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "review", {{"log", history}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); TaskInspector inspector(&client, &board);
    inspector.resize(380, 760); inspector.show(); board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions()); QTRY_COMPARE(inspector.displayedCount(), 1); inspector.selectTask("uuid-1");
    auto *tabs = inspector.findChild<QTabBar *>("taskDetailTabs"); QTRY_VERIFY(tabs->isEnabled()); tabs->setCurrentIndex(2);
    auto *activity = inspector.findChild<ActivityView *>("taskDetailActivity"); QTRY_VERIFY(activity->plainText().contains("EVENT_95_END"));
    auto *more = inspector.findChild<QPushButton *>("taskDetailMore");
    for (int n = 0; n < 9; ++n) more->click();
    QTRY_VERIFY(activity->plainText().contains("EVENT_16_END")); QVERIFY(!activity->plainText().contains("EVENT_15_END"));
    more->click(); QTRY_VERIFY(activity->plainText().contains("EVENT_15_END")); QVERIFY(!activity->plainText().contains("EVENT_95_END"));
    auto *latest = inspector.findChild<QPushButton *>("taskHistoryLatest"); QVERIFY(latest->isVisibleTo(&inspector));
    more->click(); QTRY_VERIFY(activity->plainText().contains("EVENT_1_END")); QVERIFY(more->isHidden());
    // A new appended event cannot shift the anchored older page during a poll.
    bool appended = false; board.saveTaskNote("uuid-1", "EVENT_96_END", {}, [&] { appended = true; }, [](const QString &) {});
    QTRY_VERIFY(appended); QTest::qWait(100); QVERIFY(activity->plainText().contains("EVENT_1_END")); QVERIFY(!activity->plainText().contains("EVENT_96_END"));
    latest->click(); QTRY_VERIFY(activity->plainText().contains("EVENT_96_END")); QVERIFY(latest->isHidden());
}

void TestTasksView::inspectorDiscardsHistoryAfterLeavingTheProject() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions());
    QTRY_VERIFY(inspector.displayedCount()); inspector.selectTask("uuid-1");
    // A listing has started the asynchronous detail fetch. Leave before its reply.
    board.setContext({}, {}, {}, {}, {}, {}); inspector.setContext({}, {}, {}, {});
    QTest::qWait(150);
    QCOMPARE(inspector.displayedCount(), 0);
    QVERIFY(inspector.findChild<QTextBrowser *>("taskInspectorDetail")->toPlainText().isEmpty());
    QVERIFY(!inspector.findChild<QPushButton *>("taskOpenActivity")->isEnabled());
}

void TestTasksView::inspectorKeepsSelectionInAFixedLowerPane() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "todo"),
        FakeTasksCli::task(3, "active", {{"session", FakeTasksCli::pinned("codex/api")}, {"stage_at", now()}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(380, 760); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions());
    QTRY_COMPARE(inspector.displayedCount(), 2);
    auto *todo = inspector.findChild<QAbstractButton *>("taskTile:todo");
    QVERIFY(todo->property("compact").toBool()); QCOMPARE(todo->property("count").toInt(), 2);
    auto *first = inspector.findChild<TaskRow *>("taskInspectorRow:T-000001");
    auto *second = inspector.findChild<TaskRow *>("taskInspectorRow:T-000002"); QVERIFY(first && second);
    auto *details = inspector.findChild<QWidget *>("taskDetailsPane"); QVERIFY(details->isVisible());
    auto *split = inspector.findChild<QSplitter *>("taskInspectorSplit"); QVERIFY(split); QCOMPARE(split->count(), 2);
    auto *scroll = inspector.findChild<QScrollArea *>("taskInspectorList");
    QTest::qWait(30); const auto sizes = split->sizes(); const auto firstRect = first->geometry(); const auto secondRect = second->geometry();
    const auto listRect = scroll->geometry(); const auto detailTop = details->mapTo(&inspector, QPoint());
    QTest::mouseClick(first, Qt::LeftButton, {}, QPoint(65, 25));
    QTRY_COMPARE(inspector.selectedTask(), QString("uuid-1"));
    QTRY_VERIFY(inspector.findChild<QTextBrowser *>("taskInspectorDetail")->toPlainText().contains("Task 1"));
    QCOMPARE(split->sizes(), sizes); QCOMPARE(first->geometry(), firstRect); QCOMPARE(second->geometry(), secondRect);
    QCOMPARE(scroll->geometry(), listRect); QCOMPARE(details->mapTo(&inspector, QPoint()), detailTop);
    auto *sections = inspector.findChild<QTabBar *>("taskDetailTabs");
    for (int n = 0; n < sections->count(); ++n) {
        QVERIFY(sections->tabRect(n).width() >= sections->fontMetrics().horizontalAdvance(sections->tabText(n)) + 16);
        QVERIFY(sections->rect().contains(sections->tabRect(n)));
    }
    QCOMPARE(sections->elideMode(), Qt::ElideNone);
    sections->setCurrentIndex(2); QCOMPARE(split->sizes(), sizes); QCOMPARE(second->geometry(), secondRect);
    QTest::mouseClick(second, Qt::LeftButton, {}, QPoint(65, 25));
    QTRY_COMPARE(inspector.selectedTask(), QString("uuid-2")); QCOMPARE(split->sizes(), sizes);
    QTest::mouseClick(second, Qt::LeftButton, {}, QPoint(65, 25)); QCOMPARE(inspector.selectedTask(), QString("uuid-2"));
    QVERIFY(!inspector.findChild<QPushButton *>("taskProjectBoard"));
    auto *scope = inspector.findChild<QToolButton *>("taskOnlySession"); QVERIFY(scope && scope->isVisible());
    scope->click(); QVERIFY(board.onlyThisSession()); scope->click(); QVERIFY(!board.onlyThisSession());
    inspector.findChild<QToolButton *>("taskSettings")->click();
    QPointer<TaskSettingsDialog> settings; QTRY_VERIFY((settings = board.findChild<TaskSettingsDialog *>()));
    settings->findChild<QSpinBox *>("agentLimit")->setValue(3); settings->findChild<QPushButton *>("saveTaskSettings")->click();
    QTRY_VERIFY(!settings || !settings->isVisible());
    bool saved = false;
    for (const auto &call : FakeTasksCli::calls(dir.path())) if (call.contains("settings") && call.contains("--set")) saved = true;
    QVERIFY(saved);
    inspector.resize(960, 760); QTRY_VERIFY(!todo->property("compact").toBool());
    inspector.resize(280, 760); QTRY_VERIFY(todo->property("compact").toBool());
    for (int n = 0; n < sections->count(); ++n) QVERIFY(sections->rect().contains(sections->tabRect(n)));
    inspector.resize(380, 760);
    // Card actions invoke the same guarded command as on the project board.
    action(second, "sendNow")->click();
    QTRY_COMPARE(inspector.findChild<QAbstractButton *>("taskTile:active")->property("count").toInt(), 2);
    inspector.findChild<QAbstractButton *>("taskTile:active")->click(); QCOMPARE(inspector.displayedCount(), 2);
    inspector.selectTask("uuid-2");
    QTRY_VERIFY(inspector.findChild<QTextBrowser *>("taskInspectorDetail")->toPlainText().contains("Task 2"));
    const auto preview = qEnvironmentVariable("HGS_TASK_BOARD_PANEL_PREVIEW");
    if (!preview.isEmpty()) {
        QTest::qWait(50); QVERIFY(inspector.grab().save(preview));
        inspector.resize(960, 760); QTest::qWait(50); QVERIFY(inspector.grab().save(preview + ".wide.png"));
        inspector.setTheme(false); inspector.resize(380, 760); QTest::qWait(50); QVERIFY(inspector.grab().save(preview + ".light.png"));
    }
}

void TestTasksView::initTestCase() {
    QVERIFY(m_settings.isValid());
    QCoreApplication::setOrganizationName("hgs-tests"); QCoreApplication::setApplicationName("tasks-view");
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
}

void TestTasksView::renamedTaskKeepsItsEmphasisAndRejectsAReusedName() {
    auto target = FakeTasksCli::pinned("codex/old"); target["session_id"] = "launch_id:original";
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo", {{"target", target}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    auto known = sessions(); known[0].name = "codex/new"; known[0].id = "launch_id:original";
    view.setContext("project-1", "orchard", "", "codex/new", "\ncodex/new", known);
    QTRY_VERIFY(row(view, 1));
    QVERIFY(row(view, 1)->property("mine").toBool());
    QVERIFY(row(view, 1)->findChild<QLabel *>("taskMeta")->text().contains("new"));
    QVERIFY(!row(view, 1)->findChild<QLabel *>("taskMeta")->text().contains("old"));
    known[0].name = "codex/old"; known[0].id = "launch_id:replacement";
    view.setContext("project-1", "orchard", "", "codex/old", "\ncodex/old", known);
    QVERIFY(!row(view, 1)->property("mine").toBool());
    QVERIFY(!action(row(view, 1), "sendNow")->property("blockedReason").toString().isEmpty());
}

void TestTasksView::taskImagesAreSavedWithTheDescription() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path()));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    auto *composer = view.composer(); QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    composer->editor()->setPlainText("A screenshot");
    QVERIFY(composer->addAttachment("screen.png", "image/png", "immutable bytes"));
    QTest::keyClick(composer->editor(), Qt::Key_Return);
    QTRY_COMPARE(view.board().tasks.size(), 1);
    QTRY_VERIFY(composer->editor()->toPlainText().isEmpty());
    QFile state(dir.filePath("tasks.json")); QVERIFY(state.open(QIODevice::ReadOnly));
    const auto saved = QJsonDocument::fromJson(state.readAll()).object().value("tasks").toArray().first().toObject();
    const auto file = saved.value("attachments").toArray().first().toObject();
    QCOMPARE(file.value("name").toString(), QString("screen.png"));
    QCOMPARE(QByteArray::fromBase64(file.value("data_base64").toString().toLatin1()), QByteArray("immutable bytes"));
    QVERIFY(saved.value("text").toString().contains("[Image #1]"));
}

void TestTasksView::tilesCountAndChooseTheFirstStage() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "todo"),
        FakeTasksCli::task(3, "review", {{"session", FakeTasksCli::pinned("codex/api")}, {"summary", "Fixed."}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    QSignalSpy counts(&view, &TasksView::countsChanged);
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_COMPARE(tile(view, "todo")->property("count").toInt(), 2);
    QCOMPARE(tile(view, "review")->property("count").toInt(), 1);
    // In review opens first while it has tasks.
    QVERIFY(tile(view, "review")->isChecked()); QCOMPARE(view.selectedStage(), QString("review"));
    QCOMPARE(visibleRows(view), QStringList({"T-000003"}));
    QVERIFY(!counts.isEmpty()); QCOMPARE(counts.last().value(0).toInt(), 1); QCOMPARE(counts.last().value(1).toInt(), 2);
    QCOMPARE(FakeTasksCli::calls(dir.path()).first(), QStringList({"task", "list", "--project", "project-1"}));
    tile(view, "todo")->click(); QCOMPARE(visibleRows(view), QStringList({"T-000001", "T-000002"}));
    QVERIFY(view.findChild<QToolButton *>("taskOnlySession")); QVERIFY(view.findChild<QToolButton *>("taskSettings"));
    const auto preview = qEnvironmentVariable("HGS_TASKS_PREVIEW");
    if (!preview.isEmpty()) for (const bool dark : {true, false}) {
        view.setTheme(dark); QTest::qWait(50); QVERIFY(view.grab().save(preview + (dark ? "-dark.png" : "-light.png")));
    }
}

void TestTasksView::rowsShowTheQueueAndTheOpenSessionsTasks() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo", {{"target", FakeTasksCli::pinned("codex/web")}}),
        FakeTasksCli::task(2, "todo", {{"target", FakeTasksCli::pinned("codex/api")}, {"text", "Add rate limiting to POST /v1/orders"}}),
        FakeTasksCli::task(3, "todo"),
        FakeTasksCli::task(4, "active", {{"session", FakeTasksCli::pinned("codex/web")}, {"author", QJsonObject{{"kind", "agent"}, {"machine", "m"}, {"session", "codex/web"}}}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_VERIFY(row(view, 2)); tile(view, "todo")->click();
    QCOMPARE(row(view, 1)->findChild<QLabel *>("taskGlyph")->text(), QString("1"));
    QCOMPARE(row(view, 2)->findChild<QLabel *>("taskGlyph")->text(), QString("2"));
    // The open session's own task is highlighted; the others are not.
    QVERIFY(row(view, 2)->property("mine").toBool()); QVERIFY(!row(view, 1)->property("mine").toBool());
    QVERIFY(row(view, 2)->findChild<QLabel *>("taskText")->text().startsWith("Add rate limiting"));
    // A session is named as the session list names it.
    QVERIFY(row(view, 1)->findChild<QLabel *>("taskMeta")->text().contains("web") && !row(view, 1)->findChild<QLabel *>("taskMeta")->text().contains("codex/web"));
    QVERIFY(row(view, 3)->findChild<QLabel *>("taskMeta")->text().contains("any session"));
    for (const char *name : {"edit", "toBacklog", "top", "sendNow"}) QVERIFY2(action(row(view, 1), name), name);
    QVERIFY(!row(view, 1)->findChild<QWidget *>("taskStatus")->isVisible());
    QVERIFY(!row(view, 1)->findChild<QWidget *>("taskGlyph")->property("spinning").toBool());
    tile(view, "active")->click();
    // In progress names the session, then its status as the session list shows it, and spins.
    auto *status = row(view, 4)->findChild<QWidget *>("taskStatus"); QVERIFY(status); QTRY_VERIFY(status->isVisible());
    QCOMPARE(status->property("caption").toString(), QString("Working"));
    QCOMPARE(status->property("kind").toInt(), int(SessionStatusBadge::Working));
    auto *metaLabel = row(view, 4)->findChild<QLabel *>("taskMeta"); const auto meta = metaLabel->text();
    QVERIFY2(meta.contains("web") && !meta.contains("Working"), qPrintable(meta));
    QVERIFY(status->x() >= metaLabel->x() + metaLabel->width());
    QVERIFY2(row(view, 4)->findChild<QLabel *>("taskTail")->text().contains("filed by web"), qPrintable(row(view, 4)->findChild<QLabel *>("taskTail")->text()));
    QVERIFY(row(view, 4)->findChild<QWidget *>("taskGlyph")->property("spinning").toBool());
    QVERIFY(action(row(view, 4), "cancel"));
}

void TestTasksView::actionsRunHgsAndRefresh() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "todo"),
        FakeTasksCli::task(3, "review", {{"session", FakeTasksCli::pinned("codex/api")}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_COMPARE(tile(view, "todo")->property("count").toInt(), 2); tile(view, "todo")->click();
    action(row(view, 2), "top")->click();
    QTRY_COMPARE(visibleRows(view), QStringList({"T-000002", "T-000001"}));
    QVERIFY(FakeTasksCli::calls(dir.path()).contains(QStringList({"task", "move", "uuid-2", "--top", "--project", "project-1"})));
    action(row(view, 1), "toBacklog")->click();
    QTRY_COMPARE(tile(view, "backlog")->property("count").toInt(), 1);
    // An Any session task goes to the open session.
    action(row(view, 2), "sendNow")->click();
    QTRY_COMPARE(tile(view, "active")->property("count").toInt(), 1);
    QVERIFY(FakeTasksCli::calls(dir.path()).contains(QStringList({"task", "send", "uuid-2", "--session", "codex/api", "--project", "project-1"})));
    tile(view, "review")->click(); action(row(view, 3), "accept")->click();
    QTRY_COMPARE(tile(view, "done")->property("count").toInt(), 1);
    QVERIFY(FakeTasksCli::calls(dir.path()).contains(QStringList({"task", "stage", "uuid-3", "done", "--project", "project-1"})));
    // A refused command is reported and changes nothing.
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("stage"); fail.close();
    QSignalSpy notices(&view, &TasksView::notice);
    tile(view, "done")->click(); action(row(view, 3), "backToTodo")->click();
    QTRY_COMPARE(notices.size(), 1); QCOMPARE(notices[0][0].toString(), QString("hgs refused: stage")); QVERIFY(notices[0][1].toBool());
    QCOMPARE(tile(view, "done")->property("count").toInt(), 1);
}

void TestTasksView::sendNowNamesWhyASessionCannotTakeIt() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {
        FakeTasksCli::task(1, "todo", {{"target", FakeTasksCli::pinned("codex/gone")}}),
        FakeTasksCli::task(2, "todo", {{"target", FakeTasksCli::pinned("codex/stopped")}}),
        FakeTasksCli::task(3, "todo", {{"target", FakeTasksCli::pinned("codex/ios", "orchard-mac")}}),
        FakeTasksCli::task(4, "todo", {{"target", FakeTasksCli::pinned("codex/web")}}),
        FakeTasksCli::task(5, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_VERIFY(row(view, 5)); tile(view, "todo")->click();
    const auto blocked = [&](int n) { return action(row(view, n), "sendNow")->property("blockedReason").toString(); };
    QCOMPARE(blocked(1), QString("gone no longer exists — choose another session"));
    QCOMPARE(blocked(2), QString("The agent is not running. Resume it before sending."));
    QCOMPARE(action(row(view, 2), "sendNow")->toolTip(), blocked(2));
    QCOMPARE(blocked(4), QString());
    // A click on a blocked Send now says why beside it, as a blocked message send does, and sends nothing.
    QSignalSpy notices(&view, &TasksView::notice);
    action(row(view, 2), "sendNow")->click();
    QTRY_VERIFY(QToolTip::isVisible());
    QCOMPARE(QToolTip::text(), QString("The agent is not running. Resume it before sending."));
    QTest::qWait(100);
    QCOMPARE(notices.size(), 0);
    const auto calls = FakeTasksCli::calls(dir.path());
    QVERIFY(std::none_of(calls.cbegin(), calls.cend(), [](const QStringList &call) { return call.contains("uuid-2") && call.contains("send"); }));
    // A session on another computer takes it there; hgs runs the send on that computer.
    QCOMPARE(blocked(3), QString());
    action(row(view, 3), "sendNow")->click();
    QTRY_VERIFY(FakeTasksCli::calls(dir.path()).contains(QStringList({"task", "send", "uuid-3", "--project", "project-1"})));
    // An Any session task goes to the open session, there too.
    view.setContext("project-1", "orchard", "orchard-mac", "codex/ios", "orchard-mac\ncodex/ios", sessions());
    QTRY_VERIFY(row(view, 5) && action(row(view, 5), "sendNow")->property("blockedReason").toString().isEmpty());
    action(row(view, 5), "sendNow")->click();
    QTRY_VERIFY(FakeTasksCli::calls(dir.path()).contains(
        QStringList({"task", "send", "uuid-5", "--session", "codex/ios", "--host", "orchard-mac", "--project", "project-1"})));
}

void TestTasksView::theInputFilesATaskForTheOpenSession() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path()));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    auto *composer = view.composer(); auto *send = composer->findChild<QPushButton *>("sendMessage");
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    QCOMPARE(composer->sessionKey(), QString("task-draft\n\ncodex/api")); QCOMPARE(send->text(), QString("To do"));
    // The input names the session as the session list does; the session and the stage stand out.
    auto *status = composer->findChild<QLabel *>("messageStatus");
    QCOMPARE(composer->editor()->placeholderText(), QString("Task prompt for orchard / api…"));
    QCOMPARE(status->textFormat(), Qt::RichText);
    QVERIFY2(status->text().contains(">api</b>") && status->text().contains(">To do</b>"), qPrintable(status->text()));
    QCOMPARE(status->accessibleName(), QString("Task for api · Enter adds to To do · Shift+Enter for a new line"));
    // The emphasis is drawn, not printed as markup.
    const QImage drawn = status->grab().toImage(); status->setTextFormat(Qt::PlainText);
    const QImage printed = status->grab().toImage(); status->setTextFormat(Qt::RichText);
    QVERIFY(drawn != printed);
    composer->editor()->setFocus(); QTest::keyClicks(composer->editor(), "Add rate limiting");
    QTest::keyClick(composer->editor(), Qt::Key_Return);
    QTRY_COMPARE(tile(view, "todo")->property("count").toInt(), 1);
    QTRY_VERIFY(composer->editor()->toPlainText().isEmpty());
    QVERIFY(FakeTasksCli::calls(dir.path()).contains(QStringList({"task", "add", "--project", "project-1", "--target", "codex/api"})));
    QVERIFY(FakeTasksCli::stdins(dir.path()).contains("Add rate limiting"));
    // While Backlog is open, a new task goes there; a remote session names its computer.
    view.setContext("project-1", "orchard", "orchard-mac", "codex/ios", "orchard-mac\ncodex/ios", sessions());
    tile(view, "backlog")->click(); QCOMPARE(send->text(), QString("Backlog"));
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    QTest::keyClicks(composer->editor(), "Ship the iOS build"); QTest::keyClick(composer->editor(), Qt::Key_Return);
    QTRY_COMPARE(tile(view, "backlog")->property("count").toInt(), 1);
    QVERIFY(FakeTasksCli::calls(dir.path()).contains(QStringList({"task", "add", "--project", "project-1", "--target", "codex/ios", "--host", "orchard-mac", "--backlog"})));
    // A refused add keeps the prompt and reports why.
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("add"); fail.close();
    QSignalSpy notices(&view, &TasksView::notice);
    QTest::keyClicks(composer->editor(), "Kept"); QTest::keyClick(composer->editor(), Qt::Key_Return);
    QTRY_COMPARE(notices.size(), 1); QCOMPARE(notices[0][0].toString(), QString("hgs refused: add"));
    QTRY_COMPARE(composer->editor()->toPlainText(), QString("Kept"));
}

void TestTasksView::onlyThisSessionFiltersAndIsRemembered() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo", {{"target", FakeTasksCli::pinned("codex/web")}}),
        FakeTasksCli::task(2, "todo", {{"target", FakeTasksCli::pinned("codex/api")}}), FakeTasksCli::task(3, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_COMPARE(tile(view, "todo")->property("count").toInt(), 3);
    auto *only = view.findChild<QToolButton *>("taskOnlySession"); only->click();
    QCOMPARE(tile(view, "todo")->property("count").toInt(), 2); QCOMPARE(visibleRows(view), QStringList({"T-000002", "T-000003"}));
    // Queue numbers stay the project's.
    QCOMPARE(row(view, 3)->findChild<QLabel *>("taskGlyph")->text(), QString("3"));
    QVERIFY(QSettings().value("tasks/onlySession").toBool());
    TasksView again(&client); again.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QVERIFY(again.findChild<QToolButton *>("taskOnlySession")->isChecked());
}

void TestTasksView::doneShowsTheLastWeekAndOlderOnRequest() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "done", {{"stage_at", now() - 2 * Hour}}),
        FakeTasksCli::task(2, "done", {{"stage_at", now() - 10 * 24 * Hour}}), FakeTasksCli::task(3, "done", {{"stage_at", now() - Hour}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_COMPARE(tile(view, "done")->property("count").toInt(), 2);
    tile(view, "done")->click(); QCOMPARE(visibleRows(view), QStringList({"T-000001", "T-000003"}));
    auto *older = view.findChild<QPushButton *>("showOlderTasks"); QVERIFY(older->isVisible()); QCOMPARE(older->text(), QString("Show older (1)"));
    older->click(); QCOMPARE(visibleRows(view), QStringList({"T-000001", "T-000003", "T-000002"})); QCOMPARE(older->text(), QString("Hide older"));
}

void TestTasksView::aFailedListingIsShownInsteadOfTasks() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}));
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("list"); fail.close();
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    auto *error = view.findChild<QLabel *>("tasksError");
    QTRY_VERIFY(error->isVisible()); QVERIFY(error->text().contains("hgs refused: list"));
    QVERIFY(visibleRows(view).isEmpty());
}

void TestTasksView::aSessionOutsideProjectsExplainsWhy() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path()));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext({}, {}, "", "codex/api", "\ncodex/api", sessions());
    auto *empty = view.findChild<QLabel *>("taskEmpty");
    QVERIFY(empty->isVisible()); QVERIFY(empty->text().startsWith("Tasks belong to a project"));
    QVERIFY(!view.composer()->findChild<QPushButton *>("sendMessage")->isEnabled());
    QCOMPARE(view.composer()->findChild<QLabel *>("messageStatus")->text(), QString("This session is in no project; tasks belong to a project."));
    QTest::qWait(100); QVERIFY(FakeTasksCli::calls(dir.path()).isEmpty());
}

void TestTasksView::theTabBadgeCountsReviewAndTodo() {
    QWidget host; TaskTabBadge badge(&host); host.show();
    QVERIFY(!badge.isVisible());
    badge.setCounts(1, 12); QVERIFY(badge.isVisible());
    QCOMPARE(badge.property("review").toInt(), 1); QCOMPARE(badge.property("todo").toInt(), 12);
    QCOMPARE(badge.accessibleName(), QString("1 to review, 12 to do"));
    QVERIFY(badge.sizeHint().width() > badge.sizeHint().height());
    badge.setCounts(0, 0); QVERIFY(!badge.isVisible());
}

void TestTasksView::rowsWorkFromTheKeyboard() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    QVERIFY(QTest::qWaitForWindowActive(&view));
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_VERIFY(row(view, 1)); auto *taskRow = row(view, 1);
    QCOMPARE(taskRow->focusPolicy(), Qt::TabFocus); QVERIFY(taskRow->accessibleName().startsWith("T-000001"));
    QVERIFY(!action(taskRow, "sendNow")->isVisible());
    // Focus shows the row's actions and keeps them while it moves among them.
    taskRow->setFocus(); QTRY_VERIFY(action(taskRow, "sendNow")->isVisible());
    action(taskRow, "top")->setFocus(); QTest::qWait(20); QVERIFY(action(taskRow, "sendNow")->isVisible());
    taskRow->setFocus(); QTest::keyClick(taskRow, Qt::Key_Return);
    QTRY_VERIFY(FakeTasksCli::calls(dir.path()).contains(QStringList({"task", "get", "uuid-1", "--project", "project-1"})));
}

void TestTasksView::leavingAProjectClearsItsCounts() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "review")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    QSignalSpy counts(&view, &TasksView::countsChanged);
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_VERIFY(!counts.isEmpty() && counts.last().value(1).toInt() == 1);
    view.setContext({}, {}, "", "codex/api", "\ncodex/api", sessions());
    QCOMPARE(counts.last().value(0).toInt(), 0); QCOMPARE(counts.last().value(1).toInt(), 0);
}

void TestTasksView::aFailedListingBacksOffAndClearsItsCounts() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setRefreshIntervals(100, 3000); QSignalSpy counts(&view, &TasksView::countsChanged);
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    QTRY_COMPARE(tile(view, "todo")->property("count").toInt(), 1);
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("list"); fail.close();
    QTRY_VERIFY(view.findChild<QLabel *>("tasksError")->isVisible());
    // The counts of a project that cannot be read are not shown as current.
    QCOMPARE(tile(view, "todo")->property("count").toInt(), 0);
    QCOMPARE(counts.last().value(0).toInt(), 0); QCOMPARE(counts.last().value(1).toInt(), 0);
    // A refusal is retried at the slow pace, not every 100 ms.
    const auto lists = [&] { int n = 0; for (const auto &call : FakeTasksCli::calls(dir.path())) n += call.value(1) == "list"; return n; };
    const int failing = lists(); QTest::qWait(700); QVERIFY2(lists() - failing <= 1, qPrintable(QString::number(lists() - failing)));
}

void TestTasksView::unreadableTaskFilesAreNamed() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}, 2));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView view(&client); view.resize(760, 560); view.show();
    view.setContext("project-1", "orchard", "", "codex/api", "\ncodex/api", sessions());
    auto *damaged = view.findChild<QLabel *>("tasksDamaged");
    QTRY_VERIFY(damaged->isVisible()); QCOMPARE(damaged->text(), QString("2 task files of orchard could not be read."));
}

void TestTasksView::aRowOpensOnlyOnAClickOnItself() {
    QWidget host; auto *taskRow = new TaskRow(&host); host.resize(600, 80); taskRow->resize(600, 80); host.show();
    taskRow->setTask(TaskBoard::item(FakeTasksCli::task(1, "todo")), {}, TaskColors::of(true), 0);
    QSignalSpy opened(taskRow, &TaskRow::opened);
    QTest::mouseRelease(taskRow, Qt::LeftButton, {}, QPoint(100, 50)); QCOMPARE(opened.size(), 0);   // pressed elsewhere
    auto *actions = taskRow->findChild<QWidget *>("taskActions");
    const QPoint gap = actions->mapTo(taskRow, QPoint(actions->width() - 1, actions->height() / 2));
    QTest::mouseClick(taskRow, Qt::LeftButton, {}, gap); QCOMPARE(opened.size(), 0);                // among the actions
    QTest::mouseClick(taskRow, Qt::LeftButton, {}, QPoint(100, 50)); QCOMPARE(opened.size(), 1);
}

QTEST_MAIN(TestTasksView)
#include "test_tasksview.moc"
