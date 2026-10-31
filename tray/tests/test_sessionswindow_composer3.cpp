// U62 (#712): Activity and the composer: drafts, delivery, recovery and cold caches, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void copySessionReferencePreservesSelectionAndDraft();
    void activityMarkReadKeepsReadingPosition_data();
    void activityMarkReadKeepsReadingPosition();
    void resumeBehindOffersTheLostDialogue();
    void startAndSendSurvivesTheStaleResume();
    void activityEscapingAndActions();
    void providerErrorWaitsForNativePrompt_data();
    void providerErrorWaitsForNativePrompt();
    void quotaFailureExplainsRecoveryAndKeepsDraft();
    void firstMessageWaitsForNativePrompt_data();
    void firstMessageWaitsForNativePrompt();
    void resumedConversationPausesAndClearsOnceVerified();
    void failedMessagesReconcileAndRetryWithoutLosingDrafts();
    void scheduledSettingsApplyOncePerReady_data();
    void scheduledSettingsApplyOncePerReady();
    void inspectionBehindSettingsReceiptKeepsTheChange();
    void activityAccountPickerSwitchesTheSession();
    void externalQueueSurvivesDurableDraftRecheck();
    void escapeInActivityInterruptsAndRestoresPrompt();
    void stopNoticesExplainOriginalTargetAndCanBeDismissed_data();
    void stopNoticesExplainOriginalTargetAndCanBeDismissed();
    void escapeInterruptsFromAnywhereAndNeverCloses();
    void suggestedMessageIsPlaceholderAndTabTakesIt();
};

void TestSessionsWindow::copySessionReferencePreservesSelectionAndDraft()
{
    QTemporaryDir fixture;QVERIFY(fixture.isValid());const auto program=fixture.filePath("hgs");
    QFile file(program);QVERIFY(file.open(QIODevice::WriteOnly));
    const auto uri=QStringLiteral("zerus://session?machine=11111111-1111-4111-8111-111111111111&host=fixture&name=codex%2Fhgs%2Fdashboard&conversation=conversation-one&run=run");
    file.write("#!/usr/bin/env python3\nimport runpy,sys\nif sys.argv[1:2]==['reference']:\n"
        " open(__file__+'.args','w').write(''.join(v+'\\n' for v in sys.argv[1:]));sys.stdout.write(open(__file__+'.reference').read())\n"
        "else:\n sys.argv[0]="+FakeCli::pythonString(script())+";runpy.run_path(sys.argv[0],run_name='__main__')\n");file.close();
    QVERIFY(FakeCli::makeRunnable(file.fileName()));
    QFile response(program+".reference");QVERIFY(response.open(QIODevice::WriteOnly));response.write(QJsonDocument(QJsonObject{{"uri",uri}}).toJson());response.close();
    auto state=fleet();auto box=state.local();box.sessions[0].runId="run";box.sessions[0].conversationId="conversation-one";state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program));window.setFleet(state);window.show();window.showSession({},box.sessions[0].name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer, "Keep this draft");
    QSignalSpy copied(&window,&SessionsWindow::copyTextRequested),activated(&window,&SessionsWindow::sessionActivated);
    QVERIFY(!window.findChild<QPushButton *>("copySessionReferenceAction"));
    auto *list=window.findChild<QListWidget *>("sessionList");
    auto *menu=window.findChild<QMenu *>("sessionContextMenu");QVERIFY(menu);
    const auto referenceAction=[&]() {
        const QPoint point=list->visualItemRect(list->currentItem()).center();
        QContextMenuEvent event(QContextMenuEvent::Mouse,point,list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(),&event);
        return menu->findChild<QAction *>("contextCopyReference");
    };
    auto *action=referenceAction();QVERIFY(action);QVERIFY(action->isEnabled());action->trigger();menu->hide();
    QTRY_COMPARE(copied.size(),1);QCOMPARE(copied[0][0].toString(),uri);QCOMPARE(activated.size(),0);QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    QFile args(program+".args");QVERIFY(args.open(QIODevice::ReadOnly));QCOMPARE(args.readAll(),QByteArray("reference\ncodex/hgs/dashboard\n--json\n--run\nrun\n--conversation\nconversation-one\n"));
    box.ok=false;state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    action=referenceAction();QVERIFY(action);QVERIFY(!action->isEnabled());menu->hide();
}

void TestSessionsWindow::activityMarkReadKeepsReadingPosition_data()
{
    QTest::addColumn<QString>("theme");QTest::newRow("dark")<<QString("dark");QTest::newRow("light")<<QString("light");
}

void TestSessionsWindow::activityMarkReadKeepsReadingPosition()
{
    QFETCH(QString,theme);QSettings().setValue("workspace/theme",theme);
    auto state=fleet();auto box=state.local();auto &session=box.sessions[0];session.conversationId="conversation-one";
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());QVERIFY(state.setReviewLater({},session,true));
    setJsonSetting("attention/sessionMarks",QJsonDocument(state.attentionMarks()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program());window.resize(1000,760);window.setFleet(state);window.show();
    auto *client=window.findChild<HgsClient *>();QSignalSpy inspections(client,&HgsClient::inspectionReady);
    window.showSession({},session.name);WAIT_VERIFY(!inspections.isEmpty());
    QJsonArray events;for(int i=1;i<=30;++i)events<<QJsonObject{{"seq",i},{"at",1791018000+i},{"type","AgentMessage"},{"detail",QString("Read message %1\n\nKeep this position and selection.").arg(i)}};
    client->inspectionReady({},session.name,{{"tracked",true},{"conversation_id",session.conversationId},{"phase","working"},{"activity","busy"},{"cursor",30},{"events",events},
        {"session_usage",QJsonObject{{"context",QJsonObject{{"used",139000},{"limit",258400}}}}}});
    auto *button=window.findChild<QPushButton *>("activityMarkRead");QVERIFY(button);QVERIFY(button->isVisible());QVERIFY(!button->icon().isNull());
    QCOMPARE(static_cast<ToolbarChip *>(button)->tone(),ChipTone::Accent);
    auto *browser=window.findChild<ActivityView *>("mainActivity")->browser();auto *composer=window.findChild<MessageComposer *>("messageComposer");
    setComposerText(composer, "Unsent draft");WAIT_VERIFY(!window.findChild<ActivityView *>("mainActivity")->renderPending());
    const auto initialGeometry=browser->geometry();
    browser->setTextCursor(browser->document()->find("Read message 1"));browser->verticalScrollBar()->setValue(0);QTest::qWait(20);
    const auto selection=browser->textCursor().selectedText();QVERIFY(!selection.isEmpty());
    const auto geometry=browser->geometry();const auto revision=browser->document()->revision();
    auto *context=window.findChild<QPushButton *>("activityContext");
    auto *latest=window.findChild<ActivityView *>("mainActivity")->jumpButton();QVERIFY(latest->isVisible());
    const auto bounds=[&](QWidget *widget){return QRect(widget->mapTo(&window,QPoint()),widget->size());};
    // Jump to latest shares the existing toolbar and cannot cover a message.
    QCOMPARE(latest->parentWidget(),composer->toolbar());QVERIFY(latest->text().isEmpty());QVERIFY(!latest->icon().isNull());
    QVERIFY(latest->toolTip().contains("Jump to latest"));QVERIFY(bounds(latest).top()>bounds(browser).bottom());
    QVERIFY(qAbs(bounds(latest).center().y()-bounds(context).center().y())<=1);
    QVERIFY(qAbs(bounds(button).center().y()-bounds(context).center().y())<=1);
    QVERIFY(bounds(button).top()>bounds(browser).bottom());QVERIFY(bounds(button).right()<bounds(context).left());
    QCOMPARE(browser->geometry(),initialGeometry);
    QVERIFY(bounds(button).right()<bounds(context).left());
    QCOMPARE(button->height(),24);
    QVERIFY(qAbs(bounds(context).right()-bounds(composer).right())<=2);
    for(int poll=0;poll<3;++poll){window.setFleet(state);QVERIFY(button->isVisible());}
    for(auto *filter:window.findChildren<QPushButton *>("sessionFilter"))if(filter->property("filter")=="attention")filter->click();
    const auto preview=qEnvironmentVariable("HGS_ATTENTION_PREVIEW");
    if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+"/mark-read-"+theme+".png"));}
    QSignalSpy marks(&window,&SessionsWindow::attentionMarksChanged);button->click();QCOMPARE(marks.size(),1);QVERIFY(button->isHidden());
    QCoreApplication::sendPostedEvents(nullptr,QEvent::LayoutRequest);
    QCOMPARE(browser->geometry(),geometry);QCOMPARE(browser->document()->revision(),revision);QCOMPARE(browser->verticalScrollBar()->value(),0);
    QCOMPARE(browser->textCursor().selectedText(),selection);QCOMPARE(composer->editor()->toPlainText(),QString("Unsent draft"));
    QVERIFY(!latest->isVisible() || bounds(composer->toolbar()).contains(bounds(latest)));
    auto *list=window.findChild<SessionList *>("sessionList");QVERIFY(list->currentItem()->data(SessionRoles::Key).toString().contains(session.name));
    window.setFleet(state);QVERIFY(button->isHidden());
    session.phase="input";session.activity="waiting";session.attentionId="new-question";state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);QVERIFY(button->isVisible());
    button->click();QVERIFY(button->isHidden());
    // Manual reminders remain actionable even while the machine is offline.
    QVERIFY(state.setReviewLater({},session,true));setJsonSetting("attention/sessionMarks",QJsonDocument(state.attentionMarks()).toJson(QJsonDocument::Compact));
    box.ok=false;state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);QVERIFY(button->isVisible());button->click();QVERIFY(button->isHidden());
    QTest::qWait(20);const auto beforeJump=browser->geometry();
    latest->click();QTest::qWait(20);QVERIFY(latest->isHidden());QCOMPARE(browser->geometry(),beforeJump);
    QCOMPARE(browser->verticalScrollBar()->value(),browser->verticalScrollBar()->maximum());
}

// U3: hgs refuses a stale resume; the dialog shows both times and the missing
// count, offers the lost dialogue as the default, and each choice sends its own
// flag (or nothing for Cancel).
void TestSessionsWindow::resumeBehindOffersTheLostDialogue()
{
    QTemporaryDir temp; const auto program = temp.filePath("hgs"), log = temp.filePath("resume.log");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(QStringLiteral(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a[0].startswith('@'):a=a[1:]
if a[0]=='inspect':print((root/'details.json').read_text() if (root/'details.json').exists() else '{}')
elif a[0]=='resume':
 (root/'resume.log').open('a').write(json.dumps(sys.argv[1:])+'\n')
 if (root/'behind').exists():
  if (root/'plain').exists():
   print('codex/hgs/dashboard: the native transcript stops 14400 s before the events Zerus recorded (transcript_behind); resume with --accept-stale or --with-lost-dialogue', file=sys.stderr)
  else:
   print(json.dumps({'error':'transcript_behind','transcript_at':1792868400,'events_at':1792869000,'missing':3}), file=sys.stderr)
  sys.exit(1)
 print(json.dumps({'status':'resumed','request_id':'r'}))
else:print('{}')
)PY").toUtf8());
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QFile behind(temp.filePath("behind")); QVERIFY(behind.open(QIODevice::WriteOnly)); behind.write("1"); behind.close();
    { QFile details(temp.filePath("details.json")); QVERIFY(details.open(QIODevice::WriteOnly));
      details.write(QJsonDocument(QJsonObject{{"tracked", true}, {"run_id", "run-behind"}, {"conversation_id", "conversation-behind"},
          {"runtime_state", "saved"}, {"process_state", "exited"}, {"resumable", true}}).toJson()); }
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) if (session.name == "codex/hgs/dashboard") { session.state = "paused"; session.runId = "run-behind"; session.conversationId = "conversation-behind"; }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    auto *menu = window.findChild<QMenu *>("sessionContextMenu"); QVERIFY(menu);
    int row = -1; for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString().endsWith("codex/hgs/dashboard")) row = i;
    QVERIFY(row >= 0);
    list->setCurrentRow(row);
    const auto resumes = [&] { QFile file(log); return file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()).split('\n', Qt::SkipEmptyParts) : QStringList(); };
    const auto answer = [&](const QString &button) {
        auto *poll = new QTimer(&window); poll->setInterval(5); auto *age = new QElapsedTimer; age->start();
        QObject::connect(poll, &QTimer::timeout, &window, [poll, age, &window, button] {
            auto *dialog = window.findChild<QMessageBox *>("resumeBehindConfirm");
            if (!(dialog && dialog->isVisible()) && age->elapsed() < 5000) return;
            poll->stop(); poll->deleteLater(); delete age;
            if (!dialog || !dialog->isVisible()) return;
            QCOMPARE(dialog->text().contains(QStringLiteral("stops at")), true);
            if (button == "cancel") dialog->button(QMessageBox::Cancel)->click();
            else if (auto *wanted = dialog->findChild<QPushButton *>(button)) wanted->click();
        });
        poll->start();
    };
    const auto pausedFleet = [&] {
        auto st = fleet(); auto b = st.local();
        for (auto &s : b.sessions) if (s.name == "codex/hgs/dashboard") { s.state = "paused"; s.runId = "run-behind"; s.conversationId = "conversation-behind"; }
        st.setLocal(b, QDateTime::currentMSecsSinceEpoch());
        return st;
    };
    const auto waitFlag = [&](const QString &flag) {
        QElapsedTimer t; t.start();
        while (t.elapsed() < 8000 && !(resumes().isEmpty() ? false : resumes().last().contains(flag))) QTest::qWait(20);
    };
    // Each scenario gets a fresh window, so no refused-resume state carries over.
    const auto scenario = [&](const QString &button, bool shot, bool *sawDefault, const QString &flag) {
        SessionsWindow w(FakeCli::program(program)); w.resize(1280, 760); w.setFleet(pausedFleet()); w.show();
        auto *l = w.findChild<QListWidget *>("sessionList"); auto *m = w.findChild<QMenu *>("sessionContextMenu"); QVERIFY(l && m);
        int r = -1; for (int i = 0; i < l->count(); ++i) if (l->item(i)->data(SessionRoles::Key).toString().endsWith("codex/hgs/dashboard")) r = i;
        QVERIFY(r >= 0); l->setCurrentRow(r); m->hide(); QTest::keyClick(l, Qt::Key_Menu);
        auto *action = m->findChild<QAction *>("contextChangeSession"); QVERIFY(action);
        auto *poll = new QTimer(&w); poll->setInterval(5); auto *age = new QElapsedTimer; age->start();
        QObject::connect(poll, &QTimer::timeout, &w, [poll, age, &w, button, shot, sawDefault] {
            auto *dialog = w.findChild<QMessageBox *>("resumeBehindConfirm");
            if (!(dialog && dialog->isVisible()) && age->elapsed() < 5000) return;
            poll->stop(); poll->deleteLater(); delete age;
            if (!dialog || !dialog->isVisible()) return;
            QVERIFY(dialog->text().contains(QStringLiteral("transcript")));
            if (sawDefault) *sawDefault = dialog->defaultButton() && dialog->defaultButton()->objectName() == "resumeLostDialogue";
            if (shot) { if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty()) dialog->grab().save(QDir(shots).filePath("resume-behind.png")); }
            if (button == "cancel") dialog->button(QMessageBox::Cancel)->click();
            else if (auto *wanted = dialog->findChild<QPushButton *>(button)) wanted->click();
        });
        poll->start();
        action->trigger();
        QElapsedTimer waited; waited.start();
        while (waited.elapsed() < 10000) {
            QTest::qWait(20);
            if (!flag.isEmpty() && !resumes().isEmpty() && resumes().last().contains(flag)) break;
        }
    };
    // Cancel: the refused attempt sends nothing more.
    int before = resumes().size();
    scenario("cancel", false, nullptr, {});
    QCOMPARE(resumes().size(), before + 1);
    // The plain-text refusal (no JSON times) still shows the dialog and sends
    // --accept-stale.
    scenario("resumeAsIs", false, nullptr, "--accept-stale");
    QVERIFY2(resumes().last().contains("--accept-stale"), qPrintable(resumes().join(QStringLiteral(" | "))));
    // The JSON refusal with the times: the lost dialogue is the default.
    { QFile plain(temp.filePath("plain")); QVERIFY(plain.open(QIODevice::WriteOnly)); plain.write("1"); plain.close(); }
    bool sawDefault = false;
    scenario("resumeLostDialogue", true, &sawDefault, "--with-lost-dialogue");
    QVERIFY2(resumes().last().contains("--with-lost-dialogue"), qPrintable(resumes().join(QStringLiteral(" | "))));
    QVERIFY(sawDefault);
}

// L5: a queued start-and-send survives the stale-resume refusal: after the
// owner chooses in the dialog, the message is sent once the session is ready.
void TestSessionsWindow::startAndSendSurvivesTheStaleResume()
{
    QTemporaryDir temp; const auto program = temp.filePath("hgs");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a[0].startswith('@'):a=a[1:]
def details():
    name='ready.json' if (root/'ready').exists() else 'details.json'
    return (root/name).read_text() if (root/name).exists() else '{}'
if a[0]=='inspect':print(details())
elif a[0]=='resume':
    (root/'resume.log').open('a').write(json.dumps(sys.argv[1:])+'\n')
    forced = any(x in a for x in ('--with-lost-dialogue','--accept-stale'))
    if (root/'behind').exists() and not forced:
        print('the native transcript stops 14400 s before the events Zerus recorded (transcript_behind)', file=sys.stderr); sys.exit(1)
    (root/'behind').unlink(missing_ok=True)
    (root/'ready').write_text('1')
    print(json.dumps({'status':'resumed','request_id':'r'}))
elif a[0]=='send':
    p=json.load(sys.stdin)
    (root/'send.log').open('a').write(json.dumps({'name':a[1],'text':p.get('text','')})+'\n')
    print(json.dumps(dict(request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],status='submitted')))
else:print('{}')
)PY");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    { QFile behind(temp.filePath("behind")); QVERIFY(behind.open(QIODevice::WriteOnly)); behind.write("1"); }
    const QJsonObject savedDetails{{"tracked", true}, {"run_id", "run-l5"}, {"conversation_id", "conversation-l5"},
        {"runtime_state", "saved"}, {"process_state", "exited"}, {"resumable", true}};
    { QFile details(temp.filePath("details.json")); QVERIFY(details.open(QIODevice::WriteOnly));
      details.write(QJsonDocument(savedDetails).toJson()); }
    const QJsonObject readyDetails{{"tracked", true}, {"run_id", "run-l5b"}, {"conversation_id", "conversation-l5"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"phase", "idle"}, {"activity", "idle"}};
    { QFile ready(temp.filePath("ready.json")); QVERIFY(ready.open(QIODevice::WriteOnly));
      // A resume starts a new run in the same conversation.
      ready.write(QJsonDocument(readyDetails).toJson()); }
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) if (session.name == "codex/hgs/dashboard") { session.state = "paused"; session.runId = "run-l5"; session.conversationId = "conversation-l5"; }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    int row = -1; for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString().endsWith("codex/hgs/dashboard")) row = i;
    QVERIFY(row >= 0); list->setCurrentRow(row);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    auto *client = window.findChild<HgsClient *>(); QVERIFY(client);
    QSignalSpy writes(client, &HgsClient::writeDone);
    QTRY_VERIFY(composer->sessionKey().endsWith("codex/hgs/dashboard"));
    // Resume is enabled by the listing; its async guard also needs inspected identity.
    settleSelectedInspection(composer, client, {}, QStringLiteral("codex/hgs/dashboard"), savedDetails);
    setComposerText(composer, QStringLiteral("carry on"));
    auto *send = composer->findChild<QPushButton *>("sendMessage"); QVERIFY(send);
    QTRY_VERIFY(send->isEnabled());
    // The start-and-send path can ask up to three times: the start choice, the
    // other-Windows-session warning, and the stale-resume choice. One poller
    // answers whichever is up, starting before the click.
    auto *poll = new QTimer(&window); poll->setInterval(5); auto *age = new QElapsedTimer; age->start();
    QObject::connect(poll, &QTimer::timeout, &window, [poll, age] {
        if (auto *modal = QApplication::activeModalWidget()) {
            for (const char *name : {"resumeAndSend", "launchAnyway", "resumeLostDialogue"})
                if (auto *button = modal->findChild<QPushButton *>(QString::fromLatin1(name)); button && button->isVisible()) {
                    button->click();
                    return;
                }
            return;
        }
        if (age->elapsed() > 20000) { poll->stop(); poll->deleteLater(); delete age; }
    });
    poll->start();
    send->click();
    const auto resumeLog = [&] { QFile file(temp.filePath("resume.log")); return file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString(); };
    const auto sendLog = [&] { QFile file(temp.filePath("send.log")); return file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString(); };
    // The start choice, a refused resume, the stale choice and the forced
    // resume take more than QTRY's 5 s on a loaded Windows runner.
    QTRY_VERIFY2_WITH_TIMEOUT(resumeLog().contains("--with-lost-dialogue"), qPrintable(resumeLog()), 30000);
    // L5: the queued message is sent once the session is ready - and only once.
    QTRY_VERIFY(std::any_of(writes.cbegin(), writes.cend(), [](const QList<QVariant> &write) {
        return write[0].toString() == QStringLiteral("resume codex/hgs/dashboard") && write[1].toBool();
    }));
    // Deterministically deliver the new inspection before the listing catches
    // up. It must retain the queued draft instead of attempting a send against
    // the old paused row and cancelling its continuation.
    client->inspectionReady({}, QStringLiteral("codex/hgs/dashboard"), readyDetails);
    QTRY_VERIFY(composer->isWaitingToSend(composer->sessionKey()));
    QVERIFY(sendLog().isEmpty());
    // The resumed session is read now that it is live; the queued draft follows.
    for (auto &session : box.sessions) if (session.name == "codex/hgs/dashboard") {
        session.state = "running"; session.runId = "run-l5b"; session.conversationId = "conversation-l5";
        session.phase = "idle"; session.activity = "idle"; session.processState = "running";
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    window.showSession({}, QStringLiteral("codex/hgs/dashboard"));
    client->inspectionReady({}, QStringLiteral("codex/hgs/dashboard"), readyDetails);
    auto *status = composer->findChild<QLabel *>("messageStatus");
    QTRY_VERIFY2(sendLog().contains("carry on"), qPrintable(status ? status->text() + QStringLiteral(" | ") + sendLog() : sendLog()));
    QCOMPARE(sendLog().count(QStringLiteral("carry on")), 1);
}

void TestSessionsWindow::activityEscapingAndActions()
{
    QJsonObject data{{"tracked", true}, {"conversation_id", "conversation-one"}, {"phase", "idle"}, {"activity", "idle"},
        {"cwd", "/workspace/zerus"}, {"prompt", "<b>literal request</b>"}, {"cursor", 5},
        {"events", QJsonArray{QJsonObject{{"seq", 4}, {"at", 1790928000}, {"type", "PostToolUse"}, {"tool", "Bash"}, {"detail", "<img src='file:///not-loaded'>"}},
            QJsonObject{{"seq", 5}, {"at", 1790928001}, {"type", "Stop"}, {"detail", QString("A response paragraph.\n\n").repeated(60)}}}}};
    const auto fixture = programInspecting("activity-actions-hgs", data); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(fixture); window.setFleet(fleet()); window.setClipboardMode(true); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); list->setCurrentRow(0);
    auto *client = window.findChild<HgsClient *>(); QVERIFY(client);
    client->inspectionReady({}, "codex/hgs/dashboard", data);
    auto *activity = window.findChild<QTextBrowser *>("activity");
    QTRY_VERIFY(activity->toPlainText().contains("<b>literal request</b>"));
    QTRY_VERIFY(activity->toPlainText().contains("<img src='file:///not-loaded'>"));
    QVERIFY(window.findChild<QAction *>("pauseAction")->isEnabled());
    QSignalSpy opened(&window, &SessionsWindow::sessionActivated);
    QSignalSpy copied(&window, &SessionsWindow::copySessionCommandRequested);
    auto *view = window.findChild<ActivityView *>("mainActivity");
    auto *scroll = activity->verticalScrollBar(); QTRY_VERIFY(scroll->maximum() > 0);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Keep my draft");
    scroll->setValue(0);
    auto cursor = activity->textCursor(); cursor.movePosition(QTextCursor::Start); cursor.movePosition(QTextCursor::NextCharacter, QTextCursor::KeepAnchor, 5);
    activity->setTextCursor(cursor);
    const auto selectionAnchor = cursor.anchor(), selectionPosition = cursor.position();
    const auto selectedText = cursor.selectedText();
    const auto selectedKey = list->currentItem()->data(SessionRoles::Key).toString();
    const bool initiallyCollapsed = list->currentItem()->data(SessionRoles::RowCollapsed).toBool();
    const auto point = list->visualItemRect(list->currentItem()).center();
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, point);
    QCOMPARE(scroll->value(), 0); QVERIFY(activity->textCursor().hasSelection());
    // Double-click changes only row density, including the release at the
    // original point after the row's height changes. Keep the reading position.
    QTest::mouseDClick(list->viewport(), Qt::LeftButton, {}, point);
    QTest::mouseRelease(list->viewport(), Qt::LeftButton, {}, point);
    QCOMPARE(list->currentItem()->data(SessionRoles::RowCollapsed).toBool(), !initiallyCollapsed);
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), selectedKey); QVERIFY(list->currentItem()->isSelected());
    QCOMPARE(scroll->value(), 0);
    QCOMPARE(activity->textCursor().anchor(), selectionAnchor); QCOMPARE(activity->textCursor().position(), selectionPosition);
    QCOMPARE(activity->textCursor().selectedText(), selectedText);
    QCOMPARE(opened.size(), 0); QCOMPARE(copied.size(), 0); QCOMPARE(composer->editor()->toPlainText(), QString("Keep my draft"));
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); tabs->setCurrentIndex(1);
    scroll->setValue(0); QTest::keyClick(list, Qt::Key_Return);
    QCOMPARE(tabs->currentIndex(), 0); QTRY_COMPARE(scroll->value(), scroll->maximum());
    QVERIFY(!activity->textCursor().hasSelection()); QVERIFY(view->jumpButton()->isHidden());
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), selectedKey);
    QCOMPARE(opened.size(), 0); QCOMPARE(copied.size(), 0); QCOMPARE(composer->editor()->toPlainText(), QString("Keep my draft"));
    window.findChild<QAction *>("openSessionAction")->trigger(); QCOMPARE(opened.size(), 1);
    QCOMPARE(opened[0][1].toString(), QStringLiteral("codex/hgs/dashboard"));
    QSignalSpy completed(client, &HgsClient::writeDone);
    auto *pause = window.findChild<QAction *>("pauseAction"); pause->trigger();
    QVERIFY(!pause->isEnabled()); QTRY_COMPARE(completed.size(), 1);
    QVERIFY(completed[0][1].toBool());
}

void TestSessionsWindow::providerErrorWaitsForNativePrompt_data()
{
    QTest::addColumn<QString>("state");
    QTest::newRow("provider-error")<<QString("error");
    QTest::newRow("interrupted")<<QString("interrupted");
}

void TestSessionsWindow::providerErrorWaitsForNativePrompt()
{
    QFETCH(QString,state);
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>();
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *send = composer->findChild<QPushButton *>("sendMessage");
    auto *status = composer->findChild<QLabel *>("messageStatus");
    WAIT_VERIFY(status->text().contains("starting"));
    setComposerText(composer, "повтори");
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"activity", state=="error"?"attention":"unknown"}, {"phase", state},
        {"events", QJsonArray{}}, {"cursor", 0}};
    if(state=="error")details["provider_error"]=QJsonObject{{"error_kind","capacity"}};
    const auto update = [&] { client->inspectionReady({}, "codex/hgs/dashboard", details, {}); };
    update(); QVERIFY(!send->isEnabled()); // Older/absent evidence never enables retry.
    details[state+"_message_can_send"] = false; details[state+"_message_reason"] = "Terminal has a draft";
    update(); QVERIFY(!send->isEnabled()); QCOMPARE(status->text(), QString("Terminal has a draft"));
    details[state+"_message_can_send"] = true; details[state+"_message_reason"] = "";
    update(); QVERIFY(send->isEnabled()); QCOMPARE(composer->editor()->toPlainText(), QString("повтори"));
    for (const QString &phase : {QString("approval"), QString("input"), QString("unknown")}) {
        details["phase"] = phase; update(); QVERIFY(!send->isEnabled());
    }
    details["phase"] = state; details["error"] = "Tracking mismatch"; update(); QVERIFY(!send->isEnabled());
    details.remove("error"); details["process_state"] = "exited"; update(); QVERIFY(!send->isEnabled());
    QCOMPARE(composer->editor()->toPlainText(), QString("повтори"));
}

void TestSessionsWindow::quotaFailureExplainsRecoveryAndKeepsDraft()
{
    auto state=fleet();auto box=state.local();box.sessions[0].runId="run-one";box.sessions[0].conversationId="conversation-one";state.setLocal(box,0);
    SessionsWindow window(program());window.setFleet(state);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();auto *composer=window.findChild<MessageComposer *>("messageComposer");
    QTRY_VERIFY(composer->findChild<QLabel *>("messageStatus")->text().contains("starting"));
    setComposerText(composer, "Continue after account access is restored");
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"last_event_at",2000000000},{"runtime_state","live"},{"process_state","running"},
        {"activity","attention"},{"phase","error"},{"error_message_can_send",true},
        {"provider_error",QJsonObject{{"error_kind","quota"},{"detail","7d limit reached. Resets in 4d18h"}}},
        {"events",QJsonArray{}},{"cursor",0}};
    client->inspectionReady({},"codex/hgs/dashboard",details,{});
    auto *chip=window.findChild<ToolbarChip *>("recoveryChip");QVERIFY(chip && chip->isVisible());
    QCOMPARE(chip->fullLabel(),QString("Provider error"));QCOMPARE(chip->tone(),ChipTone::Danger);
    auto *panel=window.findChild<QFrame *>("recoveryPanel");QVERIFY(panel && !panel->isVisible());
    window.activateWindow();QTest::mouseClick(chip,Qt::LeftButton);QTRY_VERIFY(panel->isVisible());
    const auto *row=window.findChild<SessionList *>("sessionList")->currentItem();
    QCOMPARE(row->data(SessionRoles::Status).toString(),QString("Usage limit reached"));
    QVERIFY(!row->data(SessionRoles::Working).toBool());QVERIFY(row->data(SessionRoles::Attention).toBool());
    QVERIFY(panel->findChild<QLabel *>("providerErrorHelp")->text().contains("will not retry automatically"));
    QVERIFY(panel->findChild<QPushButton *>("providerErrorTerminal")->isVisible());
    QVERIFY(panel->findChild<QPushButton *>("providerErrorRefresh")->isVisible());
    auto *action=panel->findChild<QPushButton *>("providerErrorTerminal");action->setFocus();
    QTRY_COMPARE(QApplication::focusWidget(),action);
    client->inspectionReady({},"codex/hgs/dashboard",details,{});
    QCOMPARE(QApplication::focusWidget(),action);
    QCOMPARE(composer->editor()->toPlainText(),QString("Continue after account access is restored"));
    const auto preview=qEnvironmentVariable("HGS_QUOTA_PREVIEW");
    if(!preview.isEmpty()) {QDir().mkpath(preview);window.resize(1120,800);QTest::qWait(30);QVERIFY(window.grab().save(preview+"/quota-failure.png"));}
    // Switching session closes the popover instead of showing this session's error elsewhere.
    window.showSession("mac","claude/infra/review");QVERIFY(!panel->isVisible());window.showSession({},"codex/hgs/dashboard");
    // Restored native progress clears the failure without submitting the draft.
    details["phase"]="working";details["activity"]="busy";details.remove("provider_error");
    client->inspectionReady({},"codex/hgs/dashboard",details,{});
    QVERIFY(!panel->isVisible());QVERIFY(chip->isHidden());
    QCOMPARE(composer->editor()->toPlainText(),QString("Continue after account access is restored"));
}

void TestSessionsWindow::firstMessageWaitsForNativePrompt_data()
{
    QTest::addColumn<QString>("startup");
    QTest::newRow("new") << QString("new");
    QTest::newRow("resumed") << QString("resumed");
    QTest::newRow("forked") << QString("forked");
}

void TestSessionsWindow::firstMessageWaitsForNativePrompt()
{
    QFETCH(QString, startup);
    const bool resumed = startup == "resumed";
    QFile::remove(m_dir->filePath("first-payload.json"));
    const QString program = m_dir->filePath("first-message-hgs");
    QFile file(program); QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
if args[0]=='inspect': print((root/'first-state.json').read_text())
elif args[0]=='send':
 p=json.load(sys.stdin)
 (root/'first-payload.json').write_text(json.dumps(p))
 print(json.dumps({'status':'submitted','request_id':p['request_id'],'name':args[1],'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],'first_message':not p['expected_conversation_id'],'submitted_text':p['text']}))
)PY");
    file.close(); QVERIFY(FakeCli::makeRunnable(file.fileName()));
    QJsonObject state{{"tracked", true}, {"run_id", "fresh-run"}, {"conversation_id", ""},
        {"runtime_state", "live"}, {"process_state", "running"}, {"activity", "unknown"}, {"phase", "unknown"},
        {"first_message_can_send", false}, {"first_message_reason", "Finish login in Terminal"}, {"events", QJsonArray{}}, {"cursor", 0}};
    if (startup == "forked") {
        state["fork_parent_id"] = "parent-conversation";
        state["fork_source_name"] = "codex/hgs/source";
    }
    if (resumed) {
        state["conversation_id"] = "saved-conversation"; state["expected_id"] = "saved-conversation";
        state["resume_message_can_send"] = false; state["resume_message_reason"] = "Finish login in Terminal";
    }
    // Polls read first-state.json while it changes: replace it atomically, or a torn
    // read fails an inspection and the session refuses to send.
    const auto save = [&] { return FakeCli::replace(m_dir->filePath("first-state.json"), QJsonDocument(state).toJson()); };
    QVERIFY(save());
    SessionsWindow window(FakeCli::program(program)); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *send = window.findChild<MessageComposer *>("messageComposer")->findChild<QPushButton *>("sendMessage"); auto *status = window.findChild<MessageComposer *>("messageComposer")->findChild<QLabel *>("messageStatus");
    QTRY_COMPARE(status->text(), QString("Finish login in Terminal"));
    setComposerText(composer, "First message after login"); QVERIFY(composer->addAttachment("notes.txt", "text/plain", "keep attachment"));
    QVERIFY(!send->isEnabled()); QTest::keyClick(composer->editor(), Qt::Key_Return);
    QVERIFY(!QFile::exists(m_dir->filePath("first-payload.json")));
    state["first_message_can_send"] = true; state["first_message_reason"] = ""; state["model"] = "gpt-6-astra"; state["effort"] = "xhigh"; QVERIFY(save());
    if (resumed) { state["resume_message_can_send"] = true; state["resume_message_reason"] = ""; QVERIFY(save()); }
    client->requestInspection({}, "codex/hgs/dashboard", 0); QTRY_VERIFY(send->isEnabled());
    QVERIFY(window.findChild<MessageComposer *>("messageComposer")->findChild<QPushButton *>("sessionModelSettings")->text().contains("gpt-6-astra"));
    QSignalSpy delivered(client, &HgsClient::messageSent); QTest::keyClick(composer->editor(), Qt::Key_Return); QTRY_COMPARE(delivered.size(), 1);
    QFile captured(m_dir->filePath("first-payload.json")); QVERIFY(captured.open(QIODevice::ReadOnly));
    const auto payload = QJsonDocument::fromJson(captured.readAll()).object();
    QCOMPARE(payload["expected_run_id"].toString(), "fresh-run");
    QCOMPARE(payload["expected_conversation_id"].toString(), resumed ? "saved-conversation" : "");
    QCOMPARE(payload["text"].toString(), "[File #1] First message after login"); QCOMPARE(payload["attachments"].toArray().size(), 1);
    QCOMPARE(composer->editor()->toPlainText(), QString());
    state.remove("expected_id"); state.remove("resume_message_can_send");
    state["conversation_id"] = resumed ? "saved-conversation" : "confirmed-first";
    state["activity"] = "busy"; state["phase"] = "working"; QVERIFY(save());
    client->requestInspection({}, "codex/hgs/dashboard", 0);
    setComposerText(composer, "Next message"); QTRY_VERIFY(send->isEnabled());
}

void TestSessionsWindow::resumedConversationPausesAndClearsOnceVerified()
{
    // Codex defers SessionStart after resume, so the binding still expects the
    // saved conversation. Only hgs's verification enables Pause and Clear.
    const QString program = m_dir->filePath("resumed-actions-hgs");
    QFile file(program); QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(R"PY(#!/usr/bin/env python3
import sys,pathlib
root=pathlib.Path(__file__).parent
print((root/'resumed-state.json').read_text() if sys.argv[1:2]==['inspect'] else '{}')
)PY");
    file.close(); QVERIFY(FakeCli::makeRunnable(file.fileName()));
    QJsonObject state{{"tracked", true}, {"run_id", "resumed-run"}, {"conversation_id", "saved-conversation"},
        {"expected_id", "saved-conversation"}, {"runtime_state", "live"}, {"process_state", "running"},
        {"activity", "unknown"}, {"phase", "starting"}, {"resume_message_can_send", false},
        {"resume_message_reason", "Codex has not opened the saved conversation yet"}, {"clear_context_supported", false},
        {"allowed_actions", QJsonArray{"rename", "terminate", "forget"}}, {"events", QJsonArray{}}, {"cursor", 0}};
    const auto save = [&] { return FakeCli::replace(m_dir->filePath("resumed-state.json"), QJsonDocument(state).toJson()); };
    QVERIFY(save());
    SessionsWindow window(FakeCli::program(program)); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *pause = window.findChild<QAction *>("pauseAction"); auto *clear = window.findChild<QPushButton *>("clearSessionAction");
    settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", state);
    QVERIFY(!pause->isEnabled()); QVERIFY(!clear->isEnabled());
    // Verified: the exact process holds the saved history at an empty composer.
    // An hgs that reports this without allowing the pause keeps it disabled.
    state["activity"] = "idle"; state["phase"] = "idle"; state["resume_message_can_send"] = true; state["resume_message_reason"] = "";
    QVERIFY(save()); settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", state);
    QVERIFY(!pause->isEnabled()); QVERIFY(!clear->isEnabled());
    state["clear_context_supported"] = true; state["allowed_actions"] = QJsonArray{"pause", "rename", "terminate", "forget"};
    QVERIFY(save()); settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", state);
    QVERIFY(pause->isEnabled()); QVERIFY(clear->isEnabled());
}

void TestSessionsWindow::failedMessagesReconcileAndRetryWithoutLosingDrafts()
{
    QTemporaryDir directory;const auto program=directory.filePath("hgs");QFile script(program);QVERIFY(script.open(QIODevice::WriteOnly));
    script.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path(__file__).parent;args=sys.argv[1:]
if args[0]=='inspect':
 if (root/'hold-inspection').exists():
  (root/'inspection-entered').write_text('held')
  end=time.monotonic()+3
  while not (root/'release-inspection').exists():
   if time.monotonic()>=end:sys.exit('held inspection gate expired')
   time.sleep(.01)
  print('held fixture inspection refused',file=sys.stderr);sys.exit(1)
 print((root/'inspect.json').read_text())
elif args[0]=='send':
 p=json.load(sys.stdin)
 with (root/'requests.jsonl').open('a') as f:f.write(json.dumps(p)+'\n')
 time.sleep(.15)
 if not (root/'succeed').exists():print('terminal not ready',file=sys.stderr);sys.exit(1)
 print(json.dumps({'status':'submitted','request_id':p['request_id'],'name':args[1],'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],'submitted_text':p['text'],'submitted_at':time.time()}))
)PY");script.close();QVERIFY(FakeCli::makeRunnable(script.fileName()));
    QJsonObject details{{"tracked",true},{"run_id","run-ready"},{"conversation_id","conversation-ready"},{"runtime_state","live"},{"process_state","running"},{"activity","idle"},{"phase","idle"},{"events",QJsonArray{}},{"cursor",0}};
    // Atomic: a background inspection must never read a truncated file and reset the run identity a retry checks.
    const auto save=[&]{return FakeCli::replace(directory.filePath("inspect.json"),QJsonDocument(details).toJson());};QVERIFY(save());
    SessionsWindow window(FakeCli::program(program));keepDetailsPollsOut(window);window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *send=composer->findChild<QPushButton *>("sendMessage");
    auto *client=window.findChild<HgsClient *>();auto *view=composer->parentWidget()->findChild<ActivityView *>();QVERIFY(view);
    const auto key=composer->sessionKey();QCOMPARE(key,QString("\ncodex/hgs/dashboard"));
    const auto attachmentAdmission=[&] {
        return QString::fromUtf8(QJsonDocument(QJsonObject{{"key",composer->sessionKey()},
            {"draft_loading",composer->isDraftLoading(key)},{"sending",composer->isSending(key)},
            {"preparing",composer->isPreparingSending(key)},{"waiting",composer->isWaitingToSend(key)},
            {"editable",composer->canEditAttachments()},{"attachments",composer->canAttachFiles()},
            {"read_only",composer->editor()->isReadOnly()},{"session_blocked",composer->editor()->property("sessionBlocked").toBool()},
            {"status",composer->findChild<QLabel *>("messageStatus")->text()},
            {"inspection",client->inspectionInFlight({},"codex/hgs/dashboard")}}).toJson(QJsonDocument::Compact));
    };
    QSignalSpy failures(client,&HgsClient::messageFailed),sent(client,&HgsClient::messageSent);
    setComposerText(composer, "Original request");QTRY_VERIFY(send->isEnabled());send->click();QTRY_COMPARE(failures.size(),1);
    QTRY_VERIFY(view->browser()->toPlainText().contains("Not sent"));
    setComposerText(composer, "Keep this different draft");
    const auto now=QDateTime::currentMSecsSinceEpoch()/1000.;
    QJsonArray events{QJsonObject{{"seq",1},{"type","TurnStarted"},{"detail","Original request"},{"at",now-600}},
        QJsonObject{{"seq",2},{"type","TurnStarted"},{"detail","Original request"},{"at",now+1},{"agent_id","child"}}};
    details["events"]=events;details["cursor"]=2;QVERIFY(save());
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);client->inspectionReady({},"codex/hgs/dashboard",details);
    QTRY_VERIFY(view->browser()->toPlainText().contains("Not sent"));
    events.append(QJsonObject{{"seq",3},{"type","TurnStarted"},{"detail","Original request"},{"at",now+2}});
    details["events"]=events;details["cursor"]=3;QVERIFY(save());
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);client->inspectionReady({},"codex/hgs/dashboard",details);
    QTRY_VERIFY(!view->browser()->toPlainText().contains("Not sent"));QCOMPARE(composer->editor()->toPlainText(),QString("Keep this different draft"));
    // Receipt reconciliation and current input admission are independent. A
    // deliberately held inspection can fail after the receipt disappeared;
    // only a subsequent confirmed read unlocks this exact draft. This models
    // the missing fixture precondition, not the unrecorded Windows failure state.
    const auto marker=[&](const QString &name) { QFile file(directory.filePath(name));return file.open(QIODevice::WriteOnly)&&file.write("gate")==4; };
    const auto releaseInspection=qScopeGuard([&] { marker("release-inspection");QFile::remove(directory.filePath("hold-inspection")); });
    QVERIFY(marker("hold-inspection"));QSignalSpy inspectionErrors(client,&HgsClient::inspectionFailed);
    composer->sessionRefreshRequested(key);QTRY_VERIFY(QFileInfo::exists(directory.filePath("inspection-entered")));
    QVERIFY(client->inspectionInFlight({},"codex/hgs/dashboard"));QVERIFY(composer->canAttachFiles());
    QVERIFY(marker("release-inspection"));QVERIFY(QFile::remove(directory.filePath("hold-inspection")));
    QTRY_VERIFY(std::any_of(inspectionErrors.cbegin(),inspectionErrors.cend(),[](const QList<QVariant> &reply) {
        return reply[0].toString().isEmpty()&&reply[1].toString()=="codex/hgs/dashboard"
            &&reply[2].toString().contains("held fixture inspection refused");
    }));
    QVERIFY(!view->browser()->toPlainText().contains("Not sent"));QCOMPARE(composer->sessionKey(),key);
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this different draft"));
    QVERIFY(!composer->canAttachFiles());QVERIFY(!composer->canEditAttachments());QVERIFY(composer->editor()->isReadOnly());
    QCOMPARE(failures.size(),1);QCOMPARE(sent.size(),0);
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);
    QTRY_VERIFY2(composer->sessionKey()==key&&composer->canAttachFiles(),qPrintable(attachmentAdmission()));
    setComposerText(composer, "Retry with a file");QCOMPARE(composer->editor()->toPlainText(),QString("Retry with a file"));
    QVERIFY2(composer->sessionKey()==key&&composer->canAttachFiles(),qPrintable(attachmentAdmission()));QVERIFY(composer->addAttachment("original.txt","text/plain","original"));send->click();QTRY_COMPARE(failures.size(),2);
    const auto id=QString::number(failures[1][0].toULongLong());QTRY_VERIFY(view->browser()->toPlainText().contains("Not sent"));
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);
    QTRY_VERIFY2(composer->sessionKey()==key&&composer->canAttachFiles(),qPrintable(attachmentAdmission()));
    setComposerText(composer, "Next draft");QCOMPARE(composer->editor()->toPlainText(),QString("Next draft"));composer->findChild<QPushButton *>("removeAttachment")->click();
    QVERIFY2(composer->sessionKey()==key&&composer->canAttachFiles(),qPrintable(attachmentAdmission()));QVERIFY(composer->addAttachment("next.txt","text/plain","next"));
    QFile success(directory.filePath("succeed"));QVERIFY(success.open(QIODevice::WriteOnly));success.close();
    {
        QSemaphore entered,release;std::atomic<int> reads=0;
        AttachmentFiles::setIoObserver([&]{if(++reads==1){entered.release();release.acquire();}});
        auto unblock=qScopeGuard([&]{release.release();AttachmentFiles::setIoObserver({});AttachmentFiles::pool().waitForDone();});
        view->messageActionRequested(id,"retry");view->messageActionRequested(id,"retry");QTRY_COMPARE(entered.available(),1);
        QCOMPARE(sent.size(),0);bool heartbeat=false;QTimer::singleShot(0,&window,[&]{heartbeat=true;});QTRY_VERIFY(heartbeat);
        window.showSession({},"kimi/docs/research");setComposerText(composer,"Other session draft");view->messageActionRequested(id,"retry");
        QCOMPARE(sent.size(),0);release.release();QTRY_COMPARE(sent.size(),1);
    }
    QCOMPARE(composer->editor()->toPlainText(),QString("Other session draft"));
    window.showSession({},"codex/hgs/dashboard");
    QTRY_COMPARE(composer->editor()->toPlainText(),QString("[File #2] Next draft"));
    QVERIFY(composer->draftMatches(window.findChild<QListWidget *>("sessionList")->currentItem()->data(Qt::UserRole).toString(),"[File #2] Next draft",{{"next.txt","text/plain","next","[File #2]"}}));
    QFile calls(directory.filePath("requests.jsonl"));QVERIFY(calls.open(QIODevice::ReadOnly));const auto requests=calls.readAll().trimmed().split('\n');QCOMPARE(requests.size(),3);
    const auto retried=QJsonDocument::fromJson(requests.last()).object();QCOMPARE(retried["text"].toString(),QString("[File #1] Retry with a file"));
    QCOMPARE(retried["attachments"].toArray()[0].toObject()["reference"].toString(),QString("[File #1]"));
    QCOMPARE(QByteArray::fromBase64(retried["attachments"].toArray()[0].toObject()["data_base64"].toString().toLatin1()),QByteArray("original"));
    QTRY_VERIFY(!view->browser()->toPlainText().contains("Not sent"));
    QFile::remove(directory.filePath("succeed"));QTRY_VERIFY(send->isEnabled());send->click();QTRY_COMPARE(failures.size(),3);
    const auto deletedId=QString::number(failures[2][0].toULongLong());
    {
        QSemaphore entered,release;AttachmentFiles::setIoObserver([&]{entered.release();release.acquire();});
        auto unblock=qScopeGuard([&]{release.release();AttachmentFiles::setIoObserver({});AttachmentFiles::pool().waitForDone();});
        view->messageActionRequested(deletedId,"retry");QTRY_COMPARE(entered.available(),1);
        view->messageActionRequested(deletedId,"delete");release.release();AttachmentFiles::pool().waitForDone();QTest::qWait(100);
        QCOMPARE(failures.size(),3);QCOMPARE(sent.size(),1); // A removed receipt cannot replay after the attachment read.
        QFile log(directory.filePath("requests.jsonl"));QVERIFY(log.open(QIODevice::ReadOnly));QCOMPARE(log.readAll().trimmed().split('\n').size(),4);
    }
    QTRY_VERIFY(!view->browser()->toPlainText().contains("Not sent"));QCOMPARE(composer->editor()->toPlainText(),QString("[File #2] Next draft"));
    composer->findChild<QPushButton *>("removeAttachment")->click();setComposerText(composer, "Sent through Terminal");
    send->click();QTRY_COMPARE(failures.size(),4);
    events.append(QJsonObject{{"seq",4},{"type","TurnStarted"},{"detail","Sent through Terminal"},{"at",QDateTime::currentMSecsSinceEpoch()/1000.+2}});
    details["events"]=events;details["cursor"]=4;QVERIFY(save());
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);client->inspectionReady({},"codex/hgs/dashboard",details);
    QTRY_VERIFY(!view->browser()->toPlainText().contains("Not sent"));QVERIFY(composer->editor()->toPlainText().isEmpty());
}

void TestSessionsWindow::scheduledSettingsApplyOncePerReady_data()
{
    // What the attempt at the next Ready returns. There is none when the run or
    // its conversation changes first.
    // Without listedChooser only inspect of the selected session shows the
    // chooser, as when it closed again between two fleet listings.
    QTest::addColumn<bool>("keepSelected"); QTest::addColumn<QString>("retry"); QTest::addColumn<QString>("changed");
    QTest::addColumn<bool>("listedChooser");
    QTest::newRow("background-applied") << false << QString("applied") << QString() << true;
    QTest::newRow("background-failed") << false << QString("Native model picker unavailable") << QString() << true;
    QTest::newRow("background-new-run") << false << QString() << QString("run") << true;
    QTest::newRow("background-new-conversation") << false << QString() << QString("conversation") << true;
    QTest::newRow("selected-applied") << true << QString("applied") << QString() << true;
    QTest::newRow("selected-inspected-chooser") << true << QString("applied") << QString() << false;
}

void TestSessionsWindow::scheduledSettingsApplyOncePerReady()
{
    // Each settings change and inspection runs the fake CLI, which is slow on Windows runners.
    constexpr int SettingsTimeout = 20000;
    // A chooser can appear between the idle listing and the dispatch of a
    // pending change; dispatch then schedules it again. The change stays
    // pending for one attempt per Ready transition, never a polling loop.
    QFETCH(bool, keepSelected); QFETCH(QString, retry); QFETCH(QString, changed); QFETCH(bool, listedChooser);
    QTemporaryDir directory; QVERIFY(directory.isValid());
    const QString program = directory.filePath("hgs"), snapshot = directory.filePath("snapshot.json"), capture = directory.filePath("calls.jsonl");
    QJsonObject details{{"tracked", true}, {"run_id", "run-original"}, {"conversation_id", "conversation-one"},
        {"activity", "busy"}, {"phase", "tool"}, {"model", "gpt-6.1-sol"}, {"effort", "high"},
        {"settings_change_supported", true}, {"settings_apply_when", "ready"},
        {"model_options", QJsonArray{QJsonObject{{"id", "gpt-6.1-sol"}, {"effort_options", QJsonArray{"high", "xhigh"}}}}},
        {"cursor", 0}, {"events", QJsonArray{}}};
    QVERIFY(FakeCli::replace(snapshot, QJsonDocument(details).toJson()));
    QFile executable(program); QVERIFY(executable.open(QIODevice::WriteOnly));
    QByteArray body = "#!/usr/bin/env python3\nimport json,sys,pathlib,time\nroot=pathlib.Path(__file__).parent\nargs=sys.argv[1:]\n";
    body += "outcomes=['scheduled','scheduled']+" + QJsonDocument(QJsonArray{retry}).toJson(QJsonDocument::Compact) + "\n";
    // As in modelSettingsKeepSessionIdentity: replace snapshot.json atomically.
    body += R"PY(def publish(data):
 staged=root/'snapshot.tmp';staged.write_text(json.dumps(data))
 for _ in range(250):
  try:return staged.replace(root/'snapshot.json')
  except PermissionError:time.sleep(.02)
 sys.exit('snapshot.json stayed open')
if args[0]=='inspect':
 if args[1]=='codex/hgs/dashboard':print((root/'snapshot.json').read_text())
 else:print(json.dumps({'tracked':True,'run_id':'run-other','conversation_id':'conversation-other','activity':'idle','phase':'idle','cursor':0,'events':[]}))
elif args[0]=='settings':
 p=json.load(sys.stdin)
 calls=root/'calls.jsonl'
 previous=calls.read_text().count('\n') if calls.exists() else 0
 with calls.open('a') as out: out.write(json.dumps({'args':args,'payload':p})+'\n')
 time.sleep(.12)
 result=outcomes[previous] if previous<len(outcomes) else 'unexpected settings call'
 data=json.loads((root/'snapshot.json').read_text())
 if result=='scheduled':
  # The terminal shows a chooser: dispatch schedules, and an apply of the
  # pending change stays scheduled.
  if 'expected_pending_id' not in p:
   data['pending_model']=p['model'];data['pending_effort']=p['effort'];data['pending_settings_id']=p['request_id'];publish(data)
  pending=p.get('expected_pending_id',p['request_id'])
 elif result=='applied':
  data['model']=p['model'];data['effort']=p['effort']
  for field in ['pending_model','pending_effort','pending_settings_id']:data.pop(field,None)
  publish(data)
 else:
  print(result,file=sys.stderr);sys.exit(1)
 receipt={'status':result,'request_id':p['request_id'],'name':args[1],'run_id':p['expected_run_id'],
  'conversation_id':p['expected_conversation_id'],'model':p['model'],'effort':p['effort']}
 if result=='scheduled':receipt.update(pending_settings_id=pending,apply_when='ready')
 print(json.dumps(receipt))
)PY";
    executable.write(body); executable.close(); QVERIFY(FakeCli::makeRunnable(executable.fileName()));
    auto current = fleet(); auto box = current.local(); box.sessions[0].runId = "run-original";
    current.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.setDetailsPollForTests(100, 200);
    window.setFleet(current); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspections(client, &HgsClient::inspectionReady), changes(client, &HgsClient::settingsFinished);
    QTRY_VERIFY(!inspections.isEmpty());
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    composer->findChild<QPushButton *>("sessionModelSettings")->click();
    auto *efforts = composer->findChild<QComboBox *>("sessionEffortChoice"); efforts->setCurrentIndex(efforts->findData("xhigh"));
    auto *apply = composer->findChild<QPushButton *>("applySessionSettings"); QVERIFY(apply->isEnabled()); apply->click();
    if (!keepSelected) window.showSession({}, "kimi/docs/research");
    QTRY_COMPARE_WITH_TIMEOUT(changes.size(), 1, SettingsTimeout); QVERIFY(changes[0][1].toBool());
    QCOMPARE(changes[0][2].toJsonObject().value("status").toString(), QString("scheduled"));
    const auto list = [&](const QString &activity, const QString &phase) {
        box.sessions[0].activity = activity; box.sessions[0].phase = phase; box.sessions[0].processState = "running";
        current.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(current);
    };
    const auto calls = [&] {
        QFile recorded(capture); return recorded.open(QIODevice::ReadOnly) ? recorded.readAll().count('\n') : -1;
    };
    const auto published = [&] {
        QFile file(snapshot); return QJsonDocument::fromJson(file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray()).object();
    };
    // Waits for two of the selected session's inspections, then counts calls.
    QString selectedName = keepSelected ? "codex/hgs/dashboard" : "kimi/docs/research";
    const auto settledCalls = [&] {
        const auto seen = inspections.size(); QTest::qWait(150);
        const auto inspected = [&] { return std::count_if(inspections.cbegin() + seen, inspections.cend(),
            [&](const QList<QVariant> &reply) { return reply[1].toString() == selectedName; }); };
        if (!QTest::qWaitFor([&] { return inspected() >= 2; }, SettingsTimeout)) return -1;
        return changes.size() == calls() ? int(changes.size()) : -1;
    };
    // The idle listing starts the automatic apply, but a chooser appeared in
    // the meantime: dispatch schedules it.
    list("idle", "idle");
    QTRY_COMPARE_WITH_TIMEOUT(changes.size(), 2, SettingsTimeout); QVERIFY(changes[1][1].toBool());
    QCOMPARE(changes[1][2].toJsonObject().value("status").toString(), QString("scheduled"));
    // No second attempt while the session stays Ready or shows the chooser.
    list("idle", "idle"); list("idle", "idle"); QCOMPARE(settledCalls(), 2);
    if (listedChooser) {
        list("busy", "input"); QCOMPARE(settledCalls(), 2);
        list("busy", "input"); QCOMPARE(settledCalls(), 2);
    }
    if (!changed.isEmpty()) {
        // A new run or conversation never receives the earlier one's change.
        if (changed == "run") box.sessions[0].runId = "run-two";
        else box.sessions[0].conversationId = "conversation-two";
        list("idle", "idle"); list("busy", "tool"); list("idle", "idle");
        QCOMPARE(settledCalls(), 2);
    } else {
        list("idle", "idle");
        if (keepSelected) {
            // The selected session also needs inspect to report Ready.
            QCOMPARE(settledCalls(), 2);
            auto ready = published();
            QVERIFY(ready.contains("pending_settings_id")); ready["settings_apply_when"] = "now";
            QVERIFY(FakeCli::replace(snapshot, QJsonDocument(ready).toJson()));
        }
        QTRY_COMPARE_WITH_TIMEOUT(changes.size(), 3, SettingsTimeout); QCOMPARE(changes[2][1].toBool(), retry == "applied");
        auto *notice = window.findChild<QLabel *>("notice"); QVERIFY(notice);
        if (retry == "applied") QVERIFY(notice->isHidden());
        else { QVERIFY(notice->isVisible()); QVERIFY(notice->text().contains(retry)); }
        // Applied or failed: later Ready transitions start nothing.
        list("busy", "tool"); list("idle", "idle"); list("busy", "input"); list("idle", "idle");
        QCOMPARE(settledCalls(), 3);
    }
    const auto attempts = changed.isEmpty() ? 3 : 2;
    // Selecting the session with inspect reporting Ready starts nothing either.
    auto ready = published();
    ready["settings_apply_when"] = "now"; QVERIFY(FakeCli::replace(snapshot, QJsonDocument(ready).toJson()));
    selectedName = "codex/hgs/dashboard"; window.showSession({}, selectedName); QCOMPARE(settledCalls(), attempts);
    QFile recorded(capture); QVERIFY(recorded.open(QIODevice::ReadOnly));
    QString pendingId;
    for (const auto &line : recorded.readAll().split('\n')) {
        if (line.isEmpty()) continue;
        const auto payload = QJsonDocument::fromJson(line).object().value("payload").toObject();
        QCOMPARE(payload.value("expected_run_id").toString(), QString("run-original"));
        QCOMPARE(payload.value("effort").toString(), QString("xhigh"));
        if (pendingId.isEmpty()) { QVERIFY(!payload.contains("expected_pending_id")); pendingId = payload.value("request_id").toString(); }
        else QCOMPARE(payload.value("expected_pending_id").toString(), pendingId);
    }
}

void TestSessionsWindow::inspectionBehindSettingsReceiptKeepsTheChange()
{
    // An inspection that read the session before a settings change and answers
    // after its scheduled receipt must not drop the queued change: the idle
    // listing still starts its automatic apply (zerus-ssoi, slow Windows runners).
    // The held inspection answers only once the window has the receipt; a fixed
    // delay let a late process exit deliver it first on macOS (zerus-6qdm).
    constexpr int Timeout = 20000;
    QTemporaryDir directory; QVERIFY(directory.isValid());
    const QString program = directory.filePath("hgs"), snapshot = directory.filePath("snapshot.json");
    QJsonObject details{{"tracked", true}, {"run_id", "run-original"}, {"conversation_id", "conversation-one"},
        {"activity", "busy"}, {"phase", "tool"}, {"model", "gpt-6.1-sol"}, {"effort", "high"},
        {"settings_change_supported", true}, {"settings_apply_when", "ready"},
        {"model_options", QJsonArray{QJsonObject{{"id", "gpt-6.1-sol"}, {"effort_options", QJsonArray{"high", "xhigh"}}}}},
        {"cursor", 0}, {"events", QJsonArray{}}};
    QVERIFY(FakeCli::replace(snapshot, QJsonDocument(details).toJson()));
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,sys,pathlib,time
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
def publish(data):
 staged=root/'snapshot.tmp';staged.write_text(json.dumps(data))
 for _ in range(250):
  try:return staged.replace(root/'snapshot.json')
  except PermissionError:time.sleep(.02)
 sys.exit('snapshot.json stayed open')
if args[0]=='inspect':
 if args[1]!='codex/hgs/dashboard':
  print(json.dumps({'tracked':True,'run_id':'run-other','conversation_id':'conversation-other','activity':'idle','phase':'idle','cursor':0,'events':[]}));sys.exit()
 text=(root/'snapshot.json').read_text()
 if (root/'hold').exists() and not (root/'held').exists():
  # Read before the change; answer only once the test has its receipt.
  (root/'held').write_text('1')
  for _ in range(1000):
   if (root/'release').exists():break
   time.sleep(.01)
  data=json.loads(text);data['fixture_reply']='held';text=json.dumps(data)
 print(text)
elif args[0]=='settings':
 p=json.load(sys.stdin)
 calls=root/'calls.jsonl'
 previous=calls.read_text().count('\n') if calls.exists() else 0
 with calls.open('a') as out: out.write(json.dumps({'args':args,'payload':p})+'\n')
 data=json.loads((root/'snapshot.json').read_text())
 receipt={'request_id':p['request_id'],'name':args[1],'run_id':p['expected_run_id'],
  'conversation_id':p['expected_conversation_id'],'model':p['model'],'effort':p['effort']}
 if previous==0:
  data['pending_model']=p['model'];data['pending_effort']=p['effort'];data['pending_settings_id']=p['request_id'];publish(data)
  receipt.update(status='scheduled',pending_settings_id=p['request_id'],apply_when='ready')
 else:
  data['model']=p['model'];data['effort']=p['effort']
  for field in ['pending_model','pending_effort','pending_settings_id']:data.pop(field,None)
  publish(data);receipt['status']='applied'
 print(json.dumps(receipt))
)PY"));
    auto current = fleet(); auto box = current.local(); box.sessions[0].runId = "run-original";
    current.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.setDetailsPollForTests(100, 200);
    window.setFleet(current); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspections(client, &HgsClient::inspectionReady), changes(client, &HgsClient::settingsFinished);
    QTRY_VERIFY(!inspections.isEmpty());
    // The next inspection reads the session now and answers after the receipt.
    QFile hold(directory.filePath("hold")); QVERIFY(hold.open(QIODevice::WriteOnly)); hold.close();
    QTRY_VERIFY_WITH_TIMEOUT(QFileInfo::exists(directory.filePath("held")), Timeout);
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    composer->findChild<QPushButton *>("sessionModelSettings")->click();
    auto *efforts = composer->findChild<QComboBox *>("sessionEffortChoice"); efforts->setCurrentIndex(efforts->findData("xhigh"));
    auto *apply = composer->findChild<QPushButton *>("applySessionSettings"); QVERIFY(apply->isEnabled()); apply->click();
    QTRY_COMPARE_WITH_TIMEOUT(changes.size(), 1, Timeout);
    QCOMPARE(changes[0][2].toJsonObject().value("status").toString(), QString("scheduled"));
    // The receipt met the held inspection in flight; only now may it answer.
    QVERIFY(client->inspectionInFlight({}, "codex/hgs/dashboard"));
    const auto heldReply = [&](qsizetype from) {
        for (auto index = from; index < inspections.size(); ++index)
            if (inspections[index][2].toJsonObject().value("fixture_reply") == "held") return index;
        return qsizetype(-1);
    };
    const auto seen = inspections.size();
    QCOMPARE(heldReply(0), qsizetype(-1));
    QFile release(directory.filePath("release")); QVERIFY(release.open(QIODevice::WriteOnly)); release.close();
    // The held reply, from before the change, arrives after the receipt.
    QTRY_VERIFY_WITH_TIMEOUT(heldReply(seen) >= 0, Timeout);
    const auto held = inspections[heldReply(seen)];
    QCOMPARE(held[1].toString(), QString("codex/hgs/dashboard"));
    QVERIFY(!held[2].toJsonObject().contains("pending_settings_id"));
    box.sessions[0].activity = "idle"; box.sessions[0].phase = "idle"; box.sessions[0].processState = "running";
    current.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(current);
    QTRY_COMPARE_WITH_TIMEOUT(changes.size(), 2, Timeout);
    QCOMPARE(changes[1][2].toJsonObject().value("status").toString(), QString("applied"));
}

// U53 (#618): Activity's composer names the session's account beside the
// model; a confirmed choice runs switch-account for exactly that session and
// the notice says how it ended; the next read names the new account.
void TestSessionsWindow::activityAccountPickerSwitchesTheSession()
{
    QTemporaryDir directory; QVERIFY(directory.isValid());
    const QString program = directory.filePath("hgs");
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,sys,pathlib
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
moved=(root/'moved').exists()
if a[:1]==['inspect']:
 if a[1]!='codex/hgs/dashboard':
  print(json.dumps({'tracked':True,'run_id':'run-other','conversation_id':'conversation-other','activity':'idle','phase':'idle','cursor':0,'events':[]}));sys.exit()
 print(json.dumps({'tracked':True,'run_id':'run-2' if moved else 'run-1','conversation_id':'conversation-one','activity':'idle','phase':'idle',
  'agent':'codex','account_id':'second' if moved else 'work','account_label':'Second' if moved else 'Work','cursor':0,'events':[]}))
elif a[:1]==['account']:
 w=lambda used:[{'window_minutes':10080,'used_percent':used,'resets_at':'2099-01-01T00:00:00Z'}]
 print(json.dumps({'profiles':[{'id':'work','provider':'codex','label':'Work','account_status':{'status':'ok','checked_at':4102444800,'windows':w(81)}},
  {'id':'second','provider':'codex','label':'Second','account_status':{'status':'ok','checked_at':4102444800,'windows':w(9)}}]}))
elif a[:1]==['switch-account']:
 with (root/'switches.jsonl').open('a') as out: out.write(json.dumps(a)+'\n')
 (root/'moved').write_text('1')
 print(json.dumps({'status':'switched','session':a[1],'from':{'account_id':'work','label':'Work'},'to':{'account_id':'second','label':'Second'}}))
else:print('{}')
)PY"));
    SessionsWindow window(FakeCli::program(program)); window.setDetailsPollForTests(100, 200);
    window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *chip = composer->accountChip();
    QTRY_COMPARE(chip->fullLabel(), QStringLiteral("Work"));
    QTRY_VERIFY(chip->isVisible() && chip->isEnabled());
    const auto rows = [&] {
        QList<QPushButton *> found;
        for (auto *row : chip->accountPopover()->findChildren<QPushButton *>("sessionAccountOption")) if (row->isVisible()) found << row;
        return found;
    };
    // The accounts page reads the catalog; the menu lists both Codex accounts.
    QTRY_VERIFY(([&] { chip->click(); const bool two = rows().size() == 2; if (!two) chip->accountPopover()->hide(); return two; }()));
    QCOMPARE(rows()[1]->property("account").toString(), QStringLiteral("second"));
    QCOMPARE(rows()[1]->findChild<QLabel *>("sessionAccountOptionUsage")->text(), QStringLiteral("7d 9% used"));
    rows()[1]->click();
    chip->accountPopover()->findChild<QPushButton *>("sessionAccountMove")->click();
    QCOMPARE(chip->fullLabel(), QStringLiteral("Switching to Second…"));
    QTRY_COMPARE(window.findChild<QLabel *>("notice")->text(), QStringLiteral("Moved to Second; the agent resumed the same conversation."));
    QFile calls(directory.filePath("switches.jsonl")); QVERIFY(calls.open(QIODevice::ReadOnly));
    const auto lines = calls.readAll().trimmed().split('\n');
    QCOMPARE(lines.size(), 1);
    const auto args = QJsonDocument::fromJson(lines[0]).array();
    QCOMPARE(args[0].toString(), QStringLiteral("switch-account")); QCOMPARE(args[1].toString(), QStringLiteral("codex/hgs/dashboard"));
    QCOMPARE(args[2].toString(), QStringLiteral("--account")); QCOMPARE(args[3].toString(), QStringLiteral("second"));
    QTRY_COMPARE(chip->fullLabel(), QStringLiteral("Second"));
}

void TestSessionsWindow::externalQueueSurvivesDurableDraftRecheck()
{
    const QString name="external/project/chat",run="external-run",conversation="external-conversation";
    const QJsonObject source{{"schema_version",1},{"kind","chatgpt_work"},{"evidence","explicit_registration"}};
    const QJsonObject details{{"name",name},{"cmd","external"},{"agent","external"},{"tracked",true},{"state","running"},
        {"run_id",run},{"conversation_id",conversation},{"phase","idle"},{"activity","unknown"},{"process_state","external"},{"runtime_state","external"},
        {"backend","external-workstream"},{"external_protocol","cooperative-inbox-v1"},{"send_delivery","cooperative_inbox"},
        {"external_cloud_control",false},{"client_context",source},{"send_supported",true},{"send_available",true},
        {"terminal_supported",false},{"attachments_supported",false},{"allowed_actions",QJsonArray{}},{"events",QJsonArray{}},{"cursor",0}};
    const auto fake=programInspecting("external-queue",details);QVERIFY(!fake.isEmpty());
    // A live native catalog stream must not suppress the external inbox's
    // scoped inspection: watch-v1 does not expose that Activity source.
    const auto streamProgram = m_dir->filePath("external-catalog-stream");
    QVERIFY(FakeCli::install(streamProgram, R"PY(#!/usr/bin/env python3
import json, sys
if sys.argv[1:] == ['--help']:
    print('Event stream ABI: watch-v1')
    sys.exit(0)
if sys.argv[1:] != ['watch', '--json']:
    sys.exit(2)
def send(seq, t, **fields):
    print(json.dumps(dict(v=1, seq=seq, t=t, **fields)), flush=True)
send(1, 'hello', host='fixture', topics=['catalog', 'catalog.meta', 'activity'])
send(2, 'snapshot', topic='catalog', items=[], page=0, last=True, cursor=1)
send(3, 'snapshot', topic='catalog.meta', value={'host': 'fixture', 'ok': True}, page=0, last=True, cursor=1)
for line in sys.stdin:
    pass
)PY"));
    EventStreams streams(FakeCli::program(streamProgram)); streams.setHosts({QString()});
    QTRY_VERIFY_WITH_TIMEOUT(streams.live({}), 15000);
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="external";session.runId=run;session.conversationId=conversation;
    session.tracked=true;session.activity="unknown";session.phase="idle";session.processState="external";session.runtimeState="external";
    session.clientContext=source;session.terminalSupported=false;session.actionsReported=true;box.sessions={session};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(fake);
    auto *client = window.findChild<HgsClient *>();
    QSignalSpy inspected(client, &HgsClient::inspectionReady);
    window.setFleet(state);window.show();window.showSession({},name);
    auto *rows = window.findChild<SessionList *>("sessionList");
    QTRY_VERIFY(rows && rows->currentItem());
    QTRY_COMPARE(rows->currentItem()->data(SessionRoles::Agent).toString(), QString("chatgpt_work"));
    QVERIFY(rows->currentItem()->data(SessionRoles::Meta).toString().contains("ChatGPT Work"));
    QVERIFY(rows->currentItem()->data(Qt::AccessibleDescriptionRole).toString().contains("ChatGPT Work"));
    QVERIFY(rows->currentItem()->data(SessionRoles::Model).toString().isEmpty());
    QTRY_VERIFY(!inspected.isEmpty());
    auto *composer=window.findChild<MessageComposer *>("messageComposer");
    settleSelectedInspection(composer,client,{},name,details);setComposerText(composer,"Check this scoped instruction");
    auto *button=composer->findChild<QPushButton *>("sendMessage");QTRY_VERIFY(button->isEnabled());QCOMPARE(button->text(),QString("Queue"));
    QSignalSpy sent(client,&HgsClient::messageSent);QSignalSpy failed(client,&HgsClient::messageFailed);
    QTest::mouseClick(button,Qt::LeftButton);
    // FakeCli need not execute a real action: reaching either native callback
    // proves the asynchronous draft guard did not mistake an inbox for a dead process.
    QTRY_VERIFY_WITH_TIMEOUT(!sent.isEmpty()||!failed.isEmpty(),10000);
    const auto result=!sent.isEmpty()?sent.first():failed.first();
    QCOMPARE(result[2].toString(),name);
}

void TestSessionsWindow::escapeInActivityInterruptsAndRestoresPrompt()
{
    QTemporaryDir fixture; QFile fake(fixture.filePath("hgs")), original(script());
    QVERIFY(original.open(QIODevice::ReadOnly)); QVERIFY(fake.open(QIODevice::WriteOnly));
    // Acknowledge interrupts like the CLI and record each request.
    fake.write(original.readAll().replace("if False:pass\n", "if False:pass\n"
        "elif c=='interrupt':\n payload=sys.stdin.read().rstrip('\\n')\n"
        " open(__file__+'.interrupts','a').write(payload+'\\n')\n"
        " print(json.dumps({'request_id':json.loads(payload)['request_id'],'name':a[1],'run_id':'run-one',"
        "'conversation_id':'conversation-one','status':'submitted'}))\n"));
    fake.close(); QVERIFY(FakeCli::makeRunnable(fake.fileName()));
    const auto requests = [&] { QFile log(fake.fileName() + ".interrupts"); return log.open(QIODevice::ReadOnly) ? log.readAll().count('\n') : 0; };

    SessionsWindow window(FakeCli::program(fake.fileName())); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *browser = window.findChild<ActivityView *>("mainActivity")->browser(); auto *notice = window.findChild<QLabel *>("notice");
    const QString prompt = "Fix the flaky test\nand explain the cause.";
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
        {"process_state", "running"}, {"activity", "busy"}, {"phase", "working"}, {"interrupt_supported", true},
        {"turn_started", 1790928000.0}, {"prompt", prompt}, {"events", QJsonArray{}}, {"cursor", 0}};
    const auto update = [&] { client->inspectionReady({}, "codex/hgs/dashboard", details, {}); };
    const auto interrupted = [&] { details["phase"] = "interrupted"; details["activity"] = "unknown"; details["interrupted_message_can_send"] = true; update(); };
    update(); QVERIFY(composer->findChild<QPushButton *>("interruptAgent")->isEnabled());

    // Escape in the transcript stops the turn instead of closing the window,
    // and the interrupted prompt returns to the empty message field.
    browser->setFocus(); QTest::keyClick(browser, Qt::Key_Escape);
    QTRY_COMPARE(requests(), 1); QVERIFY(window.isVisible());
    QTRY_VERIFY(notice->text().contains("Interrupt sent"));
    interrupted();
    QCOMPARE(composer->editor()->toPlainText(), prompt); QVERIFY(composer->editor()->hasFocus());

    // Escape in the message field stops the next turn; a started draft is kept.
    details["phase"] = "working"; details["activity"] = "busy"; details["turn_started"] = 1790928100.0; details.remove("interrupted_message_can_send");
    update(); setComposerText(composer, "my own draft"); composer->editor()->setFocus(); notice->clear();
    QTest::keyClick(composer->editor(), Qt::Key_Escape);
    QTRY_COMPARE(requests(), 2); QTRY_VERIFY(notice->text().contains("Interrupt sent"));
    interrupted(); QCOMPARE(composer->editor()->toPlainText(), QString("my own draft"));

    // Without a working turn, Escape only leaves the field.
    details["phase"] = "idle"; details["activity"] = "idle"; update();
    composer->editor()->setFocus(); QTest::keyClick(composer->editor(), Qt::Key_Escape);
    QVERIFY(!composer->editor()->hasFocus()); QTest::qWait(150); QCOMPARE(requests(), 2);
}

void TestSessionsWindow::stopNoticesExplainOriginalTargetAndCanBeDismissed_data()
{
    QTest::addColumn<QString>("outcome");
    for (const auto *outcome : {"inactive", "failed", "navigated"}) QTest::newRow(outcome) << QString(outcome);
}

void TestSessionsWindow::stopNoticesExplainOriginalTargetAndCanBeDismissed()
{
    QFETCH(QString, outcome);
    QTemporaryDir directory;
    const auto program = directory.filePath("hgs"), state = directory.filePath("details.json");
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"activity", "busy"}, {"phase", "working"},
        {"interrupt_supported", true}, {"turn_started", 1790928000.0}, {"prompt", "Original turn prompt"}, {"events", QJsonArray{}}, {"cursor", 0}};
    QVERIFY(FakeCli::replace(state, QJsonDocument(details).toJson()));
    QVERIFY(FakeCli::replace(directory.filePath("failure"), outcome == "failed" ? QByteArray("hgs: Native interruption could not be verified") : QByteArray("hgs: This turn is no longer working")));
    // Polls read details.json while the interrupt rewrites it: replace it, so no
    // inspection reads a truncated file and fails. Windows refuses the rename while a
    // poll has the file open; retry then.
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path(__file__).parent
if sys.argv[1]=='inspect': print((root/'details.json').read_text())
elif sys.argv[1]=='interrupt':
 p=json.load(sys.stdin)
 with (root/'calls').open('a') as f:f.write('call\n')
 d=json.loads((root/'details.json').read_text());d.update(activity='idle',phase='idle',interrupt_supported=False)
 staged=root/'details.tmp';staged.write_text(json.dumps(d))
 for _ in range(250):
  try:staged.replace(root/'details.json');break
  except PermissionError:time.sleep(.02)
 else:sys.exit('details.json stayed open')
 time.sleep(.15)
 print((root/'failure').read_text(),file=sys.stderr);sys.exit(1)
else: print('{}')
)PY"); fixture.close(); QVERIFY(FakeCli::makeRunnable(program));
    SessionsWindow window(FakeCli::program(program)); window.resize(1080, 760); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *notice = window.findChild<QLabel *>("notice"); auto *dismiss = window.findChild<QPushButton *>("dismissNotice");
    QVERIFY(dismiss); QVERIFY(dismiss->isHidden());
    auto *stop = composer->findChild<QPushButton *>("interruptAgent"); QTRY_VERIFY(stop->isEnabled());
    setComposerText(composer, "Keep my draft");
    QSignalSpy refresh(&window, &SessionsWindow::refreshRequested);
    window.showNotificationNotice("Earlier information"); stop->click();
    if (outcome == "navigated") window.showSession({}, "kimi/docs/research");
    QTRY_VERIFY(notice->text().startsWith("Stop codex/hgs/dashboard ("));
    QVERIFY(!notice->text().contains("kimi/docs/research")); QVERIFY(!refresh.isEmpty());
    QCOMPARE(notice->property("error").toBool(), outcome == "failed");
    if (outcome == "failed") {
        QVERIFY(notice->text().contains("Native interruption could not be verified"));
        QVERIFY(notice->text().contains("Open Terminal to check the agent"));
        QVERIFY(notice->toolTip().contains("Terminal"));
        // An older information timeout must not erase the newer error.
        QTest::qWait(5100); QVERIFY(notice->isVisible()); QVERIFY(dismiss->isVisible());
    } else QVERIFY(notice->text().contains("No action needed"));
    if (outcome == "navigated") window.showSession({}, "codex/hgs/dashboard");
    QTRY_VERIFY(!stop->isEnabled()); QCOMPARE(composer->editor()->toPlainText(), QString("Keep my draft"));
    QVERIFY(dismiss->isVisible()); QVERIFY(!dismiss->accessibleName().isEmpty());
    auto *statusBar = window.findChild<QFrame *>("workspaceStatus");
    QVERIFY(statusBar->rect().contains(QRect(dismiss->mapTo(statusBar, QPoint()), dismiss->size())));
    dismiss->click(); QVERIFY(notice->isHidden()); QVERIFY(dismiss->isHidden()); QVERIFY(notice->text().isEmpty()); QVERIFY(notice->toolTip().isEmpty());
    QFile calls(directory.filePath("calls")); QVERIFY(calls.open(QIODevice::ReadOnly)); QCOMPARE(calls.readAll(), QByteArray("call\n"));
}

void TestSessionsWindow::escapeInterruptsFromAnywhereAndNeverCloses()
{
    QTemporaryDir fixture; QFile fake(fixture.filePath("hgs")), original(script());
    QVERIFY(original.open(QIODevice::ReadOnly)); QVERIFY(fake.open(QIODevice::WriteOnly));
    fake.write(original.readAll().replace("if False:pass\n", "if False:pass\n"
        "elif c=='interrupt':\n payload=sys.stdin.read().rstrip('\\n')\n"
        " open(__file__+'.interrupts','a').write(payload+'\\n')\n"
        " print(json.dumps({'request_id':json.loads(payload)['request_id'],'name':a[1],'run_id':'run-one',"
        "'conversation_id':'conversation-one','status':'submitted'}))\n"));
    fake.close(); QVERIFY(FakeCli::makeRunnable(fake.fileName()));
    const auto requests = [&] { QFile log(fake.fileName() + ".interrupts"); return log.open(QIODevice::ReadOnly) ? log.readAll().count('\n') : 0; };

    SessionsWindow window(FakeCli::program(fake.fileName())); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *list = window.findChild<QWidget *>("sessionList"); auto *search = window.findChild<QLineEdit *>("search");
    const QString prompt = "Rebuild the release";
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
        {"process_state", "running"}, {"activity", "busy"}, {"phase", "tool"}, {"interrupt_supported", true},
        {"turn_started", 1790928000.0}, {"prompt", prompt}, {"events", QJsonArray{}}, {"cursor", 0}};
    const auto update = [&] { client->inspectionReady({}, "codex/hgs/dashboard", details, {}); };
    update(); QVERIFY(composer->findChild<QPushButton *>("interruptAgent")->isEnabled());

    // The session list had focus after choosing the session: Escape still
    // stops the working agent and the window stays open.
    list->setFocus(); QTest::keyClick(list, Qt::Key_Escape);
    QTRY_COMPARE(requests(), 1); QVERIFY(window.isVisible());
    details["phase"] = "interrupted"; details["activity"] = "unknown"; details["interrupted_message_can_send"] = true; update();
    QCOMPARE(composer->editor()->toPlainText(), prompt);

    // Nothing is working: Escape never closes Zerus.
    details["phase"] = "idle"; details["activity"] = "idle"; details.remove("interrupted_message_can_send"); update();
    list->setFocus(); QTest::keyClick(list, Qt::Key_Escape); QTest::qWait(150);
    QVERIFY(window.isVisible()); QCOMPARE(requests(), 1);

    // A search being typed is cleared first, without stopping the agent.
    details["phase"] = "working"; details["activity"] = "busy"; details["turn_started"] = 1790928100.0; update();
    search->setFocus(); search->setText("release"); QTest::keyClick(search, Qt::Key_Escape);
    QVERIFY(search->text().isEmpty()); QTest::qWait(150); QCOMPARE(requests(), 1); QVERIFY(window.isVisible());
}

void TestSessionsWindow::suggestedMessageIsPlaceholderAndTabTakesIt()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *editor = composer->editor(); const QString placeholder = editor->placeholderText();
    QJsonObject details{{"tracked", true}, {"run_id", "run-one"}, {"conversation_id", "conversation-one"}, {"runtime_state", "live"},
        {"process_state", "running"}, {"activity", "idle"}, {"phase", "idle"}, {"events", QJsonArray{}}, {"cursor", 0},
        {"prompt_suggestion", QJsonObject{{"text", "run the tests"}, {"at", 1790928000.0}}}};
    const auto update = [&] { client->inspectionReady({}, "codex/hgs/dashboard", details, {}); };
    update(); QCOMPARE(editor->placeholderText(), QString("run the tests"));

    // Tab takes the suggestion into an empty field and keeps moving focus otherwise.
    editor->setPlainText("x"); editor->setFocus(); QTest::keyClick(editor, Qt::Key_Tab);
    QCOMPARE(editor->toPlainText(), QString("x"));
    editor->clear(); editor->setFocus(); QTest::keyClick(editor, Qt::Key_Tab);
    QCOMPARE(editor->toPlainText(), QString("run the tests"));

    // A new turn makes it stale.
    editor->clear(); details["phase"] = "working"; details["activity"] = "busy"; update();
    QCOMPARE(editor->placeholderText(), placeholder);
    editor->setFocus(); QTest::keyClick(editor, Qt::Key_Tab); QVERIFY(editor->toPlainText().isEmpty());

    // Sending answers it: the same suggestion does not return before the next one.
    details["phase"] = "idle"; details["activity"] = "idle"; update();
    QCOMPARE(editor->placeholderText(), QString("run the tests"));
    auto *send = composer->findChild<QPushButton *>("sendMessage");
    editor->setPlainText("something else"); QVERIFY(send->isEnabled()); send->click();
    QCOMPARE(editor->placeholderText(), placeholder);
    update(); QCOMPARE(editor->placeholderText(), placeholder);
    details["prompt_suggestion"] = QJsonObject{{"text", "push it"}, {"at", 1790928100.0}}; update();
    QCOMPARE(editor->placeholderText(), QString("push it"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_composer3.moc"
