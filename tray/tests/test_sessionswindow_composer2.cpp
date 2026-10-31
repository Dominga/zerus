// U62 (#712): Activity and the composer: drafts, delivery, recovery and cold caches, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void refreshErrorsStayInComposerAndFooterWithoutMovingViews();
    void sessionLimitRecoveryShowsResetAndKeepsDraft();
    void heldMessagesShowTheTerminalDraftThatHoldsThem();
    void aHeldMessageThatFailsComesBackToTheComposer();
    void coldCacheClearKeepsDraftAndPinsIdentity();
    void coldCacheExplainsUnavailableContextActions();
    void stoppedSessionStartsFreshOrSendsAfterStart();
    void localStartsAskAboutAnotherWindowsSession();
    void localStartConfirmationKeepsIdentityAcrossRefresh_data();
    void localStartConfirmationKeepsIdentityAcrossRefresh();
    void clearInspectionUpdatesActivityAndCountersImmediately();
    void coldCacheCompactWaitsForSuccess_data();
    void coldCacheCompactWaitsForSuccess();
    void composerNavigationDuringPolling();
    void messageDeliveryKeepsSessionIdentityAndDrafts_data();
    void messageDeliveryKeepsSessionIdentityAndDrafts();
    void modelSettingsKeepSessionIdentity_data();
    void modelSettingsKeepSessionIdentity();
};

void TestSessionsWindow::refreshErrorsStayInComposerAndFooterWithoutMovingViews()
{
    const QJsonObject details{{"tracked",true},{"run_id","fixture-run"},{"conversation_id","fixture-conversation"},
        {"runtime_state","live"},{"process_state","running"},{"phase","idle"},{"activity","idle"},
        {"events",QJsonArray{}},{"cursor",0}};
    const auto fixture = programInspecting("inline-status", details); QVERIFY(!fixture.isEmpty());
    // A poll that succeeds clears the injected failure before the views are checked.
    SessionsWindow window(fixture); window.setFleet(fleet()); keepDetailsPollsOut(window); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);
    setComposerText(composer, "Keep my exact draft");
    auto cursor = composer->editor()->textCursor(); cursor.setPosition(4); cursor.setPosition(9, QTextCursor::KeepAnchor);
    composer->editor()->setTextCursor(cursor); composer->editor()->setFocus(); QTest::qWait(20);
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);
    const auto tabPosition = tabs->mapTo(&window, QPoint()); const auto tabSize = tabs->size();
    const auto editorRect = composer->editor()->geometry();
    client->inspectionFailed({}, "codex/hgs/dashboard", "timed out"); QTest::qWait(20);
    QVERIFY(window.findChild<QLabel *>("hint")->isHidden());
    auto *status = composer->findChild<QLabel *>("messageStatus");
    QVERIFY(status->text().contains("Could not refresh activity: timed out")); QVERIFY(status->isVisible());
    QCOMPARE(tabs->mapTo(&window, QPoint()), tabPosition); QCOMPARE(tabs->size(), tabSize);
    QCOMPARE(composer->editor()->geometry(), editorRect); QVERIFY(composer->editor()->isReadOnly());
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep my exact draft"));
    QCOMPARE(composer->editor()->textCursor().position(), 9); QCOMPARE(composer->editor()->textCursor().anchor(), 4);
    const auto preview = qEnvironmentVariable("ZERUS_INLINE_STATUS_PREVIEW");
    if (!preview.isEmpty()) { QDir().mkpath(preview); QVERIFY(window.grab().save(preview + "/activity-error.png")); }
    auto *terminal = window.findChild<TerminalView *>(); tabs->setCurrentWidget(terminal); QTest::qWait(20);
    auto *footer = window.findChild<QLabel *>("sessionFeedback");
    QVERIFY(footer->isVisible()); QVERIFY(footer->text().contains("timed out"));
    QCOMPARE(tabs->mapTo(&window, QPoint()), tabPosition); QCOMPARE(tabs->size(), tabSize);
    if (!preview.isEmpty()) QVERIFY(window.grab().save(preview + "/terminal-error.png"));
    client->inspectionReady({}, "codex/hgs/dashboard", details); QTest::qWait(20);
    QVERIFY(footer->isHidden()); QVERIFY(!composer->editor()->isReadOnly());
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep my exact draft"));
}

void TestSessionsWindow::sessionLimitRecoveryShowsResetAndKeepsDraft()
{
    SessionsWindow window(program());window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();auto *composer=window.findChild<MessageComposer *>("messageComposer");
    QTRY_VERIFY(composer->findChild<QLabel *>("messageStatus")->text().contains("starting"));
    setComposerText(composer, "Continue this draft after reset");
    window.activateWindow();composer->editor()->setFocus();QTRY_VERIFY(composer->editor()->hasFocus());
    const auto reset=QDateTime::currentMSecsSinceEpoch()/1000.0+7200;
    QJsonObject job{{"id","limit-job"},{"name","codex/hgs/dashboard"},{"class","session_limit"},{"session_limit_mode","reset"},
        {"state","waiting"},{"attempt",0},{"due_at",reset},{"not_before",reset},{"action","continue_message"}};
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"runtime_state","live"},{"process_state","running"},{"activity","attention"},{"phase","error"},
        {"last_event_at",2000000000},{"cursor",0},{"recovery",job},
        {"provider_error",QJsonObject{{"error_kind","session_limit"},{"detail","You've hit your session limit · resets 2:10pm (Europe/Moscow)"}}}};
    client->inspectionReady({},"codex/hgs/dashboard",details,{});
    auto *chip=window.findChild<ToolbarChip *>("recoveryChip");QVERIFY(chip->isVisible());
    QVERIFY(chip->fullLabel().contains("Waiting for reset"));QCOMPARE(chip->tone(),ChipTone::Warning);
    QCOMPARE(composer->editor()->toPlainText(),QString("Continue this draft after reset"));QVERIFY(composer->editor()->hasFocus());
    QTest::mouseClick(chip,Qt::LeftButton);auto *panel=static_cast<RecoveryUi::Panel *>(window.findChild<QFrame *>("recoveryPanel"));QTRY_VERIFY(panel->isVisible());
    QVERIFY(panel->detailText().contains("Europe/Moscow"));QVERIFY(!panel->findChild<QPushButton *>("recoveryNow")->isEnabled());
    // Interval mode shows the ordinary countdown and permits an explicit retry before reset.
    job["session_limit_mode"]="interval";job["not_before"]=0;job["due_at"]=QDateTime::currentMSecsSinceEpoch()/1000.0+300;
    job["delays"]=QJsonArray{300,0};details["recovery"]=job;client->inspectionReady({},"codex/hgs/dashboard",details,{});
    QVERIFY(chip->fullLabel().contains("Retry in"));QVERIFY(panel->findChild<QPushButton *>("recoveryNow")->isEnabled());
    job["session_limit_mode"]="reset";job["state"]="blocked";job["reason"]="The reset time is unknown";
    details["recovery"]=job;client->inspectionReady({},"codex/hgs/dashboard",details,{});
    QVERIFY(panel->detailText().contains("reset time is unknown"));QVERIFY(!panel->findChild<QPushButton *>("recoveryNow")->isVisible());
    QCOMPARE(composer->editor()->toPlainText(),QString("Continue this draft after reset"));
}

void TestSessionsWindow::heldMessagesShowTheTerminalDraftThatHoldsThem()
{
    QTemporaryDir temp; const auto program = temp.filePath("hgs");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,os,pathlib,sys
a=sys.argv[1:]
if a[0].startswith('@'):a=a[1:]
here=pathlib.Path(__file__).parent
draft=(here/'draft').exists()
if a[0]=='inspect':
 print(json.dumps({'tracked':True,'run_id':'run-one','conversation_id':'conv-one','runtime_state':'live','process_state':'running',
  'activity':'idle','phase':'idle','input_hold_supported':True,
  'held_inputs':[{'request_id':'held-1','text':'Also run the tests','queued_at':1.0}] if draft else []}))
elif a[0]=='terminal':
 body=json.load(sys.stdin)
 with open(here/'terminal.jsonl','a') as f:f.write(json.dumps(body)+'\n')
 base={'request_id':body['request_id'],'name':a[1],'run_id':'run-one','conversation_id':'conv-one'}
 if body['action']=='draft':print(json.dumps(dict(base,status='draft',draft_supported=True,draft='fix the build' if draft else None,draft_hash='b'*64)))
 else:(here/'draft').unlink();print(json.dumps(dict(base,status='submitted')))
else:print('{}')
)PY"); fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QVERIFY(QFile(temp.filePath("draft")).open(QIODevice::WriteOnly));
    SessionsWindow window(FakeCli::program(program)); window.setFleet(fleet());
    window.setDetailsPollForTests(300, 24 * 60 * 60 * 1000); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *bar = window.findChild<QFrame *>("sessionTerminalDraft"); QVERIFY(bar);
    QTRY_VERIFY_WITH_TIMEOUT(bar->isVisible(), 15000);
    QVERIFY(bar->findChild<QLabel *>("terminalDraftText")->text().contains("fix the build"));
    bar->findChild<QPushButton *>("terminalDraftSend")->click();
    QTRY_VERIFY_WITH_TIMEOUT(!bar->isVisible(), 15000);
    QList<QJsonObject> calls; QFile log(temp.filePath("terminal.jsonl")); QVERIFY(log.open(QIODevice::ReadOnly));
    for (const auto &line : log.readAll().split('\n')) if (!line.isEmpty()) calls << QJsonDocument::fromJson(line).object();
    QVERIFY(!calls.isEmpty());
    QCOMPARE(calls.last()["action"].toString(), QString("submit_draft"));
    QCOMPARE(calls.last()["draft_hash"].toString(), QString(64, 'b'));
    QCOMPARE(calls.last()["expected_run_id"].toString(), QString("run-one"));
    // Nothing waits any more: the bar stays hidden.
    QTest::qWait(1000); QVERIFY(!bar->isVisible());
}

// Owner, 2026-10-28: text in the agent's Terminal input held every message
// sent from Activity. While messages wait for an agent that is not working,
// Activity shows that draft with Send it (Enter in Terminal for exactly it).
void TestSessionsWindow::aHeldMessageThatFailsComesBackToTheComposer()
{
    // #511 N5: the message hgs held, then could not deliver, comes back to the
    // session's composer with why.
    QTemporaryDir temp; const auto program = temp.filePath("hgs");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,os,pathlib,sys
a=sys.argv[1:]
if a[0].startswith('@'):a=a[1:]
here=pathlib.Path(__file__).parent
held=json.loads((here/'held.json').read_text()) if (here/'held.json').exists() else []
settled=json.loads((here/'settled.json').read_text()) if (here/'settled.json').exists() else []
if a[0]=='inspect':
 print(json.dumps({'tracked':True,'run_id':'run-one','conversation_id':'conv-one','runtime_state':'live','process_state':'running',
  'activity':'idle','phase':'idle','input_hold_supported':True,'held_inputs':held,'held_settled':settled}))
elif a[0]=='send':
 body=json.load(sys.stdin)
 with open(here/'sends.jsonl','a') as f:f.write(json.dumps(body)+'\n')
 held.append({'request_id':body['request_id'],'text':body['text'],'queued_at':1.0});(here/'held.json').write_text(json.dumps(held))
 print(json.dumps({'request_id':body['request_id'],'name':a[1],'run_id':'run-one','conversation_id':'conv-one','first_message':False,
  'status':'queued','text':body['text']}))
else:print('{}')
)PY"); fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    SessionsWindow window(FakeCli::program(program)); window.setFleet(fleet());
    window.setDetailsPollForTests(300, 24 * 60 * 60 * 1000); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    auto *send = composer->findChild<QPushButton *>("sendMessage"); QVERIFY(send);
    QVERIFY(window.findChild<QFrame *>("sessionHeldInputs"));
    // The first read must show input_hold_supported before the send.
    QTest::qWait(700);
    setComposerText(composer, "Also run the tests");
    QTRY_VERIFY_WITH_TIMEOUT(send->isEnabled(), 15000);
    send->click();
    QTRY_VERIFY_WITH_TIMEOUT(QFileInfo::exists(temp.filePath("sends.jsonl")), 15000);
    QFile log(temp.filePath("sends.jsonl")); QVERIFY(log.open(QIODevice::ReadOnly));
    const auto body = QJsonDocument::fromJson(log.readAll().split('\n').first()).object(); log.close();
    QVERIFY(body["queue_when_unconfirmed"].toBool());
    QTRY_VERIFY_WITH_TIMEOUT(composer->editor()->toPlainText().isEmpty(), 15000);
    QVERIFY(FakeCli::replace(temp.filePath("held.json"), "[]"));
    QVERIFY(FakeCli::replace(temp.filePath("settled.json"), QJsonDocument(QJsonArray{QJsonObject{{"request_id", body["request_id"]},
        {"text", body["text"]}, {"status", "failed"}, {"error", "session identity changed; nothing was sent"}}}).toJson()));
    QTRY_COMPARE_WITH_TIMEOUT(composer->editor()->toPlainText(), QStringLiteral("Also run the tests"), 15000);
    QTRY_VERIFY(window.findChild<QLabel *>("notice")->text().contains(QStringLiteral("Not delivered: session identity changed")));
    QVERIFY(window.findChild<QLabel *>("notice")->property("error").toBool());
    // Only once: a new draft stays as it is on the next reads.
    setComposerText(composer, "Something else");
    QTest::qWait(1000);
    QCOMPARE(composer->editor()->toPlainText(), QStringLiteral("Something else"));
}

void TestSessionsWindow::coldCacheClearKeepsDraftAndPinsIdentity()
{
    QTemporaryDir temp;const auto program=temp.filePath("hgs"),capture=temp.filePath("clear.json");
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib,time
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
if 'clear-context' in args:
 p=json.load(sys.stdin);p['args']=args;staged=root/'clear.tmp';staged.write_text(json.dumps(p));staged.replace(root/'clear.json');time.sleep(.2);print(json.dumps(dict(request_id=p['request_id'],name=args[2],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],status='confirmed')))
elif 'inspect' in args: print((root/'details.json').read_text() if (root/'details.json').exists() else '{}')
else: print('{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"phase","idle"},{"activity","idle"},{"runtime_state","live"},{"process_state","running"},
        {"conversation_id","conversation-one"},{"clear_context_supported",true},{"cache_hint",QJsonObject{{"status","cold"},{"tokens",756000}}}};
    // Background inspections read this file at any time: replace it atomically and
    // write it before selecting the session, so a slow first inspection cannot
    // deliver empty details after the test applies its own.
    const auto writeDetails=[&]{QVERIFY(FakeCli::replace(temp.filePath("details.json"),QJsonDocument(details).toJson()));};
    writeDetails();
    SessionsWindow window(FakeCli::program(program));window.resize(1080,760);window.setFleet(fleet());window.show();window.showSession("mac","claude/infra/review");QTest::qWait(150);
    auto *client=window.findChild<HgsClient *>();
    const auto applyDetails=[&]{writeDetails();client->inspectionReady("mac","claude/infra/review",details);};
    // The window inspects on its own, and each reply carries details.json as it was
    // when that inspection started, so one still in flight can bring back identity
    // from before the last change and cancel or allow a confirmation. HgsClient runs
    // one inspection per session: once ours starts none is stale, and its reply is
    // current.
    const auto settle=[&]{QSignalSpy replies(client,&HgsClient::inspectionReady);qsizetype issued=-1;
        QTRY_VERIFY(issued>=0 || (client->requestInspection("mac","claude/infra/review") && (issued=replies.size(),true)));
        QTRY_VERIFY(replies.size()>issued);};
    applyDetails();
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer, "Keep this draft");
    auto *cacheChip=window.findChild<ToolbarChip *>("cacheChip");QVERIFY(cacheChip && cacheChip->isVisible());
    QCOMPARE(cacheChip->fullLabel(),QString("Cold cache"));QCOMPARE(cacheChip->tone(),ChipTone::Danger);
    QTest::mouseClick(cacheChip,Qt::LeftButton);QTRY_VERIFY(cacheChip->popover()->isVisible());
    QVERIFY(window.findChild<QLabel *>("cacheWarning")->text().contains("cold cache"));
    auto *clear=window.findChild<QPushButton *>("cacheClearContext");QVERIFY(clear->isVisible());QVERIFY(clear->isEnabled());QCOMPARE(clear->text(),QString("Clear context"));
    // Every wrapped line fits: the popover is as tall as its content needs at its real width.
    for(auto *label:window.findChild<QWidget *>("cachePopover")->findChildren<QLabel *>())
        QVERIFY2(label->height()>=label->heightForWidth(label->width()),qPrintable(label->objectName()+QString(" %1 < %2").arg(label->height()).arg(label->heightForWidth(label->width()))));
    QSignalSpy launched(&window,&SessionsWindow::newSessionRequested);QSignalSpy finished(client,&HgsClient::sessionActionFinished);
    // A change made while the confirmation is open settles before the answer.
    const auto confirmClear=[&](const std::function<void()> &change={}){QTimer::singleShot(0,&window,[&,change]{
        if(change){change();settle();}
        auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);
        QCOMPARE(dialog->defaultButton(),dialog->button(QMessageBox::Cancel));
        QCOMPARE(dialog->escapeButton(),dialog->button(QMessageBox::Cancel));
        QVERIFY(dialog->text().contains("claude/infra/review"));QVERIFY(dialog->text().contains("mac"));QVERIFY(dialog->text().contains("draft"));
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}QFAIL("Missing clear confirmation");
    });};
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);dialog->button(QMessageBox::Cancel)->click();});
    clear->click();QVERIFY(!QFileInfo::exists(capture));QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    confirmClear([&]{details["conversation_id"]="changed";applyDetails();});
    clear->click();QVERIFY(!QFileInfo::exists(capture));details["conversation_id"]="conversation-one";applyDetails();settle();
    confirmClear();clear->click();QVERIFY(!clear->isEnabled());QTRY_VERIFY(QFileInfo::exists(capture));
    QFile sent(capture);QVERIFY(sent.open(QIODevice::ReadOnly));const auto payload=QJsonDocument::fromJson(sent.readAll()).object();sent.close();
    QCOMPARE(payload["expected_run_id"].toString(),QString("run-one"));QCOMPARE(payload["expected_conversation_id"].toString(),QString("conversation-one"));
    QCOMPARE(payload["args"].toArray(),QJsonArray({"@mac","clear-context","claude/infra/review","--json"}));
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));QVERIFY(!window.findChild<NewSessionDialog *>());QCOMPARE(launched.size(),0);
    QTRY_COMPARE(finished.size(),1);QVERIFY(finished[0][1].toBool());QVERIFY(QFile::remove(capture));applyDetails();
    // The send warning offers the same reset, without sending the draft.
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("coldCacheConfirm");QVERIFY(dialog);QVERIFY(dialog->text().contains("token usage"));
        for(auto *button:dialog->buttons())if(button->text()=="Clear context"){confirmClear();button->click();return;}QFAIL("Missing clear option");});
    composer->findChild<QPushButton *>("sendMessage")->click();QTRY_VERIFY(QFileInfo::exists(capture));QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    QTRY_COMPARE(finished.size(),2);QVERIFY(QFile::remove(capture));
    // The permanent Details action works with Terminal selected and a warm cache.
    details.remove("cache_hint");applyDetails();
    window.findChild<QPushButton *>("toggleInspector")->setChecked(true);
    window.findChild<QPushButton *>("toggleInspector")->setChecked(true);
    auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");tabs->setCurrentIndex(1);QTest::qWait(30);
    auto *detailsClear=window.findChild<QPushButton *>("clearSessionFromDetails");QVERIFY(detailsClear->isVisible());QVERIFY(detailsClear->isEnabled());
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);dialog->button(QMessageBox::Cancel)->click();});
    detailsClear->click();QVERIFY(!QFileInfo::exists(capture));QCOMPARE(tabs->currentIndex(),1);
    confirmClear([&]{details["run_id"]="new-run";applyDetails();});
    detailsClear->click();QVERIFY(!QFileInfo::exists(capture));settle();
    confirmClear();detailsClear->click();QTRY_VERIFY(QFileInfo::exists(capture));QCOMPARE(tabs->currentIndex(),1);
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));QCOMPARE(launched.size(),0);
    QTRY_COMPARE(finished.size(),3);QVERIFY(QFile::remove(capture));
    window.findChild<QPushButton *>("toggleInspector")->setChecked(false);
    // The header icon exposes the same confirmed reset with the inspector hidden.
    auto *more=window.findChild<QPushButton *>("more");QVERIFY(more);QVERIFY(more->menu());
    auto *headerClear=window.findChild<QPushButton *>("clearSessionAction");QVERIFY(headerClear);
    QVERIFY(headerClear->isVisible());QVERIFY(headerClear->isEnabled());QVERIFY(!more->menu()->findChild<QAction *>("clearSessionAction"));
    QCOMPARE(headerClear->accessibleName(),QString("Clear session"));
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);dialog->button(QMessageBox::Cancel)->click();});
    headerClear->click();QVERIFY(!QFileInfo::exists(capture));QCOMPARE(tabs->currentIndex(),1);
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    // The menu names the icon and asks the same confirmation.
    auto *menuClear=more->menu()->findChild<QAction *>("menuClearSessionAction");QVERIFY(menuClear);
    emit more->menu()->aboutToShow();QVERIFY(menuClear->isVisible());QVERIFY(menuClear->isEnabled());QCOMPARE(menuClear->text(),QString("Clear session…"));
    bool asked=false;
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);asked=true;dialog->button(QMessageBox::Cancel)->click();});
    menuClear->trigger();QVERIFY(asked);QVERIFY(!QFileInfo::exists(capture));QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    details["clear_context_supported"]=false;applyDetails();QVERIFY(!headerClear->isEnabled());QVERIFY(!detailsClear->isEnabled());
    emit more->menu()->aboutToShow();QVERIFY(!menuClear->isEnabled());
    details["clear_context_supported"]=true;applyDetails();QVERIFY(headerClear->isEnabled());
    details.remove("conversation_id");applyDetails();QTRY_VERIFY(!headerClear->isEnabled());
    details["conversation_id"]="conversation-one";applyDetails();QTRY_VERIFY(headerClear->isEnabled());settle();
    confirmClear();headerClear->click();QVERIFY(!headerClear->isEnabled());
    QTRY_VERIFY(QFileInfo::exists(capture));QVERIFY(sent.open(QIODevice::ReadOnly));
    const auto menuPayload=QJsonDocument::fromJson(sent.readAll()).object();sent.close();
    QCOMPARE(menuPayload["expected_run_id"].toString(),QString("new-run"));
    QCOMPARE(menuPayload["expected_conversation_id"].toString(),QString("conversation-one"));
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));QCOMPARE(tabs->currentIndex(),1);
    QTRY_COMPARE(finished.size(),4);QVERIFY(!menuPayload.contains("dismiss_prompt"));QVERIFY(QFile::remove(capture));
    // While Claude shows a chooser, an explicit reset first dismisses it with Esc.
    details["phase"]="input";details["activity"]="busy";details["clear_context_supported"]=false;
    details["clear_context_dismiss"]=true;applyDetails();QTRY_VERIFY(headerClear->isEnabled());settle();
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);
        QCOMPARE(dialog->defaultButton(),dialog->button(QMessageBox::Cancel));
        QVERIFY(dialog->text().contains("Esc"));QVERIFY(dialog->text().contains("denied"));QVERIFY(dialog->text().contains("draft"));
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){
            QCOMPARE(button->text(),QString("Dismiss and clear"));button->click();return;}
        QFAIL("Missing dismissal confirmation");});
    headerClear->click();QTRY_VERIFY(QFileInfo::exists(capture));QVERIFY(sent.open(QIODevice::ReadOnly));
    const auto dismissPayload=QJsonDocument::fromJson(sent.readAll()).object();sent.close();
    QCOMPARE(dismissPayload["dismiss_prompt"].toBool(),true);QCOMPARE(dismissPayload["expected_run_id"].toString(),QString("new-run"));
    QTRY_COMPARE(finished.size(),5);QVERIFY(QFile::remove(capture));
    // A chooser that closed before confirmation is not dismissed.
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);
        details["clear_context_dismiss"]=false;details["clear_context_supported"]=true;details["phase"]="idle";details["activity"]="idle";
        applyDetails();settle();
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}});
    headerClear->click();QTest::qWait(100);QVERIFY(!QFileInfo::exists(capture));
    details["clear_context_supported"]=false;applyDetails();QVERIFY(!headerClear->isEnabled());tabs->setCurrentIndex(0);
    QTest::qWait(350);details["phase"]="compacting";details["activity"]="busy";details["clear_context_supported"]=false;details.remove("cache_hint");details["session_usage"]=QJsonObject{{"status","ok"},{"context",QJsonObject{{"used",215900},{"limit",258400}}}};
    applyDetails();settle();QTest::qWait(50);QVERIFY(!headerClear->isEnabled());
    auto *progress=window.findChild<ActivityView *>("mainActivity")->compactionIndicator();auto *context=window.findChild<QWidget *>("activityContext");
    QVERIFY(progress->isVisible());QVERIFY(qAbs(progress->mapToGlobal(progress->rect().center()).y()-context->mapToGlobal(context->rect().center()).y())<=2);
    const auto preview=qEnvironmentVariable("HGS_WORKTREE_PREVIEW");if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+"/compaction-footer.png"));}
}

void TestSessionsWindow::coldCacheExplainsUnavailableContextActions()
{
    QTemporaryDir temp;const auto program=temp.filePath("hgs");
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import pathlib,sys
p=pathlib.Path(__file__).parent/'details.json'
print(p.read_text() if 'inspect' in sys.argv and p.exists() else '{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    // The main turn finished, but a workflow child still works: the host names it.
    const QString working=QString::fromUtf8("Wait until the agent reports Ready. Still working: subagent “<b>workflow-subagent</b>” running Bash");
    // What a tooltip displays: Qt renders a tooltip that looks like HTML as rich text.
    const auto shownTip=[](const QString &tip){if(!Qt::mightBeRichText(tip))return tip;QTextDocument document;document.setHtml(tip);return document.toPlainText();};
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},{"runtime_state","live"},{"process_state","running"},
        {"activity","busy"},{"phase","working"},{"compact_context_supported",false},{"clear_context_supported",false},
        {"compact_context_reason",working},{"clear_context_reason",working},{"cache_hint",QJsonObject{{"status","cold"},{"tokens",756000}}}};
    // Inspections read details.json at any time: replace it atomically and write it
    // before selecting the session.
    const auto writeDetails=[&]{QVERIFY(FakeCli::replace(temp.filePath("details.json"),QJsonDocument(details).toJson()));};
    writeDetails();
    SessionsWindow window(FakeCli::program(program));keepDetailsPollsOut(window);window.resize(1080,760);window.setFleet(fleet());
    window.show();window.showSession("mac","claude/infra/review");
    auto *client=window.findChild<HgsClient *>();auto *composer=window.findChild<MessageComposer *>("messageComposer");
    const auto apply=[&]{writeDetails();settleSelectedInspection(composer,client,"mac","claude/infra/review",details);};
    apply();setComposerText(composer,"Keep this draft");const auto key=composer->sessionKey();
    auto *send=composer->findChild<QPushButton *>("sendMessage");QVERIFY(send->isEnabled());
    // Opens the send confirmation once it is shown, records its context actions and cancels.
    struct Shown{bool seen=false,compact=false,clear=false,plain=false;QString compactTip,clearTip,line;} shown;
    const auto confirm=[&]{
        shown={};auto *poll=new QTimer(&window);poll->setInterval(5);QElapsedTimer age;age.start();
        QObject::connect(poll,&QTimer::timeout,&window,[poll,age,&window,&shown,shownTip]{
            auto *dialog=window.findChild<QMessageBox *>("coldCacheConfirm");
            if(!(dialog&&dialog->isVisible())&&age.elapsed()<5000)return;
            poll->stop();poll->deleteLater();if(!dialog||!dialog->isVisible())return;
            shown.seen=true;shown.line=dialog->informativeText();
            // The reported names stay literal text.
            const auto labels=dialog->findChildren<QLabel *>();
            shown.plain=std::any_of(labels.cbegin(),labels.cend(),[&](const QLabel *label){return label->text()==shown.line;})
                && std::all_of(labels.cbegin(),labels.cend(),[&](const QLabel *label){
                    return label->text()!=shown.line||label->textFormat()==Qt::PlainText;});
            for(auto *button:dialog->buttons()){
                if(button->text()=="Compact and continue"){shown.compact=button->isEnabled();shown.compactTip=shownTip(button->toolTip());}
                if(button->text()=="Clear context"){shown.clear=button->isEnabled();shown.clearTip=shownTip(button->toolTip());}
            }
            dialog->button(QMessageBox::Cancel)->click();});
        poll->start();send->click();};
    confirm();QVERIFY(shown.seen);QVERIFY(!shown.compact);QVERIFY(!shown.clear);QVERIFY(shown.plain);
    QCOMPARE(shown.compactTip,working+'.');QCOMPARE(shown.clearTip,working+'.');
    QCOMPARE(shown.line,"Compact and continue and Clear context are unavailable. "+working+'.');
    QVERIFY(!composer->isSending(key));QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    // The persistent clear controls carry the same reason.
    auto *cacheClear=window.findChild<QPushButton *>("cacheClearContext");auto *detailsClear=window.findChild<QPushButton *>("clearSessionFromDetails");
    auto *headerClear=window.findChild<QPushButton *>("clearSessionAction");QVERIFY(headerClear);
    QVERIFY(!cacheClear->isEnabled());QVERIFY(!detailsClear->isEnabled());QVERIFY(!headerClear->isEnabled());
    QCOMPARE(shownTip(cacheClear->toolTip()),working+'.');QCOMPARE(shownTip(detailsClear->toolTip()),working+'.');QCOMPARE(shownTip(headerClear->toolTip()),working+'.');
    // An offline host's last report no longer explains the actions.
    auto offline=fleet();auto peer=*offline.peer("mac");peer.ok=false;offline.setPeer(peer,QDateTime::currentMSecsSinceEpoch());
    window.setFleet(offline);QVERIFY(!cacheClear->isEnabled());QVERIFY(!detailsClear->isEnabled());
    QCOMPARE(cacheClear->toolTip(),QString::fromUtf8("Clear this agent’s conversation, like /clear in Terminal. Your unsent message stays here."));
    QCOMPARE(detailsClear->toolTip(),QString("Start this agent's conversation from scratch, like /clear in Terminal. Your unsent draft is kept."));
    QCOMPARE(headerClear->toolTip(),"Clear session\n"+detailsClear->toolTip());
    window.setFleet(fleet());apply();QCOMPARE(shownTip(cacheClear->toolTip()),working+'.');QCOMPARE(shownTip(detailsClear->toolTip()),working+'.');
    // A Claude chooser still allows Dismiss and clear, so that action keeps its usual text.
    details["clear_context_dismiss"]=true;apply();
    QVERIFY(detailsClear->isEnabled());QVERIFY(!cacheClear->isEnabled());QCOMPARE(shownTip(cacheClear->toolTip()),working+'.');
    const QString usualClear="Start this agent's conversation from scratch, like /clear in Terminal. Your unsent draft is kept.";
    QCOMPARE(detailsClear->toolTip(),usualClear);QCOMPARE(headerClear->toolTip(),"Clear session\n"+usualClear);
    details.remove("clear_context_dismiss");
    // Each action names its own reason; an available one keeps its usual text.
    const QString resuming="The agent has not confirmed the resumed conversation yet";
    details["compact_context_reason"]=resuming;details["clear_context_supported"]=true;details["clear_context_reason"]="";apply();
    confirm();QVERIFY(shown.seen);QVERIFY(!shown.compact);QVERIFY(shown.clear);
    QCOMPARE(shown.compactTip,resuming+'.');QVERIFY(shown.clearTip.isEmpty());
    QCOMPARE(shown.line,"Compact and continue is unavailable. "+resuming+'.');
    const QString usualCache=QString::fromUtf8("Clear this agent’s conversation, like /clear in Terminal. Your unsent message stays here.");
    QVERIFY(cacheClear->isEnabled());QCOMPARE(cacheClear->toolTip(),usualCache);QCOMPARE(detailsClear->toolTip(),usualClear);
    const QString unconfirmed="The last compaction was not confirmed. Check Terminal.";
    details["compact_context_supported"]=true;details["compact_context_reason"]="";
    details["clear_context_supported"]=false;details["clear_context_reason"]=unconfirmed;apply();
    confirm();QVERIFY(shown.seen);QVERIFY(shown.compact);QVERIFY(!shown.clear);
    QCOMPARE(shown.compactTip,QString("Run native /compact, then send this draft only after successful completion."));
    QCOMPARE(shown.clearTip,unconfirmed);QCOMPARE(shown.line,"Clear context is unavailable. "+unconfirmed);
    // An older host reports no reason: the actions stay disabled without one.
    details["compact_context_supported"]=false;details.remove("compact_context_reason");details.remove("clear_context_reason");apply();
    confirm();QVERIFY(shown.seen);QVERIFY(!shown.compact);QVERIFY(!shown.clear);QVERIFY(shown.line.isEmpty());
    QCOMPARE(shown.compactTip,QString("Run native /compact, then send this draft only after successful completion."));QVERIFY(shown.clearTip.isEmpty());
    QCOMPARE(cacheClear->toolTip(),usualCache);QCOMPARE(detailsClear->toolTip(),usualClear);QCOMPARE(headerClear->toolTip(),"Clear session\n"+usualClear);
    QVERIFY(!composer->isSending(key));QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
}

void TestSessionsWindow::stoppedSessionStartsFreshOrSendsAfterStart()
{
    QTemporaryDir temp;const auto program=temp.filePath("hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a[0].startswith('@'):a=a[1:]
if a[0]=='resume':
 with open(root/'resume.log','a') as log:log.write(json.dumps(a)+'\n')
elif a[0] in ['send','compact-context']:
 p=json.load(sys.stdin);staged=root/(a[0]+'.tmp');staged.write_text(json.dumps(p));staged.replace(root/(a[0]+'.json'))
 print(json.dumps(dict(request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],first_message=not p['expected_conversation_id'],status='submitted')))
elif a[0]=='inspect':print((root/'details.json').read_text() if a[1]=='codex/website/navigation' and (root/'details.json').exists() else '{}')
else:print('{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    const QString name="codex/website/navigation";
    auto state=fleet();auto box=state.local();
    const auto setState=[&](const QString &value,const QString &run,const QString &conversation){
        for(auto &session:box.sessions)if(session.name==name){session.state=value;session.runId=run;session.conversationId=conversation;
            session.processState=value=="running"?"running":"exited";session.activity="idle";session.phase="idle";}
        state.setLocal(box,QDateTime::currentMSecsSinceEpoch());};
    setState("stopped","run-old","conversation-old");
    QJsonObject details{{"tracked",true},{"run_id","run-old"},{"conversation_id","conversation-old"},{"runtime_state","saved"},{"process_state","exited"}};
    {QFile initial(temp.filePath("details.json"));QVERIFY(initial.open(QIODevice::WriteOnly));initial.write(QJsonDocument(details).toJson());}
    SessionsWindow window(FakeCli::program(program));window.setFleet(state);window.show();window.showSession({},name);QTest::qWait(100);
    auto *client=window.findChild<HgsClient *>();QSignalSpy writes(client,&HgsClient::writeDone);
    // The window polls a starting or compacting session on its own, and each reply
    // carries details.json as it was when that inspection started. HgsClient runs one
    // inspection per session: once ours starts after the write, a stale reply cannot
    // arrive after the new details and cancel the continuation as another run.
    const auto settle=[&]{QSignalSpy replies(client,&HgsClient::inspectionReady);qsizetype issued=-1;
        const auto ours=[&]{qsizetype count=0;for(const auto &reply:replies)count+=reply.at(1).toString()==name;return count;};
        QTRY_VERIFY(issued>=0 || (client->requestInspection({},name) && (issued=ours(),true)));
        QTRY_VERIFY(ours()>issued);};
    // The test and the fixture both replace files atomically: a polled file is never read empty.
    const auto apply=[&]{QVERIFY(FakeCli::replace(temp.filePath("details.json"),QJsonDocument(details).toJson()));
        settle();client->inspectionReady({},name,details);};apply();
    const auto resumes=[&]{QFile log(temp.filePath("resume.log"));return log.open(QIODevice::ReadOnly)?QString::fromUtf8(log.readAll()).split('\n',Qt::SkipEmptyParts):QStringList();};
    const QString freshArgs=R"(["resume", "codex/website/navigation", "--fresh", "--expected-run-id", "run-old", "-d"])";
    // A stopped agent cannot clear natively; the header menu offers a confirmed fresh start.
    auto *fresh=window.findChild<QAction *>("freshSessionAction");QVERIFY(fresh);QVERIFY(fresh->isVisible());QVERIFY(fresh->isEnabled());
    QVERIFY(!window.findChild<QPushButton *>("clearSessionAction")->isVisible());
    auto *more=window.findChild<QPushButton *>("more");emit more->menu()->aboutToShow();
    QVERIFY(!more->menu()->findChild<QAction *>("menuClearSessionAction")->isVisible());
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("startFreshConfirm");QVERIFY(dialog);
        QCOMPARE(dialog->defaultButton(),dialog->button(QMessageBox::Cancel));QVERIFY(dialog->text().contains(name));
        dialog->button(QMessageBox::Cancel)->click();});
    fresh->trigger();QTest::qWait(100);QVERIFY(resumes().isEmpty());
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("startFreshConfirm");QVERIFY(dialog);
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}QFAIL("Missing fresh confirmation");});
    fresh->trigger();QTRY_COMPARE(writes.size(),1);QVERIFY(writes[0][1].toBool());QCOMPARE(resumes(),QStringList{freshArgs});
    // Send stays available and asks how to start; cancelling keeps the draft.
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *send=composer->findChild<QPushButton *>("sendMessage");
    auto *status=composer->findChild<QLabel *>("messageStatus");
    setComposerText(composer, "Continue after start");QTRY_VERIFY(send->isEnabled());
    QCOMPARE(send->text(),QString("Send"));QVERIFY(!send->icon().isNull());QVERIFY(status->text().contains("resume it or start fresh"));
    const auto choose=[&](const char *button){QTimer::singleShot(0,&window,[&,button]{auto *dialog=window.findChild<QMessageBox *>("startToSendConfirm");QVERIFY(dialog);
        QCOMPARE(dialog->defaultButton(),dialog->button(QMessageBox::Cancel));
        if(qstrcmp(button,"cancel")==0)dialog->button(QMessageBox::Cancel)->click();else dialog->findChild<QPushButton *>(button)->click();});};
    choose("cancel");send->click();QTest::qWait(100);QCOMPARE(resumes().size(),1);
    QCOMPARE(composer->editor()->toPlainText(),QString("Continue after start"));
    // A resume that reports another conversation never receives the draft.
    choose("resumeAndSend");send->click();QTRY_COMPARE(writes.size(),2);
    QCOMPARE(resumes().last(),QString(R"(["resume", "codex/website/navigation", "-d"])"));
    QVERIFY(!send->isEnabled());QVERIFY(status->text().contains("Waiting for the session to start"));
    QCOMPARE(send->text(), QString("Waiting to send…")); QVERIFY(composer->editor()->isReadOnly());
    QVERIFY(window.findChild<QPushButton *>("cancelCompactSend")->isVisible());
    setState("running","run-resumed","conversation-other");window.setFleet(state);
    details=QJsonObject{{"tracked",true},{"run_id","run-resumed"},{"conversation_id","conversation-other"},{"runtime_state","live"},
        {"process_state","running"},{"activity","idle"},{"phase","idle"}};apply();
    QTRY_VERIFY(status->text().contains("unexpected conversation"));QVERIFY(!QFileInfo::exists(temp.filePath("send.json")));
    QCOMPARE(composer->editor()->toPlainText(),QString("Continue after start"));
    // A fresh start delivers the unchanged draft once the new run accepts input.
    setState("stopped","run-old","conversation-old");window.setFleet(state);
    details=QJsonObject{{"tracked",true},{"run_id","run-old"},{"conversation_id","conversation-old"},{"runtime_state","saved"},{"process_state","exited"}};apply();
    QTRY_VERIFY(send->isEnabled());choose("startFreshAndSend");send->click();QTRY_COMPARE(writes.size(),3);QCOMPARE(resumes().last(),freshArgs);
    setState("running","run-new",{});window.setFleet(state);
    details=QJsonObject{{"tracked",true},{"run_id","run-new"},{"runtime_state","live"},{"process_state","running"},
        {"activity","idle"},{"phase","idle"},{"first_message_can_send",true}};apply();
    QTRY_VERIFY(QFileInfo::exists(temp.filePath("send.json")));
    QFile sent(temp.filePath("send.json"));QVERIFY(sent.open(QIODevice::ReadOnly));const auto payload=QJsonDocument::fromJson(sent.readAll()).object();
    QCOMPARE(payload["expected_run_id"].toString(),QString("run-new"));QVERIFY(QJsonDocument(payload).toJson().contains("Continue after start"));
    QVERIFY(!window.findChild<QMessageBox *>("coldCacheConfirm"));
    // A resumed conversation compacts before delivering the captured message.
    sent.close(); QTRY_VERIFY(!composer->isSending(composer->sessionKey()));
    QVERIFY(QFile::remove(temp.filePath("send.json")));
    setState("stopped","run-old","conversation-old");window.setFleet(state);
    details=QJsonObject{{"tracked",true},{"run_id","run-old"},{"conversation_id","conversation-old"},{"runtime_state","saved"},{"process_state","exited"}};apply();
    setComposerText(composer, "Continue with a compacted conversation");
    QTRY_VERIFY2(send->isEnabled(),qPrintable(status->text()));choose("resumeCompactAndSend");send->click();QTRY_COMPARE(writes.size(),4);
    QVERIFY(status->text().contains("then compact"));
    window.showSession({},"codex/hgs/dashboard");
    setComposerText(composer, "Keep the other dialog draft");
    QVERIFY(!window.findChild<QPushButton *>("cancelCompactSend")->isVisible());
    QVERIFY(!QFileInfo::exists(temp.filePath("compact-context.json")));
    setState("running","run-resumed","conversation-old");window.setFleet(state);
    details=QJsonObject{{"tracked",true},{"run_id","run-resumed"},{"conversation_id","conversation-old"},
        {"runtime_state","live"},{"process_state","running"},{"activity","idle"},{"phase","idle"},
        {"compact_context_supported",true},{"cache_hint",QJsonObject{{"status","cold"}}}};apply();
    QTRY_VERIFY(QFileInfo::exists(temp.filePath("compact-context.json")));
    QVERIFY(!QFileInfo::exists(temp.filePath("send.json")));
    QFile compacted(temp.filePath("compact-context.json"));QVERIFY(compacted.open(QIODevice::ReadOnly));
    const auto compactPayload=QJsonDocument::fromJson(compacted.readAll()).object();
    QCOMPARE(compactPayload["expected_run_id"].toString(),QString("run-resumed"));
    QCOMPARE(compactPayload["expected_conversation_id"].toString(),QString("conversation-old"));
    details["compact_context_request"]=QJsonObject{{"status","completed"},{"request_id",compactPayload["request_id"]}};
    apply();QTRY_VERIFY(QFileInfo::exists(temp.filePath("send.json")));
    QFile finalSend(temp.filePath("send.json"));QVERIFY(finalSend.open(QIODevice::ReadOnly));
    const auto finalPayload=QJsonDocument::fromJson(finalSend.readAll()).object();
    QCOMPARE(finalPayload["text"].toString(),QString("Continue with a compacted conversation"));
    QCOMPARE(finalPayload["expected_compaction_id"],compactPayload["request_id"]);
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep the other dialog draft"));
    QVERIFY(!window.findChild<QMessageBox *>("coldCacheConfirm"));

}

void TestSessionsWindow::localStartsAskAboutAnotherWindowsSession()
{
    // Spec D5: resuming, opening or forking a saved session of this Windows computer
    // asks first when its session host runs in another Windows session.
    const auto reset = qScopeGuard([] { LocalSessionHost::resetOwnWindowsSessionForTests(); });
    LocalSessionHost::setOwnWindowsSessionForTests(1);
    const QString name = "codex/website/navigation";
    auto state = fleet(); auto box = state.local(); box.metrics["platform"] = "windows";
    box.localHost = QJsonObject{{"state", "running"}, {"pid", 4242}, {"protocol", 1}, {"conpty", "bundled"},
        {"integrity", "medium"}, {"windows_session", 2}};
    for (auto &session : box.sessions) if (session.name == name) session.runId = "saved-run";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    const QJsonObject details{{"tracked", true}, {"run_id", "saved-run"}, {"conversation_id", "conversation-one"},
        {"fork_supported", true}, {"runtime_state", "saved"}, {"process_state", "exited"},
        {"events", QJsonArray{}}, {"cursor", 0}};
    const auto fixture = programInspecting("other-windows-session-hgs", details); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(fixture); window.setFleet(state); keepDetailsPollsOut(window); window.show(); window.showSession({}, name);
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    QSignalSpy opened(&window, &SessionsWindow::sessionActivated);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Keep this draft");
    settleSelectedInspection(composer, client, {}, name, details);
    auto *resume = window.findChild<QAction *>("pauseAction"); QTRY_VERIFY(resume->isEnabled());
    LaunchWarnings warnings(window);
    // Cancel starts nothing and keeps the draft.
    resume->trigger(); QCOMPARE(warnings.shown, 1);
    QVERIFY(warnings.texts[0].contains("another Windows session (2)"));
    QVERIFY(resume->isEnabled()); QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft"));
    auto *open = window.findChild<QAction *>("openSessionAction"); QVERIFY(open->isEnabled());
    open->trigger(); QCOMPARE(warnings.shown, 2); QCOMPARE(opened.size(), 0);
    QTest::qWait(200); QCOMPARE(writes.size(), 0);
    // A listing update rebuilds the entries while the warning is open. The accepted
    // launch still belongs to the same selected session and must resume exactly once.
    auto duringWarning = state; auto updated = duringWarning.local();
    SessionInfo earlier; earlier.name = "codex/a-first"; earlier.cmd = "codex";
    updated.sessions.prepend(earlier); duringWarning.setLocal(updated, QDateTime::currentMSecsSinceEpoch());
    QTimer::singleShot(0, &window, [&] { window.setFleet(duringWarning); });
    warnings.answer = "launchAnyway"; resume->trigger(); QCOMPARE(warnings.shown, 3);
    QTRY_COMPARE_WITH_TIMEOUT(writes.size(), 1, 15000); QCOMPARE(writes[0][0].toString(), "resume " + name);
    QTRY_VERIFY(open->isEnabled()); open->trigger(); QCOMPARE(warnings.shown, 4); QCOMPARE(opened.size(), 1);
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft"));
    // A host in the GUI's own Windows session asks nothing.
    LocalSessionHost::setOwnWindowsSessionForTests(2);
    QTRY_VERIFY(resume->isEnabled()); resume->trigger(); QTRY_COMPARE_WITH_TIMEOUT(writes.size(), 2, 15000); QCOMPARE(warnings.shown, 4);
    // Cancel in the warning keeps the fork dialog and its name; Launch anyway forks once.
    LocalSessionHost::setOwnWindowsSessionForTests(1); warnings.answer = "cancel";
    // Fork support arrives with the first inspection, which a loaded runner may delay.
    auto *fork = window.findChild<QAction *>("forkSessionAction"); QTRY_VERIFY_WITH_TIMEOUT(fork->isEnabled(), 20000);
    bool checked = false;
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("forkSessionDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        auto *forkName = dialog->findChild<QLineEdit *>("forkSessionName"); forkName->setText("forked-here");
        auto *create = dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Ok);
        create->click(); QCOMPARE(warnings.shown, 5);
        QVERIFY(dialog->isVisible()); QCOMPARE(forkName->text(), QString("forked-here")); QCOMPARE(writes.size(), 2);
        warnings.answer = "launchAnyway"; create->click(); QCOMPARE(warnings.shown, 6); checked = true;
    });
    fork->trigger(); QVERIFY(checked);
    QTRY_COMPARE_WITH_TIMEOUT(writes.size(), 3, 15000);
    QVERIFY(writes[2][2].toString().startsWith("fork\n" + name + "\n-n\nforked-here\n-d"));
    QTest::qWait(200); QCOMPARE(writes.size(), 3); QCOMPARE(warnings.shown, 6);
}

void TestSessionsWindow::localStartConfirmationKeepsIdentityAcrossRefresh_data()
{
    QTest::addColumn<QString>("run"); QTest::addColumn<QString>("conversation"); QTest::addColumn<bool>("activate");
    QTest::newRow("same-identity") << QString("saved-run") << QString("conversation-one") << true;
    QTest::newRow("changed-run") << QString("replacement-run") << QString("conversation-one") << false;
    QTest::newRow("changed-conversation") << QString("saved-run") << QString("replacement-conversation") << false;
}

void TestSessionsWindow::localStartConfirmationKeepsIdentityAcrossRefresh()
{
    QFETCH(QString, run); QFETCH(QString, conversation); QFETCH(bool, activate);
    const auto reset = qScopeGuard([] { LocalSessionHost::resetOwnWindowsSessionForTests(); });
    LocalSessionHost::setOwnWindowsSessionForTests(1);
    const QString name = "codex/website/navigation";
    auto state = fleet(); auto box = state.local(); box.metrics["platform"] = "windows";
    box.localHost = QJsonObject{{"state", "running"}, {"windows_session", 2}};
    for (auto &session : box.sessions) if (session.name == name) {
        session.state = "paused"; session.runId = "saved-run"; session.conversationId = "conversation-one";
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    const QJsonObject details{{"tracked", true}, {"run_id", "saved-run"}, {"conversation_id", "conversation-one"},
        {"runtime_state", "saved"}, {"process_state", "exited"}, {"terminal_supported", true},
        {"events", QJsonArray{}}, {"cursor", 0}};
    const auto fixture = programInspecting("refresh-during-launch-warning", details); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(fixture); window.setFleet(state); window.show(); window.showSession({}, name);
    auto *client = window.findChild<HgsClient *>();
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    settleSelectedInspection(composer, client, {}, name, details);
    setComposerText(composer, "Keep this draft");
    auto *open = window.findChild<QAction *>("openSessionAction"); QTRY_VERIFY(open->isEnabled());
    QSignalSpy opened(&window, &SessionsWindow::sessionActivated);
    LaunchWarnings warnings(window, "launchAnyway");
    bool rebuilt = false;
    QTimer::singleShot(0, &window, [&] {
        auto *warning = window.findChild<QMessageBox *>("otherWindowsSessionWarning"); QVERIFY(warning);
        for (auto &session : box.sessions) if (session.name == name) {
            session.runId = run; session.conversationId = conversation;
        }
        // The nested modal loop can replace the row storage even when its
        // identity stays the same. An actual new identity must still refuse.
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); rebuilt = true;
    });
    open->trigger();
    QVERIFY(rebuilt); QCOMPARE(warnings.shown, 1);
    QCOMPARE(opened.size(), activate ? 1 : 0);
    if (activate) { QCOMPARE(opened[0][0].toString(), QString()); QCOMPARE(opened[0][1].toString(), name); }
    else QVERIFY(window.findChild<QLabel *>("notice")->text().contains("session changed", Qt::CaseInsensitive));
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft"));
}

void TestSessionsWindow::clearInspectionUpdatesActivityAndCountersImmediately()
{
    QTemporaryDir temp;const auto program=temp.filePath("hgs");
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys
p=pathlib.Path(__file__).parent/'details.json'
print(p.read_text() if 'inspect' in sys.argv and p.exists() else '{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"phase","idle"},{"activity","idle"},{"runtime_state","live"},{"process_state","running"},
        {"conversation_id","old"},{"cursor",40},{"events",QJsonArray{QJsonObject{{"seq",40},{"at",1791018000.0},{"type","Stop"},{"agent_id",""},{"detail","Earlier answer"}}}},
        {"session_usage",QJsonObject{{"status","ok"},{"context",QJsonObject{{"used",109000},{"limit",258400}}}}},
        {"cache_hint",QJsonObject{{"status","cold"},{"tokens",109000}}}};
    // Polls read details.json while it changes: replace it atomically, or a torn read
    // fails an inspection and the session refuses to send.
    const auto writeDetails=[&]{QVERIFY(FakeCli::replace(temp.filePath("details.json"),QJsonDocument(details).toJson()));};
    // The first real inspection must already see the original conversation.
    // An empty startup reply would reset the earlier timeline during draft load.
    writeDetails();
    SessionsWindow window(FakeCli::program(program));window.resize(1080,760);window.setFleet(fleet());window.show();window.showSession("mac","claude/infra/review");
    auto *client=window.findChild<HgsClient *>();
    auto *composer=window.findChild<MessageComposer *>("messageComposer");
    const auto apply=[&]{writeDetails();client->inspectionReady("mac","claude/infra/review",details);};
    settleSelectedInspection(composer,client,"mac","claude/infra/review",details);
    apply();auto *context=window.findChild<QPushButton *>("activityContext");QVERIFY(context->text().contains("42%"));
    auto *cache=window.findChild<ToolbarChip *>("cacheChip");QVERIFY(cache->isActive());
    setComposerText(composer, "Keep this draft");
    auto *activity=window.findChild<ActivityView *>("mainActivity");QVERIFY(activity);
    QTRY_VERIFY(activity->plainText().contains("Earlier answer"));
    settleSelectedInspection(composer,client,"mac","claude/infra/review",details);
    details["conversation_id"]="new";details["session_usage"]=QJsonObject{{"status","unavailable"}};details.remove("cache_hint");
    details["session_clear"]=QJsonObject{{"type","SessionCleared"},{"at",1791018003.0},{"activity_key","clear-new"}};
    details["cleared_conversations"]=QJsonArray{"old"};details["cursor"]=41;
    details["events"]=QJsonArray{QJsonObject{{"seq",41},{"at",1791018004.0},{"type","UserPromptSubmit"},{"agent_id",""},{"detail","Fresh request"}}};
    apply();
    // The first new-conversation reply paints now, before the follow-up read
    // with its reset journal cursor returns.
    QTRY_VERIFY(activity->plainText().contains("Session cleared"));
    // Earlier activity stays in place above the boundary.
    const auto timeline=activity->plainText();
    QVERIFY(timeline.indexOf("Earlier answer")>=0);QVERIFY(timeline.indexOf("Earlier answer")<timeline.indexOf("Session cleared"));
    QVERIFY(timeline.indexOf("Session cleared")<timeline.indexOf("Fresh request"));
    QVERIFY(context->text().isEmpty());QVERIFY(!cache->isActive());QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    details["session_usage"]=QJsonObject{{"status","ok"},{"context",QJsonObject{{"used",32},{"limit",258400}}}};
    apply();QVERIFY(context->text().contains("32/"));QVERIFY(!context->text().contains("109"));
}

void TestSessionsWindow::coldCacheCompactWaitsForSuccess_data()
{
    QTest::addColumn<QString>("outcome");
    for (const auto &value:{"completed","failed","cancelled","unchanged","draft","identity","cancel",
        "navigation","hidden","navigation-identity","navigation-cancel"}) QTest::newRow(value)<<QString(value);
}

void TestSessionsWindow::coldCacheCompactWaitsForSuccess()
{
    QFETCH(QString,outcome);
    QTemporaryDir temp;const auto program=temp.filePath("hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a[0].startswith('@'):a=a[1:]
if a[0] in ['compact-context','send']:
 p=json.load(sys.stdin)
 if a[0]=='send':
  with (root/'send-targets.txt').open('a') as log:log.write(a[1]+'\n')
 staged=root/(a[0]+'.tmp');staged.write_text(json.dumps(p));staged.replace(root/(a[0]+'.json'))
 print(json.dumps(dict(request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],status='submitted')))
elif a[0]=='inspect':print((root/'details.json').read_text() if a[1]=='claude/infra/review' and (root/'details.json').exists() else '{}')
else:print('{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},{"activity","idle"},{"phase","idle"},
        {"runtime_state","live"},{"process_state","running"},{"compact_context_supported",true},{"cache_hint",QJsonObject{{"status","cold"},{"tokens",756000}}}};
    {QFile initial(temp.filePath("details.json"));QVERIFY(initial.open(QIODevice::WriteOnly));initial.write(QJsonDocument(details).toJson());}
    SessionsWindow window(FakeCli::program(program));window.setFleet(fleet());window.show();window.showSession("mac","claude/infra/review");QTest::qWait(100);
    // A completion that arrives while the held send is still being prepared is
    // picked up by the next compaction poll, which reads details.json: poll often.
    constexpr int compactPollMs = 200; window.setCompactPollIntervalForTests(compactPollMs);
    auto *client=window.findChild<HgsClient *>();
    // zerus-pvfa15: a failure prints the synthetic phases seen so far, never message
    // or draft text, before the window and its fixture folder are destroyed.
    const qint64 t0 = QDateTime::currentMSecsSinceEpoch();
    const QString reviewKey = window.findChild<MessageComposer *>("messageComposer")->sessionKey();
    QStringList phases; qsizetype droppedPhases = 0;
    const auto bounded = [](QString value) { return value.remove('\r').replace('\n', '|').left(160); };
    const auto flag = [](bool value) { return value ? QStringLiteral("1") : QStringLiteral("0"); };
    const auto notePhase = [&](const QString &line) {
        if (phases.size() < 300) phases << QStringLiteral("+%1ms %2").arg(QString::number(QDateTime::currentMSecsSinceEpoch() - t0), line);
        else ++droppedPhases;
    };
    const auto fields = [&](const QJsonObject &data) {
        QStringList values;
        for (const char *field : {"tracked", "run_id", "conversation_id", "runtime_state", "process_state", "activity", "phase", "error", "expected_id"})
            if (data.contains(QLatin1String(field)))
                values << QStringLiteral("%1=%2").arg(QLatin1String(field), bounded(data.value(QLatin1String(field)).toVariant().toString()));
        const auto compact = data.value(QStringLiteral("compact_context_request")).toObject();
        if (!compact.isEmpty()) values << QStringLiteral("compact=%1/%2").arg(bounded(compact.value(QStringLiteral("status")).toString()),
            compact.value(QStringLiteral("request_id")).toString().left(8));
        return values.join(' ');
    };
    QObject phaseContext; // Disconnects these recorders before the helpers above are destroyed.
    connect(client, &HgsClient::sessionActionFinished, &phaseContext, [&](quint64 action, bool ok, const QJsonObject &receipt, const QString &error) {
        notePhase(QStringLiteral("sessionActionFinished request=%1 ok=%2 receipt=%3 error=%4").arg(QString::number(action), flag(ok),
            receipt.value(QStringLiteral("request_id")).toString().left(8), bounded(error)));
    });
    connect(client, &HgsClient::messageSent, &phaseContext, [&](quint64 message, const QString &host, const QString &name, const QJsonObject &receipt) {
        notePhase(QStringLiteral("messageSent request=%1 host=%2 name=%3 run=%4 conversation=%5").arg(QString::number(message), host, name,
            receipt.value(QStringLiteral("run_id")).toString(), receipt.value(QStringLiteral("conversation_id")).toString()));
    });
    connect(client, &HgsClient::messageFailed, &phaseContext, [&](quint64 message, const QString &host, const QString &name, const QString &detail, bool uncertain) {
        notePhase(QStringLiteral("messageFailed request=%1 host=%2 name=%3 uncertain=%4 detail=%5").arg(QString::number(message), host, name,
            flag(uncertain), bounded(detail)));
    });
    connect(client, &HgsClient::inspectionLoadingChanged, &phaseContext,
        [&](const QString &host, const QString &name, const QString &archive, const QString &agent, bool loading) {
            if (loading) notePhase(QStringLiteral("inspectionStarted host=%1 name=%2 archive=%3 agent=%4").arg(host, name, archive, agent));
        });
    connect(client, &HgsClient::inspectionReady, &phaseContext, [&](const QString &host, const QString &name, const QJsonObject &data, const QString &archive) {
        notePhase(QStringLiteral("inspectionReady host=%1 name=%2 archive=%3 %4").arg(host, name, archive, fields(data)));
    });
    connect(client, &HgsClient::inspectionFailed, &phaseContext, [&](const QString &host, const QString &name, const QString &detail, const QString &archive) {
        notePhase(QStringLiteral("inspectionFailed host=%1 name=%2 archive=%3 detail=%4").arg(host, name, archive, bounded(detail)));
    });
    const auto snapshot = qScopeGuard([&] {
        if (!QTest::currentTestFailed()) return;
        QStringList lines{QStringLiteral("outcome=%1 t0=%2").arg(outcome, QString::number(t0))};
        const auto *shown = window.findChild<MessageComposer *>("messageComposer");
        if (!shown) lines << QStringLiteral("composer absent");
        else {
            const auto *status = shown->findChild<QLabel *>("messageStatus"); const auto *send = shown->findChild<QPushButton *>("sendMessage");
            lines << QStringLiteral("composer selected=%1 waiting=%2 preparing=%3 sending=%4 draftLoading=%5 hasDraft=%6 draftMatches=%7 readOnly=%8 send=%9 status=%10").arg(
                flag(shown->sessionKey() == reviewKey), flag(shown->isWaitingToSend(reviewKey)), flag(shown->isPreparingSending(reviewKey)),
                flag(shown->isSending(reviewKey)), flag(shown->isDraftLoading(reviewKey)), flag(shown->hasDraft(reviewKey)),
                flag(shown->draftMatches(reviewKey, QStringLiteral("Keep until compacted"), {})), flag(shown->editor()->isReadOnly()),
                bounded(send ? send->text() : QString()), bounded(status ? status->text() : QString()));
        }
        const auto *notice = window.findChild<QLabel *>("notice"); const auto *cancel = window.findChild<QPushButton *>("cancelCompactSend");
        lines << QStringLiteral("window visible=%1 cancelCompactSend=%2 noticeError=%3 notice=%4").arg(flag(window.isVisible()),
            flag(cancel && cancel->isVisible()), flag(notice && notice->property("error").toBool()), bounded(notice ? notice->text() : QString()));
        QStringList files;
        for (const char *name : {"compact-context.json", "send.tmp", "send.json", "send-targets.txt"})
            files << QStringLiteral("%1=%2").arg(QLatin1String(name), flag(QFileInfo::exists(temp.filePath(QLatin1String(name)))));
        lines << QStringLiteral("files %1").arg(files.join(' '));
        QFile compactRequest(temp.filePath("compact-context.json"));
        if (compactRequest.open(QIODevice::ReadOnly)) lines << QStringLiteral("compact-context request=%1").arg(
            QJsonDocument::fromJson(compactRequest.read(65536)).object().value(QStringLiteral("request_id")).toString().left(8));
        QFile sentTo(temp.filePath("send-targets.txt"));
        if (sentTo.open(QIODevice::ReadOnly)) lines << QStringLiteral("send-targets %1").arg(bounded(QString::fromUtf8(sentTo.read(1024))));
        QFile served(temp.filePath("details.json"));
        if (served.open(QIODevice::ReadOnly)) lines << QStringLiteral("details.json %1").arg(fields(QJsonDocument::fromJson(served.read(65536)).object()));
        lines << QStringLiteral("test details %1").arg(fields(details));
        lines << phases;
        if (droppedPhases) lines << QStringLiteral("%1 later phases dropped").arg(QString::number(droppedPhases));
        for (const auto &line : std::as_const(lines)) qWarning().noquote() << "pvfa15" << line;
    });
    // Polls read details.json while it changes: replace it atomically, or a torn read
    // fails an inspection and the held send is refused.
    const auto apply=[&]{QVERIFY(FakeCli::replace(temp.filePath("details.json"),QJsonDocument(details).toJson()));client->inspectionReady("mac","claude/infra/review",details);};apply();
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer, "Keep until compacted");
    QSignalSpy finished(client,&HgsClient::sessionActionFinished);
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("coldCacheConfirm");QVERIFY(dialog);
        for(auto *button:dialog->buttons())if(button->text()=="Compact and continue"){QVERIFY(button->isEnabled());button->click();return;}QFAIL("No compact option");});
    QVERIFY(composer->findChild<QPushButton *>("sendMessage")->isEnabled());
    composer->findChild<QPushButton *>("sendMessage")->click();QTRY_COMPARE(finished.size(),1);QVERIFY(finished[0][1].toBool());
    notePhase(QStringLiteral("test compact receipt observed"));
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep until compacted"));QVERIFY(!QFileInfo::exists(temp.filePath("send.json")));
    QVERIFY(composer->editor()->isReadOnly()); QVERIFY(!composer->canAttachFiles());
    QCOMPARE(composer->findChild<QPushButton *>("sendMessage")->text(), QString("Waiting to send…"));
    QVERIFY(composer->findChild<QLabel *>("messageStatus")->text().contains("This exact message"));
    QTest::keyClicks(composer->editor(), "should not change the accepted message");
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep until compacted"));
    QFile request(temp.filePath("compact-context.json"));QVERIFY(request.open(QIODevice::ReadOnly));const auto payload=QJsonDocument::fromJson(request.readAll()).object();
    const auto id=payload.value("request_id").toString();
    notePhase(QStringLiteral("test compacting apply follows"));
    details["compact_context_request"]=QJsonObject{{"request_id",id},{"status","compacting"}};details["activity"]="busy";details["phase"]="compacting";apply();
    QVERIFY(window.findChild<QPushButton *>("cancelCompactSend")->isVisible());QVERIFY(window.findChild<QPushButton *>("cancelCompactSend")->height()<=24);QVERIFY(!QFileInfo::exists(temp.filePath("send.json")));
    if(!qEnvironmentVariable("HGS_COMPACT_PREVIEW").isEmpty())QVERIFY(window.grab().save(qEnvironmentVariable("HGS_COMPACT_PREVIEW")));
    if(outcome=="draft") { window.findChild<QPushButton *>("cancelCompactSend")->click(); setComposerText(composer, "Edited draft"); }
    if(outcome=="identity")details["conversation_id"]="another";
    const bool navigated = outcome.startsWith("navigation") || outcome == "hidden";
    const QString compactKey = composer->sessionKey();
    if (navigated) {
        // Only the background compaction poll can now continue the send.
        window.showSession({},"codex/hgs/dashboard");
        QVERIFY(!window.findChild<QPushButton *>("cancelCompactSend")->isVisible());
        setComposerText(composer, "Unrelated session draft");
        if (outcome == "navigation-cancel") {
            window.showSession("mac","claude/infra/review"); apply();
            QVERIFY(composer->editor()->isReadOnly());
            QCOMPARE(composer->editor()->toPlainText(),QString("Keep until compacted"));
            window.findChild<QPushButton *>("cancelCompactSend")->click();
            window.showSession({},"codex/hgs/dashboard");
        }
        if (outcome == "navigation-identity") details["conversation_id"]="another";
        if (outcome == "hidden") window.hide();
    }
    if(outcome=="cancel") {
        window.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&window));
        window.findChild<QPushButton *>("cancelCompactSend")->click();
        QVERIFY(!composer->editor()->isReadOnly()); QVERIFY(composer->canAttachFiles());
        QCOMPARE(QApplication::focusWidget(), composer->editor());
    }
    notePhase(QStringLiteral("test outcome apply follows"));
    details["compact_context_request"]=QJsonObject{{"request_id",id},{"status",navigated||outcome=="draft"||outcome=="identity"||outcome=="cancel"?"completed":outcome}};
    details["activity"]="idle";details["phase"]="idle";
    if (navigated) QVERIFY(FakeCli::replace(temp.filePath("details.json"), QJsonDocument(details).toJson()));
    else apply();
    notePhase(QStringLiteral("test outcome applied failed=%1").arg(flag(QTest::currentTestFailed())));
    if(outcome=="completed" || outcome=="navigation" || outcome=="hidden") {
        QTRY_VERIFY(QFileInfo::exists(temp.filePath("send.json")));QFile sent(temp.filePath("send.json"));QVERIFY(sent.open(QIODevice::ReadOnly));
        const auto message=QJsonDocument::fromJson(sent.readAll()).object();QCOMPARE(message["text"].toString(),QString("Keep until compacted"));QCOMPARE(message["expected_compaction_id"].toString(),id);
        if (navigated) {
            QCOMPARE(composer->editor()->toPlainText(),QString("Unrelated session draft"));
            QVERIFY(!composer->editor()->isReadOnly());
            window.show(); window.showSession("mac","claude/infra/review"); apply();
            QTRY_VERIFY(composer->editor()->toPlainText().isEmpty());
            apply(); QTest::qWait(100);
        }
        QFile targets(temp.filePath("send-targets.txt")); QVERIFY(targets.open(QIODevice::ReadOnly));
        QCOMPARE(targets.readAll(),QByteArray("claude/infra/review\n"));
    } else if (navigated) {
        // A poll resolves the changed identity; a cancelled send stops polling.
        if (outcome == "navigation-identity") QTRY_VERIFY(!composer->isWaitingToSend(compactKey));
        QTest::qWait(3 * compactPollMs); QVERIFY(!QFileInfo::exists(temp.filePath("send.json")));
        QCOMPARE(composer->editor()->toPlainText(),QString("Unrelated session draft"));
        window.showSession("mac","claude/infra/review"); apply();
        QCOMPARE(composer->editor()->toPlainText(),QString("Keep until compacted"));
        QVERIFY(!composer->editor()->isReadOnly());
    } else {QTest::qWait(100);QVERIFY(!QFileInfo::exists(temp.filePath("send.json")));QCOMPARE(composer->editor()->toPlainText(),outcome=="draft"?QString("Edited draft"):QString("Keep until compacted"));
        // Input may stay locked for a moment after the outcome: a failed inspection, for
        // example, blocks it until the next poll (zerus-eyvo, seen once on a Windows runner).
        // Wait for the editor and name the lock if it stays; a send holding it leaves send.json.
        const auto lock=[&]{return QStringList{QString("waiting=%1").arg(composer->isWaitingToSend(composer->sessionKey())?"yes":"no"),
            QString("loading=%1").arg(composer->isDraftLoading(composer->sessionKey())?"yes":"no"),"send="+composer->findChild<QPushButton *>("sendMessage")->text(),
            "hint="+composer->editor()->toolTip(),"status="+composer->findChild<QLabel *>("messageStatus")->text()}.join("; ");};
        QTRY_VERIFY2(!composer->editor()->isReadOnly(),qPrintable(lock()));QVERIFY(!QFileInfo::exists(temp.filePath("send.json")));
        QCOMPARE(composer->editor()->toPlainText(),outcome=="draft"?QString("Edited draft"):QString("Keep until compacted"));QVERIFY(!composer->isWaitingToSend(composer->sessionKey()));}
}

void TestSessionsWindow::composerNavigationDuringPolling()
{
    SessionsWindow window(program()); window.resize(1400, 900); window.setFleet(fleet()); window.show();
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspections(client, &HgsClient::inspectionReady);
    window.showSession({}, "codex/hgs/dashboard"); QTRY_VERIFY(!inspections.isEmpty());
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *editor = composer->editor(); window.activateWindow(); editor->setFocus(); QTRY_VERIFY(editor->hasFocus());
    const QString draft = QString::fromUtf8("1. вышел новый релиз zrok / ziti -- посмотри не надо ли обновить на сервере 2. посмотри йф");
    editor->insertPlainText(draft);
    const auto end = editor->cursorRect();
    QTest::keyClick(editor, Qt::Key_Home);
    QCOMPARE(editor->textCursor().position(), 0);
    for (int i = 0; i < 5; ++i) {
        window.setFleet(fleet());
        QTest::qWait(100);
        QCOMPARE(editor->textCursor().position(), 0);
        QVERIFY(editor->hasFocus());
    }
    QTest::keyClicks(editor, "X"); QCOMPARE(editor->toPlainText(), "X" + draft);
    QTest::mouseClick(editor->viewport(), Qt::LeftButton, Qt::NoModifier, end.center());
    const int position = editor->textCursor().position();
    QVERIFY(position > 1);
    window.setFleet(fleet()); QTest::qWait(200);
    QCOMPARE(editor->textCursor().position(), position);
    auto expected = editor->toPlainText(); expected.insert(position, "Y");
    QTest::keyClicks(editor, "Y"); QCOMPARE(editor->toPlainText(), expected);
}

void TestSessionsWindow::messageDeliveryKeepsSessionIdentityAndDrafts_data()
{
    QTest::addColumn<QString>("promptEvent");
    QTest::addColumn<bool>("busy");
    QTest::addColumn<bool>("providerFailure");
    QTest::newRow("submitted") << QString("UserPromptSubmit") << false << false;
    QTest::newRow("working") << QString("UserPromptSubmit") << true << false;
    QTest::newRow("queued") << QString("UserPromptQueued") << true << false;
    QTest::newRow("kimi-turn-started") << QString("TurnStarted") << false << false;
    QTest::newRow("provider-error-retry") << QString("UserPromptSubmit") << false << true;
}

void TestSessionsWindow::messageDeliveryKeepsSessionIdentityAndDrafts()
{
    QFETCH(QString, promptEvent);
    QFETCH(bool, busy);
    QFETCH(bool, providerFailure);
    const QString capture = m_dir->filePath("message-capture.json");
    const QString program = m_dir->filePath("message-hgs");
    QFile::remove(program + ".details"); // Each data row starts with fresh native state.
    QFile file(program); QVERIFY(file.open(QIODevice::WriteOnly));
    const QByteArray captureLiteral = QJsonDocument(QJsonArray{capture}).toJson(QJsonDocument::Compact);
    QByteArray body = "#!/usr/bin/env python3\nimport sys,json,time,pathlib\nargs=sys.argv[1:]\nif args[0].startswith('@'): args=args[1:]\n";
    body += "capture=" + captureLiteral + "[0]\n";
    body += R"PY(if args[0]=='inspect':
 snapshot=pathlib.Path(__file__+'.details')
 print(snapshot.read_text() if snapshot.exists() else json.dumps({'tracked':True,'run_id':'run-ready','conversation_id':'conversation-ready','runtime_state':'live','process_state':'running','activity':'idle','phase':'idle','last_event_at':2000000000,'events':[],'cursor':0}))
elif args[0]=='send':
 payload=json.load(sys.stdin)
 with open(capture,'w') as f: json.dump({'args':args,'payload':payload},f)
 time.sleep(0.15)
 print(json.dumps({'status':'submitted','request_id':payload['request_id'],'name':args[1],'run_id':payload['expected_run_id'],'conversation_id':payload['expected_conversation_id'],'submitted_text':payload['text'],'submitted_at':time.time()}))
)PY";
    if (busy) body.replace("'activity':'idle','phase':'idle'", "'activity':'busy','phase':'tool'");
    if (providerFailure) body.replace("'activity':'idle','phase':'idle'",
        "'activity':'attention','phase':'error','provider_error':{'error_kind':'capacity'},'error_message_can_send':True");
    file.write(body); file.close(); QVERIFY(FakeCli::makeRunnable(file.fileName()));
    SessionsWindow window(FakeCli::program(program)); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    auto *send = window.findChild<MessageComposer *>("messageComposer")->findChild<QPushButton *>("sendMessage"); QVERIFY(send);
    auto *client = window.findChild<HgsClient *>(); QVERIFY(client);
    const auto applyDetails = [&](const QJsonObject &details) {
        QVERIFY(FakeCli::replace(program + ".details", QJsonDocument(details).toJson()));
        // Drain old replies, then start an inspection reading the new atomic
        // snapshot. All later automatic polls now agree with the test state.
        QSignalSpy replies(client, &HgsClient::inspectionReady); qsizetype issued = -1;
        QTRY_VERIFY(issued >= 0 || (client->requestInspection({}, "codex/hgs/dashboard") && (issued = replies.size(), true)));
        QTRY_VERIFY(std::any_of(replies.cbegin() + issued, replies.cend(), [](const QList<QVariant> &reply) {
            return reply[0].toString().isEmpty() && reply[1].toString() == "codex/hgs/dashboard" && reply[3].toString().isEmpty();
        }));
        client->inspectionReady({}, "codex/hgs/dashboard", details);
    };
    QSignalSpy delivered(client, &HgsClient::messageSent);
    const QString message = QString::fromUtf8("Review this screenshot.\nКавычки: '$HOME' `whoami`");
    setComposerText(composer, message);
    QVERIFY(composer->addAttachment("notes.txt", "text/plain", QByteArray("review notes")));
    QTRY_VERIFY(send->isEnabled()); window.activateWindow(); composer->editor()->setFocus();
    QTRY_COMPARE(QApplication::focusWidget(),composer->editor());
    QTest::keyClick(composer->editor(),Qt::Key_Return); QVERIFY(!send->isEnabled());
    QCOMPARE(QApplication::focusWidget(),composer->editor());
    QCOMPARE(delivered.size(), 0); // The transport has not acknowledged this request yet.
    QTRY_VERIFY(composer->editor()->toPlainText().isEmpty());
    QCOMPARE(composer->findChildren<QPushButton *>("removeAttachment").size(), 0);
    QTRY_VERIFY(window.findChild<QTextBrowser *>("activity")->toPlainText().contains("Review this screenshot."));
    if(promptEvent=="UserPromptQueued") {
        QTRY_COMPARE(delivered.size(),1);
        QCOMPARE(QApplication::focusWidget(),composer->editor());QVERIFY(!composer->editor()->isReadOnly());
    }
    window.showSession({}, "kimi/docs/research"); setComposerText(composer, "Separate Kimi draft");
    QTRY_COMPARE(delivered.size(), 1);
    QCOMPARE(composer->editor()->toPlainText(), QString("Separate Kimi draft"));
    QFile captured(capture); QVERIFY(captured.open(QIODevice::ReadOnly));
    const auto result = QJsonDocument::fromJson(captured.readAll()).object();
    QCOMPARE(result.value("args").toArray().at(1).toString(), QString("codex/hgs/dashboard"));
    const auto payload = result.value("payload").toObject();
    QCOMPARE(payload.value("text").toString(), "[File #1] " + message);
    QCOMPARE(payload.value("attachments").toArray()[0].toObject().value("reference").toString(), QString("[File #1]"));
    QCOMPARE(payload.value("expected_run_id").toString(), QString("run-ready"));
    QCOMPARE(payload.value("expected_conversation_id").toString(), QString("conversation-ready"));
    QCOMPARE(QByteArray::fromBase64(payload.value("attachments").toArray().at(0).toObject().value("data_base64").toString().toLatin1()), QByteArray("review notes"));
    window.showSession({}, "codex/hgs/dashboard");
    QCOMPARE(composer->editor()->toPlainText(), QString());
    QJsonObject details{{"tracked", true}, {"run_id", "run-ready"}, {"conversation_id", "conversation-ready"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"activity", "idle"}, {"phase", "idle"}, {"last_event_at", 2000000000},
        {"cursor", 1}, {"events", QJsonArray{QJsonObject{{"seq", 1}, {"at", QDateTime::currentMSecsSinceEpoch() / 1000.0},
        {"type", promptEvent}, {"detail", payload.value("text")}}}}};
    applyDetails(details);
    auto *activity = window.findChild<QTextBrowser *>("activity"); QVERIFY(activity);
    QTRY_VERIFY(!window.findChild<ActivityView *>("mainActivity")->renderPending());
    QTRY_COMPARE(activity->toPlainText().count("Review this screenshot."), 1);
    // The native attachment receipt replaces the local acknowledgement in one
    // poll. Only the file's storage metadata changes; the visible card should
    // not be replaced at all, and must never transiently appear twice.
    auto attachment = QJsonObject{{"name","notes.txt"},{"mime","text/plain"},{"bytes",12},
        {"reference","[File #1]"},{"request_id",payload["request_id"]},{"index",0}};
    details["attachment_messages"] = QJsonArray{QJsonObject{{"type",promptEvent},{"source","hgs_delivery"},
        {"message_id",payload["request_id"]},{"at",details["events"].toArray().first().toObject()["at"]},
        {"detail",payload["text"]},{"submitted_text",payload["text"]},{"attachments",QJsonArray{attachment}}}};
    details["events"] = QJsonArray{};
    QSignalSpy documentChanges(activity, &QTextBrowser::textChanged);
    applyDetails(details);
    QTRY_VERIFY(!window.findChild<ActivityView *>("mainActivity")->renderPending());
    QCOMPARE(documentChanges.size(), 0);
    QTRY_COMPARE(activity->toPlainText().count("Review this screenshot."), 1);
    window.showSession({}, "codex/website/navigation");
    setComposerText(composer, "Draft for a saved session");
    // Send stays available; it asks how to start the saved session first.
    QVERIFY(send->isEnabled());
    QVERIFY(composer->findChild<QLabel *>("messageStatus")->text().contains("resume it or start fresh"));
    window.showSession({}, "kimi/docs/research");
    QCOMPARE(composer->editor()->toPlainText(), QString("Separate Kimi draft"));
}

void TestSessionsWindow::modelSettingsKeepSessionIdentity_data()
{
    QTest::addColumn<QString>("state"); QTest::addColumn<QString>("applyError");
    QTest::addColumn<bool>("preflightRefusal"); QTest::addColumn<bool>("keepSelected");
    QTest::newRow("paused") << QString("paused") << QString() << false << false;
    QTest::newRow("stopped") << QString("stopped") << QString() << false << false;
    QTest::newRow("busy-background-applied") << QString("running") << QString() << false << false;
    QTest::newRow("busy-background-failed-once") << QString("running") << QString("Native model picker unavailable") << false << false;
    QTest::newRow("busy-background-consumed") << QString("running")
        << QString("hgs: pending settings changed; refresh before applying") << true << false;
    QTest::newRow("busy-selected-consumed") << QString("running")
        << QString("pending settings changed; refresh before applying") << true << true;
    QTest::newRow("busy-background-target-changed") << QString("running")
        << QString("pending settings target changed; refresh before applying") << true << false;
    QTest::newRow("busy-selected-target-changed") << QString("running")
        << QString("hgs: pending settings target changed; refresh before applying") << true << true;
    QTest::newRow("busy-background-native-error-with-similar-detail") << QString("running")
        << QString("settings change not confirmed: pending settings changed; refresh before applying") << false << false;
}

void TestSessionsWindow::modelSettingsKeepSessionIdentity()
{
    QFETCH(QString, state); QFETCH(QString, applyError);
    QFETCH(bool, preflightRefusal); QFETCH(bool, keepSelected);
    QTemporaryDir directory; QVERIFY(directory.isValid());
    const QString program = directory.filePath("hgs"), snapshot = directory.filePath("snapshot.json"), capture = directory.filePath("calls.jsonl");
    QJsonObject details{{"tracked", true}, {"run_id", "run-original"}, {"conversation_id", "conversation-one"},
        {"activity", "busy"}, {"phase", "tool"}, {"model", "gpt-6.1-sol"}, {"effort", "high"},
        {"settings_change_supported", true}, {"settings_apply_when", state == "running" ? "ready" : "resume"},
        {"model_options", QJsonArray{QJsonObject{{"id", "gpt-6.1-sol"}, {"effort_options", QJsonArray{"high", "xhigh"}}}}},
        {"cursor", 0}, {"events", QJsonArray{}}};
    const auto saveSnapshot = [&] { return FakeCli::replace(snapshot, QJsonDocument(details).toJson()); };
    QVERIFY(saveSnapshot());
    QFile executable(program); QVERIFY(executable.open(QIODevice::WriteOnly));
    QByteArray body = "#!/usr/bin/env python3\nimport json,sys,pathlib,time\nroot=pathlib.Path(__file__).parent\nargs=sys.argv[1:]\n";
    body += "apply_error=" + QJsonDocument(QJsonArray{applyError}).toJson(QJsonDocument::Compact) + "[0]\n";
    body += preflightRefusal ? "preflight=True\n" : "preflight=False\n";
    // Polls read snapshot.json while a settings call rewrites it: replace it, so no
    // inspection reads a truncated file and fails. Windows refuses the rename while
    // a poll has the file open; retry then.
    body += R"PY(def publish(data):
 staged=root/'snapshot.tmp';staged.write_text(json.dumps(data))
 for _ in range(250):
  try:return staged.replace(root/'snapshot.json')
  except PermissionError:time.sleep(.02)
 sys.exit('snapshot.json stayed open')
if args[0]=='inspect':
 print((root/'snapshot.json').read_text())
elif args[0]=='settings':
 p=json.load(sys.stdin)
 calls=root/'calls.jsonl'
 previous=calls.read_text().count('\n') if calls.exists() else 0
 with calls.open('a') as out: out.write(json.dumps({'args':args,'payload':p})+'\n')
 time.sleep(.12)
 data=json.loads((root/'snapshot.json').read_text())
 if previous and apply_error:
  if preflight:
   # A send or another client consumed the pending request before this guard.
   data['model']=p['model'];data['effort']=p['effort']
   for field in ['pending_model','pending_effort','pending_settings_id']:data.pop(field,None)
   publish(data)
  print(apply_error,file=sys.stderr);sys.exit(1)
 result='applied' if previous else 'scheduled'
 if result=='scheduled':
  data['pending_model']=p['model'];data['pending_effort']=p['effort'];data['pending_settings_id']=p['request_id']
 else:
  data['model']=p['model'];data['effort']=p['effort'];data.pop('pending_model',None);data.pop('pending_effort',None)
 publish(data)
 print(json.dumps({'status':result,'request_id':p['request_id'],'name':args[1],'run_id':p['expected_run_id'],
 'conversation_id':p['expected_conversation_id'],'model':p['model'],'effort':p['effort']}))
)PY";
    executable.write(body); executable.close(); QVERIFY(FakeCli::makeRunnable(executable.fileName()));
    auto current = fleet(); auto box = current.local(); box.sessions[0].state = state; box.sessions[0].runId = "run-original";
    current.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.setFleet(current); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspections(client, &HgsClient::inspectionReady), changes(client, &HgsClient::settingsFinished);
    QTRY_VERIFY(!inspections.isEmpty());
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Saved draft for original session");
    QVERIFY(composer->addAttachment("notes.txt", "text/plain", "keep"));
    auto *settings = window.findChild<MessageComposer *>("messageComposer")->findChild<QPushButton *>("sessionModelSettings"); settings->click();
    auto *efforts = window.findChild<MessageComposer *>("messageComposer")->findChild<QComboBox *>("sessionEffortChoice"); efforts->setCurrentIndex(efforts->findData("xhigh"));
    auto *apply = window.findChild<MessageComposer *>("messageComposer")->findChild<QPushButton *>("applySessionSettings"); QVERIFY(apply->isEnabled()); apply->click();
    if (!keepSelected) { window.showSession({}, "kimi/docs/research"); setComposerText(composer, "Other session draft"); }
    const QString currentDraft = keepSelected ? "[File #1] Saved draft for original session" : "Other session draft";
    QTRY_COMPARE(changes.size(), 1); QVERIFY(changes[0][1].toBool()); QCOMPARE(composer->editor()->toPlainText(), currentDraft);
    if (state == "running") {
        box.sessions[0].phase = "idle"; box.sessions[0].activity = "idle"; box.sessions[0].processState = "running";
        current.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(current);
        QTRY_COMPARE(changes.size(), 2); QCOMPARE(changes[1][1].toBool(), applyError.isEmpty());
        auto *notice = window.findChild<QLabel *>("notice"); QVERIFY(notice);
        if (!applyError.isEmpty() && !preflightRefusal) {
            QVERIFY(notice->isVisible()); QVERIFY(notice->property("error").toBool());
            QVERIFY(notice->text().contains(applyError));
        } else QVERIFY(notice->isHidden());
        window.setFleet(current); window.setFleet(current); QTest::qWait(80); QCOMPARE(changes.size(), 2);
        QCOMPARE(composer->editor()->toPlainText(), currentDraft);
    } else {
        window.setFleet(current); QTest::qWait(80); QCOMPARE(changes.size(), 1);
    }
    QFile recorded(capture); QVERIFY(recorded.open(QIODevice::ReadOnly));
    QString originalRequest;
    for (const auto &line : recorded.readAll().split('\n')) {
        if (line.isEmpty()) continue;
        const auto call = QJsonDocument::fromJson(line).object(), payload = call.value("payload").toObject();
        QCOMPARE(call.value("args").toArray(), QJsonArray({"settings", "codex/hgs/dashboard", "--json"}));
        QCOMPARE(payload.value("expected_run_id").toString(), QString("run-original"));
        QCOMPARE(payload.value("expected_conversation_id").toString(), QString("conversation-one"));
        QCOMPARE(payload.value("effort").toString(), QString("xhigh"));
        if (originalRequest.isEmpty()) { QVERIFY(!payload.contains("expected_pending_id")); originalRequest = payload.value("request_id").toString(); }
        else QCOMPARE(payload.value("expected_pending_id").toString(), originalRequest);
    }
    window.showSession({}, "codex/hgs/dashboard");
    if (preflightRefusal) QTRY_VERIFY(!settings->toolTip().contains("Pending:"));
    QCOMPARE(composer->editor()->toPlainText(), QString("[File #1] Saved draft for original session"));
    QCOMPARE(composer->findChildren<QPushButton *>("removeAttachment").size(), 1);
    QTest::qWait(80); QCOMPARE(changes.size(), state == "running" ? 2 : 1);
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_composer2.moc"
