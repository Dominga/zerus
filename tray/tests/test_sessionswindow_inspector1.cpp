// U62 (#712): The inspector: its cards, tasks, processes and subagents, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void sessionIdentifiersRemainInDetailsAcrossNavigation();
    void gitPublicationChecksKeepDraftFocusAndSharedCards();
    void subagentNoteNeverHidesTheSessionHint();
    void inspectorIsAColumnOfCards_data();
    void inspectorIsAColumnOfCards();
    void liveStreamFollowsGitFoldersInsteadOfPolling();
    void usageLimitBecomesToolbarChip();
    void earlyQuotaResetExplainsPendingNativeChoiceAndKeepsDraft();
    void openingChildClearsHiddenAccountUsageRecoveryNotice();
    void backgroundShellRowKeepsActiveAndOpensItsProcesses();
    void processPollingFollowsVisibilityAndSettings();
    void tasksTabFollowsTheOpenSessionsProject();
    void tasksDropOnTheirProjectsSessionsAndHeader();
    void separatesConversationProcessAndTerminal();
};

void TestSessionsWindow::sessionIdentifiersRemainInDetailsAcrossNavigation()
{
    QSettings().setValue("workspace/inspectorVisible", true); QSettings().setValue("workspace/inspectorTab", 1);
    auto state = fleet(); auto box = state.local();
    box.ok = false; box.sessions[0].conversationId = "12345678-9abc-4def-8123-456789abcdef";
    box.sessions[1].conversationId.clear(); box.sessions[1].runId = "unconfirmed-run";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.resize(820, 700); window.setFleet(state); window.show();
    window.showSession({}, box.sessions[0].name);
    QVERIFY(!window.findChild<QLabel *>("sessionId"));
    auto *value = window.findChild<QLabel *>("detailsIdentityValue"); QVERIFY(value);
    auto *title = window.findChild<QLabel *>("detailsIdentityTitle"); QVERIFY(title);
    auto *copy = window.findChild<QPushButton *>("copySessionIdAction"); QVERIFY(copy); QTRY_VERIFY(copy->isVisible());
    QVERIFY(window.findChild<QWidget *>("sessionInspector")->isAncestorOf(value));
    const auto copyFits = [&] {
        for (auto *parent = copy->parentWidget(); parent; parent = parent->parentWidget())
            if (!parent->rect().contains(QRect(copy->mapTo(parent, QPoint()), copy->size()))) return false;
        return true;
    };
    QTRY_VERIFY(copyFits());
    window.resize(1280, 700);
    window.findChild<QSplitter *>("sessionWorkSplitter")->setSizes({600, 420});
    QTRY_VERIFY(value->width() >= value->fontMetrics().horizontalAdvance(value->text()));
    QTRY_VERIFY(copyFits());
    // #542: at 290 px (QA #500 finding 2) and the narrowest inspector the ID
    // never overlaps its title or the copy button: it takes a line of its own
    // and shows whole while that line is long enough.
    auto *row = window.findChild<QWidget *>("detailsIdentity"); QVERIFY(row);
    const auto apart = [&] {
        const auto rect = [row](QWidget *widget) { return QRect(widget->mapTo(row, QPoint()), widget->size()); };
        return !rect(value).intersects(rect(copy)) && (!title->isVisible() || !rect(value).intersects(rect(title)));
    };
    auto *splitter = window.findChild<QSplitter *>("sessionWorkSplitter");
    const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    for (const int width : {420, 290, 240}) {
        splitter->setSizes({splitter->width() - width, width});
        QTRY_VERIFY(apart()); QTRY_VERIFY(copyFits());
        QTRY_VERIFY(value->width() >= qMin(value->fontMetrics().horizontalAdvance(value->text()), row->width()));
        if (title->isVisible()) QVERIFY(value->y() >= copy->y() + copy->height());
        if (!shots.isEmpty()) QVERIFY(window.findChild<QWidget *>("sessionInspectorPanel")->grab().save(QDir(shots).filePath(QString("u542-session-id-%1.png").arg(width))));
    }
    splitter->setSizes({600, 420});
    QTRY_VERIFY(value->width() >= value->fontMetrics().horizontalAdvance(value->text()));
    QCOMPARE(title->text(), QString("Session ID")); QCOMPARE(value->text(), box.sessions[0].conversationId);
    QVERIFY(value->textInteractionFlags().testFlag(Qt::TextSelectableByMouse));
    QCOMPARE(copy->accessibleName(), QString("Copy session ID"));
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Keep this draft");
    QSignalSpy copied(&window, &SessionsWindow::copyTextRequested);
    copy->click(); QCOMPARE(copied.size(), 1); QCOMPARE(copied[0][0].toString(), box.sessions[0].conversationId);
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft"));
    // The header icon copies the same identity without showing it outside Details.
    auto *headerCopy = window.findChild<QPushButton *>("headerCopySessionIdAction"); QVERIFY(headerCopy); QVERIFY(headerCopy->isVisible());
    QVERIFY(!window.findChild<QWidget *>("sessionInspector")->isAncestorOf(headerCopy));
    QCOMPARE(headerCopy->accessibleName(), QString("Copy session ID"));
    headerCopy->click(); QCOMPARE(copied.size(), 2); QCOMPARE(copied[1][0].toString(), box.sessions[0].conversationId);
    const auto preview = qEnvironmentVariable("HGS_PREVIEW_DIR");
    if (!preview.isEmpty()) { QDir().mkpath(preview); QVERIFY(window.grab().save(preview + "/copy-session-id.png")); }
    window.showSession({}, box.sessions[1].name);
    QCOMPARE(title->text(), QString("Run ID")); QCOMPARE(value->text(), QString("unconfirmed-run"));
    QCOMPARE(copy->accessibleName(), QString("Copy run ID")); QCOMPARE(headerCopy->accessibleName(), QString("Copy run ID"));
    window.showSession({}, box.sessions[0].name); QCOMPARE(value->text(), box.sessions[0].conversationId);
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft"));
    window.findChild<HgsClient *>()->inspectionReady({}, box.sessions[0].name,
        {{"tracked", true}, {"conversation_id", "new-conversation"}, {"run_id", "new-run"}});
    QCOMPARE(value->text(), QString("new-conversation"));
    headerCopy->click(); QCOMPARE(copied.size(), 3); QCOMPARE(copied[2][0].toString(), QString("new-conversation"));
    box.sessions[1].runId.clear(); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    window.showSession({}, box.sessions[1].name); QCOMPARE(value->text(), QString("Not confirmed")); QVERIFY(!copy->isEnabled());
    QVERIFY(!headerCopy->isEnabled()); copy->click(); headerCopy->click(); QCOMPARE(copied.size(), 3);
}

void TestSessionsWindow::gitPublicationChecksKeepDraftFocusAndSharedCards()
{
    QTemporaryDir fixture; QVERIFY(fixture.isValid());
    const auto program = fixture.filePath("hgs"); QFile file(program); QVERIFY(file.open(QIODevice::WriteOnly));
    file.write("#!/usr/bin/env python3\nimport runpy,sys,time\nif sys.argv[1:2]==['git-status']:\n"
        " open(__file__+'.calls','a').write(' '.join(sys.argv[1:])+'\\n');time.sleep(0.4)\n"
        " print('{\"state\":\"ok\",\"root\":\"/repo\",\"branch\":\"main\",\"upstream\":\"origin/main\",\"changed_files\":4,\"ahead\":2,\"behind\":0,\"remote_state\":\"verified\",\"remote_age_seconds\":0}')\n"
        "else:\n sys.argv[0]=" + FakeCli::pythonString(script()) + ";runpy.run_path(sys.argv[0],run_name='__main__')\n"); file.close();
    QVERIFY(FakeCli::makeRunnable(file.fileName()));
    auto state = fleet(); auto box = state.local();
    for (int i = 0; i < 2; ++i) { box.sessions[i].cwd = "/repo"; box.sessions[i].gitRoot = "/repo"; box.sessions[i].gitBranch = "main"; box.sessions[i].gitMetadataState = "ok"; }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.resize(1120, 680); window.setFleet(state); window.show();
    window.showSession({}, box.sessions[0].name);
    auto *list = window.findChild<SessionList *>("sessionList"); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QVERIFY(list); QVERIFY(composer); setComposerText(composer, "Keep this unsent draft");
    composer->editor()->setFocus(); window.activateWindow(); QTRY_VERIFY(composer->editor()->hasFocus());
    const auto currentKey = list->currentItem()->data(SessionRoles::Key).toString();
    auto count = [&] { int result = 0; for (int i = 0; i < list->count(); ++i)
        if (list->item(i)->data(SessionRoles::GitStatus).toJsonObject().value("changed_files").toInt() == 4) ++result;
        return result; };
    QTRY_COMPARE(count(), 2);
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep this unsent draft")); QVERIFY(composer->editor()->hasFocus());
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), currentKey);
    QFile calls(program + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly)); QCOMPARE(calls.readAll().count('\n'), 1);
    QVERIFY(list->currentItem()->data(Qt::AccessibleDescriptionRole).toString().contains("4 changed or new files"));
    const auto directory = qEnvironmentVariable("HGS_GIT_STATUS_PREVIEW");
    if (!directory.isEmpty()) { QDir().mkpath(directory); QVERIFY(window.grab().save(directory + "/integrated.png")); }
}

// N35 of #499: the record keeps its last subagent note ("allowed with its
// group") for good. It never replaces the session's own hint; only a member's
// ask that waits for its lead says so, in words.
void TestSessionsWindow::subagentNoteNeverHidesTheSessionHint()
{
    auto state = fleet(); auto arch = state.local();
    for (auto &s : arch.sessions) s.subagentConfirm = "allowed with its group";
    state.setLocal(arch, QDateTime::currentMSecsSinceEpoch());
    auto mac = *state.peer("mac"); mac.sessions[0].subagentConfirm = "asked; the team's lead answers";
    state.setPeer(mac, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(script()); keepDetailsPollsOut(window);
    window.setFleet(state); window.show();
    auto *hint = window.findChild<QLabel *>("hint"); QVERIFY(hint);
    window.showSession({}, "codex/website/navigation");
    WAIT_COMPARE(hint->text(), QString("Conversation saved. Resume to continue with the same context."));
    window.showSession({}, "kimi/docs/research");
    WAIT_VERIFY2(!hint->text().contains("with its group"), qPrintable(hint->text()));
    window.showSession("mac", "claude/infra/review");
    WAIT_COMPARE(hint->text(), QString("The agent asks to start subagents. Its team's lead answers."));
}

// G8: the session row reads the team list summary, which now carries each
// task's phase and the phase records. A team like 528aebfe - phase 1 and 31
// finished, 32 in work, 33 and 34 drafts - reads the phase-32 tasks bar and
// 2/5 phases on the row, and the shot shows it.
void TestSessionsWindow::inspectorIsAColumnOfCards_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QString("dark");
    QTest::newRow("light") << QString("light");
}

// U25 part 6 (spec 6): the inspector is one column of collapsible cards -
// Environment, Context, Team, Tasks, Iron rules, Worktrees, Changes - with
// the spec's open/collapsed defaults, a short value on the right of each
// header, and links from outside that unfold the right card.
void TestSessionsWindow::inspectorIsAColumnOfCards()
{
    QFETCH(QString, theme);
    QSettings().setValue("workspace/theme", theme);
    const auto restore = qScopeGuard([] {
        QSettings().remove("workspace/inspectorVisible");
        QSettings settings; settings.beginGroup("ui/collapsed/inspector"); settings.remove(""); settings.endGroup();
    });
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QJsonObject details{{"tracked", true}, {"conversation_id", "5f0c2a1e-7d3b-4c8e-9a61-2b7e4d9c0f13"}, {"run_id", "run-1"},
        {"cwd", "/work/orbit"}, {"last_event_at", 1793050800},
        {"session_usage", QJsonObject{{"status", "ok"}, {"context", QJsonObject{{"used", 72000}, {"limit", 200000}}}}},
        {"task_lists", QJsonObject{{"main", QJsonObject{{"run_id", "run-1"}, {"items", QJsonArray{
            QJsonObject{{"title", "Plan Checkout v2 with the team"}, {"status", "completed"}},
            QJsonObject{{"title", "Hand out the first wave"}, {"status", "completed"}},
            QJsonObject{{"title", "Review #4 Address validation"}, {"status", "in_progress"}},
            QJsonObject{{"title", "Review the next reports"}, {"status", "pending"}},
            QJsonObject{{"title", "Write the release notes"}, {"status", "pending"}}}}}}}}};
    const QJsonObject git{{"state", "ok"}, {"path", "/work/orbit"}, {"branch", "feat/checkout-v2"}, {"changed_files", 3}, {"conflicts", 0},
        {"changes", QJsonObject{{"added", 184}, {"deleted", 37}, {"tracked_files", 3}, {"untracked_files", 0}, {"truncated", false},
            {"files", QJsonArray{QJsonObject{{"path", "services/payments/src/cards.rs"}, {"added", 142}, {"deleted", 30}},
                QJsonObject{{"path", "services/payments/migrations/0043_card_index.sql"}, {"added", 18}, {"deleted", 0}},
                QJsonObject{{"path", "openapi/cards.yaml"}, {"added", 24}, {"deleted", 7}}}}}}};
    const QJsonObject diff{{"state", "ok"}, {"path", "/work/orbit"}, {"diff", "diff --git a/openapi/cards.yaml b/openapi/cards.yaml\n+  saved: true\n"},
        {"truncated", false}, {"bytes", 64}};
    const auto json = [](const QJsonObject &value) { return FakeCli::pythonString(QString::fromUtf8(QJsonDocument(value).toJson(QJsonDocument::Compact))); };
    const QByteArray answers = "elif c=='inspect':print(" + json(details) + ")\n"
        "elif c=='git-status':print(" + json(git) + ")\n"
        "elif c=='git-diff':print(" + json(diff) + ")\n";
    const QString fixture = m_dir->filePath("hgs-inspector-" + theme);
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answers)));
    // A fork build turns extensions on by default (ZERUS_FORK_BUILD); the
    // SocratiCode card is checked on its own below.
    Extensions::setEnabled(QStringLiteral("socraticode"), false);
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 1000); window.setFleet(fleet()); window.show();
    window.showSession({}, "codex/hgs/dashboard");
    window.findChild<QPushButton *>("toggleInspector")->setChecked(true);
    auto *inspector = qobject_cast<QScrollArea *>(window.findChild<QWidget *>("sessionInspector"));
    QVERIFY(inspector); QVERIFY(!inspector->findChild<QTabBar *>());
    QVERIFY(!window.findChild<QTabWidget *>("sessionInspector"));
    // The cards in the spec's order with its defaults, then Project tasks (upstream's Tasks tab); SocratiCode only with its extension.
    QStringList order;
    for (auto *card : inspector->findChildren<QFrame *>("inspectorCard")) if (!card->isHidden()) order << card->property("card").toString();
    QCOMPARE(order, (QStringList{"environment", "context", "team", "tasks", "rules", "worktrees", "changes", "projecttasks"}));
    for (const char *open : {"environment", "context", "team", "tasks"}) QVERIFY2(inspectorCardOpen(window, open), open);
    for (const char *closed : {"rules", "worktrees", "changes", "projecttasks"}) QVERIFY2(!inspectorCardOpen(window, closed), closed);
    // Environment: the Session ID row with its copy button, then the rows.
    const auto row = [&window](const char *name) {
        for (auto *value : window.findChildren<QLabel *>("inspectorEnvValue")) if (value->property("row") == name) return value->text();
        return QString();
    };
    auto *environment = inspectorCard(window, "environment");
    QTRY_COMPARE_WITH_TIMEOUT(environment->findChild<QLabel *>("detailsIdentityValue")->text(), QString("5f0c2a1e-7d3b-4c8e-9a61-2b7e4d9c0f13"), 15000);
    QVERIFY(environment->findChild<QPushButton *>("copySessionIdAction")->isVisible());
    QTRY_COMPARE_WITH_TIMEOUT(row("branch"), QString("feat/checkout-v2"), 15000);
    QVERIFY2(row("machine").startsWith("arch") || row("machine").contains("this computer"), qPrintable(row("machine")));
    QVERIFY2(row("folder").endsWith("/work/orbit"), qPrintable(row("folder")));
    QVERIFY(!row("model").isEmpty());
    QCOMPARE(row("last event"), QDateTime::fromSecsSinceEpoch(1793050800).toLocalTime().toString("d MMM HH:mm"));
    // Keep cache warm: the 1-8 h slider shows while the switch is on.
    auto *warm = environment->findChild<QCheckBox *>("keepCacheWarm"); auto *hours = environment->findChild<QSlider *>("keepCacheWarmHours");
    QVERIFY(warm && hours); QCOMPARE(hours->minimum(), 1); QCOMPARE(hours->maximum(), 8);
    QVERIFY(!hours->isVisible());
    // Context: only what hgs reports.
    auto *context = inspectorCard(window, "context");
    QTRY_COMPARE_WITH_TIMEOUT(context->findChild<QLabel *>("inspectorContextUsed")->text(), QString("36% used"), 15000);
    QCOMPARE(context->findChild<QLabel *>("inspectorContextTokens")->text(), QString("72k / 200k"));
    QCOMPARE(context->findChild<QLabel *>("inspectorContextLeft")->text(), QString("128k left in the window"));
    QVERIFY(!context->findChild<QWidget *>("inspectorContextRows")->isVisible());
    QVERIFY(context->findChild<QPushButton *>("clearSessionFromDetails")->isVisible());
    // Tasks: "N of M completed" on the header.
    QTRY_COMPARE_WITH_TIMEOUT(inspectorCard(window, "tasks")->findChild<QLabel *>("inspectorCardValue")->text(), QString("2 of 5 completed"), 15000);
    // Changes, collapsed: "+184 −37" on the header.
    auto *changes = inspectorCard(window, "changes");
    QTRY_VERIFY_WITH_TIMEOUT(changes->findChild<QLabel *>("inspectorCardValue")->isVisible(), 15000);
    QCOMPARE(changes->findChild<QLabel *>("inspectorCardValue")->text(), QString("+184 −37"));
    const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    const auto shot = [&](const QString &name) {
        if (shots.isEmpty()) return;
        QTest::qWait(150);
        QVERIFY(window.findChild<QFrame *>("sessionInspectorPanel")->grab().save(QDir(shots).filePath(name + "-" + theme + ".png")));
    };
    shot("ux-6-inspector-top");
    // #648: the Tasks card as the prototype: "Main agent · <state word>", the
    // plan as a checklist, one muted subagent line and one Reports to row.
    {auto *tasks = inspectorCard(window, "tasks");
     const auto text = tasks->findChild<QTextBrowser *>("subagents")->toPlainText();
     QVERIFY2(text.contains("Main agent · "), qPrintable(text.left(300)));
     QVERIFY(text.contains("Plan Checkout v2 with the team")); QVERIFY(text.contains("Planned")); QVERIFY(text.contains("Done"));
     QVERIFY(!text.contains("TASKS")); QVERIFY(!text.contains("No task list reported")); QVERIFY(!text.contains("SUBAGENTS"));
     QVERIFY(text.contains("No subagent activity yet"));
     QVERIFY(!tasks->findChild<QPushButton *>("linkSessionLead"));
     if (auto *lead = tasks->findChild<QComboBox *>("sessionLeadChoice")) {
         QCOMPARE(lead->itemText(0), QString("nobody")); QCOMPARE(lead->accessibleName(), QString("Lead session"));
         auto *label = tasks->findChild<QLabel *>("sessionLeadLabel"); QVERIFY(label); QCOMPARE(label->text(), QString("Reports to"));
     }
     if (!shots.isEmpty()) { QTest::qWait(150); QVERIFY(tasks->grab().save(QDir(shots).filePath("648-tasks-card-" + theme + ".png"))); }}
    // Links from outside unfold the right card: the git chip opens Changes,
    // N iron rules opens Iron rules, the context indicator opens Context.
    emit window.findChild<SessionList *>("sessionList")->gitActivated("arch\ncodex/hgs/dashboard");
    QTRY_VERIFY_WITH_TIMEOUT(inspectorCardOpen(window, "changes"), 15000);
    QTRY_COMPARE_WITH_TIMEOUT(changes->findChildren<QWidget *>("inspectorChangeRow").size(), 3, 15000);
    QCOMPARE(changes->findChildren<QLabel *>("inspectorChangeAdded").first()->text(), QString("+142"));
    QCOMPARE(changes->findChildren<QLabel *>("inspectorChangeDeleted").first()->text(), QString("−30"));
    emit window.findChild<MessageComposer *>("messageComposer")->ironRulesRequested();
    QTRY_VERIFY_WITH_TIMEOUT(inspectorCardOpen(window, "rules"), 15000);
    // Folded cards keep their value: Context shows its share only while folded.
    auto *contextValue = context->findChild<QLabel *>("inspectorCardValue");
    QVERIFY(!contextValue->isVisible());
    for (const char *fold : {"environment", "context", "team"}) inspectorCard(window, fold)->findChild<QPushButton *>("collapsibleHeader")->click();
    QVERIFY(!inspectorCardOpen(window, "context"));
    QTRY_VERIFY_WITH_TIMEOUT(contextValue->isVisible(), 15000); QCOMPARE(contextValue->text(), QString("36%"));
    QTest::qWait(50); inspector->verticalScrollBar()->setValue(0);
    shot("ux-6-inspector-collapsed");
    window.findChild<QPushButton *>("activityContext")->click();
    QTRY_VERIFY_WITH_TIMEOUT(inspectorCardOpen(window, "context"), 15000);
    // Commit changes… fills the composer and never sends; Open diff shows the diff.
    auto *editor = window.findChild<MessageComposer *>("messageComposer")->editor(); editor->clear();
    changes->findChild<QPushButton *>("inspectorCommitChanges")->click();
    QCOMPARE(editor->toPlainText(), QString("Commit the current changes with a short message."));
    changes->findChild<QPushButton *>("inspectorOpenDiff")->click();
    QDialog *dialog = nullptr;
    QTRY_VERIFY_WITH_TIMEOUT((dialog = window.findChild<QDialog *>("inspectorDiffDialog")), 15000);
    QVERIFY(dialog->findChild<QPlainTextEdit *>("inspectorDiffText")->toPlainText().contains("+  saved: true"));
    QVERIFY(dialog->findChild<QPlainTextEdit *>("inspectorDiffText")->isReadOnly());
    dialog->close();
    // The state of every card is kept.
    QVERIFY(Settings::value("ui/collapsed/inspector/changes").toBool());
    QVERIFY(!Settings::value("ui/collapsed/inspector/environment", true).toBool());
}

void TestSessionsWindow::liveStreamFollowsGitFoldersInsteadOfPolling()
{
    // While the computer's event stream is live, its sessions' checkout folders
    // follow the stream (git topic) and no git-status child runs; without it
    // the window polls them again.
    QTemporaryDir fixture; QVERIFY(fixture.isValid());
    const auto program = fixture.filePath("hgs"); QFile file(program); QVERIFY(file.open(QIODevice::WriteOnly));
    file.write(R"PY(#!/usr/bin/env python3
import json, runpy, sys
args = sys.argv[1:]
if args[:1] == ['git-status']:
    open(__file__ + '.calls', 'a').write(' '.join(args) + '\n')
    print(json.dumps({'state': 'ok', 'root': '/repo', 'branch': 'main', 'changed_files': 9, 'ahead': 0, 'behind': 0,
                      'remote_state': 'verified', 'remote_age_seconds': 0}))
    sys.exit(0)
if args == ['--help']:
    print('usage: hgs ...\n       Event stream ABI: watch-v1')
    sys.exit(0)
if args == ['watch', '--json']:
    seq = 0
    def send(t, **fields):
        global seq
        seq += 1
        print(json.dumps(dict(v=1, seq=seq, t=t, **fields)), flush=True)
    send('hello', host='fixture', topics=['catalog', 'catalog.meta', 'activity', 'git'])
    send('snapshot', topic='catalog', items=[], page=0, last=True, cursor=1)
    send('snapshot', topic='catalog.meta', value={'host': 'fixture', 'ok': True}, page=0, last=True, cursor=1)
    for line in sys.stdin:
        op = json.loads(line)
        if op.get('topic') == 'git' and op.get('op') == 'subscribe':
            open(__file__ + '.subscribed', 'a').write(op['path'] + '\n')
            send('state', topic='git', key=op['path'], rev=seq, value={'state': 'ok', 'path': op['path'], 'root': op['path'],
                 'branch': 'main', 'changed_files': 4, 'ahead': 0, 'behind': 0, 'remote_state': 'verified',
                 'remote_checked_at': 100.0, 'remote_age_seconds': 0.0})
    sys.exit(0)
)PY" "sys.argv[0] = " + FakeCli::pythonString(script()) + "\nrunpy.run_path(sys.argv[0], run_name='__main__')\n"); file.close();
    QVERIFY(FakeCli::makeRunnable(file.fileName()));
    HgsClient::setGitStatusIntervalForTests(300);
    EventStreams streams(FakeCli::program(program)); streams.setHosts({QString()});
    QTRY_VERIFY(streams.live({}));
    auto state = fleet(); auto box = state.local();
    for (int i = 0; i < 2; ++i) { box.sessions[i].cwd = "/repo"; box.sessions[i].gitRoot = "/repo"; box.sessions[i].gitBranch = "main"; box.sessions[i].gitMetadataState = "ok"; }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.resize(1120, 680); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    auto count = [&](int changed) { int result = 0; for (int i = 0; i < list->count(); ++i)
        if (list->item(i)->data(SessionRoles::GitStatus).toJsonObject().value("changed_files").toInt() == changed) ++result;
        return result; };
    QTRY_COMPARE(count(4), 2);
    QVERIFY(streams.coversGit({}, "/repo"));
    QFile subscribed(program + ".subscribed"); QVERIFY(subscribed.open(QIODevice::ReadOnly));
    QCOMPARE(subscribed.readAll(), QByteArray("/repo\n"));
    // Rebuilds past the poll interval start no child for a followed folder.
    QTest::qWait(400); window.setFleet(state); QTest::qWait(300);
    QVERIFY(!QFile::exists(program + ".calls"));
    QCOMPARE(count(4), 2);
    // The stream stops: the next rebuild polls the folder again.
    streams.stream({})->stop();
    QVERIFY(!streams.coversGit({}, "/repo"));
    QTest::qWait(400); window.setFleet(state);
    QTRY_COMPARE(count(9), 2);
    QFile calls(program + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly));
    QCOMPARE(calls.readAll(), QByteArray("git-status --path /repo --json\n"));
    HgsClient::setGitStatusIntervalForTests(15000);
}

void TestSessionsWindow::usageLimitBecomesToolbarChip()
{
    SessionsWindow window(program());window.resize(1100,760);window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<AccountUsageStore *>()->findChild<HgsClient *>();
    auto *chip=window.findChild<ToolbarChip *>("usageLimitChip");QVERIFY(chip);QVERIFY(chip->isHidden());
    const auto now=QDateTime::currentSecsSinceEpoch();
    client->accountsReady(1,{},QJsonObject{{"id","native-codex"},{"status","ok"},{"windows",QJsonArray{
        QJsonObject{{"window_minutes",300},{"used_percent",100},{"resets_at",now+3600}}}}});
    QTRY_VERIFY(chip->isVisible());QVERIFY(chip->fullLabel().startsWith("Limit reached"));QCOMPARE(chip->tone(),ChipTone::Danger);
    QVERIFY(chip->toolTip().contains("Resets in"));
    QTest::mouseClick(chip,Qt::LeftButton);QTRY_VERIFY(chip->popover()->isVisible());
    QVERIFY(window.findChild<QLabel *>("activityUsageWarning")->text().contains("Resets in"));
    auto *refresh=window.findChild<QPushButton *>("usageLimitRefresh");QVERIFY(refresh->isVisible());refresh->click();
    QVERIFY(static_cast<AccountUsage::RefreshButton *>(window.findChild<QPushButton *>("sessionUsageRefresh"))->isRefreshing());
    // Routine re-rendering (refresh, polling inspections) keeps the open popover.
    QVERIFY(chip->popover()->isVisible());
    window.findChild<HgsClient *>()->inspectionReady({},"codex/hgs/dashboard",{{"tracked",true},{"conversation_id","conversation-one"}});
    QVERIFY(chip->popover()->isVisible());
    // Switching session closes the popover rather than showing another account's limit.
    window.showSession("mac","claude/infra/review");QVERIFY(!chip->popover()->isVisible());
}

void TestSessionsWindow::earlyQuotaResetExplainsPendingNativeChoiceAndKeepsDraft()
{
    const auto now=QDateTime::currentSecsSinceEpoch();const QString hash(64,'a');
    const auto quotaWindow=[now](int minutes,qint64 resetIn){return QJsonObject{{"window_minutes",minutes},{"used_percent",0},{"resets_at",double(now+resetIn)}};};
    auto state=fleet();auto box=state.local();auto &session=box.sessions[0];
    session.cmd="claude";session.runId="run-one";session.conversationId="conversation-one";session.accountId="native-claude";state.setLocal(box,0);
    const QJsonObject card{{"source","claude_usage_limit"},{"question_id","claude-usage-limit:"+hash},{"question_hash",hash},
        {"run_id","run-one"},{"conversation_id","conversation-one"},{"can_answer",true},{"answer_transport","claude_tui"},
        {"questions",QJsonArray{QJsonObject{{"id","usage_limit"},{"question","What do you want to do?"},{"header","Usage limit"},
            {"options",QJsonArray{QJsonObject{{"id","limit_0"},{"label","Stop and wait for limit to reset"}},
                QJsonObject{{"id","limit_1"},{"label","Switch to usage credits"}}}}}}}};
    QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"runtime_state","live"},{"process_state","running"},{"last_event_at",double(now-60)},{"provider_status_at",double(now-60)},
        {"phase","input"},{"activity","busy"},{"attention_id","claude-usage-limit:"+hash},
        {"provider_error",QJsonObject{{"error_kind","quota"},{"at",double(now-60)},{"detail","Weekly limit reached"}}},
        {"pending_questions",QJsonArray{card}},{"events",QJsonArray{}},{"cursor",0}};
    const QJsonObject usage{{"id","native-claude"},{"status","ok"},{"checked_at",double(now)},
        {"windows",QJsonArray{quotaWindow(300,3600),quotaWindow(10080,86400)}}};
    SessionsWindow window(programInspecting("claude-limit-reset",details,{},usage));window.setFleet(state);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer,"Continue after my reset");
    auto *chip=window.findChild<ToolbarChip *>("usageLimitChip");
    QTRY_VERIFY(chip->isVisible());QCOMPARE(chip->fullLabel(),QString("Usage available"));QCOMPARE(chip->tone(),ChipTone::Neutral);
    QVERIFY(chip->toolTip().contains("Answer any open usage-limit choice"));
    auto *question=window.findChild<QuestionCard *>("questionCard");QVERIFY(question->isVisible());
    const auto options=question->findChildren<QAbstractButton *>("questionOption");QCOMPARE(options.size(),2);
    for(const auto *option:options)QVERIFY(!option->isChecked());
    QVERIFY(!composer->findChild<QPushButton *>("sendMessage")->isEnabled());
    QCOMPARE(composer->editor()->toPlainText(),QString("Continue after my reset"));
}

void TestSessionsWindow::openingChildClearsHiddenAccountUsageRecoveryNotice()
{
    const auto now=QDateTime::currentSecsSinceEpoch();
    auto state=fleet();auto box=state.local();auto &session=box.sessions[0];
    session.cmd="claude";session.runId="run-one";session.conversationId="conversation-one";session.accountId="native-claude";state.setLocal(box,0);
    const QJsonObject children{{"review",QJsonObject{{"name","Review"},{"state","working"}}}};
    const QJsonObject details{{"tracked",true},{"run_id","run-one"},{"conversation_id","conversation-one"},
        {"runtime_state","live"},{"process_state","running"},{"last_event_at",double(now-60)},{"provider_status_at",double(now-60)},
        {"phase","error"},{"activity","attention"},{"subagents",children},
        {"provider_error",QJsonObject{{"error_kind","quota"},{"at",double(now-60)},{"detail","Weekly limit reached"}}},
        {"events",QJsonArray{}},{"cursor",0}};
    const QJsonObject child{{"parent_conversation_id","conversation-one"},{"conversation_id","child-conversation"},
        {"run_id","run-one"},{"events",QJsonArray{}}};
    const QJsonObject usage{{"id","native-claude"},{"status","ok"},{"checked_at",double(now)},
        {"windows",QJsonArray{QJsonObject{{"window_minutes",300},{"used_percent",0},{"resets_at",double(now+3600)}},
            QJsonObject{{"window_minutes",10080},{"used_percent",0},{"resets_at",double(now+86400)}}}}};
    SessionsWindow window(programInspecting("quota-child-switch",details,child,usage));window.setFleet(state);window.show();window.showSession({},session.name);
    auto *help=window.findChild<QLabel *>("providerErrorHelp");
    QTRY_VERIFY(help->text().contains("Latest account usage is below the limit"));
    auto *stack=window.findChild<QStackedWidget *>("activityStack");
    window.findChild<QTextBrowser *>("subagents")->anchorClicked(QUrl("hgs-agent:review"));
    QCOMPARE(stack->currentIndex(),1);
    QVERIFY(window.findChild<ToolbarChip *>("usageLimitChip")->isHidden());
    QVERIFY2(!help->text().contains("Latest account usage is below the limit"),qPrintable(help->text()));
    QVERIFY(help->text().contains("will not retry automatically"));
    // Returning projects the same fresh parent-account evidence again.
    window.findChild<QPushButton *>("subagentBack")->click();QCOMPARE(stack->currentIndex(),0);
    QTRY_VERIFY(help->text().contains("Latest account usage is below the limit"));
}

void TestSessionsWindow::backgroundShellRowKeepsActiveAndOpensItsProcesses()
{
    // A main Stop can leave two build/check commands running, as in the owner
    // report. Both the list and opened header retain Background until they exit.
    QSettings().setValue("processes/enabled", true);
    auto state = fleet(); auto box = state.local(); auto &s = box.sessions[1];
    s.activity = "idle"; s.phase = "idle"; s.backgroundProcesses = 2;
    s.backgroundStarted = QDateTime::currentSecsSinceEpoch() - 90;
    s.backgroundCommand = "build --wait"; s.scheduledJobs = 1;
    s.conversationId = "background-conversation"; s.replyId = "progress-reply"; s.sharedRead = true;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(script()); window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList");
    QListWidgetItem *row = nullptr;
    for (int i=0;i<list->count();++i) if (list->item(i)->data(SessionRoles::Key).toString()==QString("\n")+s.name) row=list->item(i);
    QVERIFY(row);
    QCOMPARE(row->data(SessionRoles::Status).toString(),QString("Background"));
    QCOMPARE(SessionDelegate::statusOf(list->model()->index(list->row(row),0),true).kind,SessionStatusBadge::Background);
    QVERIFY(row->data(SessionRoles::Working).toBool());
    QVERIFY(!row->data(SessionRoles::Unread).toBool());
    QVERIFY(state.local().sessions[1].unreadReply);
    QVERIFY(!row->data(SessionRoles::Scheduled).toBool());
    QCOMPARE(row->data(SessionRoles::WorkingSince).toDouble(),s.backgroundStarted);
    QVERIFY(row->data(SessionRoles::Detail).toString().contains("build --wait"));
    list->setCurrentItem(row);
    QSignalSpy terminated(&window,&SessionsWindow::sessionActivated);
    QContextMenuEvent event(QContextMenuEvent::Mouse,list->visualItemRect(row).center(),list->viewport()->mapToGlobal(list->visualItemRect(row).center()));
    QApplication::sendEvent(list->viewport(),&event);
    auto *menu=window.findChild<QMenu *>("sessionContextMenu"); QVERIFY(menu);
    auto *action=menu->findChild<QAction *>("contextOpenProcesses"); QVERIFY(action); QVERIFY(action->isEnabled());
    action->trigger(); menu->hide();
    QCOMPARE(window.findChild<QTabWidget *>("sessionDetailTabs")->currentWidget(),window.findChild<ProcessView *>());
    QCOMPARE(terminated.size(),0);
    auto *badge=window.findChild<QLabel *>("badge"); QVERIFY(badge);
    QVERIFY(badge->text().startsWith("Background"));
    QVERIFY(badge->width() >= badge->fontMetrics().horizontalAdvance(badge->text()) + 16);
    const QString preview=qEnvironmentVariable("ZERUS_BACKGROUND_PREVIEW");
    if(!preview.isEmpty())QVERIFY(window.grab().save(preview));
    box.sessions[1].backgroundProcesses=0; box.sessions[1].scheduledJobs=0;
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    for (int i=0;i<list->count();++i) if (list->item(i)->data(SessionRoles::Key).toString()==QString("\n")+s.name) {
        QCOMPARE(list->item(i)->data(SessionRoles::Status).toString(),QString("Ready"));
        QVERIFY(!list->item(i)->data(SessionRoles::Working).toBool());
        QVERIFY(list->item(i)->data(SessionRoles::Unread).toBool());
    }
}

void TestSessionsWindow::processPollingFollowsVisibilityAndSettings()
{
    QTemporaryDir temp;const auto program=temp.filePath("hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
if a and a[0]=='inspect':
 with (root/'inspections.jsonl').open('a') as f:f.write(json.dumps(a)+'\n')
 value=dict(tracked=True,run_id='run',conversation_id='conversation',activity='idle',phase='idle',events=[],cursor=0)
 if '--skip-processes' not in a:value['processes']=dict(active_count=1,items=[dict(id='job',command='sleep 300',status='running',owner='main',capabilities=dict(output=False,stop=False))])
 print(json.dumps(value))
else:print('{}')
# Last, once logged, and only for requests (the Terminal tab's attach does not
# go through ProcessRunner).
if a[:1]!=['a']:
 with (root/'done').open('a') as f:f.write('.\n')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    // Requests launched just before a change may still be starting (slowly on
    // Windows): settle() waits until every launched request has logged, so only
    // requests launched afterwards count.
    std::atomic<int> launches=0;ProcessRunner::setLaunchObserver([&launches]{++launches;});
    const auto observer=qScopeGuard([]{ProcessRunner::setLaunchObserver({});ProcessSettings::setPollFloorForTests(0);});
    const auto done=[&]{QFile file(temp.filePath("done"));return file.open(QIODevice::ReadOnly)?int(file.readAll().count('\n')):0;};
    const auto settle=[&]{QTest::qWait(200);const int launched=launches;QTRY_VERIFY_WITH_TIMEOUT(done()>=launched,10000);};
    SessionsWindow window(FakeCli::program(program));window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    // Shorter polls in the production order, on a 100 ms tick: Processes every 0.2 s
    // (set below), Activity every 1 s, Terminal every 5 s. A wait for a poll that must
    // not happen covers an Activity interval and process start-up; polls that must
    // happen are awaited without a tight limit, however slowly processes start.
    constexpr int activityMs=1000;window.setDetailsPollForTests(100,activityMs);
    auto log=[&]{QList<QJsonArray> result;QFile file(temp.filePath("inspections.jsonl"));if(file.open(QIODevice::ReadOnly))while(!file.atEnd())result<<QJsonDocument::fromJson(file.readLine()).array();return result;};
    auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");QVERIFY(tabs);
    const int processes=tabs->indexOf(window.findChild<ProcessView *>());QVERIFY(processes>=0);
    QTRY_VERIFY(!log().isEmpty());QTest::qWait(200);
    QVERIFY(!tabs->isTabVisible(processes));for(const auto &call:log())QVERIFY(call.contains("--skip-processes"));
    auto *settings=window.findChild<QWidget *>("settingsPage");
    settings->findChild<QDoubleSpinBox *>("activeSeconds")->setValue(1);
    settings->findChild<QDoubleSpinBox *>("backgroundSeconds")->setValue(5);
    QCOMPARE(Settings::value("processes/activeSeconds").toDouble(),1.);
    ProcessSettings::setPollFloorForTests(0.2);QSettings().setValue("processes/activeSeconds",0.2);
    const auto before=log().size();QTRY_VERIFY(log().size()>before);
    QVERIFY(log().last().contains("--skip-processes"));QCOMPARE(tabs->tabText(processes),QString("Processes"));
    auto *enabled=settings->findChild<QCheckBox *>("processesEnabled");QVERIFY(enabled);enabled->setChecked(true);QVERIFY(tabs->isTabVisible(processes));
    // Only the Processes interval can poll the open tab again within 3 s: Activity
    // now waits a minute and Terminal 5 s.
    window.setDetailsPollForTests(100,60000);
    tabs->setCurrentIndex(processes);QTRY_VERIFY(!log().last().contains("--skip-processes"));
    const auto active=log().size();QTRY_VERIFY_WITH_TIMEOUT(log().size()>active,3000);QVERIFY(!log().last().contains("--skip-processes"));
    window.setDetailsPollForTests(100,activityMs);
    tabs->setCurrentIndex(1);settle();const auto terminal=log().size();QTest::qWait(activityMs+400);QCOMPARE(log().size(),terminal);
    for(const auto &call:log())QCOMPARE(call[1].toString(),QString("codex/hgs/dashboard"));
    tabs->setCurrentIndex(0);QTRY_VERIFY(log().size()>terminal); // Activity refreshes immediately on return.
    // Every Activity poll after settle() is at least the background interval, here
    // 0.2 s, after the last Processes poll: it would include processes if enabled.
    QSettings().setValue("processes/backgroundSeconds",0.2);
    tabs->setCurrentIndex(processes);QTest::qWait(200);enabled->setChecked(false);QVERIFY(!tabs->isTabVisible(processes));QCOMPARE(tabs->currentIndex(),0);
    settle();const auto disabled=log().size();QTRY_VERIFY(log().size()>disabled);
    for(const auto &call:log().sliced(disabled))QVERIFY(call.contains("--skip-processes"));
    window.hide();settle();const auto hidden=log().size();QTest::qWait(activityMs+400);QCOMPARE(log().size(),hidden);
}

void TestSessionsWindow::tasksTabFollowsTheOpenSessionsProject()
{
    SessionOrganization org; const auto work=org.createGroup("Work");
    org.moveSession(org.observe("arch","codex/hgs/dashboard",""),work); org.moveSession(org.observe("arch","kimi/docs/research",""),work);
    setJsonSetting("workspace/organization",QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    // The fake replica: one task in review and two in To do, one pinned to the
    // project's other session.
    QJsonArray tasks;
    for (const auto &[id, stage, session] : QList<std::tuple<QString, QString, QString>>{{"T-000001","review",""},{"T-000002","todo","kimi/docs/research"},{"T-000003","todo",""}}) {
        QJsonObject task{{"uuid",id},{"id",id},{"ref","Work/"+id},{"text","Task "+id},{"stage",stage},{"summary",""},{"cancel_reason",""},
            {"cancelled_at",QJsonValue()},{"created_at",1},{"stage_at",1},{"writable",true},{"author",QJsonObject{{"kind","person"}}},
            {"target",session.isEmpty()?QJsonObject{{"kind","any"}}:QJsonObject{{"kind","session"},{"machine","m"},{"session",session},{"host",""}}},
            {"session",QJsonValue()},{"rank","V"}};
        tasks<<task;
    }
    const QString fixture = tasksFixture("tasks-hgs", tasks); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.setFleet(fleet()); window.show();
    window.showSession({}, "codex/hgs/dashboard");
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); auto *view = window.findChild<TasksView *>();
    QVERIFY(tabs && view); QCOMPARE(tabs->indexOf(view), -1); QVERIFY(!view->isVisible());
    for (int i = 0; i < tabs->count(); ++i) QVERIFY(tabs->tabText(i).compare("Task board", Qt::CaseInsensitive) != 0);
    auto *board = window.findChild<TaskInspector *>(); QVERIFY(board);
    // WhiteWh fork: the inspector is a column of cards; the board is the Project tasks card.
    auto *side = window.findChild<QWidget *>("sessionInspector"); QVERIFY(side);
    if (!side->isVisible()) window.findChild<QAbstractButton *>("toggleInspector")->click();
    openInspectorCard(window, "projecttasks"); QTRY_VERIFY(board->isVisible());
    QTRY_COMPARE(board->displayedCount(), 1); // Review is the initial stage.
    QFile calls(fixture + ".tasks"); QVERIFY(calls.open(QIODevice::ReadOnly));
    QCOMPARE(QJsonDocument::fromJson(calls.readLine()).array(), QJsonArray({"task", "list", "--project", work}));
    board->selectTask("T-000002"); QTRY_COMPARE(board->selectedStage(), QString("todo"));
    auto *row = board->findChild<TaskRow *>("taskInspectorRow:T-000002"); QTRY_VERIFY(row);
    QVERIFY(row->findChild<QToolButton *>("taskAction:sendNow")->isEnabled());
    auto *messages = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(messages);
    // The agent's own plan has its own card (the fork's Tasks card), apart from the project's tasks.
    QVERIFY(inspectorCard(window, "tasks")->isAncestorOf(window.findChild<QTextBrowser *>("subagents")));
    QVERIFY(inspectorCard(window, "projecttasks")->isAncestorOf(board) && !inspectorCard(window, "tasks")->isAncestorOf(board));
    messages->editor()->setPlainText("Keep my Activity draft");
    QTest::mouseClick(row, Qt::LeftButton, {}, QPoint(60, row->height() - 15));
    auto *taskView = window.findChild<QWidget *>("taskDetailsPane");
    QTRY_COMPARE(tabs->currentWidget(), taskView); QCOMPARE(tabs->tabText(tabs->indexOf(taskView)), QString("Task View"));
    auto *taskComposer = window.findChild<MessageComposer *>("taskCommentComposer"); QVERIFY(taskComposer && taskComposer->isVisible());
    QVERIFY(taskComposer->findChild<ToolbarChip *>("voiceRecord"));
    QTRY_VERIFY(taskComposer->sessionKey().endsWith("T-000002"));
    taskComposer->editor()->setPlainText("A task comment draft");
    tabs->setCurrentIndex(0); QCOMPARE(messages->editor()->toPlainText(), QString("Keep my Activity draft"));
    tabs->setCurrentWidget(taskView); QCOMPARE(taskComposer->editor()->toPlainText(), QString("A task comment draft"));
    auto *inspector = window.findChild<QWidget *>("sessionInspector");
    QVERIFY(inspectorCardOpen(window, "projecttasks")); QCOMPARE(board->objectName(), QString("taskInspector"));
    QVERIFY(inspector->isVisible()); QCOMPARE(messages->editor()->toPlainText(), QString("Keep my Activity draft"));
    // A session in no project explains where tasks live.
    window.showSession({}, "codex/website/navigation");
    QTRY_VERIFY(view->findChild<QLabel *>("taskEmpty")->text().startsWith("Tasks belong to a project"));
}

void TestSessionsWindow::tasksDropOnTheirProjectsSessionsAndHeader()
{
    SessionOrganization org; const auto work=org.createGroup("Work"), docs=org.createGroup("Docs");
    org.moveSession(org.observe("arch","codex/hgs/dashboard",""),work); org.moveSession(org.observe("arch","kimi/docs/research",""),work);
    org.moveSession(org.observe("mac","claude/infra/review",""),docs);
    setJsonSetting("workspace/organization",QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    const auto any=FakeTasksCli::task(1,"todo"), pinned=FakeTasksCli::task(2,"todo",{{"target",FakeTasksCli::pinned("kimi/docs/research")}});
    const QString fixture=tasksFixture("tasks-drop-hgs",{any,pinned}); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.setFleet(fleet()); window.show();
    window.showSession({}, "codex/hgs/dashboard");
    auto *list=window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    const auto find=[&](const QString &group, const QString &session) -> QListWidgetItem * {
        for(int i=0;i<list->count();++i) {
            auto *item=list->item(i); if(item->data(SessionRoles::Group)!=group) continue;
            if(session.isEmpty() ? item->data(SessionRoles::Header).toBool() : item->data(SessionRoles::TaskTarget).toStringList().value(1)==session) return item;
        }
        return nullptr;
    };
    // Live sessions name where a task dropped on them goes; a project header names none.
    QTRY_VERIFY(find(work,"kimi/docs/research") && find(docs,"claude/infra/review") && find(work,{}));
    QCOMPARE(find(work,"kimi/docs/research")->data(SessionRoles::TaskTarget).toStringList(), QStringList({"", "kimi/docs/research"}));
    QCOMPARE(find(docs,"claude/infra/review")->data(SessionRoles::TaskTarget).toStringList(), QStringList({"mac", "claude/infra/review"}));
    QVERIFY(find(work,{})->data(SessionRoles::TaskTarget).toStringList().isEmpty());
    const auto drop=[&](const QJsonObject &task, QListWidgetItem *item) {
        list->scrollToItem(item); const QPoint point=list->visualItemRect(item).center();
        std::unique_ptr<QMimeData> mime(TaskDrag::of(work, TaskBoard::item(task)).mimeData());
        QDragEnterEvent enter(point,Qt::MoveAction,mime.get(),Qt::LeftButton,Qt::NoModifier); QApplication::sendEvent(list->viewport(),&enter);
        QDragMoveEvent move(point,Qt::MoveAction,mime.get(),Qt::LeftButton,Qt::NoModifier); QApplication::sendEvent(list->viewport(),&move);
        if(!move.isAccepted()) return false;
        QDropEvent event(QPointF(point),Qt::MoveAction,mime.get(),Qt::LeftButton,Qt::NoModifier); QApplication::sendEvent(list->viewport(),&event);
        return event.isAccepted();
    };
    const auto called=[&](const QJsonArray &argv) {
        QFile calls(fixture+".tasks"); if(!calls.open(QIODevice::ReadOnly)) return false;
        while(!calls.atEnd()) if(QJsonDocument::fromJson(calls.readLine()).array()==argv) return true;
        return false;
    };
    // Another project's session refuses the task; a session of its project pins it there.
    QVERIFY(!drop(any, find(docs,"claude/infra/review")));
    QVERIFY(drop(any, find(work,"kimi/docs/research")));
    QTRY_VERIFY(called({"task","target","uuid-1","kimi/docs/research","--project",work}));
    // The project's header makes a pinned task Any session.
    QVERIFY(drop(pinned, find(work,{})));
    QTRY_VERIFY(called({"task","target","uuid-2","any","--project",work}));
}

void TestSessionsWindow::separatesConversationProcessAndTerminal()
{
    SessionInfo session; session.tracked = true; session.state = "running";
    session.activity = "unknown"; session.phase = "unknown";
    session.processState = "running"; session.conversationState = "ended";
    QCOMPARE(SessionsWindow::status(session), QString("Status unknown"));
    session.activity = "idle";
    QCOMPARE(SessionsWindow::status(session), QString("Ready"));
    session.processState = "exited";
    QCOMPARE(SessionsWindow::status(session), QString("Agent exited"));
    session.state = "stopped";
    QCOMPARE(SessionsWindow::status(session), QString("Stopped"));
    session.state = "paused";
    QCOMPARE(SessionsWindow::status(session), QString("Paused"));
    QCOMPARE(SessionsWindow::status(session, false), QString("Offline"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_inspector1.moc"
