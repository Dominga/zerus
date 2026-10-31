// U62 (#712): Questions and their answers, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void questionReplyPreviewUsesCompletePrompt();
    void nativeColdCacheChoiceKeepsExactAnswer_data();
    void nativeColdCacheChoiceKeepsExactAnswer();
    void coldQuestionSubmissionRevalidatesConsent_data();
    void coldQuestionSubmissionRevalidatesConsent();
    void questionAnswersStayWithOriginalSession();
    void queuedQuestionAnswersDisappearAfterSubmission();
    void terminalChoiceOpensOnlyTheRequestingTerminal_data();
    void terminalChoiceOpensOnlyTheRequestingTerminal();
    void optionalQuestionKeepsComposerAvailable();
    void optionalQuestionNotificationsAreOptIn();
    void questionCompletionKeepsInputFocus_data();
    void questionCompletionKeepsInputFocus();
    void questionFooterHasRoomAndKeepsAnswers_data();
    void questionFooterHasRoomAndKeepsAnswers();
    void startupTrustAnswerBeforeConversation_data();
    void startupTrustAnswerBeforeConversation();
};

void TestSessionsWindow::questionReplyPreviewUsesCompletePrompt()
{
    auto state=fleet();auto box=state.local();auto &session=box.sessions[0];
    const QString answer="Review the desktop changes on both machines.";
    const QJsonObject reply{{"questionItemId","reply-one"},{"question",QString(300,'q')},{"answer",answer}};
    session.prompt="<send_user_message_question_reply>\n"+QString::fromUtf8(QJsonDocument(QJsonArray{reply}).toJson(QJsonDocument::Compact))
        +"\n</send_user_message_question_reply>";
    // Listing preparation parses the answers with the row, off the GUI thread.
    session.promptAnswers=offGuiThread([&session]{return QuestionReply::previewOf(session.prompt);});
    session.activitySummary="Working";session.activityDetail=session.prompt.left(240)+QChar(0x2026);
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);window.show();window.showSession({},session.name);
    auto *list=window.findChild<SessionList *>("sessionList");auto *row=list->currentItem();QVERIFY(row);
    QCOMPARE(row->data(SessionRoles::Detail).toString(),"Working: Your answer: "+answer);
    QVERIFY(row->data(Qt::AccessibleTextRole).toString().contains(answer));
    QVERIFY(!row->data(Qt::AccessibleTextRole).toString().contains("send_user_message_question_reply"));
    QCOMPARE(session.activityDetail,session.prompt.left(240)+QChar(0x2026));
}

void TestSessionsWindow::nativeColdCacheChoiceKeepsExactAnswer_data()
{
    QTest::addColumn<int>("choice"); QTest::addColumn<QString>("change");
    for (int choice = 0; choice < 4; ++choice)
        QTest::newRow(qPrintable(QString("choice-%1").arg(choice))) << choice << QString();
    for (const auto *change : {"hash", "run", "source"}) QTest::newRow(change) << 0 << QString(change);
}

void TestSessionsWindow::nativeColdCacheChoiceKeepsExactAnswer()
{
    QFETCH(int, choice); QFETCH(QString, change);
    QTemporaryDir directory;
    const auto program = directory.filePath("hgs"), state = directory.filePath("details.json"), capture = directory.filePath("answer.json");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path(__file__).parent
if sys.argv[1]=='inspect': print((root/'details.json').read_text())
elif sys.argv[1]=='answer':
 p=json.load(sys.stdin)
 with (root/'calls').open('a') as f:f.write('call\n')
 (root/'answer.json').write_text(json.dumps(p))
 time.sleep(.1)
 print(json.dumps(dict(status='answered',request_id=p['request_id'],name=sys.argv[2],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],question_id=p['question_id'],question_hash=p['expected_question_hash'])))
else: print('{}')
)PY");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(program));
    QJsonArray options;
    const QStringList labels{"Compact and continue", "Start a new session", "Continue as-is", "Don't ask me again"};
    for (int index = 0; index < labels.size(); ++index)
        options.append(QJsonObject{{"id", QString("cache_%1").arg(index)}, {"label", labels[index]}});
    QJsonObject question{{"question_id", "kimi-cache:exact"}, {"question_hash", "hash-cold"}, {"run_id", "run-cold"},
        {"conversation_id", "conversation-cold"}, {"can_answer", true}, {"source", "kimi_cache_hint"},
        {"answer_transport", "kimi_tui"}, {"tool_call_id", "kimi_cache_hint"}, {"agent_id", "main"},
        {"questions", QJsonArray{QJsonObject{{"id", "cache"}, {"header", "Context"}, {"question", "Cache expired"},
            {"body", "The next message re-sends the entire history at full price."},
            {"multi_select", false}, {"allow_other", false}, {"options", options}}}}};
    QJsonObject details{{"tracked", true}, {"run_id", "run-cold"}, {"conversation_id", "conversation-cold"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"phase", "input"}, {"activity", "busy"},
        {"pending_questions", QJsonArray{question}}, {"cache_hint", QJsonObject{{"status", "cold"}, {"tokens", 490252}}}};
    QVERIFY(FakeCli::replace(state, QJsonDocument(details).toJson()));
    SessionsWindow window(FakeCli::program(program)); window.setFleet(fleet()); keepDetailsPollsOut(window); window.show(); window.showSession({}, "kimi/docs/research");
    auto *card = window.findChild<QuestionCard *>("questionCard"); auto *client = window.findChild<HgsClient *>();
    QTRY_VERIFY(card->isVisible()); QTRY_VERIFY(!card->isDraftLoading());
    // A change made while the answer's draft is saved must stay the latest evidence.
    // An inspection that read details.json before it would restore the reviewed
    // question, and the held answer would be sent. Settle one before submitting.
    settleSelectedInspection(window.findChild<MessageComposer *>("messageComposer"), client, {}, "kimi/docs/research", details);
    const auto key = window.findChild<QListWidget *>("sessionList")->currentItem()->data(Qt::UserRole).toString();
    for (auto *button : card->findChildren<QAbstractButton *>("questionOption"))
        if (button->property("optionId") == QString("cache_%1").arg(choice)) button->click();
    auto *submit = card->findChild<QPushButton *>("submitQuestionAnswer"); QVERIFY(submit->isEnabled());
    ComposerDraftStore::flushWrites();
    QSemaphore entered, release;
    if (!change.isEmpty()) ComposerDraftStore::setWriteObserver([&] { entered.release(); release.acquire(); });
    auto unblock = qScopeGuard([&] { release.release(10); ComposerDraftStore::flushWrites(); ComposerDraftStore::setWriteObserver({}); });
    bool unwantedConfirmation = false;
    QTimer modal; modal.setInterval(5);
    connect(&modal, &QTimer::timeout, &window, [&] {
        if (auto *box = window.findChild<QMessageBox *>("questionColdCacheConfirm")) {
            unwantedConfirmation = true; box->reject();
        }
    });
    QSignalSpy completed(client, &HgsClient::questionAnswered);
    modal.start(); submit->click(); QVERIFY(!unwantedConfirmation); modal.stop();
    if (!change.isEmpty()) {
        QTRY_VERIFY(entered.available() > 0); entered.acquire();
        ComposerDraftStore::setWriteObserver({});
        if (change == "hash") question["question_hash"] = "changed";
        if (change == "source") question["source"] = "unreviewed";
        if (change == "run") { question["run_id"] = "changed"; details["run_id"] = "changed"; }
        details["pending_questions"] = QJsonArray{question};
        QVERIFY(FakeCli::replace(state, QJsonDocument(details).toJson()));
        client->inspectionReady({}, "kimi/docs/research", details, {});
        release.release(10); ComposerDraftStore::flushWrites(); QTest::qWait(200);
        QVERIFY(!QFileInfo::exists(capture)); QCOMPARE(completed.size(), 0); return;
    }
    card->answerRequested(key, "kimi-cache:exact", QJsonArray{}); // No duplicate delivery during the save barrier.
    QTRY_COMPARE(completed.size(), 1);
    QFile result(capture); QVERIFY(result.open(QIODevice::ReadOnly));
    const auto payload = QJsonDocument::fromJson(result.readAll()).object();
    QCOMPARE(payload["question_id"].toString(), QString("kimi-cache:exact"));
    QCOMPARE(payload["expected_question_hash"].toString(), QString("hash-cold"));
    QCOMPARE(payload["expected_run_id"].toString(), QString("run-cold"));
    QCOMPARE(payload["expected_conversation_id"].toString(), QString("conversation-cold"));
    const auto answer = payload["answers"].toArray().first().toObject();
    QCOMPARE(answer["question_id"].toString(), QString("cache"));
    QCOMPARE(answer["selected_option_ids"].toArray(), QJsonArray{QString("cache_%1").arg(choice)});
    QFile calls(directory.filePath("calls")); QVERIFY(calls.open(QIODevice::ReadOnly)); QCOMPARE(calls.readAll(), QByteArray("call\n"));
}

void TestSessionsWindow::coldQuestionSubmissionRevalidatesConsent_data()
{
    QTest::addColumn<QString>("change");
    QTest::addColumn<bool>("sent");
    for (const auto *name : {"cancel", "escape", "close", "run", "conversation", "question-id", "hash",
            "schema", "source", "tool", "tool-call", "approval-identity", "capability", "offline", "selection", "entry-identity", "raw-draft", "page", "sending"})
        QTest::newRow(name) << QString::fromLatin1(name) << false;
    for (const auto *name : {"proceed", "benign-poll", "duplicate", "warm", "ttl", "skip", "approval", "broader-approval"})
        QTest::newRow(name) << QString::fromLatin1(name) << true;
}

void TestSessionsWindow::coldQuestionSubmissionRevalidatesConsent()
{
    QFETCH(QString, change); QFETCH(bool, sent);
    QTemporaryDir directory;
    const auto program = directory.filePath("hgs"), state = directory.filePath("details.json"), capture = directory.filePath("answer.json");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path(__file__).parent
if sys.argv[1]=='inspect': print((root/'details.json').read_text())
elif sys.argv[1]=='answer':
 p=json.load(sys.stdin)
 with (root/'calls').open('a') as f:f.write('call\n')
 staged=root/'answer.tmp';staged.write_text(json.dumps({'args':sys.argv[1:],'payload':p}));staged.replace(root/'answer.json')
 time.sleep(.1)
 print(json.dumps(dict(status='answered',request_id=p['request_id'],name=sys.argv[2],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],question_id=p['question_id'],question_hash=p['expected_question_hash'])))
else: print('{}')
)PY");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    const bool approval = change == "approval" || change == "broader-approval";
    QJsonArray prompts;
    for (int index = 0; index < (approval ? 1 : 2); ++index)
        prompts.append(QJsonObject{{"id", QString("q_%1").arg(index)}, {"header", "Scope"}, {"question", "Which area?"},
            {"multi_select", false}, {"allow_other", !approval}, {"options", QJsonArray{
                QJsonObject{{"id", approval ? "allow" : "documents"}, {"label", approval ? "Allow once" : "Documents"}},
                QJsonObject{{"id", approval ? "deny" : "source"}, {"label", approval ? "Always allow" : "Source"}, {"scope", approval ? "broader" : "request"}}}}});
    QJsonObject question{{"question_id", "interaction-cold"}, {"question_hash", "hash-cold"}, {"run_id", "run-cold"},
        {"conversation_id", "conversation-cold"}, {"can_answer", true}, {"can_skip", true}, {"questions", prompts},
        {"source", change == "skip" ? "codex_async" : "claude"}, {"tool", "AskUserQuestion"}, {"tool_call_id", "toolu-cold"}, {"approval", approval},
        {"approval_choices", change == "broader-approval"}, {"optional", change == "skip"}};
    if (change == "skip") question["can_answer"] = false;
    QJsonObject details{{"tracked", true}, {"run_id", "run-cold"}, {"conversation_id", "conversation-cold"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"phase", "input"}, {"activity", "busy"},
        {"pending_questions", QJsonArray{question}}, {"cache_hint", QJsonObject{{"status", "cold"}, {"tokens", 12000}}},
        {"compact_context_supported", true}, {"clear_context_supported", true}};
    const qint64 expiry = QDateTime::currentSecsSinceEpoch() + (change == "ttl" ? 1 : 600);
    if (change == "warm" || change == "ttl") {
        details.remove("cache_hint");
        details["session_usage"] = QJsonObject{{"prompt_cache", QJsonObject{{"status", "warm"}, {"expires_at", expiry}}}};
    }
    const auto writeDetails = [&] { QVERIFY(FakeCli::replace(state, QJsonDocument(details).toJson())); };
    writeDetails();
    SessionsWindow window(FakeCli::program(program)); window.resize(1080, 760); window.setFleet(fleet());
    keepDetailsPollsOut(window); window.show(); window.showSession({}, "kimi/docs/research");
    auto *client = window.findChild<HgsClient *>(); auto *card = window.findChild<QuestionCard *>("questionCard");
    QTRY_VERIFY(card->isVisible()); QTRY_VERIFY(!card->isDraftLoading());
    // A change made while the confirmation is open must stay the latest evidence. An
    // inspection that read details.json before it would restore the reviewed identity,
    // on Windows even before the accepted confirmation returns, and the answer would be
    // sent. Settle one here, so none is running when the confirmation opens.
    settleSelectedInspection(window.findChild<MessageComposer *>("messageComposer"), client, {}, "kimi/docs/research", details);
    auto *list = window.findChild<QListWidget *>("sessionList");
    const QString key = list->currentItem()->data(Qt::UserRole).toString();
    auto *tabs = card->findChild<QTabBar *>("questionTabs");
    if (!approval) {
        for (auto *button : card->findChildren<QAbstractButton *>("questionOther")) button->click();
        for (auto *editor : card->findChildren<QLineEdit *>("questionFreeText")) editor->setText("  Exact  draft  ");
        tabs->setCurrentIndex(1);
    } else if (change == "broader-approval") {
        for (auto *button : card->findChildren<QAbstractButton *>("questionOption"))
            if (button->property("optionId") == "deny") button->click();
    }
    auto *action = card->findChild<QPushButton *>(change == "skip" ? "skipQuestion" : change == "approval" ? "denyToolRequest" : "submitQuestionAnswer");
    if (change == "broader-approval") { card->setReviewDelayForTests(200); window.activateWindow(); QTRY_VERIFY(action->isEnabled()); }
    QVERIFY(action->isEnabled());
    const auto before = card->submissionSnapshot(key, "interaction-cold");
    QJsonArray requested;
    QSignalSpy answerSignal(card, &QuestionCard::answerRequested);
    QSignalSpy completed(client, &HgsClient::questionAnswered);
    QSignalSpy failed(client, &HgsClient::questionAnswerFailed);
    QSignalSpy submitted(client, &HgsClient::questionAnswerSubmitted);
    bool shown = false;
    QTimer modal;
    modal.setInterval(5);
    connect(&modal, &QTimer::timeout, &window, [&] {
        auto *box = window.findChild<QMessageBox *>("questionColdCacheConfirm");
        if (!box || !box->isVisible()) return;
        modal.stop(); shown = true;
        QCOMPARE(box->buttons().size(), 2);
        QCOMPARE(box->defaultButton(), qobject_cast<QPushButton *>(box->button(QMessageBox::Cancel)));
        QVERIFY(box->text().contains("Compaction or clearing can invalidate"));
        QVERIFY(!QFileInfo::exists(capture));
        QVERIFY(!card->submissionSnapshot(key, "interaction-cold").value("sending").toBool());
        if (change == "cancel") { box->button(QMessageBox::Cancel)->click(); return; }
        if (change == "escape") { QTest::keyClick(box, Qt::Key_Escape); return; }
        if (change == "close") { box->close(); return; }
        if (change == "duplicate") card->answerRequested(key, "interaction-cold", QJsonArray{QJsonObject{{"question_id", "q_0"}, {"selected_option_ids", QJsonArray{}}, {"text", "Exact  draft"}}, QJsonObject{{"question_id", "q_1"}, {"selected_option_ids", QJsonArray{}}, {"text", "Exact  draft"}}});
        if (change == "selection") window.showSession({}, "codex/hgs/dashboard");
        else if (change == "offline" || change == "entry-identity") {
            auto boxes = fleet(); auto boxState = boxes.local();
            for (auto &session : boxState.sessions)
                if (session.name == "kimi/docs/research") { if (change == "offline") boxState.ok = false; else session.created += 1; }
            boxes.setLocal(boxState, QDateTime::currentMSecsSinceEpoch()); window.setFleet(boxes);
        } else if (change == "raw-draft") card->findChildren<QLineEdit *>("questionFreeText").first()->setText(" Exact  draft  ");
        else if (change == "page") tabs->setCurrentIndex(0);
        else if (change == "sending") card->setSending(key, "interaction-cold");
        else {
            if (change == "run") { details["run_id"] = "run-new"; question["run_id"] = "run-new"; }
            if (change == "conversation") { details["conversation_id"] = "conversation-new"; question["conversation_id"] = "conversation-new"; }
            if (change == "question-id") question["question_id"] = "interaction-new";
            if (change == "hash") question["question_hash"] = "hash-new";
            if (change == "schema") { auto prompt = prompts.first().toObject(); prompt["question"] = "Changed disclosure"; prompts[0] = prompt; question["questions"] = prompts; }
            if (change == "source") question["source"] = "changed-source";
            if (change == "tool") question["tool"] = "changed-tool";
            if (change == "tool-call") question["tool_call_id"] = "toolu-new";
            if (change == "approval-identity") question["approval"] = true;
            if (change == "benign-poll") { question["created_at"] = 42; question["answer_unavailable_reason"] = "Updated evidence"; details["last_event_at"] = 43; }
            if (change == "capability") question["can_answer"] = false;
            details["pending_questions"] = QJsonArray{question};
            writeDetails(); client->inspectionReady({}, "kimi/docs/research", details);
        }
        for (auto *button : box->buttons()) if (box->buttonRole(button) == QMessageBox::AcceptRole) button->click();
    });
    // No inspection is needed for click-time TTL expiry (whole seconds, as CacheStatus compares).
    if (change == "ttl") QTRY_VERIFY(QDateTime::currentSecsSinceEpoch() >= expiry);
    modal.start();
    if (change == "broader-approval") { action->click(); QVERIFY(!shown); QVERIFY(!QFileInfo::exists(capture)); QCOMPARE(action->text(), QString("Confirm")); }
    action->click();
    modal.stop();
    QCOMPARE(shown, change != "warm");
    QVERIFY(!answerSignal.isEmpty()); requested = answerSignal.first().at(2).value<QJsonArray>();
    if (!sent) {
        // An erroneous QProcess dispatch can write after the modal returns.
        // Drain the fixture's completion interval and watch all answer outcomes.
        QTest::qWait(300);
        QVERIFY(!QFileInfo::exists(capture));
        QVERIFY(!QFileInfo::exists(directory.filePath("calls")));
        QCOMPARE(completed.size(), 0); QCOMPARE(failed.size(), 0); QCOMPARE(submitted.size(), 0);
        if (change == "cancel" || change == "escape" || change == "close") QCOMPARE(card->submissionSnapshot(key, "interaction-cold"), before);
        if (change == "raw-draft") QCOMPARE(card->findChildren<QLineEdit *>("questionFreeText").first()->text(), QString(" Exact  draft  "));
        return;
    }
    card->answerRequested(key, "interaction-cold", requested); // In-flight repeats create no second request or modal.
    QTRY_VERIFY(QFileInfo::exists(capture));
    QFile result(capture); QVERIFY(result.open(QIODevice::ReadOnly));
    const auto record = QJsonDocument::fromJson(result.readAll()).object();
    QCOMPARE(record["args"].toArray(), QJsonArray({"answer", "kimi/docs/research", "--json"}));
    const auto payload = record["payload"].toObject();
    QCOMPARE(payload["answers"].toArray(), requested);
    QCOMPARE(payload["question_id"].toString(), QString("interaction-cold"));
    QCOMPARE(payload["expected_question_hash"].toString(), QString("hash-cold"));
    QCOMPARE(payload["expected_run_id"].toString(), QString("run-cold"));
    QCOMPARE(payload["expected_conversation_id"].toString(), QString("conversation-cold"));
    QTRY_COMPARE(completed.size(), 1);
    QFile calls(directory.filePath("calls")); QVERIFY(calls.open(QIODevice::ReadOnly)); QCOMPARE(calls.readAll(), QByteArray("call\n"));
}

void TestSessionsWindow::questionAnswersStayWithOriginalSession()
{
    QTemporaryDir directory;
    const QString program = directory.filePath("hgs");
    QFile file(program); QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
args=sys.argv[1:]
root=pathlib.Path(__file__).parent
question={'question_id':'interaction-one','question_hash':'exact-hash','run_id':'run-one','conversation_id':'conv-one','can_answer':True,
 'questions':[{'id':'q_0','header':'Scope','question':'Which area should I inspect?','multi_select':False,'allow_other':True,
 'options':[{'id':'opt_0_0','label':'Documents','description':'Read the project docs'},{'id':'opt_0_1','label':'Source','description':'Read source files'}]}]}
if args[0]=='inspect':
 pending=args[1]=='kimi/docs/research' and not (root/'done').exists()
 print(json.dumps({'tracked':True,'run_id':'run-one','conversation_id':'conv-one','runtime_state':'live','process_state':'running',
 'activity':'busy' if pending else 'idle','phase':'input' if pending else 'idle','last_event_at':2000000000,
 'pending_questions':[question] if pending else [],'events':[],'cursor':0}))
elif args[0]=='answer':
 payload=json.load(sys.stdin)
 (root/'answer.json').write_text(json.dumps({'args':args,'payload':payload}))
 with (root/'calls').open('a') as f:f.write('call\n')
 time.sleep(.15)
 (root/'done').touch()
 print(json.dumps({'status':'answered','request_id':payload['request_id'],'name':args[1],
 'run_id':payload['expected_run_id'],'conversation_id':payload['expected_conversation_id'],
 'question_id':payload['question_id'],'question_hash':payload['expected_question_hash']}))
)PY");
    file.close(); QVERIFY(FakeCli::makeRunnable(file.fileName()));
    SessionsWindow window(FakeCli::program(program)); window.setFleet(fleet()); window.show(); window.showSession({}, "kimi/docs/research");
    auto *card = window.findChild<QuestionCard *>("questionCard"); QVERIFY(card);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    auto *client = window.findChild<HgsClient *>(); QVERIFY(client);
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    QSignalSpy answered(client, &HgsClient::questionAnswered);
    QTRY_VERIFY(card->isVisible()); QTRY_VERIFY(!card->isDraftLoading()); QVERIFY(!composer->isInputVisible()); QVERIFY(composer->toolbar()->isVisible());
    if (!qEnvironmentVariable("HGS_PREVIEW_DIR").isEmpty()) {
        window.resize(1240, 900); QTest::qWait(30);
        QVERIFY(window.grab().save(qEnvironmentVariable("HGS_PREVIEW_DIR") + "/sessions-question.png"));
    }
    const QString key = list->currentItem()->data(Qt::UserRole).toString();
    const QJsonArray choices{QJsonObject{{"question_id", "q_0"}, {"selected_option_ids", QJsonArray{}}, {"text", "Inspect source using Enter"}}};
    card->findChild<QAbstractButton *>("questionOther")->click();
    auto *answerEditor = card->findChild<QLineEdit *>("questionFreeText");
    answerEditor->setText("Inspect source using Enter"); answerEditor->setFocus();
    QTest::keyClick(answerEditor, Qt::Key_Return);
    QTest::keyClick(answerEditor, Qt::Key_Return); // an in-flight answer cannot be sent twice
    window.showSession({}, "codex/hgs/dashboard"); setComposerText(composer, "Keep this other-session draft");
    card->answerRequested(key, "interaction-one", choices); // stale selection cannot send again
    QTRY_COMPARE(answered.size(), 1);
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep this other-session draft"));
    QFile captured(directory.filePath("answer.json")); QVERIFY(captured.open(QIODevice::ReadOnly));
    const auto record = QJsonDocument::fromJson(captured.readAll()).object();
    QCOMPARE(record["args"].toArray(), QJsonArray({"answer", "kimi/docs/research", "--json"}));
    const auto payload = record["payload"].toObject();
    QCOMPARE(payload["answers"].toArray(), choices);
    QCOMPARE(payload["expected_question_hash"].toString(), QString("exact-hash"));
    QCOMPARE(payload["expected_run_id"].toString(), QString("run-one"));
    window.showSession({}, "kimi/docs/research");
    QTRY_VERIFY(!card->isVisible()); QVERIFY(composer->isInputVisible());
    card->answerRequested(key, "interaction-one", choices); // resolved question is no longer actionable
    QTest::qWait(30);
    QFile calls(directory.filePath("calls")); QVERIFY(calls.open(QIODevice::ReadOnly)); QCOMPARE(calls.readAll(), QByteArray("call\n"));
}

void TestSessionsWindow::queuedQuestionAnswersDisappearAfterSubmission()
{
    QTemporaryDir directory;QFile program(directory.filePath("hgs"));QVERIFY(program.open(QIODevice::WriteOnly));
    // Polls read details.json while the answer rewrites it: replace it, so no inspection
    // reads a truncated file and fails. Windows refuses the rename while a poll has the
    // file open; retry then.
    program.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path(__file__).parent
if sys.argv[1]=='inspect':
 print((root/'details.json').read_text())
elif sys.argv[1]=='answer':
 p=json.load(sys.stdin)
 with (root/'calls').open('a') as f:f.write('call\n')
 details=json.loads((root/'details.json').read_text())
 q=details['pending_questions'][0]
 q['can_answer']=False;q['can_skip']=False
 q['answer_delivery']={'status':'submitted','request_id':p['request_id'],'run_id':p['expected_run_id'],
  'conversation_id':p['expected_conversation_id'],'question_id':p['question_id'],
  'question_hash':p['expected_question_hash'],'answers':p['answers']}
 details['input_queue']={'id':'native-queue-one','text':'↳ Which validation should run? → '+p['answers'][0]['text'],
  'can_send_now':True,'hint':'Interrupt and send the queued answer using the native action.'}
 staged=root/'details.tmp';staged.write_text(json.dumps(details))
 for _ in range(250):
  try:staged.replace(root/'details.json');break
  except PermissionError:time.sleep(.02)
 else:sys.exit('details.json stayed open')
 print(json.dumps(dict(q['answer_delivery'],name=sys.argv[2])))
)PY");program.close();QVERIFY(FakeCli::makeRunnable(program.fileName()));
    const QJsonObject question{{"question_id","optional-one"},{"question_hash","hash-one"},{"run_id","run-one"},
        {"conversation_id","conversation-one"},{"source","codex_async"},{"optional",true},{"can_answer",true},{"can_skip",true},
        {"questions",QJsonArray{QJsonObject{{"id","q_0"},{"question","Which validation should run?"},{"allow_other",true},{"options",QJsonArray{}}}}}};
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"runtime_state","live"},{"process_state","running"},{"activity","busy"},{"phase","thinking"},
        {"pending_questions",QJsonArray{question}},{"events",QJsonArray{}},{"cursor",0}};
    const auto save=[&] { return FakeCli::replace(directory.filePath("details.json"), QJsonDocument(details).toJson()); };QVERIFY(save());
    SessionsWindow window(FakeCli::program(program.fileName()));window.setFleet(fleet());window.show();window.activateWindow();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();auto *card=window.findChild<QuestionCard *>("questionCard");
    auto *composer=window.findChild<MessageComposer *>("messageComposer");
    auto *activity=window.findChild<ActivityView *>("mainActivity");QVERIFY(activity);
    QSignalSpy promoted(activity,&ActivityView::queueSendNowRequested);
    QSignalSpy submitted(client,&HgsClient::questionAnswerSubmitted),answered(client,&HgsClient::questionAnswered);
    QSignalSpy messages(composer,&MessageComposer::sendRequested),stopped(composer,&MessageComposer::interruptRequested);
    QTRY_VERIFY(card->isVisible()); QTRY_VERIFY(!card->isDraftLoading());auto *editor=card->findChild<QLineEdit *>("questionFreeText");QVERIFY(editor);
    editor->setText("Run unit tests and a desktop preview.");setComposerText(composer, "Keep my next-message draft");
    auto *send=card->findChild<QPushButton *>("submitQuestionAnswer");QVERIFY(send->isEnabled());
    QVERIFY(composer->isInputVisible());editor->setFocus();QTRY_VERIFY(editor->hasFocus());QTest::keyClick(editor,Qt::Key_Return);
    QTRY_COMPARE(submitted.size(),1);QCOMPARE(answered.size(),0);QVERIFY(card->isHidden());
    QCOMPARE(messages.size(),0);QCOMPARE(stopped.size(),0);
    QVERIFY(composer->editor()->hasFocus());
    QFile fixture(directory.filePath("details.json"));QVERIFY(fixture.open(QIODevice::ReadOnly));
    details=QJsonDocument::fromJson(fixture.readAll()).object();fixture.close();
    client->inspectionReady({},"codex/hgs/dashboard",details);
    QVERIFY(card->isHidden());QCOMPARE(details["pending_questions"].toArray().size(),1);
    QVERIFY(activity->findChild<QWidget *>("activityInputQueue")->isVisible());
    QVERIFY(activity->findChild<QPushButton *>("queueSendNow")->isEnabled());QCOMPARE(promoted.size(),0);
    QVERIFY(composer->isInputVisible());QCOMPARE(composer->editor()->toPlainText(),QString("Keep my next-message draft"));
    auto stale=details;stale["pending_questions"]=QJsonArray{question};
    client->inspectionReady({},"codex/hgs/dashboard",stale);QVERIFY(card->isHidden());
    auto *list=window.findChild<QListWidget *>("sessionList");
    const auto key=list->currentItem()->data(Qt::UserRole).toString();
    card->answerRequested(key,"optional-one",QJsonArray{QJsonObject{{"question_id","q_0"},{"text","Do not resend"}}});
    QCOMPARE(messages.size(),0);QCOMPARE(promoted.size(),0);
    client->inspectionReady({},"codex/hgs/dashboard",details);
    if(!qEnvironmentVariable("HGS_PREVIEW_DIR").isEmpty()) {
        window.resize(1240,900);QTest::qWait(30);
        QVERIFY(window.grab().save(qEnvironmentVariable("HGS_PREVIEW_DIR")+"/queued-question-answer.png"));
    }
    window.showSession({},"kimi/docs/research");window.showSession({},"codex/hgs/dashboard");
    QTRY_VERIFY(activity->findChild<QWidget *>("activityInputQueue")->isVisible());QVERIFY(card->isHidden());
    {
        SessionsWindow reopened(FakeCli::program(program.fileName()));reopened.setFleet(fleet());reopened.show();reopened.showSession({},"codex/hgs/dashboard");
        auto *reopenedActivity=reopened.findChild<ActivityView *>("mainActivity");
        QTRY_VERIFY(reopenedActivity->findChild<QWidget *>("activityInputQueue")->isVisible());
        QVERIFY(reopened.findChild<QuestionCard *>("questionCard")->isHidden());
        // Recognition from a persisted receipt survives a later stale snapshot.
        reopened.findChild<HgsClient *>()->inspectionReady({},"codex/hgs/dashboard",stale);
        QVERIFY(reopened.findChild<QuestionCard *>("questionCard")->isHidden());
    }
    auto nextQuestion=question;nextQuestion["question_id"]="optional-two";nextQuestion["question_hash"]="hash-two";
    nextQuestion["questions"]=QJsonArray{QJsonObject{{"id","q_0"},{"question","Another unanswered question?"},{"allow_other",true}}};
    const auto submittedQuestion=details["pending_questions"].toArray().first();
    details["pending_questions"]=QJsonArray{submittedQuestion,nextQuestion};QVERIFY(save());
    client->inspectionReady({},"codex/hgs/dashboard",details);
    // An inspection already in flight may still return the prior submitted-only
    // snapshot while the new question draft loads. Wait for the exact offered
    // form, not an empty card (which also has no draft left to load).
    QTRY_VERIFY(card->isVisible() && !card->isDraftLoading()
        && questionPrompts(card) == QStringList{"Another unanswered question?"}
        && card->findChild<QLineEdit *>("questionFreeText"));
    QVERIFY(card->isVisible());QCOMPARE(card->findChild<QLabel *>("questionPendingCount")->text(),QString("1 pending"));
    QCOMPARE(questionPrompts(card),QStringList{"Another unanswered question?"});
    QCOMPARE(card->findChild<QLabel *>("questionHeading")->text(),QString("Optional question"));
    QVERIFY(card->findChild<QToolButton *>("questionQueuePrevious")->isHidden());
    QVERIFY(card->findChild<QToolButton *>("questionQueueNext")->isHidden());
    card->findChild<QLineEdit *>("questionFreeText")->setText("Keep the remaining answer draft");
    client->inspectionReady({},"codex/hgs/dashboard",details);
    QCOMPARE(card->findChild<QLineEdit *>("questionFreeText")->text(),QString("Keep the remaining answer draft"));
    window.showSession({},"kimi/docs/research");window.showSession({},"codex/hgs/dashboard");
    QTRY_VERIFY(card->isVisible() && !card->isDraftLoading()
        && questionPrompts(card) == QStringList{"Another unanswered question?"}
        && card->findChild<QLineEdit *>("questionFreeText"));
    QCOMPARE(card->findChild<QLineEdit *>("questionFreeText")->text(),QString("Keep the remaining answer draft"));
    QFile calls(directory.filePath("calls"));QVERIFY(calls.open(QIODevice::ReadOnly));QCOMPARE(calls.readAll(),QByteArray("call\n"));
    details["pending_questions"]=QJsonArray{};details.remove("input_queue");QVERIFY(save());client->inspectionReady({},"codex/hgs/dashboard",details);
    QTRY_VERIFY(card->isHidden());QCOMPARE(composer->editor()->toPlainText(),QString("Keep my next-message draft"));
    QCOMPARE(promoted.size(),0);
}

void TestSessionsWindow::terminalChoiceOpensOnlyTheRequestingTerminal_data()
{
    QTest::addColumn<QString>("prefix");QTest::addColumn<QString>("source");QTest::addColumn<QString>("item");QTest::addColumn<QString>("option");
    QTest::addColumn<QString>("installer");QTest::addColumn<QString>("notice");
    QTest::newRow("hook review")<<"codex-hooks-trust:"<<"codex_hooks_trust"<<"hooks_trust"<<"review"<<QString()<<QString();
    // Hosts before the installer state keep the receipt without codex_update.
    QTest::newRow("codex update")<<"codex-update:"<<"codex_update"<<"codex_update"<<"update_now"<<QString()<<QString();
    QTest::newRow("codex update installing")<<"codex-update:"<<"codex_update"<<"codex_update"<<"update_now"<<"installing"
        <<"Codex in codex/hgs/dashboard (arch) is installing its update in Terminal. Resume the session after Codex exits.";
    QTest::newRow("codex update exited")<<"codex-update:"<<"codex_update"<<"codex_update"<<"update_now"<<"exited"
        <<"Codex in codex/hgs/dashboard (arch) exited after its update. Check the result in Terminal, then resume the session.";
    QTest::newRow("unknown installer state")<<"codex-update:"<<"codex_update"<<"codex_update"<<"update_now"<<"verifying"<<QString();
    QTest::newRow("installer state of another choice")<<"codex-hooks-trust:"<<"codex_hooks_trust"<<"hooks_trust"<<"review"<<"installing"<<QString();
}

void TestSessionsWindow::terminalChoiceOpensOnlyTheRequestingTerminal()
{
    QFETCH(QString,prefix);QFETCH(QString,source);QFETCH(QString,item);QFETCH(QString,option);QFETCH(QString,installer);QFETCH(QString,notice);
    QTemporaryDir directory;QFile file(directory.filePath("hgs"));QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(QString(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
args=sys.argv[1:];root=pathlib.Path(__file__).parent
hash=('b' if (root/'second').exists() else 'a')*64
question={'question_id':'%1'+hash,'question_hash':hash,'run_id':'startup-run','conversation_id':None,
 'source':'%2','answer_transport':'codex_tui','can_answer':True,
 'questions':[{'id':'%3','question':'Startup choice','allow_other':False,
 'options':[{'id':'%4','label':'Continue in Terminal'},{'id':'other','label':'Continue here'}]}]}
if args[0]=='inspect':
 print(json.dumps({'tracked':True,'run_id':'startup-run','conversation_id':None,'runtime_state':'live','process_state':'running',
 'activity':'busy','phase':'approval','pending_questions':[question] if not (root/'done').exists() else [],'events':[],'cursor':0}))
elif args[0]=='answer':
 p=json.load(sys.stdin);time.sleep(.15);(root/'done').touch()
 receipt={'status':'answered','request_id':p['request_id'],'name':args[1],
 'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],'question_id':p['question_id'],
 'question_hash':p['expected_question_hash'],'open_terminal':True}
 if '%5':receipt['codex_update']='%5'
 print(json.dumps(receipt))
)PY").arg(prefix,source,item,option,installer).toUtf8());file.close();QVERIFY(FakeCli::makeRunnable(file.fileName()));
    SessionsWindow window(FakeCli::program(file.fileName()));window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    auto *card=window.findChild<QuestionCard *>("questionCard");auto *client=window.findChild<HgsClient *>();
    auto *list=window.findChild<QListWidget *>("sessionList");auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");
    QSignalSpy answered(client,&HgsClient::questionAnswered);const auto key=list->currentItem()->data(Qt::UserRole).toString();
    // Answer only the exact question the card offers. An inspection that read the
    // fixture before the question changed can still deliver the older one, and the
    // window correctly refuses an answer to a question it does not offer.
    const auto offers=[&](const QString &question){return card->isVisible() && !card->isDraftLoading() && !card->submissionSnapshot(key,question).isEmpty();};
    const QJsonArray answer{QJsonObject{{"question_id",item},{"selected_option_ids",QJsonArray{option}},{"text",""}}};
    const QString id=prefix+QString(64,QChar('a')),secondId=prefix+QString(64,QChar('b'));
    QTRY_VERIFY(offers(id));
    auto *noticeLabel=window.findChild<QLabel *>("notice");QVERIFY(noticeLabel);
    card->answerRequested(key,id,answer);QTRY_COMPARE(answered.size(),1);
    QCOMPARE(tabs->currentWidget()->objectName(),QString("terminalView"));
    // Codex's installer state is explained beside Terminal; nothing else is announced.
    QCOMPARE(noticeLabel->text(),notice);QCOMPARE(noticeLabel->isVisible(),!notice.isEmpty());
    window.findChild<QPushButton *>("dismissNotice")->click();QVERIFY(noticeLabel->isHidden());
    // A delayed review acknowledgement must not steal another session's tab.
    QVERIFY(QFile::remove(directory.filePath("done")));QFile second(directory.filePath("second"));QVERIFY(second.open(QIODevice::WriteOnly));second.close();tabs->setCurrentIndex(0);
    window.showSession({},"kimi/docs/research");window.showSession({},"codex/hgs/dashboard");QTRY_VERIFY(offers(secondId));
    card->answerRequested(key,secondId,answer);window.showSession({},"kimi/docs/research");tabs->setCurrentIndex(0);
    QTRY_COMPARE(answered.size(),2);QVERIFY(tabs->currentWidget()->objectName()!="terminalView");
    // It still names the session whose Codex is updating.
    QCOMPARE(noticeLabel->text(),notice);
}

void TestSessionsWindow::optionalQuestionKeepsComposerAvailable()
{
    QJsonObject question{{"question_id","optional-one"},{"question_hash","hash"},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"optional",true},{"can_answer",false},{"can_skip",true},{"questions",QJsonArray{QJsonObject{{"id","q_0"},{"question","Which option?"},{"required",false},{"allow_other",true},{"options",QJsonArray{}}}}}};
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},{"runtime_state","live"},{"process_state","running"},
        {"activity","idle"},{"phase","idle"},{"pending_questions",QJsonArray{}}};
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const auto fixture = m_dir->filePath("optional-question-hgs");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n",
        "if False:pass\nelif c=='inspect':print(open(__file__+'.details').read())\n")));
    QVERIFY(FakeCli::replace(fixture + ".details", QJsonDocument(details).toJson()));
    SessionsWindow window(FakeCli::program(fixture));window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *card=window.findChild<QuestionCard *>("questionCard");
    const auto applyDetails = [&] {
        if (!FakeCli::replace(fixture + ".details", QJsonDocument(details).toJson())) return false;
        // Wait for an inspection started after the atomic update: an older
        // in-flight reply must not replace the form while its draft loads.
        QSignalSpy replies(client, &HgsClient::inspectionReady); qsizetype issued = -1;
        return QTest::qWaitFor([&] {
            if (issued < 0) {
                if (!client->requestInspection({}, "codex/hgs/dashboard")) return false;
                issued = replies.size();
            }
            return std::any_of(replies.cbegin() + issued, replies.cend(), [&](const QList<QVariant> &reply) {
                return reply[0].toString().isEmpty() && reply[1].toString() == "codex/hgs/dashboard"
                    && reply[2].toJsonObject() == details && reply[3].toString().isEmpty();
            });
        });
    };
    QVERIFY(applyDetails());
    setComposerText(composer, "An unrelated follow-up");
    details["pending_questions"] = QJsonArray{question}; QVERIFY(applyDetails());
    QVERIFY(card->isVisible());QTRY_VERIFY(!card->isDraftLoading());QVERIFY(composer->isInputVisible());QVERIFY(composer->findChild<QPushButton *>("sendMessage")->isEnabled());
    QVERIFY(card->findChild<QPushButton *>("skipQuestion")->isEnabled());
    QVERIFY(applyDetails());QCOMPARE(composer->editor()->toPlainText(),QString("An unrelated follow-up"));
    auto latest=question;latest["question_id"]="latest";latest["created_at"]=20;
    latest["questions"]=QJsonArray{QJsonObject{{"id","q_0"},{"question","Most recent question?"},{"required",false},{"allow_other",true},{"options",QJsonArray{}}}};
    question["created_at"]=10;question["can_answer"]=true;
    auto olderItems=question["questions"].toArray();auto secondItem=olderItems.first().toObject();secondItem["id"]="q_1";secondItem["question"]="Another older question?";olderItems.append(secondItem);question["questions"]=olderItems;
    details["pending_questions"]=QJsonArray{question,latest,question,QJsonObject{}};QVERIFY(applyDetails());
    QCOMPARE(card->findChild<QLabel *>("questionPendingCount")->text(),QString("3 pending"));
    QStringList labels=questionPrompts(card);
    QVERIFY(labels.contains("Most recent question?"));QVERIFY(!labels.contains("Which option?"));
    auto *previous=card->findChild<QToolButton *>("questionQueuePrevious"),*next=card->findChild<QToolButton *>("questionQueueNext");
    QVERIFY(previous->isVisible());QVERIFY(!previous->isEnabled());QVERIFY(next->isEnabled());
    QCOMPARE(card->findChild<QLabel *>("questionHeading")->text(),QString("Optional question #1/2"));
    QSignalSpy answers(card,&QuestionCard::answerRequested);
    const auto firstAnswer=[card] {
        for(auto *editor:card->findChildren<QLineEdit *>("questionFreeText"))
            if(editor->property("questionId").toString()=="q_0")return editor;
        return static_cast<QLineEdit *>(nullptr);
    };
    const auto answerReady = [&](const QString &prompt) {
        return card->isVisible() && !card->isDraftLoading() && questionPrompts(card).contains(prompt) && firstAnswer();
    };
    QTRY_VERIFY(answerReady("Most recent question?"));firstAnswer()->setText("Latest draft");
    next->click();QTRY_VERIFY(!card->isDraftLoading());
    labels=questionPrompts(card);
    QVERIFY(labels.contains("Which option?"));QVERIFY(!labels.contains("Most recent question?"));
    QVERIFY(previous->isEnabled());QVERIFY(!next->isEnabled());
    QCOMPARE(card->findChild<QLabel *>("questionHeading")->text(),QString("Optional question #2/2"));
    QTRY_VERIFY(answerReady("Which option?"));firstAnswer()->setText("Older draft");
    auto *pages=card->findChild<QTabBar *>("questionTabs");pages->setCurrentIndex(1);
    previous->click();QTRY_VERIFY(!card->isDraftLoading());
    QTRY_VERIFY(answerReady("Most recent question?") && firstAnswer()->text() == "Latest draft");
    next->click();QTRY_VERIFY(!card->isDraftLoading());
    QTRY_VERIFY(answerReady("Which option?") && firstAnswer()->text() == "Older draft");
    QCOMPARE(card->findChild<QTabBar *>("questionTabs")->currentIndex(),1);
    auto newest=latest;newest["question_id"]="newest";newest["created_at"]=30;
    details["pending_questions"]=QJsonArray{newest,question,latest};QVERIFY(applyDetails());
    QCOMPARE(card->findChild<QLabel *>("questionPendingCount")->text(),QString("4 pending"));
    QCOMPARE(card->findChild<QLabel *>("questionHeading")->text(),QString("Optional question #3/3"));
    QTRY_VERIFY(answerReady("Which option?") && firstAnswer()->text() == "Older draft");
    // A required confirmation preempts browsing, then returns to the chosen draft.
    auto required=latest;required["optional"]=false;required["question_id"]="required";required["can_answer"]=true;
    details["pending_questions"]=QJsonArray{newest,question,required,latest};QVERIFY(applyDetails());
    QVERIFY(previous->isHidden());QVERIFY(next->isHidden());QVERIFY(!composer->isInputVisible());
    QCOMPARE(card->findChild<QLabel *>("questionHeading")->text(),QString("Agent needs your answer"));
    card->queueNavigationRequested(1); // Stale navigation cannot bypass a required request.
    QVERIFY(!composer->isInputVisible());QVERIFY(next->isHidden());
    details["pending_questions"]=QJsonArray{newest,question,latest};QVERIFY(applyDetails());
    QTRY_VERIFY(answerReady("Which option?") && firstAnswer()->text() == "Older draft");
    QVERIFY(composer->isInputVisible());QCOMPARE(answers.size(),0);
    // A different conversation must start from its own newest question.
    details["conversation_id"]="conversation-two";QVERIFY(applyDetails());
    QVERIFY(applyDetails()); // Initial history fetch after the conversation changed.
    QTRY_VERIFY(answerReady("Most recent question?"));
    QCOMPARE(firstAnswer()->text(),QString());
    QVERIFY(!previous->isEnabled());
    details["conversation_id"]="conversation-one";QVERIFY(applyDetails());
    QVERIFY(applyDetails());
    QTRY_VERIFY(answerReady("Which option?") && firstAnswer()->text() == "Older draft");
    // When the selected request resolves elsewhere, fall back to a pending one.
    details["pending_questions"]=QJsonArray{latest};QVERIFY(applyDetails());
    QTRY_VERIFY(answerReady("Most recent question?") && firstAnswer()->text() == "Latest draft");
    QVERIFY(previous->isHidden());QVERIFY(next->isHidden());
    details["pending_questions"]=QJsonArray{};QVERIFY(applyDetails());
    QVERIFY(card->isHidden());QVERIFY(composer->isInputVisible());QCOMPARE(composer->editor()->toPlainText(),QString("An unrelated follow-up"));
}

void TestSessionsWindow::optionalQuestionNotificationsAreOptIn()
{
    SessionsWindow window(program());
    auto *settings = window.findChild<QWidget *>("settingsPage"); QVERIFY(settings);
    auto *check = settings->findChild<QCheckBox *>("notifyOptionalQuestions"); QVERIFY(check);
    QVERIFY(!check->isChecked()); QVERIFY(!QuestionNotificationSettings::enabled());
    check->setChecked(true);
    QVERIFY(QuestionNotificationSettings::enabled());
    QCOMPARE(QSettings().value("notifications/optionalQuestions").toBool(), true);
    SessionsWindow reopened(program());
    QVERIFY(reopened.findChild<QCheckBox *>("notifyOptionalQuestions")->isChecked());
    check->setChecked(false); QVERIFY(!QuestionNotificationSettings::enabled());
}

void TestSessionsWindow::questionCompletionKeepsInputFocus_data()
{
    QTest::addColumn<QString>("action");
    for (const auto &action : {"mouse", "keyboard", "navigate", "external"}) QTest::newRow(action) << QString(action);
}

void TestSessionsWindow::questionCompletionKeepsInputFocus()
{
    QFETCH(QString, action);
    QTemporaryDir directory;
    QFile program(directory.filePath("hgs")); QVERIFY(program.open(QIODevice::WriteOnly));
    program.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path(__file__).parent
if sys.argv[1]=='inspect':
 data=json.loads((root/'details.json').read_text())
 if (root/'done').exists(): data['pending_questions']=[]
 print(json.dumps(data))
elif sys.argv[1]=='answer':
 p=json.load(sys.stdin)
 (root/'submitted').touch()
 for _ in range(1000):
  if (root/'complete').exists(): break
  time.sleep(.01)
 (root/'done').touch()
 print(json.dumps({'status':'answered','request_id':p['request_id'],'name':sys.argv[2],
  'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],
  'question_id':p['question_id'],'question_hash':p['expected_question_hash']}))
)PY");
    program.close(); QVERIFY(FakeCli::makeRunnable(program.fileName()));
    const QJsonObject question{{"question_id","question-one"},{"question_hash","hash-one"},{"run_id","run-one"},
        {"conversation_id","conversation-one"},{"can_answer",true},{"questions",QJsonArray{QJsonObject{
            {"id","q_0"},{"header","Scope"},{"question","Which area?"},{"allow_other",true},
            {"options",QJsonArray{QJsonObject{{"id","opt_0_0"},{"label","Documents"}}}}}}}};
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"runtime_state","live"},{"process_state","running"},{"pending_questions",QJsonArray{question}},
        {"session_usage",QJsonObject{{"context",QJsonObject{{"used",236000}}}}},{"events",QJsonArray{}},{"cursor",0}};
    QFile fixture(directory.filePath("details.json")); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(QJsonDocument(details).toJson()); fixture.close();
    SessionsWindow window(FakeCli::program(program.fileName())); window.setFleet(fleet()); window.show(); window.activateWindow();
    window.showSession({},"kimi/docs/research");
    auto *card=window.findChild<QuestionCard *>("questionCard"); auto *composer=window.findChild<MessageComposer *>("messageComposer");
    auto *client=window.findChild<HgsClient *>(); auto *context=window.findChild<QPushButton *>("activityContext");
    auto *submit=card->findChild<QPushButton *>("submitQuestionAnswer");
    QSignalSpy answered(client,&HgsClient::questionAnswered);
    QTRY_VERIFY(card->isVisible()); QTRY_VERIFY(!card->isDraftLoading()); QTRY_VERIFY(window.isActiveWindow());
    auto *option=card->findChild<QAbstractButton *>("questionOption"); QVERIFY(option);
    option->click(); option->setFocus(Qt::TabFocusReason); QTRY_VERIFY(option->hasFocus());
    setComposerText(composer, "Keep this draft");
    composer->editor()->moveCursor(QTextCursor::End);
    if(action=="external") {
        details["pending_questions"]=QJsonArray{};
        client->inspectionReady({},"kimi/docs/research",details);
    } else {
        submit->setFocus(Qt::TabFocusReason);
        if(action=="keyboard") QTest::keyClick(submit,Qt::Key_Space);
        else QTest::mouseClick(submit,Qt::LeftButton);
        QVERIFY(!context->hasFocus());
        QTRY_VERIFY(QFile::exists(directory.filePath("submitted")));
        if(action=="navigate") {context->setFocus(Qt::TabFocusReason); QVERIFY(context->hasFocus());}
        QFile complete(directory.filePath("complete")); QVERIFY(complete.open(QIODevice::WriteOnly)); complete.close();
        QTRY_COMPARE(answered.size(),1);
    }
    QTRY_VERIFY(card->isHidden()); QVERIFY(composer->isInputVisible());
    if(action=="navigate") QVERIFY(context->hasFocus());
    else {QVERIFY(composer->editor()->hasFocus()); QTest::keyClicks(composer->editor()," + next");}
    QCOMPARE(composer->editor()->toPlainText(),action=="navigate" ? QString("Keep this draft") : QString("Keep this draft + next"));
    // Ordinary polling must preserve the chosen target after the card disappears.
    details["pending_questions"]=QJsonArray{}; client->inspectionReady({},"kimi/docs/research",details);
    QVERIFY(action=="navigate" ? context->hasFocus() : composer->editor()->hasFocus());
}

void TestSessionsWindow::questionFooterHasRoomAndKeepsAnswers_data()
{
    QTest::addColumn<QString>("theme");QTest::addColumn<QSize>("size");
    for(const auto *theme:{"dark","light"})for(const auto size:{QSize(1100,780),QSize(960,600)})
        QTest::newRow(qPrintable(QString("%1-%2").arg(theme).arg(size.height())))<<QString(theme)<<size;
}

void TestSessionsWindow::questionFooterHasRoomAndKeepsAnswers()
{
    QFETCH(QString,theme);QFETCH(QSize,size);QSettings().setValue("workspace/theme",theme);
    auto state=fleet();auto box=state.local();auto &session=box.sessions[0];
    session.conversationId="conversation-one";session.runId="run-one";session.phase="input";session.activity="waiting";session.attentionId="question-one";
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.resize(size);window.setFleet(state);
    // The window's own inspections answer with the base fixture, which reports no
    // pending question in the same conversation. One arriving after the injected
    // details replaces them and hides the card, or clears it before the answers are
    // chosen. Inspect only on show and selection, then wait for an inspection of the
    // test's own: HgsClient runs one per session, so once its reply is in, none is
    // pending and no poll follows.
    window.setDetailsPollForTests(500,24*60*60*1000);window.show();
    auto *client=window.findChild<HgsClient *>();QSignalSpy inspections(client,&HgsClient::inspectionReady);
    window.showSession({},session.name);
    qsizetype issued=-1;
    // Under load the window's inspection may run until HgsClient's 5 s limit.
    QTRY_VERIFY_WITH_TIMEOUT(issued>=0 || (client->requestInspection({},session.name) && (issued=inspections.size(),true)),15000);
    QTRY_VERIFY_WITH_TIMEOUT(std::any_of(inspections.cbegin()+issued,inspections.cend(),[&](const QList<QVariant> &reply){
        return reply[0].toString().isEmpty() && reply[1].toString()==session.name && reply[3].toString().isEmpty();}),15000);
    QJsonArray questions;
    for(int i=0;i<3;++i)questions<<QJsonObject{{"id",QString("q_%1").arg(i)},{"header",QString("Choice %1").arg(i+1)},
        {"question","Choose the intended behavior when the requested font weight is unavailable and the text uses the regular face."},{"allow_other",true},
        {"options",QJsonArray{QJsonObject{{"id",QString("opt_%1_0").arg(i)},{"label","Show a warning (Recommended)"},{"description","Report the missing font weight once for each family and weight, while keeping the current font files."}},
            QJsonObject{{"id",QString("opt_%1_1").arg(i)},{"label","Include another font face"},{"description","Ship an additional font file with the application."}}}}};
    QJsonObject question{{"question_id","question-one"},{"question_hash","hash-one"},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"can_answer",false},{"answer_unavailable_reason","The visible Claude question differs from this request"},{"questions",questions}};
    QJsonObject details{{"tracked",true},{"conversation_id","conversation-one"},{"run_id","run-one"},{"process_state","running"},{"runtime_state","live"},
        {"activity","waiting"},{"phase","input"},{"pending_questions",QJsonArray{question}},{"session_usage",QJsonObject{{"context",QJsonObject{{"used",236000}}}}}};
    client->inspectionReady({},session.name,details);
    auto *card=window.findChild<QuestionCard *>("questionCard");QTRY_VERIFY(!card->isDraftLoading());auto *tabs=card->findChild<QTabBar *>("questionTabs");
    for(int i=0;i<3;++i){tabs->setCurrentIndex(i);for(auto *option:card->findChildren<QAbstractButton *>("questionOption"))if(option->property("optionId")==QString("opt_%1_0").arg(i))option->click();}
    auto *submit=card->findChild<QPushButton *>("submitQuestionAnswer");QVERIFY(!submit->isEnabled());
    // A refreshed capability unlocks submission without rebuilding the user's answers.
    question["can_answer"]=true;question["answer_unavailable_reason"]="";details["pending_questions"]=QJsonArray{question};
    client->inspectionReady({},session.name,details);QVERIFY(submit->isEnabled());QCOMPARE(tabs->currentIndex(),2);
    QCOMPARE(card->findChild<QLabel *>("questionProgress")->text(),QString("3 of 3 answered"));
    auto *read=window.findChild<QPushButton *>("activityMarkRead");auto *context=window.findChild<QPushButton *>("activityContext");
    QTest::qWait(30);QVERIFY(card->isVisible());QVERIFY(read->isVisible());
    const auto bounds=[&](QWidget *widget){return QRect(widget->mapTo(&window,QPoint()),widget->size());};
    QVERIFY(bounds(read).top()-bounds(card).bottom()>=8);QVERIFY(bounds(context).top()-bounds(card).bottom()>=8);
    QVERIFY(bounds(read).right()<bounds(context).left());QVERIFY(bounds(read).bottom()<window.height());
    const auto preview=qEnvironmentVariable("HGS_QUESTION_FOOTER_PREVIEW");if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+QString("/question-footer-%1-%2.png").arg(theme).arg(size.height())));}
}

void TestSessionsWindow::startupTrustAnswerBeforeConversation_data()
{
    QTest::addColumn<bool>("nullConversation"); QTest::addColumn<QString>("choice"); QTest::addColumn<QString>("provider"); QTest::addColumn<bool>("permissionMode");
    QTest::newRow("null-trust") << true << QString("trust_0") << QString("kimi") << false;
    QTest::newRow("codex-null-trust") << true << QString("trust_0") << QString("codex") << false;
    QTest::newRow("codex-empty-decline") << false << QString("trust_1") << QString("codex") << false;
    QTest::newRow("claude-null-trust") << true << QString("trust_0") << QString("claude") << false;
    QTest::newRow("null-decline") << true << QString("trust_1") << QString("kimi") << false;
    QTest::newRow("claude-null-decline") << true << QString("trust_1") << QString("claude") << false;
    QTest::newRow("empty-trust") << false << QString("trust_0") << QString("kimi") << false;
    QTest::newRow("claude-empty-trust") << false << QString("trust_0") << QString("claude") << false;
    QTest::newRow("empty-decline") << false << QString("trust_1") << QString("kimi") << false;
    QTest::newRow("claude-empty-decline") << false << QString("trust_1") << QString("claude") << false;
    QTest::newRow("claude-permission-auto") << true << QString("mode_0") << QString("claude") << true;
    QTest::newRow("claude-permission-keep") << false << QString("mode_1") << QString("claude") << true;
}

void TestSessionsWindow::startupTrustAnswerBeforeConversation()
{
    QFETCH(bool, nullConversation); QFETCH(QString, choice); QFETCH(QString, provider); QFETCH(bool, permissionMode);
    QTemporaryDir directory;
    const QString hash(64, QChar('a')), id = (permissionMode ? "claude-permissions:" : provider + "-trust:") + hash;
    const QString item = permissionMode ? "permission_mode" : "trust";
    const QJsonValue conversation = nullConversation ? QJsonValue(QJsonValue::Null) : QJsonValue(permissionMode ? "started-conversation" : "");
    const QJsonObject question{{"question_id", id}, {"question_hash", hash}, {"run_id", "startup-run"},
        {"conversation_id", conversation}, {"can_answer", true}, {"source", permissionMode ? "claude_permission_mode" : provider + "_folder_trust"},
        {"answer_transport", provider + "_tui"}, {"questions", QJsonArray{QJsonObject{{"id", item},
            {"question", permissionMode ? "Make auto mode your default permission mode?" : "Trust this folder?"}, {"body", "/work/example\nProject MCP targets:\ntracker (stdio): command=example"},
            {"allow_other", false}, {"options", QJsonArray{
                QJsonObject{{"id", permissionMode ? "mode_0" : "trust_0"}, {"label", permissionMode ? "Yes, set auto mode as my default permission mode" : "Trust this folder"}},
                QJsonObject{{"id", permissionMode ? "mode_1" : "trust_1"}, {"label", permissionMode ? "No, keep bypass permissions" : "Don't trust"}}}}}}}};
    QFile snapshot(directory.filePath("inspect.json")); QVERIFY(snapshot.open(QIODevice::WriteOnly));
    snapshot.write(QJsonDocument(QJsonObject{{"tracked", true}, {"run_id", "startup-run"},
        {"conversation_id", conversation}, {"runtime_state", "live"}, {"process_state", "running"},
        {"activity", "busy"}, {"phase", "approval"}, {"pending_questions", QJsonArray{question}},
        {"events", QJsonArray{}}, {"cursor", 0}}).toJson()); snapshot.close();
    QFile program(directory.filePath("hgs")); QVERIFY(program.open(QIODevice::WriteOnly));
    program.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
if sys.argv[1]=='inspect':
 data=json.loads((root/'inspect.json').read_text())
 if (root/'answer.json').exists(): data['pending_questions']=[]
 print(json.dumps(data))
elif sys.argv[1]=='answer':
 p=json.load(sys.stdin);(root/'answer.json').write_text(json.dumps(p))
 with (root/'calls').open('a') as f:f.write('call\n')
 print(json.dumps({'status':'answered','request_id':p['request_id'],'name':sys.argv[2],
  'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],
  'question_id':p['question_id'],'question_hash':p['expected_question_hash']}))
)PY"); program.close();
    QVERIFY(FakeCli::makeRunnable(program.fileName()));
    SessionsWindow window(FakeCli::program(program.fileName())); window.setFleet(fleet()); window.show(); window.showSession({}, "kimi/docs/research");
    auto *card = window.findChild<QuestionCard *>("questionCard"); auto *client = window.findChild<HgsClient *>();
    QSignalSpy answered(client, &HgsClient::questionAnswered), failed(client, &HgsClient::questionAnswerFailed);
    QTRY_VERIFY(card->isVisible()); QTRY_VERIFY(!card->isDraftLoading());
    auto *submit = card->findChild<QPushButton *>("submitQuestionAnswer");
    QVERIFY(!submit->isEnabled()); // Native highlighted option is not consent.
    QAbstractButton *selected = nullptr;
    for (auto *button : card->findChildren<QAbstractButton *>("questionOption")) {
        QVERIFY(!button->isChecked());
        if (button->property("optionId").toString() == choice) selected = button;
    }
    QVERIFY(selected); selected->click(); QVERIFY(submit->isEnabled()); submit->click();
    QTRY_COMPARE(answered.size(), 1); QCOMPARE(failed.size(), 0);
    QFile sent(directory.filePath("answer.json")); QVERIFY(sent.open(QIODevice::ReadOnly));
    const auto payload = QJsonDocument::fromJson(sent.readAll()).object();
    QCOMPARE(payload["expected_run_id"].toString(), "startup-run");
    QVERIFY(payload["expected_conversation_id"].isString());
    QCOMPARE(payload["expected_conversation_id"].toString(), conversation.toString());
    QCOMPARE(payload["question_id"].toString(), id); QCOMPARE(payload["expected_question_hash"].toString(), hash);
    const QJsonArray expected{QJsonObject{{"question_id", item},
        {"selected_option_ids", QJsonArray{choice}}, {"text", ""}}};
    QCOMPARE(payload["answers"].toArray(), expected);
    QTRY_VERIFY(!card->isVisible());
    QFile calls(directory.filePath("calls")); QVERIFY(calls.open(QIODevice::ReadOnly)); QCOMPARE(calls.readAll(), QByteArray("call\n"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_questions.moc"
