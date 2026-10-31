#include "FakeTasksCli.h"
#include "Settings.h"
#include "ActivityView.h"
#include "TaskDialogs.h"
#include <QSplitter>
#include <QInputDialog>
#include <QScrollBar>
#include <QScopeGuard>
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
#include <QUuid>
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
    void namedCardsOpenCentralViewAndRecoverTheOriginalIdentity();
    void taskDocumentAndDiscussionUseActivityComposition();
    void rowsAreNotSetUpAgainWhenOnlyAnotherSessionChanges();
    void taskTitleRenamesInPlaceAndKeepsRefusedEdits();
    void newCommentRecoversPreviousWithoutResending();
    void slowDetailsPublishDuringBoardRefreshesAndRejectOldSelections();
    void firstActiveFilterCombinesReviewAndWorkWithoutChangingSelection();
    void mutationAfterCapturedListAlwaysGetsAFreshReadWithoutPolling();
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

void TestTasksView::namedCardsOpenCentralViewAndRecoverTheOriginalIdentity() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path()));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); TaskInspector inspector(&client, &board);
    QWidget center; QVBoxLayout layout(&center); auto *taskView = inspector.takeDetailsPane(); layout.addWidget(taskView);
    center.resize(750, 700); center.show(); inspector.resize(300, 700); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions()); inspector.setContext("project-1", "", "codex/api", sessions());
    QSignalSpy opened(&inspector, &TaskInspector::taskViewRequested);
    auto *plus = inspector.findChild<QToolButton *>("taskNewCard"); QVERIFY(plus && plus->isEnabled());
    auto *list = inspector.findChild<QScrollArea *>("taskInspectorList"); const auto listY = list->y();
    plus->click(); QTest::qWait(20); QCOMPARE(list->y(), listY);
    auto *title = inspector.findChild<QLineEdit *>("taskNewTitle"); QVERIFY(title->isVisible()); title->setText("Named requirement");
    QTest::keyClick(title, Qt::Key_Escape); QVERIFY(title->isHidden()); QTest::qWait(20); QCOMPARE(list->y(), listY);
    plus->click(); QCOMPARE(title->text(), QString("Named requirement")); QTest::qWait(20); QCOMPARE(list->y(), listY);
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("add"); fail.close();
    QTest::keyClick(title, Qt::Key_Return);
    auto *error = inspector.findChild<QLabel *>("taskCreateError"); QTRY_VERIFY(error->isVisible()); QVERIFY(title->isReadOnly());
    QJsonObject intent = QJsonDocument::fromJson(Settings::value("tasks/newCard/project-1/request").toByteArray()).object();
    const auto identity = intent.value("request_id").toString(); QVERIFY(!QUuid(identity).isNull());
    QVERIFY(fail.remove()); QTest::keyClick(title, Qt::Key_Return);
    QTRY_COMPARE(inspector.selectedTask(), identity); QTRY_VERIFY(!opened.isEmpty());
    QTRY_COMPARE(taskView->findChild<QLabel *>("taskViewTitle")->text(), QString("Named requirement"));
    QVERIFY(!inspector.isAncestorOf(taskView)); QCOMPARE(inspector.findChild<QSplitter *>("taskInspectorSplit")->count(), 1);
    auto *input = inspector.composer(); QTRY_VERIFY(input->sessionKey().startsWith("task-description\n"));
    QTRY_VERIFY(!input->isDraftLoading(input->sessionKey())); input->editor()->setPlainText("Detailed content");
    auto *start = taskView->findChild<QPushButton *>("taskInspectorAction:sendNow"); QTRY_VERIFY(!start->isEnabled());
    QVERIFY(taskView->findChild<QLabel *>("taskStartReason")->text().contains("Save the description"));
    input->findChild<QPushButton *>("sendMessage")->click(); QTRY_VERIFY(input->sessionKey().startsWith("task-comment\n"));
    QTRY_VERIFY(taskView->findChild<QTextBrowser *>("taskInspectorDetail")->toPlainText().contains("Detailed content"));
    QCOMPARE(taskView->findChild<QLabel *>("taskViewTitle")->text(), QString("Named requirement"));
    QVERIFY(Settings::value("tasks/newCard/project-1/request").toByteArray().isEmpty());
    QFile saved(dir.filePath("tasks.json")); QVERIFY(saved.open(QIODevice::ReadOnly)); const auto state = QJsonDocument::fromJson(saved.readAll()).object(); QCOMPARE(state.value("tasks").toArray().size(), 1);
    int creates = 0;
    for (const auto &call : FakeTasksCli::calls(dir.path())) {
        QVERIFY(call.value(1) != "send"); QVERIFY(call.value(0) != "new");
        if (call.value(1) == "add") { ++creates; QCOMPARE(call.value(call.indexOf("--request-id") + 1), identity); }
    }
    QCOMPARE(creates, 2);
    auto *tabs = taskView->findChild<QTabBar *>("taskDetailTabs"); auto *footer = taskView->findChild<QWidget *>("taskInspectorActions");
    QTest::qWait(20); const int footerBottom = taskView->height() - footer->mapTo(taskView, QPoint()).y();
    for (int index : {1, 2, 0}) { tabs->setCurrentIndex(index); QTest::qWait(20); QCOMPARE(taskView->height() - footer->mapTo(taskView, QPoint()).y(), footerBottom); }
    QTRY_VERIFY(taskView->findChild<QTextBrowser *>("taskInspectorDetail")->toPlainText().contains("Detailed content"));
    const auto preview = qEnvironmentVariable("HGS_TASK_VIEW_PREVIEW"); if (!preview.isEmpty()) QVERIFY(center.grab().save(preview));
}

void TestTasksView::taskTitleRenamesInPlaceAndKeepsRefusedEdits() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo", {{"title", "Original title"}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(700, 850); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions()); QTRY_COMPARE(inspector.displayedCount(), 1); inspector.selectTask("uuid-1");
    auto *title = inspector.findChild<QLabel *>("taskViewTitle"); auto *editor = inspector.findChild<QPlainTextEdit *>("taskTitleEditor");
    QTRY_COMPARE(title->text(), QString("Original title"));
    QTest::mouseClick(title, Qt::LeftButton); QVERIFY(editor->isVisible()); QVERIFY(!title->isVisible());
    QVERIFY(inspector.findChildren<QInputDialog *>().isEmpty());
    editor->setPlainText("A longer task title edited in the actual heading"); QTest::keyClick(editor, Qt::Key_Return);
    QTRY_VERIFY(editor->isHidden()); QTRY_COMPARE(title->text(), QString("A longer task title edited in the actual heading"));
    QTest::mouseClick(title, Qt::LeftButton); editor->setPlainText("Discard this edit"); QTest::keyClick(editor, Qt::Key_Escape);
    QVERIFY(editor->isHidden()); QVERIFY(!title->text().contains("Discard"));
    board.actOnTask(board.board().tasks.first(), TaskAction::Rename); QTRY_VERIFY(editor->isVisible());
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("rename"); fail.close();
    editor->setPlainText("Keep a refused edit"); QTest::keyClick(editor, Qt::Key_Return);
    QTRY_VERIFY(inspector.findChild<QLabel *>("taskTitleError")->isVisible()); QCOMPARE(editor->toPlainText(), QString("Keep a refused edit")); QVERIFY(!editor->isReadOnly());
    QVERIFY(fail.remove()); QTest::keyClick(editor, Qt::Key_Return);
    QTRY_VERIFY(editor->isHidden()); QTRY_COMPARE(title->text(), QString("Keep a refused edit"));
}

void TestTasksView::newCommentRecoversPreviousWithoutResending() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); board.setPolling(false);
    TaskInspector inspector(&client, &board); inspector.resize(700, 850); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions()); inspector.setContext("project-1", "", "codex/api", sessions());
    QTRY_COMPARE(inspector.displayedCount(), 1); inspector.selectTask("uuid-1");
    auto *composer = inspector.composer(); QTRY_VERIFY(!composer->sessionKey().isEmpty() && !composer->isDraftLoading(composer->sessionKey()));
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("message"); fail.close();
    composer->editor()->setPlainText("Previous comment with a lost response"); composer->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(inspector.findChild<QLabel *>("taskInspectorNoteError")->isVisible()); QVERIFY(fail.remove());
    // Hold a captured old detail response across recovery and the new write.
    QFile gate(dir.filePath("get-gate")); QVERIFY(gate.open(QIODevice::WriteOnly)); gate.close();
    const auto releaseGate=qScopeGuard([&] { gate.remove(); });
    board.refresh(); QTRY_VERIFY(QFileInfo::exists(dir.filePath("get-snapshot")));
    composer->editor()->setPlainText("A new ordinary comment"); composer->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(composer->editor()->toPlainText().isEmpty());
    QFile state(dir.filePath("tasks.json")); QVERIFY(state.open(QIODevice::ReadOnly));
    const auto persisted=QJsonDocument::fromJson(state.readAll()).object().value("tasks").toArray().first().toObject().value("log").toArray();
    QStringList texts; for (const auto &entry:persisted) if(entry.toObject().value("kind")=="message") texts<<entry.toObject().value("text").toString();
    QCOMPARE(texts,QStringList({"Previous comment with a lost response","A new ordinary comment"}));
    QVERIFY(gate.remove());
    auto *discussion = inspector.findChild<QTextBrowser *>("taskInspectorDetail");
    QTRY_VERIFY(discussion->toPlainText().contains("Previous comment with a lost response") && discussion->toPlainText().contains("A new ordinary comment"));
    const auto calls = FakeTasksCli::calls(dir.path()); QList<QStringList> messages;
    for (const auto &call : calls) if (call.value(1) == "message") messages << call;
    QCOMPARE(messages.size(), 3); QVERIFY(messages[1].contains("--recover-only"));
    const auto id = [](const QStringList &call) { return call.value(call.indexOf("--request-id") + 1); };
    QCOMPARE(id(messages[0]), id(messages[1])); QVERIFY(id(messages[2]) != id(messages[0])); QVERIFY(!messages[2].contains("--recover-only"));
}

void TestTasksView::slowDetailsPublishDuringBoardRefreshesAndRejectOldSelections() {
    QTemporaryDir dir; auto one=FakeTasksCli::task(1,"todo"),two=FakeTasksCli::task(2,"todo");
    one["title"]="First snapshot"; two["title"]="Other task";
    QVERIFY(FakeTasksCli::install(dir.path(),{one,two}));
    const auto gate=dir.filePath("gated-gets"); QVERIFY(FakeCli::replace(gate,"gate"));
    const auto releaseAll=qScopeGuard([&] { QFile::remove(gate); });
    const auto started=[&](int n) { return QFileInfo::exists(dir.filePath(QString("get-started-%1").arg(n))); };
    const auto release=[&](int n) { return FakeCli::replace(dir.filePath(QString("get-release-%1").arg(n)),"release"); };
    const auto retitle=[&](const QString &title) {
        QFile file(dir.filePath("tasks.json")); if(!file.open(QIODevice::ReadOnly))return false;
        auto data=QJsonDocument::fromJson(file.readAll()).object(); file.close();
        auto tasks=data.value("tasks").toArray(); auto task=tasks[0].toObject(); task["title"]=title; tasks[0]=task; data["tasks"]=tasks;
        return FakeCli::replace(dir.filePath("tasks.json"),QJsonDocument(data).toJson());
    };
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); board.setPolling(false);
    TaskInspector inspector(&client,&board); inspector.resize(700,850); inspector.show();
    board.setContext("project-1","orchard","","codex/api","key",sessions()); inspector.setContext("project-1","","codex/api",sessions());
    QTRY_COMPARE(inspector.displayedCount(),2); inspector.selectTask("uuid-1"); QTRY_VERIFY(started(1));
    auto *title=inspector.findChild<QLabel *>("taskViewTitle"); QVERIFY(title);
    QSignalSpy updates(&board,&TasksView::boardChanged);
    // An unchanged list arrives before every slow detail reply. Each valid
    // same-selection snapshot must still publish while its next read is held.
    board.refresh(); QTRY_VERIFY(!updates.isEmpty()); QVERIFY(retitle("Second snapshot"));
    QVERIFY(release(1)); QTRY_VERIFY(started(2)); QTRY_COMPARE(title->text(),QString("First snapshot"));
    const int before=updates.size(); board.refresh(); QTRY_VERIFY(updates.size()>before); QVERIFY(retitle("Old selection must not publish"));
    QVERIFY(release(2)); QTRY_VERIFY(started(3)); QTRY_COMPARE(title->text(),QString("Second snapshot"));
    // Progress does not authorize a late reply for a different selection.
    inspector.selectTask("uuid-2"); QVERIFY(release(3)); QTRY_VERIFY(started(4));
    QCOMPARE(title->text(),QString("Second snapshot"));
    QVERIFY(release(4)); QTRY_COMPARE(title->text(),QString("Other task"));
    int reads=0; for(const auto &call:FakeTasksCli::calls(dir.path())) { QVERIFY(call.value(1)=="list" || call.value(1)=="get"); reads+=call.value(1)=="get"; }
    QCOMPARE(reads,4);
}

void TestTasksView::mutationAfterCapturedListAlwaysGetsAFreshReadWithoutPolling() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); board.resize(760, 600); board.show(); board.setPolling(false);
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    QTRY_VERIFY(row(board, 1) && row(board, 2)); QVERIFY(row(board, 1)->y() < row(board, 2)->y());
    QFile delay(dir.filePath("delay-list")); QVERIFY(delay.open(QIODevice::WriteOnly)); delay.write(".5"); delay.close();
    board.refresh(); QTRY_VERIFY(QFileInfo::exists(dir.filePath("list-snapshot")));
    board.actOnTask(TaskBoard::item(FakeTasksCli::task(2, "todo")), TaskAction::Top);
    QTRY_VERIFY(row(board, 1) && row(board, 2) && row(board, 2)->y() < row(board, 1)->y());
    int reads = 0, writes = 0;
    for (const auto &call : FakeTasksCli::calls(dir.path())) { reads += call.value(1) == "list"; writes += call.value(1) == "move"; }
    QCOMPARE(reads, 3); QCOMPARE(writes, 1);
}

void TestTasksView::firstActiveFilterCombinesReviewAndWorkWithoutChangingSelection() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "review"), FakeTasksCli::task(2, "active"), FakeTasksCli::task(3, "todo")}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(700, 850); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions()); inspector.setContext("project-1", "", "codex/api", sessions());
    auto *filter = inspector.findChild<QAbstractButton *>("taskTile:current"); QVERIFY(filter);
    QTRY_COMPARE(filter->property("count").toInt(), 2); QVERIFY(filter->isChecked()); QCOMPARE(inspector.displayedCount(), 2);
    auto *review = inspector.findChild<TaskRow *>("taskInspectorRow:T-000001"); auto *work = inspector.findChild<TaskRow *>("taskInspectorRow:T-000002");
    QVERIFY(review && work); QVERIFY(review->findChild<QLabel *>("taskMeta")->text().contains("In review")); QVERIFY(work->findChild<QLabel *>("taskMeta")->text().contains("In progress"));
    inspector.selectTask("uuid-1"); QTRY_COMPARE(inspector.selectedTask(), QString("uuid-1")); QCOMPARE(inspector.selectedStage(), QString("current")); QCOMPARE(inspector.displayedCount(), 2);
    QVERIFY(review->findChild<QToolButton *>("taskAction:accept")->property("blockedReason").toString().isEmpty());
    QVERIFY(!work->findChild<QToolButton *>("taskAction:pause")->property("blockedReason").toString().isEmpty());
    inspector.findChild<QAbstractButton *>("taskTile:review")->click(); QCOMPARE(inspector.displayedCount(), 1);
    filter->click(); QCOMPARE(inspector.displayedCount(), 2);
}

void TestTasksView::taskDocumentAndDiscussionUseActivityComposition() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo", {{"title", "A clear task title"}, {"text", "A **canonical** document."}})}));
    // The board's listing arrives after the task's own read, as on a loaded runner.
    { QFile delay(dir.filePath("delay-list")); QVERIFY(delay.open(QIODevice::WriteOnly)); delay.write("0.5"); }
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client); TaskInspector inspector(&client, &board);
    QWidget center; auto *layout = new QVBoxLayout(&center); layout->setContentsMargins(0, 0, 0, 0);
    auto *pane = inspector.takeDetailsPane(); layout->addWidget(pane); center.resize(1100, 780); center.show();
    inspector.setColumnWidth(800); board.setContext("project-1", "orchard", "", "codex/api", "key", sessions()); inspector.setContext("project-1", "", "codex/api", sessions());
    inspector.openTask("uuid-1"); auto *composer = inspector.composer(); auto *detail = pane->findChild<QTextBrowser *>("taskInspectorDetail");
    QTRY_VERIFY(detail->toPlainText().contains("canonical"));
    QVERIFY(!detail->toPlainText().contains("You · Description")); QVERIFY(detail->toPlainText().contains("Discussion"));
    QVERIFY(composer->property("messageComposer").toBool()); QVERIFY(composer->width() <= 800);
    QVERIFY(!composer->isAncestorOf(pane->findChild<QWidget *>("taskInspectorActions")));
    MessageComposer activity; activity.resize(800, 400); activity.show(); activity.setTheme(true);
    QCOMPARE(composer->editor()->height(), activity.editor()->height());
    // A comment is saved only for the board's loaded project (zerus-fexl).
    QTRY_COMPARE(board.board().projectId, QStringLiteral("project-1"));
    composer->editor()->setPlainText("A clarification before Start"); composer->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(detail->toPlainText().contains("A clarification before Start")); QTRY_VERIFY(detail->toPlainText().contains("Waiting for Start"));
    QCOMPARE(pane->findChild<QTabBar *>("taskDetailTabs")->currentIndex(), 0);
    auto *edit = pane->findChild<QPushButton *>("taskInspectorEdit"); edit->click(); QTRY_VERIFY(composer->sessionKey().startsWith("task-description"));
    QTRY_COMPARE(composer->editor()->toPlainText(), QString("A **canonical** document."));
    composer->editor()->setPlainText("An unsaved revision"); pane->findChild<QPushButton *>("taskDescriptionCancel")->click();
    QTRY_VERIFY(composer->sessionKey().startsWith("task-comment")); QVERIFY(detail->toPlainText().contains("canonical"));
    QVERIFY(!detail->toPlainText().contains("unsaved revision"));
    const auto titleTop = pane->findChild<QLabel *>("taskViewTitle")->mapTo(pane, QPoint()).y();
    QVERIFY2(titleTop >= 4, qPrintable(QString("Task heading top %1, pane height %2").arg(titleTop).arg(pane->height())));
    const auto preview = qEnvironmentVariable("HGS_TASK_THREAD_PREVIEW"); if (!preview.isEmpty()) { QTest::qWait(80); QVERIFY(center.grab().save(preview)); }
}

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
    auto *input = inspector.composer();
    QTRY_VERIFY(input->sessionKey().startsWith("task-description\n") && input->sessionKey().endsWith("uuid-1"));
    QTRY_COMPARE(input->editor()->toPlainText(), QString("Task 1"));
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
    QCOMPARE(inspector.findChild<QTabBar *>("taskDetailTabs")->currentIndex(), 0);
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail"); QTRY_VERIFY(detail->toPlainText().contains("Review this screenshot"));
    QTRY_VERIFY(detail->toPlainText().contains("screenshot.png"));
    bool media = false; for (const auto &call : FakeTasksCli::calls(dir.path())) if (call.value(1) == "message" && call.contains("--content-json")) media = true;
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
    QVERIFY(!inspector.findChild<TaskForm *>()); QVERIFY(inspector.findChildren<TaskDialog *>().isEmpty());
    auto *composer = inspector.composer();
    QTRY_VERIFY(composer->sessionKey().startsWith("task-description\n"));
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    auto *prompt = composer->editor(); prompt->setPlainText("Revised requirement");
    QFile fail(dir.filePath("fail")); QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("edit"); fail.close();
    composer->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(inspector.findChild<QLabel *>("taskInspectorNoteError")->isVisibleTo(&inspector)); QCOMPARE(prompt->toPlainText(), QString("Revised requirement"));
    QVERIFY(fail.remove()); composer->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(composer->sessionKey().startsWith("task-comment\n"));
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail"); QTRY_VERIFY(detail->toPlainText().contains("Revised requirement"));
    for (const auto &call : FakeTasksCli::calls(dir.path())) QVERIFY(call.value(1) != "target");
    auto *tabs = inspector.findChild<QTabBar *>("taskDetailTabs"); auto *footer = inspector.findChild<QWidget *>("taskInspectorActions");
    auto *send = composer->findChild<QPushButton *>("sendMessage");
    for (const int tab : {1, 2, 0}) {
        tabs->setCurrentIndex(tab); QTest::qWait(20);
        QVERIFY(!composer->isAncestorOf(footer)); QVERIFY(footer->mapTo(&inspector, QPoint()).y() < composer->mapTo(&inspector, QPoint()).y());
    }
    auto *notes = inspector.findChild<MessageComposer *>("taskCommentComposer"); QVERIFY(notes);
    auto *note = notes->editor(); note->setPlainText("Keep this note");
    QVERIFY(fail.open(QIODevice::WriteOnly)); fail.write("message"); fail.close();
    notes->findChild<QPushButton *>("sendMessage")->click();
    QTRY_VERIFY(inspector.findChild<QLabel *>("taskInspectorNoteError")->isVisibleTo(&inspector)); QCOMPARE(note->toPlainText(), QString("Keep this note"));
    QVERIFY(fail.remove()); notes->findChild<QPushButton *>("sendMessage")->click(); QTRY_VERIFY(note->toPlainText().isEmpty());
    QCOMPARE(tabs->currentIndex(), 0); QTRY_VERIFY(detail->toPlainText().contains("Keep this note"));
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
    QTRY_COMPARE(inspector.displayedCount(), 2);
    inspector.selectTask("uuid-1");
    auto *detail = inspector.findChild<QTextBrowser *>("taskInspectorDetail");
    QTRY_VERIFY(detail->toPlainText().contains("Keep <script> literal"));
    QVERIFY(detail->toPlainText().contains("Committed <abc123>"));
    auto *sections = inspector.findChild<QTabBar *>("taskDetailTabs"); sections->setCurrentIndex(1);
    QTRY_VERIFY(detail->toPlainText().contains("Committed <abc123>; installation pending.") && !detail->toPlainText().contains("Keep <script> literal"));
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
    QCOMPARE(sections->currentIndex(), 0); QVERIFY(detail->toPlainText().contains("REPORT_BODY_1"));
    QVERIFY(detail->toPlainText().contains("HISTORY_BODY"));
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
    QTRY_VERIFY(activity->plainText().contains("COMMENT_18_END") && !activity->plainText().contains("COMMENT_1_END"));
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

void TestTasksView::rowsAreNotSetUpAgainWhenOnlyAnotherSessionChanges() {
    // The window passes the task context on every session update (zerus-604d.13).
    // In review rows have no animation, so a paint means the row was set up again.
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {
        FakeTasksCli::task(1, "review", {{"session", FakeTasksCli::pinned("codex/api")}}),
        FakeTasksCli::task(2, "review", {{"session", FakeTasksCli::pinned("codex/web")}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(400, 700); inspector.show();
    const auto setContext = [&](const QString &session, const QList<TaskSession> &list) {
        board.setContext("project-1", "orchard", "", session, "key", list); inspector.setContext("project-1", "", session, list);
    };
    setContext("codex/api", sessions());
    QTRY_COMPARE(inspector.displayedCount(), 2);
    auto *api = inspector.findChild<TaskRow *>("taskInspectorRow:T-000001");
    auto *web = inspector.findChild<TaskRow *>("taskInspectorRow:T-000002"); QVERIFY(api && web);
    struct Paints : QObject {
        int count = 0;
        bool eventFilter(QObject *, QEvent *event) override { if (event->type() == QEvent::Paint) ++count; return false; }
    } apiPaints, webPaints;
    api->installEventFilter(&apiPaints); web->installEventFilter(&webPaints);
    QTest::qWait(150); apiPaints.count = webPaints.count = 0;
    // Other sessions change their status: no row is set up or drawn again.
    auto changed = sessions(); changed[1].status = QStringLiteral("Compacting"); changed[3].status = QStringLiteral("Paused");
    setContext("codex/api", changed);
    QTest::qWait(150);
    QCOMPARE(apiPaints.count, 0); QCOMPARE(webPaints.count, 0);
    // Opening the other session changes which task is its own: both rows change.
    setContext("codex/web", changed);
    QTRY_VERIFY(apiPaints.count > 0 && webPaints.count > 0);
}

void TestTasksView::inspectorKeepsSelectionInAFixedLowerPane() {
    QTemporaryDir dir; QVERIFY(FakeTasksCli::install(dir.path(), {FakeTasksCli::task(1, "todo"), FakeTasksCli::task(2, "todo"),
        FakeTasksCli::task(3, "active", {{"session", FakeTasksCli::pinned("codex/api")}, {"stage_at", now()}})}));
    HgsClient client(FakeCli::program(dir.filePath("hgs"))); TasksView board(&client);
    TaskInspector inspector(&client, &board); inspector.resize(380, 760); inspector.show();
    board.setContext("project-1", "orchard", "", "codex/api", "key", sessions());
    inspector.setContext("project-1", "", "codex/api", sessions());
    auto *todo = inspector.findChild<QAbstractButton *>("taskTile:todo");
    QTRY_COMPARE(inspector.displayedCount(), 1); todo->click(); QTRY_COMPARE(inspector.displayedCount(), 2);
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
    QVERIFY(Settings::value("tasks/onlySession").toBool());
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
