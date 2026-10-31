// U62 (#712): Push and queued messages, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void queueChordUsesTheExistingSendNowPath();
    void queueFailureNotices_data();
    void queueFailureNotices();
    void pushSendsNowWhenTheMessageIsQueued();
    void pushIgnoresQueuesWithoutTheMessage();
    void pushIgnoresTheMessageInsideAnotherWord();
    void pushDropsAfterSessionIdentityChange_data();
    void pushDropsAfterSessionIdentityChange();
    void pushFollowsTheRunThatTakesTheMessage();
#ifdef Q_OS_WIN
    void messageStartupFailureRestoresThePendingDraft();
    void queueStartupFailureReleasesThePendingAction();
    void settingsStartupFailureReleasesThePendingChange();
#endif
    void pushStaysWithItsSession();
    void pushTimesOut();
    void pushOutlastedBySlowSend();
    void pushOfAttachmentsAloneSendsWithoutPush();
    void pushDropsWhenSendFails_data();
    void pushDropsWhenSendFails();
    void pushBehindADialogIsNotSentAtTheDeadline();
    void pushReplacesAnEarlierPush();
    void pushWaitsForARunningSendNow();
    void pushIgnoresTheCodexQueueHeader();
    void pushFindsAMessageThatStartsWithAnAttachment_data();
    void pushFindsAMessageThatStartsWithAnAttachment();
};

void TestSessionsWindow::queueChordUsesTheExistingSendNowPath()
{
    QTemporaryDir fixture; QFile fake(fixture.filePath("hgs")), original(script());
    QVERIFY(original.open(QIODevice::ReadOnly)); QVERIFY(fake.open(QIODevice::WriteOnly));
    fake.write(original.readAll().replace("if False:pass\n", "if False:pass\n"
        "elif c=='inspect':print(open(__file__+'.details').read())\n"
        "elif c=='send-now':\n payload=json.loads(sys.stdin.read())\n"
        " open(__file__+'.send-now','a').write(payload['queue_id']+'\\n')\n"
        " print(json.dumps({'status':'submitted','request_id':payload['request_id'],'name':a[1],'run_id':payload['expected_run_id'],"
        "'conversation_id':payload['expected_conversation_id'],'queue_id':payload['queue_id']}))\n"));
    fake.close(); QVERIFY(FakeCli::makeRunnable(fake.fileName()));
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
        {"process_state", "running"}, {"activity", "busy"}, {"phase", "working"}, {"events", QJsonArray{}}, {"cursor", 0},
        {"input_queue", QJsonObject{{"id", "q1"}, {"text", "❯ run the release checks"}, {"can_send_now", true}, {"hint", "Interrupt and send the queue."}}}};
    const auto save = [&] { return FakeCli::replace(fake.fileName() + ".details", QJsonDocument(details).toJson(QJsonDocument::Compact)); };
    const auto requests = [&] {
        QFile log(fake.fileName() + ".send-now");
        return log.open(QIODevice::ReadOnly) ? QString::fromUtf8(log.readAll()).split('\n', Qt::SkipEmptyParts) : QStringList();
    };
    QVERIFY(save());
    SessionsWindow window(FakeCli::program(fake.fileName())); keepDetailsPollsOut(window);
    window.setFleet(fleet()); window.show(); window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", details);
    auto *sendNow = window.findChild<ActivityView *>("mainActivity")->findChild<QPushButton *>("queueSendNow");
    QTRY_VERIFY(sendNow->isVisible()); QVERIFY(sendNow->isEnabled());
    QCOMPARE(sendNow->toolTip(), QString("Interrupt and send the queue. (%1)").arg(ShortcutRegistry::instance().hint("queue.sendNow")));
    setComposerText(composer, "Typed but not sent"); composer->editor()->setFocus(); QTRY_VERIFY(composer->editor()->hasFocus());
    QSignalSpy sent(composer, &MessageComposer::sendRequested); QSignalSpy finished(client, &HgsClient::queueSendFinished);
    pressCommand(&window, "queue.sendNow");
    QTRY_COMPARE(finished.size(), 1); QVERIFY(finished[0][1].toBool());
    QCOMPARE(requests(), QStringList{"q1"}); QCOMPARE(sent.size(), 0);
    QCOMPARE(composer->editor()->toPlainText(), QString("Typed but not sent"));
    // A queue that cannot be sent now sends nothing and says so.
    details["input_queue"] = QJsonObject{{"id", "q2"}, {"text", "❯ later"}, {"can_send_now", false}};
    QVERIFY(save()); settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", details);
    pressCommand(&window, "queue.sendNow");
    QCOMPARE(window.findChild<QLabel *>("notice")->text(), QString("Nothing queued can be sent now"));
    QTest::qWait(150); QCOMPARE(requests(), QStringList{"q1"}); QCOMPARE(sent.size(), 0);
}

void TestSessionsWindow::queueFailureNotices_data()
{
    QTest::addColumn<QString>("error");
    QTest::addColumn<QString>("expectedNotice");
    QTest::addColumn<bool>("transient");
    QTest::newRow("stale native queue")
        << QString("hgs: The native queue is no longer visible; nothing was sent")
        << QString("The native queue is no longer visible; nothing was sent. Activity is being refreshed; check Terminal or the current queue before retrying.")
        << true;
    const QString generic("hgs: Queue action failed. Check Terminal before retrying.");
    QTest::newRow("generic queue failure") << generic << generic << false;
}

void TestSessionsWindow::queueFailureNotices()
{
    QFETCH(QString, error); QFETCH(QString, expectedNotice); QFETCH(bool, transient);
    QTemporaryDir fixture; QFile fake(fixture.filePath("hgs")), original(script());
    QVERIFY(original.open(QIODevice::ReadOnly)); QVERIFY(fake.open(QIODevice::WriteOnly));
    fake.write(original.readAll().replace("for value in sys.argv[1:]:print(value,file=sys.stderr)\n", "").replace("if False:pass\n", "if False:pass\n"
        "elif c=='inspect':print(open(__file__+'.details').read())\n"
        "elif c=='send-now':\n payload=json.loads(sys.stdin.read())\n"
        " open(__file__+'.send-now','a').write(payload['queue_id']+'\\n')\n"
        " print(open(__file__+'.error').read(),file=sys.stderr)\n"
        " sys.exit(1)\n"));
    fake.close(); QVERIFY(FakeCli::makeRunnable(fake.fileName()));
    QFile failure(fake.fileName() + ".error"); QVERIFY(failure.open(QIODevice::WriteOnly));
    failure.write(error.toUtf8()); failure.close();
    const QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
        {"process_state", "running"}, {"activity", "busy"}, {"phase", "working"}, {"events", QJsonArray{}}, {"cursor", 0},
        {"input_queue", QJsonObject{{"id", "q1"}, {"text", "❯ run the release checks"}, {"can_send_now", true}}}};
    QVERIFY(FakeCli::replace(fake.fileName() + ".details", QJsonDocument(details).toJson(QJsonDocument::Compact)));
    constexpr int noticeMs = 200;
    SessionsWindow window(FakeCli::program(fake.fileName())); keepDetailsPollsOut(window); window.setNoticeDurationForTests(noticeMs);
    window.setFleet(fleet()); window.show(); window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", details);
    auto *sendNow = window.findChild<ActivityView *>("mainActivity")->findChild<QPushButton *>("queueSendNow");
    QTRY_VERIFY(sendNow->isVisible()); QVERIFY(sendNow->isEnabled());
    QSignalSpy finished(client, &HgsClient::queueSendFinished), refreshed(client, &HgsClient::inspectionReady);
    QSignalSpy sent(composer, &MessageComposer::sendRequested);
    sendNow->click(); QTRY_COMPARE(finished.size(), 1);
    QVERIFY(!finished[0][1].toBool()); QCOMPARE(finished[0][3].toString(), error);
    QCOMPARE(notice(window), expectedNotice); QVERIFY(noticeIsError(window));
    QVERIFY(window.findChild<QLabel *>("notice")->isVisible());
    if (transient) {
        QVERIFY(expectedNotice.contains("nothing was sent"));
        QVERIFY(expectedNotice.contains("Activity is being refreshed"));
        QVERIFY(expectedNotice.contains("Terminal")); QVERIFY(expectedNotice.contains("before retrying"));
    }
    QTRY_VERIFY(!refreshed.isEmpty());
    if (transient) {
        QTRY_VERIFY(notice(window).isEmpty());
        QVERIFY(window.findChild<QLabel *>("notice")->isHidden());
    } else {
        QTest::qWait(3 * noticeMs);
        QCOMPARE(notice(window), expectedNotice); QVERIFY(noticeIsError(window));
        QVERIFY(window.findChild<QLabel *>("notice")->isVisible());
    }
    QFile log(fake.fileName() + ".send-now"); QVERIFY(log.open(QIODevice::ReadOnly));
    QCOMPARE(QString::fromUtf8(log.readAll()).split('\n', Qt::SkipEmptyParts), QStringList{"q1"});
    QCOMPARE(sent.size(), 0);
}

void TestSessionsWindow::pushSendsNowWhenTheMessageIsQueued()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    // Only the main message field pushes: subagent messages do not enter the session's queue.
    QVERIFY(pushes(composer)); QVERIFY(!pushes(window.findChild<MessageComposer *>("subagentComposer")));
    QSignalSpy delivered(client, &HgsClient::messageSent), queued(client, &HgsClient::queueSendFinished);
    pushMessage(window, "Run the release checks before tagging v2"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    // The agent queued it; its terminal wraps the text.
    fixture.queue("q-push", "❯ Run the release checks\n  before tagging v2"); inspectPush(window, fixture);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-push"}, PushTimeout); QTRY_COMPARE(queued.size(), 1);
    // The push is spent: it never resends text or the queue.
    inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList{"q-push"}); QCOMPARE(delivered.size(), 1);
    // With an empty field the chord sends the existing queue now.
    QCOMPARE(composer->editor()->toPlainText(), QString());
    fixture.queue("q-later", "❯ later"); inspectPush(window, fixture);
    pushMessage(window, {});
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), (QStringList{"q-push", "q-later"}), PushTimeout); QCOMPARE(delivered.size(), 1);
}

void TestSessionsWindow::pushIgnoresQueuesWithoutTheMessage()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    pushMessage(window, "Summarize the failing tests in one table"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    fixture.queue("q-other", "❯ Rename the release branch"); inspectPush(window, fixture);
    fixture.queue("q-blocked", "❯ Summarize the failing tests in one table", false); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
    fixture.queue("q-match", "❯ Rename the release branch\n❯ Summarize the   failing tests in one table"); inspectPush(window, fixture);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-match"}, PushTimeout);
}

void TestSessionsWindow::pushIgnoresTheMessageInsideAnotherWord()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    pushMessage(window, "test suite on CI"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    // Only longer words hold the message: "latest" ends with "test", "CI-only" begins with "CI".
    fixture.queue("q-latest", "❯ Rerun the latest suite on CI"); inspectPush(window, fixture);
    fixture.queue("q-longer", "❯ test suite on CI-only runners"); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
    fixture.queue("q-match", "❯ Rerun the latest suite on CI\n❯ test suite on CI"); inspectPush(window, fixture);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-match"}, PushTimeout);
}

void TestSessionsWindow::pushDropsAfterSessionIdentityChange_data()
{
    QTest::addColumn<QString>("field"); QTest::addColumn<QString>("value");
    QTest::newRow("run") << QStringLiteral("run_id") << QStringLiteral("run-two");
    QTest::newRow("conversation") << QStringLiteral("conversation_id") << QStringLiteral("conversation-two");
}

void TestSessionsWindow::pushDropsAfterSessionIdentityChange()
{
    QFETCH(QString, field); QFETCH(QString, value);
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    pushMessage(window, "Check the deployment logs again"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    fixture.details[field] = value; fixture.queue("q-changed", "❯ Check the deployment logs again"); inspectPush(window, fixture);
    QTRY_COMPARE(notice(window), QString("The message was not pushed: the session changed")); QVERIFY(noticeIsError(window));
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
}

void TestSessionsWindow::pushFollowsTheRunThatTakesTheMessage()
{
    PushFixture fixture; fixture.traceSend = true;
    fixture.details = QJsonObject{{"tracked", true}, {"run_id", "run-old"}, {"conversation_id", "conversation-old"},
        {"runtime_state", "saved"}, {"process_state", "exited"}, {"events", QJsonArray{}}, {"cursor", 0}};
    QVERIFY(fixture.install(script()));
    auto state = fleet(); auto box = state.local();
    const auto setSession = [&](const QString &value, const QString &run, const QString &conversation) {
        for (auto &session : box.sessions) if (session.name == "codex/hgs/dashboard") {
            session.state = value; session.runId = run; session.conversationId = conversation; session.currentTool.clear();
            session.processState = value == "running" ? "running" : "exited"; session.activity = "idle"; session.phase = "idle";
        }
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    };
    setSession("stopped", "run-old", "conversation-old");
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, state, fixture);
    if (QTest::currentTestFailed()) return;
    auto *client = window.findChild<HgsClient *>();
    QSignalSpy delivered(client, &HgsClient::messageSent), writes(client, &HgsClient::writeDone);
    // zerus-pvfa14: a failure prints the synthetic phases seen so far, never message
    // or draft text, before the window and fixture are destroyed.
    const qint64 t0 = QDateTime::currentMSecsSinceEpoch();
    const QString pushKey = window.findChild<MessageComposer *>("messageComposer")->sessionKey();
    QStringList phases; qsizetype droppedPhases = 0;
    const auto bounded = [](QString value) { return value.remove('\r').replace('\n', '|').left(160); };
    const auto flag = [](bool value) { return value ? QStringLiteral("1") : QStringLiteral("0"); };
    const auto notePhase = [&](const QString &line) {
        if (phases.size() < 200) phases << QStringLiteral("+%1ms %2").arg(QString::number(QDateTime::currentMSecsSinceEpoch() - t0), line);
        else ++droppedPhases;
    };
    const auto fields = [&](const QJsonObject &data) {
        QStringList values;
        for (const char *field : {"tracked", "run_id", "conversation_id", "runtime_state", "process_state", "activity", "phase", "first_message_can_send", "error"})
            if (data.contains(QLatin1String(field)))
                values << QStringLiteral("%1=%2").arg(QLatin1String(field), bounded(data.value(QLatin1String(field)).toVariant().toString()));
        if (data.contains(QStringLiteral("input_queue")))
            values << QStringLiteral("queue=%1").arg(bounded(data.value(QStringLiteral("input_queue")).toObject().value(QStringLiteral("id")).toString()));
        return values.join(' ');
    };
    QObject phaseContext; // Disconnects these recorders before the helpers above are destroyed.
    connect(client, &HgsClient::writeDone, &phaseContext, [&](const QString &operation, bool ok, const QString &detail) {
        notePhase(QStringLiteral("writeDone operation=%1 ok=%2 detail=%3").arg(operation, flag(ok), bounded(detail)));
    });
    connect(client, &HgsClient::messageSent, &phaseContext, [&](quint64 request, const QString &host, const QString &name, const QJsonObject &receipt) {
        notePhase(QStringLiteral("messageSent request=%1 host=%2 name=%3 run=%4 conversation=%5").arg(QString::number(request), host, name,
            receipt.value(QStringLiteral("run_id")).toString(), receipt.value(QStringLiteral("conversation_id")).toString()));
    });
    connect(client, &HgsClient::messageFailed, &phaseContext, [&](quint64 request, const QString &host, const QString &name, const QString &detail, bool uncertain) {
        notePhase(QStringLiteral("messageFailed request=%1 host=%2 name=%3 uncertain=%4 detail=%5").arg(QString::number(request), host, name,
            flag(uncertain), bounded(detail)));
    });
    connect(client, &HgsClient::inspectionLoadingChanged, &phaseContext,
        [&](const QString &host, const QString &name, const QString &archive, const QString &agent, bool loading) {
            notePhase(QStringLiteral("inspectionLoading host=%1 name=%2 archive=%3 agent=%4 loading=%5").arg(host, name, archive, agent, flag(loading)));
        });
    connect(client, &HgsClient::inspectionReady, &phaseContext, [&](const QString &host, const QString &name, const QJsonObject &data, const QString &archive) {
        notePhase(QStringLiteral("inspectionReady host=%1 name=%2 archive=%3 %4").arg(host, name, archive, fields(data)));
    });
    connect(client, &HgsClient::inspectionFailed, &phaseContext, [&](const QString &host, const QString &name, const QString &detail, const QString &archive) {
        notePhase(QStringLiteral("inspectionFailed host=%1 name=%2 archive=%3 detail=%4").arg(host, name, archive, bounded(detail)));
    });
    const auto snapshot = qScopeGuard([&] {
        if (!QTest::currentTestFailed()) return;
        QStringList lines{QStringLiteral("t0=%1 writes=%2 delivered=%3 confirmOpen=%4").arg(QString::number(t0), QString::number(writes.size()),
            QString::number(delivered.size()), flag(window.findChild<QMessageBox *>("startToSendConfirm") != nullptr))};
        const auto *composer = window.findChild<MessageComposer *>("messageComposer");
        const auto *status = composer ? composer->findChild<QLabel *>("messageStatus") : nullptr;
        if (!composer) lines << QStringLiteral("composer absent");
        else lines << QStringLiteral("composer selected=%1 waiting=%2 preparing=%3 sending=%4 draftLoading=%5 hasDraft=%6 draftMatches=%7 status=%8").arg(
            flag(composer->sessionKey() == pushKey), flag(composer->isWaitingToSend(pushKey)), flag(composer->isPreparingSending(pushKey)),
            flag(composer->isSending(pushKey)), flag(composer->isDraftLoading(pushKey)), flag(composer->hasDraft(pushKey)),
            flag(composer->draftMatches(pushKey, QStringLiteral("Start over and outline the plan"), {})), bounded(status ? status->text() : QString()));
        lines << QStringLiteral("notice error=%1 text=%2").arg(flag(noticeIsError(window)), bounded(notice(window)));
        for (const auto &session : box.sessions) if (session.name == "codex/hgs/dashboard")
            lines << QStringLiteral("fleet state=%1 run=%2 conversation=%3").arg(session.state, session.runId, session.conversationId);
        lines << QStringLiteral("fixture details %1").arg(fields(fixture.details));
        QFile trace(fixture.script + ".send-trace");
        if (!trace.open(QIODevice::ReadOnly)) lines << QStringLiteral("send-trace absent");
        else for (const auto &line : QString::fromUtf8(trace.read(4096)).split('\n', Qt::SkipEmptyParts).mid(0, 20))
            lines << QStringLiteral("send-trace %1").arg(bounded(line));
        lines << phases;
        if (droppedPhases) lines << QStringLiteral("%1 later phases dropped").arg(QString::number(droppedPhases));
        for (const auto &line : std::as_const(lines)) qWarning().noquote() << "pvfa14" << line;
    });
    // The stopped session starts fresh before it takes the message: the push waits for that run.
    QTimer answer; answer.setInterval(10);
    connect(&answer, &QTimer::timeout, &window, [&] {
        auto *dialog = window.findChild<QMessageBox *>("startToSendConfirm");
        if (!dialog || !dialog->isVisible()) return;
        answer.stop(); dialog->findChild<QPushButton *>("startFreshAndSend")->click();
    });
    answer.start();
    pushMessage(window, "Start over and outline the plan"); if (QTest::currentTestFailed()) return;
    notePhase(QStringLiteral("test pushed"));
    QVERIFY(!answer.isActive());
    QTRY_COMPARE(writes.size(), 1);
    notePhase(QStringLiteral("test write observed"));
    setSession("running", "run-new", {}); window.setFleet(state);
    fixture.details = QJsonObject{{"tracked", true}, {"run_id", "run-new"}, {"runtime_state", "live"}, {"process_state", "running"},
        {"activity", "idle"}, {"phase", "idle"}, {"first_message_can_send", true}, {"events", QJsonArray{}}, {"cursor", 0}};
    inspectPush(window, fixture);
    notePhase(QStringLiteral("test run-new inspection returned failed=%1").arg(flag(QTest::currentTestFailed())));
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout); QCOMPARE(delivered[0][3].toJsonObject()["run_id"].toString(), QString("run-new"));
    // The message began the run's first conversation, which the push adopts.
    fixture.details["conversation_id"] = "conversation-new"; fixture.details["activity"] = "busy"; fixture.details["phase"] = "working";
    fixture.queue("q-fresh", "❯ Start over and outline the plan"); inspectPush(window, fixture);
    notePhase(QStringLiteral("test queue inspection returned failed=%1").arg(flag(QTest::currentTestFailed())));
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-fresh"}, PushTimeout);
}

#ifdef Q_OS_WIN

void TestSessionsWindow::messageStartupFailureRestoresThePendingDraft()
{
    PushFixture fixture; fixture.traceSend = true; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *client = window.findChild<HgsClient *>();
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QVERIFY(client); QVERIFY(composer);
    const QString key = composer->sessionKey(), text = "Keep the draft after a failed start";
    setComposerText(composer, text);
    ComposerDraftStore::flushWrites();
    QVERIFY(!client->inspectionInFlight({}, "codex/hgs/dashboard"));
    // Use the existing executable seam after the real fixture inspection. The
    // separate private file passes prevalidation but fails at CreateProcess.
    const QString program = fixture.dir.filePath("invalid-send-image.exe");
    QFile executable(program); QVERIFY(executable.open(QIODevice::WriteOnly));
    const QByteArray malformed("This is not a PE image.\n");
    QCOMPARE(executable.write(malformed), qint64(malformed.size())); executable.close();
    QVERIFY(QFileInfo(program).isFile()); QVERIFY(QFileInfo(program).isExecutable());
    client->setExecutableForTests(program);
    QSignalSpy failed(client, &HgsClient::messageFailed), sent(client, &HgsClient::messageSent);
    auto *send = composer->findChild<QPushButton *>("sendMessage"); QVERIFY(send); QVERIFY(send->isEnabled());
    send->click();
    QTRY_COMPARE(failed.size(), 1);
    QVERIFY(failed[0][3].toString().startsWith("Cannot start hgs: "));
    QVERIFY(!failed[0][4].toBool());
    // This uses the real SessionsWindow pending-request registration and its
    // finishMessage path: a lost inline result leaves sending set indefinitely.
    QTRY_VERIFY(!composer->isSending(key));
    QCOMPARE(composer->editor()->toPlainText(), text); QVERIFY(composer->hasDraft(key));
    QCOMPARE(sent.size(), 0); QVERIFY(!QFileInfo::exists(fixture.script + ".send-trace"));
    QVERIFY(fixture.sentNow().isEmpty());
}

void TestSessionsWindow::queueStartupFailureReleasesThePendingAction()
{
    PushFixture fixture; fixture.queue("q-private", "❯ Exact queued message"); QVERIFY(fixture.install(script()));
    const QString validProgram = FakeCli::program(fixture.script);
    SessionsWindow window(validProgram); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *client = window.findChild<HgsClient *>();
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *activity = window.findChild<ActivityView *>("mainActivity");
    QVERIFY(client); QVERIFY(composer); QVERIFY(activity);
    const QString text = "Keep the unrelated message draft"; setComposerText(composer, text);
    const QString program = fixture.dir.filePath("invalid-queue-image.exe");
    QFile executable(program); QVERIFY(executable.open(QIODevice::WriteOnly));
    const QByteArray malformed("This is not a PE image.\n");
    QCOMPARE(executable.write(malformed), qint64(malformed.size())); executable.close();
    QVERIFY(QFileInfo(program).isFile()); QVERIFY(QFileInfo(program).isExecutable());
    client->setExecutableForTests(program);
    QSignalSpy finished(client, &HgsClient::queueSendFinished);
    auto *send = activity->findChild<QPushButton *>("queueSendNow"); QVERIFY(send); QVERIFY(send->isEnabled());
    send->click(); QTRY_COMPARE(finished.size(), 1); QVERIFY(!finished[0][1].toBool());
    QVERIFY(!finished[0][3].toString().isEmpty());
    client->setExecutableForTests(validProgram);
    settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", fixture.details);
    if (QTest::currentTestFailed()) return;
    // An ordinary refresh must find this action free after the definite refusal.
    // It never retries send-now or replaces the queued/native request identity.
    QTRY_VERIFY(send->isEnabled()); QCOMPARE(send->text(), QString("Send now"));
    QCOMPARE(finished.size(), 1); QVERIFY(fixture.sentNow().isEmpty());
    QCOMPARE(composer->editor()->toPlainText(), text);
}

void TestSessionsWindow::settingsStartupFailureReleasesThePendingChange()
{
    PushFixture fixture;
    fixture.details["model"] = "gpt-6.1-sol"; fixture.details["effort"] = "high";
    fixture.details["settings_change_supported"] = true; fixture.details["settings_apply_when"] = "ready";
    fixture.details["model_options"] = QJsonArray{QJsonObject{{"id", "gpt-6.1-sol"}, {"effort_options", QJsonArray{"high", "xhigh"}}}};
    QVERIFY(fixture.install(script())); const QString validProgram = FakeCli::program(fixture.script);
    SessionsWindow window(validProgram); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *client = window.findChild<HgsClient *>();
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(client); QVERIFY(composer);
    const QString text = "Keep this draft during the settings change"; setComposerText(composer, text);
    auto *settings = composer->findChild<QPushButton *>("sessionModelSettings"); QVERIFY(settings); QVERIFY(settings->isEnabled());
    settings->click();
    auto *effort = composer->findChild<QComboBox *>("sessionEffortChoice"); QVERIFY(effort);
    const int index = effort->findData("xhigh"); QVERIFY(index >= 0); effort->setCurrentIndex(index);
    auto *apply = composer->findChild<QPushButton *>("applySessionSettings"); QVERIFY(apply); QVERIFY(apply->isEnabled());
    const QString program = fixture.dir.filePath("invalid-settings-image.exe");
    QFile executable(program); QVERIFY(executable.open(QIODevice::WriteOnly));
    const QByteArray malformed("This is not a PE image.\n");
    QCOMPARE(executable.write(malformed), qint64(malformed.size())); executable.close();
    QVERIFY(QFileInfo(program).isFile()); QVERIFY(QFileInfo(program).isExecutable());
    client->setExecutableForTests(program);
    QSignalSpy finished(client, &HgsClient::settingsFinished);
    apply->click(); QTRY_COMPARE(finished.size(), 1); QVERIFY(!finished[0][1].toBool());
    QVERIFY(!finished[0][3].toString().isEmpty());
    client->setExecutableForTests(validProgram);
    settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", fixture.details);
    if (QTest::currentTestFailed()) return;
    QTRY_VERIFY(settings->isEnabled()); QVERIFY(!settings->toolTip().contains("Updating model settings"));
    QCOMPARE(finished.size(), 1); QCOMPARE(composer->editor()->toPlainText(), text);
}

#endif

void TestSessionsWindow::pushStaysWithItsSession()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QSignalSpy delivered(client, &HgsClient::messageSent);
    pushMessage(window, "Draft the release notes for v2"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    // Its session is no longer inspected once another one is selected: the push ends at once.
    fixture.queue("q-other-session", "❯ Draft the release notes for v2"); QVERIFY(fixture.save());
    window.showSession({}, "kimi/docs/research");
    QCOMPARE(notice(window), QString("The message to hgs / dashboard was not pushed: another session was selected"));
    QVERIFY(!noticeIsError(window));
    // Another session's queue is never sent, even when it shows the same text.
    settleSelectedInspection(composer, client, {}, "kimi/docs/research", fixture.details);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
}

void TestSessionsWindow::pushTimesOut()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    pushMessage(window, "Wait for the agent to finish the build"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    fixture.queue("q-other", "❯ Something else"); inspectPush(window, fixture);
    // A snapshot that arrives after the deadline never sends the queue, even one holding the message.
    window.setPushTimeoutForTests(0);
    fixture.queue("q-late", "❯ Wait for the agent to finish the build"); inspectPush(window, fixture);
    QTRY_COMPARE(notice(window), QString("The message was sent without push: the agent did not offer to send it now within 20 seconds"));
    QVERIFY(!noticeIsError(window));
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
}

void TestSessionsWindow::pushOutlastedBySlowSend()
{
    PushFixture fixture; QVERIFY(fixture.install(script(), PushFixture::Send::Slow));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    pushMessage(window, "Rebuild the documentation site"); if (QTest::currentTestFailed()) return;
    // The push gives up while the send is still on its way; the message itself still goes out.
    window.setPushTimeoutForTests(0);
    QTRY_COMPARE(notice(window), QString("The message will be sent without push: it was not delivered within 20 seconds"));
    QVERIFY(!noticeIsError(window)); QCOMPARE(delivered.size(), 0);
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, 15000);
    fixture.queue("q-slow", "❯ Rebuild the documentation site"); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
}

void TestSessionsWindow::pushOfAttachmentsAloneSendsWithoutPush()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    setComposerText(composer, {}); QVERIFY(composer->addAttachment("notes.txt", "text/plain", "release notes"));
    QCOMPARE(composer->editor()->toPlainText().trimmed(), QString("[File #1]"));
    pressPush(window); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    QTRY_COMPARE(notice(window), QString("Sent without push: a message without text cannot be found in the agent's queue"));
    QVERIFY(!noticeIsError(window));
    fixture.queue("q-file", "❯ [File #1] (Read the attached file at \"/fixture/notes.txt\")"); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
}

void TestSessionsWindow::pushDropsWhenSendFails_data()
{
    QTest::addColumn<bool>("uncertain"); QTest::addColumn<QString>("expected");
    QTest::newRow("refused") << false << QStringLiteral("The message was not pushed: sending did not complete");
    QTest::newRow("uncertain") << true << QStringLiteral("The message was not pushed: delivery is not confirmed");
}

void TestSessionsWindow::pushDropsWhenSendFails()
{
    QFETCH(bool, uncertain); QFETCH(QString, expected);
    PushFixture fixture; QVERIFY(fixture.install(script(), uncertain ? PushFixture::Send::Uncertain : PushFixture::Send::Refuse));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QSignalSpy failed(window.findChild<HgsClient *>(), &HgsClient::messageFailed);
    pushMessage(window, "This message is not delivered"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE(failed.size(), 1); QCOMPARE(failed[0][4].toBool(), uncertain);
    QTRY_COMPARE(notice(window), expected); QVERIFY(noticeIsError(window));
    fixture.queue("q-old", "❯ This message is not delivered"); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
    QTRY_COMPARE(composer->editor()->toPlainText(), QString("This message is not delivered"));
}

void TestSessionsWindow::pushBehindADialogIsNotSentAtTheDeadline()
{
    PushFixture fixture; fixture.details["cache_hint"] = QJsonObject{{"status", "cold"}}; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    // The cold cache warning holds the message: the push gives up while it is open,
    // and does not claim the message will be sent.
    const QString expected("The message was not pushed: it was not sent within 20 seconds");
    QString shown; bool error = true; int ticks = 0;
    QTimer answer; answer.setInterval(10);
    connect(&answer, &QTimer::timeout, &window, [&] {
        auto *dialog = window.findChild<QMessageBox *>("coldCacheConfirm");
        if (!dialog || !dialog->isVisible()) return;
        window.setPushTimeoutForTests(0);
        if (notice(window) != expected && ++ticks < 1000) return;
        shown = notice(window); error = noticeIsError(window);
        answer.stop(); dialog->button(QMessageBox::Cancel)->click();
    });
    answer.start();
    pushMessage(window, "Wait behind the cold cache warning"); if (QTest::currentTestFailed()) return;
    QVERIFY(!answer.isActive());
    QCOMPARE(shown, expected); QVERIFY(!error);
    QTest::qWait(150); QCOMPARE(delivered.size(), 0); QCOMPARE(fixture.sentNow(), QStringList());
}

void TestSessionsWindow::pushReplacesAnEarlierPush()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    pushMessage(window, "First message to push"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    pushMessage(window, "Second message to push"); if (QTest::currentTestFailed()) return;
    QCOMPARE(notice(window), QString("The earlier push was replaced by a new one")); QVERIFY(!noticeIsError(window));
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 2, PushTimeout);
    // Only the new push sends: a queue with the earlier message alone stays.
    fixture.queue("q-first", "❯ First message to push"); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
    fixture.queue("q-both", "❯ First message to push\n❯ Second message to push"); inspectPush(window, fixture);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-both"}, PushTimeout);
}

void TestSessionsWindow::pushWaitsForARunningSendNow()
{
    PushFixture fixture; fixture.queue("q-old", "❯ Earlier queued text");
    QVERIFY(fixture.install(script(), PushFixture::Send::Confirm, true));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QSignalSpy delivered(client, &HgsClient::messageSent), queued(client, &HgsClient::queueSendFinished);
    // The gated send-now request can outlast the ordinary deadline on a loaded machine.
    window.setPushTimeoutForTests(60000);
    pushMessage(window, "Push this after the earlier queue"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    // A send-now request for the earlier queue is still running when the agent queues the message.
    composer->editor()->setFocus(); QTRY_VERIFY(composer->editor()->hasFocus());
    pressCommand(&window, "queue.sendNow"); QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-old"}, PushTimeout);
    fixture.queue("q-push", "❯ Earlier queued text\n❯ Push this after the earlier queue"); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList{"q-old"});
    QVERIFY2(queued.isEmpty(), "The earlier send-now request must still be running");
    QVERIFY(notice(window) != QString("Nothing queued can be sent now"));
    // Once it has finished, a later snapshot sends the queue with the message.
    QVERIFY(fixture.release());
    QTRY_COMPARE_WITH_TIMEOUT(queued.size(), 1, 15000);
    inspectPush(window, fixture);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), (QStringList{"q-old", "q-push"}), PushTimeout);
}

void TestSessionsWindow::pushIgnoresTheCodexQueueHeader()
{
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    pushMessage(window, "interrupt"); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    // Codex's queue header says "interrupt"; only a queued item that begins with the message holds it.
    const QString header("• Messages to be submitted after next tool call (press esc to interrupt and send immediately)");
    fixture.queue("q-header", header + "\n  ↳ Check the build first"); inspectPush(window, fixture);
    fixture.queue("q-wrapped", "• Messages to be submitted after next tool call (press esc to\n  interrupt and send immediately)\n  ↳ Check the build first");
    inspectPush(window, fixture);
    fixture.queue("q-follow-up", "• Queued follow-up inputs\n  ↳ Check the build first\n    interrupt"); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
    fixture.queue("q-match", header + "\n  ↳ Check the build first\n  ↳ interrupt"); inspectPush(window, fixture);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-match"}, PushTimeout);
}

void TestSessionsWindow::pushFindsAMessageThatStartsWithAnAttachment_data()
{
    QTest::addColumn<QString>("name"); QTest::addColumn<QString>("mime"); QTest::addColumn<QString>("reference");
    QTest::addColumn<QString>("instruction"); QTest::addColumn<QString>("path");
    QTest::newRow("file") << QStringLiteral("notes.txt") << QStringLiteral("text/plain") << QStringLiteral("[File #1]")
                          << QStringLiteral("Read the attached file at") << QStringLiteral("\"/fixture/hgs/attachments/request-one/notes.txt\"");
    // The CLI quotes the path as JSON, so Windows backslashes arrive escaped.
    QTest::newRow("image") << QStringLiteral("shot.png") << QStringLiteral("image/png") << QStringLiteral("[Image #1]")
                           << QStringLiteral("Inspect the attached image at") << QStringLiteral(R"("C:\\fixture\\hgs\\attachments\\shot.png")");
}

void TestSessionsWindow::pushFindsAMessageThatStartsWithAnAttachment()
{
    QFETCH(QString, name); QFETCH(QString, mime); QFETCH(QString, reference); QFETCH(QString, instruction); QFETCH(QString, path);
    PushFixture fixture; QVERIFY(fixture.install(script()));
    SessionsWindow window(FakeCli::program(fixture.script)); openPushSession(window, fleet(), fixture);
    if (QTest::currentTestFailed()) return;
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QSignalSpy delivered(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    // Attached first, then typed.
    setComposerText(composer, {}); QVERIFY(composer->addAttachment(name, mime, "fixture bytes"));
    composer->editor()->moveCursor(QTextCursor::End); composer->editor()->insertPlainText("Check this");
    QCOMPARE(composer->editor()->toPlainText(), reference + " Check this");
    pressPush(window); if (QTest::currentTestFailed()) return;
    QTRY_COMPARE_WITH_TIMEOUT(delivered.size(), 1, PushTimeout);
    // The CLI pastes each reference followed by the file's location (src/state/attachments.rs).
    const QString pasted = reference + " (" + instruction + ' ' + path + ") Check this";
    // Only an item that begins with the message holds it: not a header row, not
    // the middle of another item, not another message's attachment.
    fixture.queue("q-header", "• " + pasted + "\n  ↳ Rename the release branch"); inspectPush(window, fixture);
    fixture.queue("q-middle", "❯ Compare the release notes " + pasted); inspectPush(window, fixture);
    fixture.queue("q-continued", "❯ Compare the release notes\n  " + pasted); inspectPush(window, fixture);
    fixture.queue("q-other", "❯ " + QString(pasted).replace("#1]", "#2]")); inspectPush(window, fixture);
    QTest::qWait(150); QCOMPARE(fixture.sentNow(), QStringList());
    // The agent's terminal wraps the location onto the next row.
    fixture.queue("q-match", "❯ Rename the release branch\n❯ " + reference + " (" + instruction + "\n  " + path + ") Check this");
    inspectPush(window, fixture);
    QTRY_COMPARE_WITH_TIMEOUT(fixture.sentNow(), QStringList{"q-match"}, PushTimeout);
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_push.moc"
