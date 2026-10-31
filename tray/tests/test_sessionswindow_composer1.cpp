// U62 (#712): Activity and the composer: drafts, delivery, recovery and cold caches, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void workspaceRestoresDraftAcrossRestartAndSessionRemoval();
    void savedDraftsGoToTheirSession_data();
    void savedDraftsGoToTheirSession();
    void independentWaitRemainsVisibleWithoutExpandingHeader_data();
    void independentWaitRemainsVisibleWithoutExpandingHeader();
    void activityReadingColumnFollowsSettingsAndEdges();
    void activitySurfaceRemainsOpaque_data();
    void activitySurfaceRemainsOpaque();
    void backgroundPollKeepsActivityAndComposerStable();
    void recoveryCountdownKeepsHistoryDraftAndFocus();
    void recoveryEndingReturnsKeyboardFocusToField();
    void resumedRecoveryLeavesTheWorkingTurnUnmarked();
    void durableSubmissionKeepsGuiResponsive_data();
    void durableSubmissionKeepsGuiResponsive();
    void coldCacheConfirmationPreservesDraftAndPinsConversation();
    void coldCacheDialogRefreshedIdentityStillSends();
    void stoppedDshSessionStartsAndSends();
};

void TestSessionsWindow::workspaceRestoresDraftAcrossRestartAndSessionRemoval()
{
    const auto state = fleet();
    {
        SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, "codex/hgs/dashboard");
        auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
        setComposerText(composer, "Keep my work after the agent exits");
        QVERIFY(composer->addAttachment("notes.txt", "text/plain", "notes snapshot"));
    }
    SessionsWindow restored(program()); restored.setFleet(state); restored.show(); restored.showSession({}, "codex/hgs/dashboard");
    auto *composer = restored.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    QTRY_COMPARE(composer->editor()->toPlainText(), "[File #1] Keep my work after the agent exits");
    restored.setFleet(FleetState());
    QTimer::singleShot(0, &restored, [&] {
        auto *dialog = restored.findChild<QDialog *>("savedDraftsDialog"); QVERIFY(dialog); QVERIFY(dialog->isVisible());
        auto *list = dialog->findChild<QListWidget *>("savedDraftList"); QTRY_COMPARE(list->count(), 1);
        QCOMPARE(dialog->findChild<QPlainTextEdit *>("savedDraftPreview")->toPlainText(), "[File #1] Keep my work after the agent exits");
        QVERIFY(!dialog->findChild<QPushButton *>("restoreSavedDraft")->isEnabled()); dialog->reject();
    });
    restored.findChild<QPushButton *>("savedDrafts")->click();
}

void TestSessionsWindow::savedDraftsGoToTheirSession_data()
{
    QTest::addColumn<QString>("theme");
    QTest::addColumn<bool>("doubleClick");
    QTest::newRow("dark") << QString("dark") << false;
    QTest::newRow("light") << QString("light") << false;
    QTest::newRow("dark-double-click") << QString("dark") << true;
    QTest::newRow("light-double-click") << QString("light") << true;
}

void TestSessionsWindow::savedDraftsGoToTheirSession()
{
    QFETCH(QString, theme); QFETCH(bool, doubleClick); QSettings().setValue("workspace/theme", theme);
    auto state = fleet(); auto box = state.local();
    box.sessions[1].conversationId = "conversation-one"; box.sessions[1].subagentSource = "hooks";
    box.sessions[1].subagents = {{"worker", QJsonObject{{"name", "Worker"}, {"state", "working"}}}};
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    const QString remote = "mac\nclaude/infra/review", child = "arch\nkimi/docs/research\nconversation-one\nworker", ended = "mac\nclaude/infra/ended";
    ComposerDraftStore store;
    for (const auto &[key, text] : {std::pair{remote, QString("Check the plan first")}, std::pair{child, QString("Ask the worker")},
                                    std::pair{ended, QString("From an ended session")}}) {
        ComposerDraft draft; draft.text = text; QVERIFY(store.save(key, draft));
    }
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    auto *childComposer = window.findChild<MessageComposer *>("subagentComposer"); QVERIFY(childComposer);
    const auto choose = [&](const QString &key, bool available) {
        QTimer::singleShot(0, &window, [&, key, available] {
            auto *dialog = window.findChild<QDialog *>("savedDraftsDialog"); QVERIFY(dialog);
            auto *list = dialog->findChild<QListWidget *>("savedDraftList"); QTRY_COMPARE(list->count(), 3);
            QVERIFY(dynamic_cast<SessionDelegate *>(list->itemDelegate()));
            for (int i = 0; i < list->count(); ++i) {
                const auto *item = list->item(i);
                QVERIFY(!item->text().section('\n', 0, 0).trimmed().isEmpty());
                QVERIFY(list->visualItemRect(item).height() >= 2 * list->fontMetrics().height());
                QVERIFY(item->toolTip().isEmpty());QVERIFY(!item->text().contains(QChar(0x00b7)));
                QVERIFY(!item->data(SessionRoles::ModifiedAt).toString().isEmpty());
                if(item->data(Qt::UserRole).toString()==remote){
                    QCOMPARE(item->data(SessionRoles::Title).toString(),QString("review"));
                    QCOMPARE(item->data(SessionRoles::Agent).toString(),QString("claude"));
                    QCOMPARE(item->data(SessionRoles::Host).toString(),QString("mac"));
                    QCOMPARE(item->data(SessionRoles::Model).toString(),state.peer("mac")->sessions[0].model);
                }
            }
            list->setFocus(); list->setCurrentRow(0);
            QTest::keyClick(list, Qt::Key_Down); QCOMPARE(list->currentRow(), 1);
            QTest::keyClick(list, Qt::Key_Up); QCOMPARE(list->currentRow(), 0);
            for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(Qt::UserRole).toString() == key) list->setCurrentRow(i);
            QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(), key);
            auto *open = dialog->findChild<QPushButton *>("openSavedDraftSession"); QCOMPARE(open->isEnabled(), available);
            const auto preview = qEnvironmentVariable("HGS_SAVED_DRAFTS_PREVIEW");
            if (key == ended && !preview.isEmpty()) {
                dialog->setWindowTitle("Saved drafts (synthetic preview)");
                QTRY_COMPARE(dialog->findChild<QPlainTextEdit *>("savedDraftPreview")->toPlainText(), QString("From an ended session"));
                list->scrollToItem(list->currentItem());
                QDir().mkpath(preview); QTest::qWait(50);
                QVERIFY(dialog->grab().save(preview + "/drafts-" + theme + ".png"));
                dialog->resize(620, 460); list->scrollToItem(list->currentItem()); QTest::qWait(50);
                QVERIFY(dialog->grab().save(preview + "/drafts-narrow-" + theme + ".png"));
            }
            if (doubleClick) {
                list->scrollToItem(list->currentItem());
                const auto point = list->visualItemRect(list->currentItem()).center();
                QTest::mouseClick(list->viewport(), Qt::LeftButton, Qt::NoModifier, point);
                QTest::mouseDClick(list->viewport(), Qt::LeftButton, Qt::NoModifier, point);
                QCOMPARE(dialog->isVisible(), !available);
                if (!available) dialog->reject();
            } else if (available) open->click(); else dialog->reject();
        });
        window.findChild<QPushButton *>("savedDrafts")->click();
    };
    choose(ended, false); QCOMPARE(composer->sessionKey(), QString("\ncodex/hgs/dashboard"));
    choose(remote, true); QCOMPARE(composer->sessionKey(), remote);
    QTRY_COMPARE(composer->editor()->toPlainText(), QString("Check the plan first"));
    // A subagent key names this computer; Go to opens the subagent in its session.
    choose(child, true); QCOMPARE(composer->sessionKey(), QString("\nkimi/docs/research"));
    QCOMPARE(childComposer->sessionKey(), child); QTRY_COMPARE(childComposer->editor()->toPlainText(), QString("Ask the worker"));
    QCOMPARE(window.findChild<SessionList *>("sessionList")->currentItem()->data(SessionRoles::ChildId).toString(), QString("worker"));
    choose(remote, true); QCOMPARE(composer->sessionKey(), remote);
    QVERIFY(window.findChild<SessionList *>("sessionList")->currentItem()->data(SessionRoles::ChildId).toString().isEmpty());
}

void TestSessionsWindow::independentWaitRemainsVisibleWithoutExpandingHeader_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark")<<QString("dark"); QTest::newRow("light")<<QString("light");
}

void TestSessionsWindow::independentWaitRemainsVisibleWithoutExpandingHeader()
{
    QFETCH(QString,theme); QSettings().setValue("workspace/theme",theme);
    auto state=fleet(); auto box=state.local(); auto &s=box.sessions[0];
    s.tracked=true; s.activity="idle"; s.phase="idle"; s.runId="waiting-run";s.conversationId="waiting-conversation";
    s.backgroundProcesses=0;s.subagentActiveCount=0;s.subagentCount=0;
    s.coordinationWait=QJsonObject{{"schema",1},{"status","ok"},{"revision",1},{"run_id",s.runId},{"conversation_id",s.conversationId},{"count",1},
        {"items",QJsonArray{QJsonObject{{"state","awaiting_result"},{"created_at",QDateTime::currentSecsSinceEpoch()-300},
            {"producer",QJsonObject{{"name","claude/project/worker-"+QString(100,'w')},{"host","peer"}}},{"label","Review the patch"}}}}};
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.resize(1000,800);window.setFleet(state);window.show();window.showSession({},s.name);
    auto *list=window.findChild<SessionList *>("sessionList");QVERIFY(list->currentItem());
    QVERIFY(list->currentItem()->data(SessionRoles::Waiting).toBool());
    QVERIFY(list->currentItem()->data(SessionRoles::Detail).toString().contains("Review the patch"));
    auto *badge=window.findChild<QLabel *>("badge");QVERIFY(badge);
    QTRY_VERIFY(badge->text().startsWith("Waiting for result ("));
    QVERIFY(!badge->text().contains("worker"));
    QVERIFY(window.width()<=1100);QVERIFY(badge->width()<=300);
    const auto preview=qEnvironmentVariable("ZERUS_WAIT_QA_DIR");
    if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+"/wait-"+theme+".png"));}
    s.coordinationWait["revision"]=2;s.coordinationWait["count"]=0;s.coordinationWait["items"]=QJsonArray{};
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QVERIFY(!list->currentItem()->data(SessionRoles::Waiting).toBool());
}

void TestSessionsWindow::activityReadingColumnFollowsSettingsAndEdges()
{
    SessionsWindow window(program()); window.resize(2200, 900); window.setFleet(fleet()); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    const auto open = [&window] {
        window.showSession({}, "codex/hgs/dashboard");
        const QJsonObject details{{"tracked", true}, {"run_id", "run"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
            {"process_state", "running"}, {"activity", "idle"}, {"phase", "idle"}, {"cursor", 2}, {"events", QJsonArray{
                QJsonObject{{"seq", 1}, {"at", 2000000000}, {"type", "UserPromptSubmit"}, {"detail", "Keep long lines readable in a maximized window."}},
                QJsonObject{{"seq", 2}, {"at", 2000000010}, {"type", "Stop"}, {"detail", "## Done\n\nActivity, questions and the message field now share **one centered column**."}}}}};
        window.findChild<HgsClient *>()->inspectionReady({}, "codex/hgs/dashboard", details);
    };
    const auto preview = [&window, &open](const QString &name) {
        const auto directory = qEnvironmentVariable("HGS_PREVIEW_DIR"); if (directory.isEmpty()) return true;
        open(); QTest::qWait(50); return window.grab().save(directory + "/" + name);
    };
    open();
    auto *activity = window.findChild<ActivityView *>("mainActivity"); QVERIFY(activity);
    auto *viewport = activity->browser()->viewport();
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    const auto left = [&window](QWidget *widget) { return widget->mapTo(&window, QPoint()).x(); };
    // A wide pane centers the transcript and the message field in one column.
    QTRY_COMPARE(viewport->width(), 900);
    const int pane = activity->width(); QVERIFY2(pane > 1500, qPrintable(QString::number(pane)));
    QCOMPARE(left(viewport) - left(activity), (pane - 900) / 2);
    QTRY_COMPARE(composer->width(), 900); QCOMPARE(left(composer), left(viewport));
    // The margins are Activity's own surface in either theme.
    const auto plainMargins = [activity, viewport] {
        const QImage surface = activity->grab().toImage();
        // The document margin inside the column is always its background.
        const QPoint empty = viewport->mapTo(activity, QPoint(1, viewport->height() / 2));
        return surface.pixelColor(20, empty.y()) == surface.pixelColor(empty)
            && surface.pixelColor(activity->width() - 30, empty.y()) == surface.pixelColor(empty);
    };
    QVERIFY(plainMargins());
    QVERIFY(preview("activity-column.png"));
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *settings = window.findChild<QWidget *>("settingsPage"); QVERIFY(settings && settings->isVisible());
    auto *slider = settings->findChild<QSlider *>("workspaceActivityWidth"); QVERIFY(slider && slider->isVisible());
    auto *value = settings->findChild<QLabel *>("workspaceActivityWidthValue"); QVERIFY(value);
    auto *full = settings->findChild<QCheckBox *>("workspaceActivityFullWidth"); QVERIFY(full && !full->isChecked());
    QCOMPARE(slider->minimum(), 480); QCOMPARE(slider->maximum(), 2400); QCOMPARE(slider->value(), 900);
    QCOMPARE(value->text(), QString("900 px"));
    slider->setValue(1100);
    QCOMPARE(QSettings().value("workspace/activityWidth").toInt(), 1100); QCOMPARE(value->text(), QString("1100 px"));
    if (!qEnvironmentVariable("HGS_PREVIEW_DIR").isEmpty())
        QVERIFY(window.grab().save(qEnvironmentVariable("HGS_PREVIEW_DIR") + "/activity-width-settings.png"));
    open();
    QTRY_COMPARE(viewport->width(), 1100); QTRY_COMPARE(composer->width(), 1100); QCOMPARE(left(composer), left(viewport));
    // Enlarged content keeps the same line length.
    window.findChild<QPushButton *>("workspaceSettings")->click();
    settings->findChild<QSlider *>("workspaceContentScale")->setValue(30);
    open();
    QTRY_COMPARE(viewport->width(), 1650); QTRY_COMPARE(composer->width(), 1650);
    window.findChild<QPushButton *>("workspaceSettings")->click();
    settings->findChild<QSlider *>("workspaceContentScale")->setValue(20);
    full->setChecked(true);
    QVERIFY(QSettings().value("workspace/activityFullWidth").toBool()); QVERIFY(!slider->isEnabled());
    open();
    QTRY_COMPARE(composer->width(), pane); QCOMPARE(left(viewport), left(activity));
    QVERIFY(preview("activity-full-width.png"));
    // Activity's own menu returns to the column and Settings follows.
    emit activity->fullWidthRequested(false);
    QTRY_COMPARE(viewport->width(), 1100); QTRY_COMPARE(composer->width(), 1100);
    QVERIFY(!QSettings().value("workspace/activityFullWidth").toBool()); QVERIFY(!full->isChecked()); QVERIFY(slider->isEnabled());
    // Dragging a column edge stores the width for both Activity views.
    auto edges = activity->browser()->findChildren<QWidget *>("activityColumnEdge");
    std::sort(edges.begin(), edges.end(), [](QWidget *a, QWidget *b) { return a->x() < b->x(); });
    QCOMPARE(edges.size(), 2); QVERIFY(edges[1]->isVisible());
    if (!qEnvironmentVariable("HGS_PREVIEW_DIR").isEmpty()) {
        QEnterEvent enter(QPointF(4, 100), edges[1]->mapToGlobal(QPointF(4, 100)), edges[1]->mapToGlobal(QPointF(4, 100)));
        QApplication::sendEvent(edges[1], &enter);
        QVERIFY(window.grab().save(qEnvironmentVariable("HGS_PREVIEW_DIR") + "/activity-column-edge.png"));
        QEvent leave(QEvent::Leave); QApplication::sendEvent(edges[1], &leave);
    }
    const QPoint start = edges[1]->mapToGlobal(QPoint(edges[1]->width() / 2, 100));
    const auto mouse = [](QWidget *widget, QEvent::Type type, const QPoint &global, Qt::MouseButton button, Qt::MouseButtons buttons) {
        QMouseEvent event(type, widget->mapFromGlobal(QPointF(global)), QPointF(global), button, buttons, Qt::NoModifier);
        QApplication::sendEvent(widget, &event);
    };
    mouse(edges[1], QEvent::MouseButtonPress, start, Qt::LeftButton, Qt::LeftButton);
    mouse(edges[1], QEvent::MouseMove, start + QPoint(60, 0), Qt::NoButton, Qt::LeftButton);
    mouse(edges[1], QEvent::MouseMove, start + QPoint(100, 0), Qt::NoButton, Qt::LeftButton);
    mouse(edges[1], QEvent::MouseButtonRelease, start + QPoint(100, 0), Qt::LeftButton, Qt::NoButton);
    QCOMPARE(viewport->width(), 1300); QTRY_COMPARE(composer->width(), 1300); QCOMPARE(left(composer), left(viewport));
    QCOMPARE(QSettings().value("workspace/activityWidth").toInt(), 1300);
    QCOMPARE(slider->value(), 1300); QCOMPARE(value->text(), QString("1300 px"));
    QCOMPARE(window.findChild<ActivityView *>("subagentActivity")->columnWidth(), 1300);
    emit activity->columnResetRequested();
    QTRY_COMPARE(viewport->width(), 900); QCOMPARE(slider->value(), 900);
    QCOMPARE(QSettings().value("workspace/activityWidth").toInt(), 900);
    settings->findChild<QComboBox *>("workspaceTheme")->setCurrentIndex(1);
    open(); QTRY_COMPARE(composer->width(), 900); QVERIFY(plainMargins());
    QVERIFY(preview("activity-column-dark.png"));
    SessionsWindow restored(program()); restored.setFleet(fleet());
    QCOMPARE(restored.findChild<ActivityView *>("mainActivity")->columnWidth(), 900);
    full->setChecked(true);
    SessionsWindow fullWidth(program()); fullWidth.setFleet(fleet());
    QCOMPARE(fullWidth.findChild<ActivityView *>("mainActivity")->columnWidth(), 0);
    QCOMPARE(fullWidth.findChild<ActivityView *>("subagentActivity")->columnWidth(), 0);
}

void TestSessionsWindow::activitySurfaceRemainsOpaque_data()
{
    QTest::addColumn<QString>("theme"); QTest::addColumn<double>("scale");
    for (const QString theme : {QString("dark"), QString("light")})
        for (double scale : {1.0, 1.25})
            QTest::newRow(qPrintable(theme+QString::number(scale))) << theme << scale;
}

void TestSessionsWindow::activitySurfaceRemainsOpaque()
{
    QFETCH(QString,theme); QFETCH(double,scale);
    QSettings().setValue("workspace/theme",theme); QSettings().setValue("workspace/contentScale",scale);
    SessionsWindow window(program()); window.resize(1180,820); window.setFleet(fleet()); window.show();
    window.showSession({},"codex/hgs/dashboard");
    const auto views=window.findChildren<ActivityView *>(); QVERIFY(views.size()>=3);
    QVERIFY(window.findChild<ActivityView *>("taskDetailActivity"));
    auto *compact=window.findChild<QCheckBox *>("workspaceCompact"); QVERIFY(compact);
    for (int pass=0;pass<2;++pass) {
        if (pass) compact->toggle(); // Repolish the workspace without changing its theme.
        for (auto *view : views) {
            auto *browser=view->browser(); browser->ensurePolished(); browser->viewport()->ensurePolished();
            const auto base=browser->viewport()->palette().brush(QPalette::Base);
            QVERIFY2(base.isOpaque(),"Workspace CSS disabled the fast Activity canvas renderer");
            QCOMPARE(base.color(),QColor(theme=="dark"?"#1c2229":"#ffffff"));
            QCOMPARE(base.color(),view->palette().color(QPalette::Window));
        }
    }
}

void TestSessionsWindow::backgroundPollKeepsActivityAndComposerStable()
{
    class StyleCounter : public QObject {
    public:
        int changes = 0;
        bool eventFilter(QObject *, QEvent *event) override {
            if (event->type() == QEvent::StyleChange) ++changes;
            return false;
        }
    } styles;
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspections(client, &HgsClient::inspectionReady);
    window.showSession({}, "codex/hgs/dashboard"); QTRY_VERIFY(!inspections.isEmpty());
    QJsonArray events;
    for (int i = 1; i <= 24; ++i) events.append(QJsonObject{{"seq", i}, {"at", 1791018000 + i},
        {"type", i % 2 ? "UserPromptSubmit" : "Stop"}, {"detail", QString("Persistent message %1\n\nText to keep visible during a background update.").arg(i)}});
    QJsonObject details{{"tracked", true}, {"conversation_id", "conversation-one"}, {"run_id", "run-one"},
        {"activity", "busy"}, {"phase", "working"}, {"last_event_at", 1791018024}, {"cursor", 24}, {"events", events}};
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    auto *activity = window.findChild<ActivityView *>("mainActivity"); QVERIFY(activity); auto *browser = activity->browser();
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    setComposerText(composer, "Unsent draft");
    QTRY_VERIFY(!activity->renderPending());
    auto selection = browser->document()->find("Persistent message 1"); browser->setTextCursor(selection);
    browser->verticalScrollBar()->setValue(0); QTest::qWait(20);
    const auto text = browser->textCursor().selectedText(); QVERIFY(!text.isEmpty());
    const auto geometry = browser->geometry(); const int scroll = browser->verticalScrollBar()->value();
    const int revision = browser->document()->revision();
    window.findChild<QLabel *>("badge")->installEventFilter(&styles);
    composer->findChild<QLabel *>("messageStatus")->installEventFilter(&styles);
    details["events"] = QJsonArray();
    for (int i = 0; i < 4; ++i) {
        window.setConnectionError({}); window.setFleet(fleet());
        details["updated"] = 1791018025 + i; client->inspectionReady({}, "codex/hgs/dashboard", details);
        QCoreApplication::sendPostedEvents(nullptr, QEvent::LayoutRequest);
        QCOMPARE(browser->document()->revision(), revision);
        QCOMPARE(browser->geometry(), geometry); QCOMPARE(browser->verticalScrollBar()->value(), scroll);
        QCOMPARE(browser->textCursor().selectedText(), text); QCOMPARE(composer->editor()->toPlainText(), QString("Unsent draft"));
    }
    QCOMPARE(styles.changes, 0);
    details["cursor"] = 25;
    details["events"] = QJsonArray{QJsonObject{{"seq", 25}, {"at", 1791018025}, {"type", "AgentMessage"}, {"detail", "A new live response"}}};
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    QVERIFY(browser->updatesEnabled()); QTRY_VERIFY(browser->toPlainText().contains("A new live response"));
    QCOMPARE(browser->textCursor().selectedText(), text); QCOMPARE(browser->verticalScrollBar()->value(), scroll);
    QCOMPARE(composer->editor()->toPlainText(), QString("Unsent draft"));
}

void TestSessionsWindow::recoveryCountdownKeepsHistoryDraftAndFocus()
{
    // Background inspections return the injected state: a poll that started
    // before an injection can finish after it (process start-up is slow on Windows).
    QTemporaryDir temp; QVERIFY(temp.isValid());
    const auto fake=temp.filePath("hgs"), inspected=temp.filePath("inspect.json");
    QVERIFY(FakeCli::replace(inspected,R"({"tracked":true,"conversation_id":"conversation-one","events":[],"cursor":0})"));
    QVERIFY(FakeCli::install(fake,"#!/usr/bin/env python3\nimport runpy,sys\nif 'inspect' in sys.argv[1:3]:sys.stdout.write(open("
        +FakeCli::pythonString(inspected)+").read())\nelse:\n sys.argv[0]="+FakeCli::pythonString(script())+";runpy.run_path(sys.argv[0],run_name='__main__')\n"));
    SessionsWindow window(FakeCli::program(fake)); window.resize(1280,900); window.setFleet(fleet()); window.show();
    auto *client=window.findChild<HgsClient *>(); QSignalSpy inspections(client,&HgsClient::inspectionReady);
    const auto inject=[&](const QJsonObject &details){
        QVERIFY(FakeCli::replace(inspected,QJsonDocument(details).toJson()));client->inspectionReady({},"codex/hgs/dashboard",details);};
    window.showSession({},"codex/hgs/dashboard");WAIT_VERIFY(!inspections.isEmpty());
    const auto at=QDateTime::currentMSecsSinceEpoch()/1000.0;
    QJsonObject job{{"id","episode"},{"name","codex/hgs/dashboard"},{"state","waiting"},{"action","continue_message"},
        {"due_at",at+20},{"attempt",0},{"delays",QJsonArray{15,30,60,300}},
        {"history",QJsonArray{QJsonObject{{"at",at},{"state","waiting"},{"attempt",0}}}}};
    QJsonObject details{{"tracked",true},{"conversation_id","conversation-one"},{"run_id","run-one"},
        {"activity","attention"},{"phase","error"},{"last_event_at",at},{"cursor",1},{"recovery",job},
        {"events",QJsonArray{QJsonObject{{"seq",1},{"at",at},{"type","StopFailure"},{"detail","503 Service unavailable"}}}}};
    inject(details);
    auto *activity=window.findChild<ActivityView *>("mainActivity");auto *browser=activity->browser();
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *editor=composer->editor();
    auto *chip=window.findChild<ToolbarChip *>("recoveryChip");QVERIFY(chip && chip->isVisible());QCOMPARE(chip->tone(),ChipTone::Warning);
    QVERIFY(chip->fullLabel().startsWith("Retry in"));QVERIFY(chip->shortLabel().endsWith(" s"));
    auto *panel=window.findChild<QFrame *>("recoveryPanel");QVERIFY(!panel->isVisible());
    setComposerText(composer,"Keep my draft");window.activateWindow();editor->setFocus();WAIT_VERIFY(editor->hasFocus());
    // Account inspection and the initial composer layout are unrelated to retries.
    // Finish them before measuring the effect of the countdown alone.
    auto *usage=static_cast<AccountUsage::RefreshButton *>(window.findChild<QPushButton *>("sessionUsageRefresh"));
    WAIT_VERIFY(!usage->isRefreshing());
    // Deferred startup callbacks can post an ancestor layout after the composer
    // already matches its own size hint. Apply that layout before the baseline.
    QCoreApplication::processEvents();
    QCoreApplication::sendPostedEvents(nullptr,QEvent::LayoutRequest);
    // Feedback is now a single line, so the layout need not implement height-for-width.
    WAIT_COMPARE(composer->height(), composer->layout()->hasHeightForWidth()
        ? composer->layout()->totalHeightForWidth(composer->width()) : composer->sizeHint().height());
    QCoreApplication::sendPostedEvents(nullptr,QEvent::LayoutRequest);
    WAIT_VERIFY(browser->toPlainText().contains("503 Service unavailable"));
    WAIT_VERIFY(!activity->renderPending());
    QCoreApplication::sendPostedEvents(nullptr,QEvent::LayoutRequest);
    const int revision=browser->document()->revision();const auto geometry=browser->geometry();const auto label=chip->fullLabel();
    // The countdown ticks while the popover is closed; the tick itself is the
    // wait, and it changes neither the draft, the focus nor the Activity.
    WAIT_VERIFY(chip->fullLabel()!=label);
    QVERIFY(editor->hasFocus());QCOMPARE(editor->toPlainText(),QString("Keep my draft"));
    QCOMPARE(browser->document()->revision(),revision);QCOMPARE(browser->geometry(),geometry);
    QVERIFY(panel->findChild<QPushButton *>("recoveryNow")->isEnabled());
    const auto preview=qEnvironmentVariable("HGS_RECOVERY_PREVIEW");
    if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+"/activity.png"));}
    QTest::mouseClick(chip,Qt::LeftButton);WAIT_VERIFY(panel->isVisible());
    if(!preview.isEmpty())QVERIFY(chip->popover()->grab().save(preview+"/recovery-popover.png"));
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<QDialog *>(QApplication::activeModalWidget());QVERIFY(dialog);
        auto *history=dialog->findChild<QPlainTextEdit *>();QVERIFY(history);QVERIFY(history->toPlainText().contains("Retry scheduled"));
        // The dialog belongs to the window, not to the popup it was opened from.
        QCOMPARE(dialog->parentWidget(),static_cast<QWidget *>(&window));QVERIFY(!panel->isVisible());dialog->reject();
    });
    panel->findChild<QPushButton *>("recoveryHistory")->click();
    window.activateWindow();WAIT_SETTLED();
    QVERIFY(!panel->isVisible());QTest::mouseClick(chip,Qt::LeftButton);WAIT_VERIFY(panel->isVisible());
    details["events"]=QJsonArray();job["state"]="cancelled";job["reason"]="Cancelled by you";details["recovery"]=job;
    panel->findChild<QPushButton *>("recoveryNow")->setFocus();
    WAIT_VERIFY(panel->findChild<QPushButton *>("recoveryNow")->hasFocus());
    inject(details);
    QVERIFY(panel->hasFocus());QVERIFY(!panel->findChild<QPushButton *>("recoveryNow")->isVisible());
    QCOMPARE(chip->tone(),ChipTone::Warning);QCOMPARE(chip->shortLabel(),QString("Off"));
    job["state"]="succeeded";details["recovery"]=job;inject(details);
    QVERIFY(chip->isHidden());QVERIFY(!panel->isVisible());WAIT_VERIFY(editor->hasFocus());QCOMPARE(editor->toPlainText(),QString("Keep my draft"));
}

void TestSessionsWindow::recoveryEndingReturnsKeyboardFocusToField()
{
    SessionsWindow window(program()); window.resize(1280,900); window.setFleet(fleet()); window.show();
    auto *client=window.findChild<HgsClient *>(); QSignalSpy inspections(client,&HgsClient::inspectionReady);
    window.showSession({},"codex/hgs/dashboard");QTRY_VERIFY(!inspections.isEmpty());
    const auto at=QDateTime::currentMSecsSinceEpoch()/1000.0;
    QJsonObject job{{"id","episode"},{"name","codex/hgs/dashboard"},{"state","waiting"},{"action","continue_message"},
        {"due_at",at+20},{"attempt",0},{"delays",QJsonArray{15,30,60,300}}};
    QJsonObject details{{"tracked",true},{"conversation_id","conversation-one"},{"run_id","run-one"},
        {"activity","attention"},{"phase","error"},{"last_event_at",at},{"cursor",1},{"recovery",job}};
    client->inspectionReady({},"codex/hgs/dashboard",details);
    auto *editor=window.findChild<MessageComposer *>("messageComposer")->editor();
    auto *chip=window.findChild<ToolbarChip *>("recoveryChip");auto *panel=window.findChild<QFrame *>("recoveryPanel");
    window.activateWindow();QVERIFY(QTest::qWaitForWindowActive(&window));
    chip->setFocus(Qt::TabFocusReason);QTRY_VERIFY(chip->hasFocus());
    QTest::keyClick(chip,Qt::Key_Return);QTRY_VERIFY(panel->isVisible());
    auto *now=panel->findChild<QPushButton *>("recoveryNow");now->setFocus();QTRY_VERIFY(now->hasFocus());
    job["state"]="succeeded";details["recovery"]=job;client->inspectionReady({},"codex/hgs/dashboard",details);
    QVERIFY(chip->isHidden());QVERIFY(!panel->isVisible());QTRY_VERIFY(editor->hasFocus());
}

void TestSessionsWindow::resumedRecoveryLeavesTheWorkingTurnUnmarked()
{
    SessionsWindow window(program()); window.resize(1280,900); window.setFleet(fleet()); window.show();
    auto *client=window.findChild<HgsClient *>(); QSignalSpy inspections(client,&HgsClient::inspectionReady);
    window.showSession({},"codex/hgs/dashboard");QTRY_VERIFY(!inspections.isEmpty());
    const auto at=QDateTime::currentMSecsSinceEpoch()/1000.0;
    QJsonObject job{{"id","episode"},{"name","codex/hgs/dashboard"},{"state","retrying"},{"action","continue_message"},
        {"acknowledged",true},{"sent_at",at-60},{"user_at",at-59},{"attempt",1},{"class_attempt",1},{"delays",QJsonArray{15,30,60,300,0}}};
    QJsonObject details{{"tracked",true},{"conversation_id","conversation-one"},{"run_id","run-one"},
        {"activity","busy"},{"phase","tool"},{"last_event_at",at},{"cursor",1},{"recovery",job}};
    client->inspectionReady({},"codex/hgs/dashboard",details);
    auto *chip=window.findChild<ToolbarChip *>("recoveryChip");
    QVERIFY(chip->isVisible());QCOMPARE(chip->fullLabel(),QString("Recovery in progress"));
    // Provider progress after the accepted continuation: the turn is ordinary work again,
    // although the episode stays open to keep its attempt budget until the turn ends.
    job["resumed_at"]=at-30;details["recovery"]=job;client->inspectionReady({},"codex/hgs/dashboard",details);
    QVERIFY(chip->isHidden());QCOMPARE(RecoveryUi::status(job),QString("Agent resumed after recovery"));
    // A further failure in that turn shows the provider error at once, before rescheduling.
    details["phase"]="error";details["activity"]="attention";
    details["provider_error"]=QJsonObject{{"source","native"},{"at",at},{"detail","API Error: 529 Overloaded"}};
    client->inspectionReady({},"codex/hgs/dashboard",details);
    QVERIFY(chip->isVisible());QCOMPARE(chip->tone(),ChipTone::Danger);
    job.remove("resumed_at");job["state"]="waiting";job["due_at"]=at+20;details["recovery"]=job;
    client->inspectionReady({},"codex/hgs/dashboard",details);
    QVERIFY(chip->isVisible());QVERIFY(chip->fullLabel().startsWith("Retry in"));
}

void TestSessionsWindow::durableSubmissionKeepsGuiResponsive_data()
{
    QTest::addColumn<QString>("operation"); QTest::addColumn<QString>("outcome");
    for (const QString operation : {"send", "question", "compact", "start"}) {
        for (const QString outcome : {"complete", "run", "conversation", "failure"})
            QTest::addRow("%s-%s", qPrintable(operation), qPrintable(outcome)) << operation << outcome;
        if (operation == "send" || operation == "question") QTest::addRow("%s-expiry", qPrintable(operation)) << operation << QString("expiry");
        if (operation == "compact" || operation == "start") QTest::addRow("%s-cancel", qPrintable(operation)) << operation << QString("cancel");
    }
    QTest::newRow("send-navigation") << QString("send") << QString("navigation");
    QTest::newRow("question-navigation") << QString("question") << QString("navigation");
}

void TestSessionsWindow::durableSubmissionKeepsGuiResponsive()
{
    QFETCH(QString, operation); QFETCH(QString, outcome);
    QTemporaryDir fixtureDir; const QString program = fixtureDir.filePath("hgs");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a[0].startswith('@'): a=a[1:]
if a[0]=='inspect':
 print((root/'details.json').read_text() if a[1]=='codex/website/navigation' else '{}')
elif a[0] in ['send','answer','compact-context','resume']:
 p={} if a[0]=='resume' else json.load(sys.stdin)
 with (root/'transport.jsonl').open('a') as output: output.write(json.dumps({'command':a[0],'name':a[1],'payload':p})+'\n')
 if a[0]=='resume': print('{}')
 else:
  result=dict(request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],status='submitted')
  if a[0]=='answer': result.update(question_id=p['question_id'],question_hash=p['expected_question_hash'])
  print(json.dumps(result))
else: print('{}')
)PY"); fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    const QString name = "codex/website/navigation";
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) if (session.name == name) {
        session.runId = "run-one"; session.conversationId = "conversation-one"; session.activity = "idle"; session.phase = "idle"; session.state = "running"; session.processState = "running";
        if (operation == "start") { session.state = "stopped"; session.processState = "exited"; }
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    QJsonObject question{{"question_id", "question-one"}, {"question_hash", "hash-one"}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"},
        {"source", "codex_async"}, {"optional", true}, {"can_answer", true}, {"questions", QJsonArray{QJsonObject{{"id", "q_0"},
            {"question", "Which test should run?"}, {"allow_other", true}, {"options", QJsonArray{}}}}}};
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
        {"process_state", "running"}, {"activity", "idle"}, {"phase", "idle"}, {"compact_context_supported", true}, {"events", QJsonArray{}}, {"cursor", 0}};
    if (operation == "start") { details["runtime_state"] = "saved"; details["process_state"] = "exited"; }
    if (operation == "compact") details["cache_hint"] = QJsonObject{{"status", "cold"}};
    if (operation == "question") { details["pending_questions"] = QJsonArray{question}; details["activity"] = "busy"; details["phase"] = "thinking"; }
    const auto save = [&] { return FakeCli::replace(fixtureDir.filePath("details.json"), QJsonDocument(details).toJson()); };
    QVERIFY(save()); SessionsWindow window(FakeCli::program(program)); window.setFleet(state); window.show(); window.showSession({}, name);
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    // A fixture inspection replies with details.json as it was when it started, so one
    // in flight across save() could restore the old identity after the injected one
    // and admit the held submission. HgsClient runs one inspection per session: once
    // ours starts, none is older, and its reply is current.
    const auto settle = [&] { QSignalSpy replies(client, &HgsClient::inspectionReady); qsizetype issued = -1;
        QTRY_VERIFY(issued >= 0 || (client->requestInspection({}, name) && (issued = replies.size(), true)));
        QTRY_VERIFY(std::any_of(replies.cbegin() + issued, replies.cend(), [&](const QList<QVariant> &reply) { return reply.at(1).toString() == name; })); };
    auto *card = window.findChild<QuestionCard *>("questionCard"); auto *send = composer->findChild<QPushButton *>("sendMessage");
    // The first fixture inspection takes up to 8 s on a loaded Windows machine.
    QSignalSpy inspected(client, &HgsClient::inspectionReady); QTRY_VERIFY_WITH_TIMEOUT(!inspected.isEmpty(), 15000);
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    QTRY_VERIFY(window.findChild<QLabel *>("detailTitle")->text().contains("navigation"));
    // Receive one actual fixture inspection before replacing its details.
    QTRY_VERIFY(operation == "question" ? card->isVisible() : composer->isInputVisible());
    client->inspectionReady({}, name, details);
    const QString key = composer->sessionKey();
    setComposerText(composer, "Exact preserved message");
    if (operation == "question") { QTRY_VERIFY(!card->isDraftLoading()); card->findChild<QLineEdit *>("questionFreeText")->setText("Exact preserved answer"); }
    ComposerDraftStore::flushWrites(); QCoreApplication::processEvents();
    QSemaphore entered, release; std::atomic<int> writes = 0;
    const QString storageRoot = ComposerDraftStore::directory();
    auto removeBlocker = qScopeGuard([&] { QFile::remove(storageRoot); });
    ComposerDraftStore::setWriteObserver([&] { if (++writes == 1) { entered.release(); release.acquire(); } });
    auto unblock = qScopeGuard([&] { release.release(); ComposerDraftStore::flushWrites(); ComposerDraftStore::setWriteObserver({}); });
    if (operation == "compact" || operation == "start") QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QMessageBox *>(operation == "compact" ? "coldCacheConfirm" : "startToSendConfirm"); QVERIFY(dialog);
        for (auto *button : dialog->buttons()) if (operation == "compact" ? button->text() == "Compact and continue" : button->objectName() == "resumeAndSend") { button->click(); return; }
        QFAIL("Expected continuation choice");
    });
    auto *submit = operation == "question" ? card->findChild<QPushButton *>("submitQuestionAnswer") : send;
    QTRY_VERIFY(submit->isEnabled());
    // An expired cache at admission opens a modal cold-cache confirmation instead,
    // and nothing here would answer it (zerus-1okk: a Windows shard hung there). Fail
    // instead: the timer can only run once the click returns or a dialog's loop runs.
    bool prompted = false;
    if (outcome == "expiry") {
        // expires_at has whole seconds: +4 leaves at least 3 s for the slow
        // replacement of details.json on Windows and this click.
        const auto deadline = QDateTime::currentSecsSinceEpoch() + 4;
        details["session_usage"] = QJsonObject{{"prompt_cache", QJsonObject{{"status", "warm"}, {"expires_at", deadline}}}};
        QVERIFY(save()); client->inspectionReady({}, name, details);
        QTimer::singleShot(0, &window, [&] {
            for (const auto *prompt : {"coldCacheConfirm", "questionColdCacheConfirm"})
                if (auto *dialog = window.findChild<QMessageBox *>(prompt)) { prompted = true; dialog->reject(); } });
    }
    submit->click(); // Admission captures this authoritative warm TTL without an intervening event turn.
    QVERIFY2(!prompted, "The warm cache expired before admission");
    QTRY_VERIFY(entered.available() == 1);
    if (outcome == "expiry") client->inspectionReady({}, name, details); // Keep latest evidence equal to the captured TTL while persistence is held.
    const auto transports = [&] { QList<QJsonObject> result; QFile file(fixtureDir.filePath("transport.jsonl"));
        if (file.open(QIODevice::ReadOnly)) while (!file.atEnd()) result << QJsonDocument::fromJson(file.readLine()).object(); return result; };
    QVERIFY(transports().isEmpty()); bool heartbeat = false; QTimer::singleShot(0, &window, [&] { heartbeat = true; }); QTRY_VERIFY(heartbeat);
    if (outcome == "navigation") {
        window.showSession({}, "codex/hgs/dashboard");
        heartbeat=false;QTimer::singleShot(0,&window,[&]{heartbeat=true;});QTRY_VERIFY(heartbeat);
        QCOMPARE(composer->sessionKey(), QString("\ncodex/hgs/dashboard"));
    }
    if (outcome == "run" || outcome == "conversation") {
        details[outcome == "run" ? "run_id" : "conversation_id"] = "changed-identity"; QVERIFY(save()); client->inspectionReady({}, name, details); settle();
    }
    if (outcome == "cancel") { auto *cancel = window.findChild<QPushButton *>("cancelCompactSend"); QVERIFY(cancel->isVisible()); cancel->click(); }
    if (outcome == "failure") { QVERIFY(QDir(storageRoot).removeRecursively()); QFile blocker(storageRoot); QVERIFY(blocker.open(QIODevice::WriteOnly)); blocker.close(); }
    if (outcome == "expiry") QTRY_VERIFY_WITH_TIMEOUT(QDateTime::currentSecsSinceEpoch() >= details["session_usage"].toObject()["prompt_cache"].toObject()["expires_at"].toInteger(), 10000);
    QVERIFY(transports().isEmpty()); release.release();
    const bool accepted = outcome == "complete" || outcome == "navigation";
    if (accepted) {
        QTRY_COMPARE(transports().size(), 1); const auto sent = transports().first(); QCOMPARE(sent["name"].toString(), name);
        const QString command = operation == "start" ? "resume" : operation == "compact" ? "compact-context" : operation == "question" ? "answer" : "send";
        QCOMPARE(sent["command"].toString(), command);
        if (operation != "start") { const auto payload = sent["payload"].toObject(); QCOMPARE(payload["expected_run_id"].toString(), QString("run-one"));
            QCOMPARE(payload["expected_conversation_id"].toString(), QString("conversation-one"));
            if (operation == "send") QCOMPARE(payload["text"].toString(), QString("Exact preserved message"));
            if (operation == "question") QCOMPARE(payload["answers"].toArray().first().toObject()["text"].toString(), QString("Exact preserved answer")); }
    } else {
        if (operation == "question") QTRY_VERIFY(!card->submissionSnapshot(key, "question-one").value("sending").toBool());
        else QTRY_VERIFY(!composer->isSending(key) && !composer->isPreparingSending(key) && !composer->isWaitingToSend(key));
        QVERIFY(transports().isEmpty());
        QCOMPARE(composer->editor()->toPlainText(), QString("Exact preserved message"));
        if (operation == "question" && outcome != "run" && outcome != "conversation") QCOMPARE(card->findChild<QLineEdit *>("questionFreeText")->text(), QString("Exact preserved answer"));
    }
    ComposerDraftStore::flushWrites(); QCoreApplication::processEvents(); QTest::qWait(100);
    QCOMPARE(transports().size(), accepted ? 1 : 0); // Repeated polls never replay the accepted operation.
}

void TestSessionsWindow::coldCacheConfirmationPreservesDraftAndPinsConversation()
{
    const auto program=m_dir->filePath("cache-hgs"),capture=m_dir->filePath("cache-send.json"),inspected=m_dir->filePath("cache-inspect.json");
    // Background inspections return the injected state: an empty reply arriving
    // late would drop the cold cache and leave no confirmation to answer.
    QJsonObject details{{"tracked",true},{"run_id","run-cache"},{"conversation_id","conversation-cache"},{"runtime_state","live"},{"process_state","running"},
        {"activity","idle"},{"phase","idle"},{"cache_hint",QJsonObject{{"status","cold"},{"tokens",756000}}}};
    const auto writeDetails=[&]{QVERIFY(FakeCli::replace(inspected,QJsonDocument(details).toJson()));};
    writeDetails();
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport sys,json,pathlib\na=sys.argv[1:]\nif a[0].startswith('@'):a=a[1:]\nif a[0]=='send':\n p=json.load(sys.stdin);target=pathlib.Path("+QJsonDocument(QJsonArray{capture}).toJson(QJsonDocument::Compact)+"[0])\n staged=target.with_name(target.name+'.tmp');staged.write_text(json.dumps(p));staged.replace(target)\n print(json.dumps(dict(status='submitted',request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'])))\nelif a[0]=='inspect':print(open("+QJsonDocument(QJsonArray{inspected}).toJson(QJsonDocument::Compact)+"[0]).read())\nelse:print('{}')\n");
    fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    // An inspection that read the file before the conversation changes must not be
    // running then. On Windows a 5 ms Qt timer posts its event, so such a reply can be
    // delivered in the same batch right after the confirmation is accepted, before
    // exec() returns: it restores the old conversation, the pinned identity matches
    // again and the draft starts sending. Inspect only on show and selection, then wait
    // for an inspection of the test's own: HgsClient runs one per session, so once its
    // reply is in, none is pending, and the next one starts with the change.
    SessionsWindow window(FakeCli::program(program));window.setFleet(fleet());
    window.setDetailsPollForTests(500,24*60*60*1000);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();QSignalSpy replies(client,&HgsClient::inspectionReady);qsizetype issued=-1;
    // Fixture processes start slowly under load on Windows: the window's inspection
    // may run until HgsClient's 5 s limit before the test's own can start.
    QTRY_VERIFY_WITH_TIMEOUT(issued>=0||(client->requestInspection({},"codex/hgs/dashboard")&&(issued=replies.size(),true)),15000);
    QTRY_VERIFY_WITH_TIMEOUT(std::any_of(replies.cbegin()+issued,replies.cend(),[](const QList<QVariant> &reply){return reply.at(1).toString()=="codex/hgs/dashboard";}),15000);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *send=composer->findChild<QPushButton *>("sendMessage");
    setComposerText(composer, "Preserve this draft");QVERIFY(send->isEnabled());const QString key=composer->sessionKey();
    // Act on the confirmation once it is actually shown; a zero timer could fire
    // in another event loop first and find no dialog at all.
    const auto onConfirm=[&](std::function<void(QMessageBox *)> act){
        auto *poll=new QTimer(&window);poll->setInterval(5);QElapsedTimer age;age.start();
        QObject::connect(poll,&QTimer::timeout,&window,[poll,act,age,&window]{
            auto *dialog=window.findChild<QMessageBox *>("coldCacheConfirm");
            if(!(dialog&&dialog->isVisible())&&age.elapsed()<5000)return;
            poll->stop();poll->deleteLater();if(dialog&&dialog->isVisible())act(dialog);});
        poll->start();};
    const auto accept=[](QMessageBox *dialog){for(auto *b:dialog->buttons())if(dialog->buttonRole(b)==QMessageBox::AcceptRole)b->click();};
    bool shown=false;onConfirm([&](QMessageBox *dialog){shown=true;dialog->button(QMessageBox::Cancel)->click();});
    send->click();QVERIFY(shown);QVERIFY(!composer->isSending(key));QVERIFY(!QFileInfo::exists(capture));QCOMPARE(composer->editor()->toPlainText(),QString("Preserve this draft"));
    // The change starts the window's next inspection after the file was replaced. No
    // poll follows, so count the replies after the injected one: that inspection's reply
    // must be seen even if it arrives before the confirmation returns.
    qsizetype injected=-1;
    shown=false;onConfirm([&](QMessageBox *dialog){shown=true;details["conversation_id"]="conversation-new";writeDetails();client->inspectionReady({},"codex/hgs/dashboard",details);injected=replies.size();accept(dialog);});
    send->click();QVERIFY(shown);QVERIFY(!composer->isSending(key));QVERIFY(!QFileInfo::exists(capture));QCOMPARE(composer->editor()->toPlainText(),QString("Preserve this draft"));
    // U1: the failure is shown where the owner looks, not swallowed silently.
    auto *status=composer->findChild<QLabel *>("messageStatus");
    QVERIFY(status);
    QTRY_VERIFY(status->text().contains(QStringLiteral("identity changed")));
    // Wait for that reply, so none is pending when the next confirmation opens.
    QTRY_VERIFY_WITH_TIMEOUT(std::any_of(replies.cbegin()+injected,replies.cend(),[](const QList<QVariant> &reply){
        return reply.at(2).toJsonObject().value("conversation_id")==QStringLiteral("conversation-new");}),10000);
    shown=false;onConfirm([&](QMessageBox *dialog){shown=true;accept(dialog);});
    send->click();QVERIFY(shown);QTRY_VERIFY(QFileInfo::exists(capture));QFile sent(capture);QVERIFY(sent.open(QIODevice::ReadOnly));
    const auto payload=QJsonDocument::fromJson(sent.readAll()).object();QCOMPARE(payload["expected_conversation_id"].toString(),QString("conversation-new"));QCOMPARE(payload["text"].toString(),QString("Preserve this draft"));
}

// U1: a session that was still starting when the cold-cache dialog opened gains
// its conversation id while the dialog is up. That is the same session, so the
// message must still go out, with the new identity, instead of vanishing.
void TestSessionsWindow::coldCacheDialogRefreshedIdentityStillSends()
{
    const auto program=m_dir->filePath("cache-retry-hgs"),capture=m_dir->filePath("cache-retry-send.json"),inspected=m_dir->filePath("cache-retry-inspect.json");
    QJsonObject details{{"tracked",true},{"run_id","run-retry"},{"conversation_id",""},{"first_message_can_send",true},
        {"runtime_state","live"},{"process_state","running"},{"activity","idle"},{"phase","idle"},
        {"cache_hint",QJsonObject{{"status","cold"},{"tokens",756000}}}};
    const auto writeDetails=[&]{QVERIFY(FakeCli::replace(inspected,QJsonDocument(details).toJson()));};
    writeDetails();
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport sys,json,pathlib\na=sys.argv[1:]\nif a[0].startswith('@'):a=a[1:]\nif a[0]=='send':\n p=json.load(sys.stdin);target=pathlib.Path("+QJsonDocument(QJsonArray{capture}).toJson(QJsonDocument::Compact)+"[0])\n staged=target.with_name(target.name+'.tmp');staged.write_text(json.dumps(p));staged.replace(target)\n print(json.dumps(dict(status='submitted',request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'])))\nelif a[0]=='inspect':print(open("+QJsonDocument(QJsonArray{inspected}).toJson(QJsonDocument::Compact)+"[0]).read())\nelse:print('{}')\n");
    fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    SessionsWindow window(FakeCli::program(program));window.setFleet(fleet());
    window.setDetailsPollForTests(500,24*60*60*1000);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();QSignalSpy replies(client,&HgsClient::inspectionReady);
    qsizetype issued=-1;
    QTRY_VERIFY_WITH_TIMEOUT(issued>=0||(client->requestInspection({},"codex/hgs/dashboard")&&(issued=replies.size(),true)),15000);
    QTRY_VERIFY_WITH_TIMEOUT(std::any_of(replies.cbegin()+issued,replies.cend(),[](const QList<QVariant> &reply){return reply.at(1).toString()=="codex/hgs/dashboard";}),15000);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *send=composer->findChild<QPushButton *>("sendMessage");
    setComposerText(composer, "Send after the start");QVERIFY(send->isEnabled());
    const auto onConfirm=[&](std::function<void(QMessageBox *)> act){
        auto *poll=new QTimer(&window);poll->setInterval(5);QElapsedTimer age;age.start();
        QObject::connect(poll,&QTimer::timeout,&window,[poll,act,age,&window]{
            auto *dialog=window.findChild<QMessageBox *>("coldCacheConfirm");
            if(!(dialog&&dialog->isVisible())&&age.elapsed()<5000)return;
            poll->stop();poll->deleteLater();if(dialog&&dialog->isVisible())act(dialog);});
        poll->start();};
    const auto accept=[](QMessageBox *dialog){for(auto *b:dialog->buttons())if(dialog->buttonRole(b)==QMessageBox::AcceptRole)b->click();};
    bool shown=false;
    onConfirm([&](QMessageBox *dialog){
        shown=true;
        // A poll during the modal loop fills the id the dialog opened without.
        details["conversation_id"]="conversation-started";writeDetails();
        client->inspectionReady({},"codex/hgs/dashboard",details);
        accept(dialog);
    });
    send->click();
    QVERIFY(shown);
    QTRY_VERIFY_WITH_TIMEOUT(QFileInfo::exists(capture),15000);
    QFile sent(capture);QVERIFY(sent.open(QIODevice::ReadOnly));
    const auto payload=QJsonDocument::fromJson(sent.readAll()).object();
    QCOMPARE(payload["expected_conversation_id"].toString(),QString("conversation-started"));
    QCOMPARE(payload["text"].toString(),QString("Send after the start"));
    // The receipt clears the sending state a moment after the fixture writes.
    QTRY_VERIFY(!composer->isSending(composer->sessionKey()));
}

// U1: a stopped dsh session has the same one-click start-and-send as
// claude/codex/kimi: Send stays enabled, says how it starts, and asks the same
// Resume-and-send question before the message waits for the new run.
void TestSessionsWindow::stoppedDshSessionStartsAndSends()
{
    QTemporaryDir temp;const auto program=temp.filePath("hgs");
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport sys,json,pathlib\nroot=pathlib.Path(__file__).parent\na=sys.argv[1:]\nif a[0].startswith('@'):a=a[1:]\nif a[0] in ('resume','fresh'):\n (root/'resume.log').open('a').write(json.dumps(sys.argv[1:])+'\\n')\n print(json.dumps(dict(status='started',request_id='r')))\nelif a[0]=='inspect':print((root/'details.json').read_text() if (root/'details.json').exists() else '{}')\nelse:print('{}')\n");
    fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    const QString name="codex/website/navigation";
    auto state=fleet();auto box=state.local();
    for(auto &session:box.sessions)if(session.name==name){session.cmd="dsh";session.state="stopped";session.tracked=true;
        session.runId="run-dsh";session.conversationId="conversation-dsh";session.processState="exited";session.activity="idle";session.phase="idle";}
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    {QFile details(temp.filePath("details.json"));QVERIFY(details.open(QIODevice::WriteOnly));
     details.write(QJsonDocument(QJsonObject{{"tracked",true},{"run_id","run-dsh"},{"conversation_id","conversation-dsh"},
                                             {"runtime_state","saved"},{"process_state","exited"}}).toJson());}
    SessionsWindow window(FakeCli::program(program));
    auto *client = window.findChild<HgsClient *>(); QVERIFY(client);
    QSignalSpy inspections(client, &HgsClient::inspectionReady);
    window.setFleet(state);window.show();window.showSession({},name);
    // The listing enables Resume before its async preparation has inspected identity.
    WAIT_VERIFY(std::any_of(inspections.cbegin(), inspections.cend(), [&name](const QList<QVariant> &reply) {
        const auto detail = reply.at(2).toJsonObject();
        return reply.at(0).toString().isEmpty() && reply.at(1).toString() == name
            && detail["run_id"] == "run-dsh" && detail["conversation_id"] == "conversation-dsh";
    }));
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *send=composer->findChild<QPushButton *>("sendMessage");
    auto *status=composer->findChild<QLabel *>("messageStatus");
    setComposerText(composer,"Continue the dsh session");
    // canStartStopped: the click that used to be refused now leads somewhere.
    QTRY_VERIFY(send->isEnabled());
    QVERIFY(status->text().contains(QStringLiteral("resume it or start fresh")));
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("startToSendConfirm");QVERIFY(dialog);
        auto *resume=dialog->findChild<QPushButton *>("resumeAndSend");QVERIFY(resume);QVERIFY(resume->isEnabled());resume->click();});
    send->click();
    const auto log=temp.filePath("resume.log");
    // The fixture's resume runs a Python process: slow on a loaded Windows runner.
    QTRY_VERIFY_WITH_TIMEOUT(QFileInfo::exists(log),30000);
    QFile started(log);QVERIFY(started.open(QIODevice::ReadOnly));
    const auto command=QString::fromUtf8(started.readAll());
    QVERIFY(command.contains(QStringLiteral("resume")));
    QVERIFY(command.contains(name));
    // The draft now waits for that run instead of being dropped.
    QTRY_VERIFY(send->text().contains(QStringLiteral("Waiting")));
    QVERIFY(status->text().contains(QStringLiteral("Waiting for the session to start")));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_composer1.moc"
