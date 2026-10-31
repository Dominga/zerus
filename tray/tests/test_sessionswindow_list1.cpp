// U62 (#712): The session list: rows, states, filters, selection and menus, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void sessionStatesAreComputedAndCountDown();
    void sessionStatesShowOnCardsAndHeader_data();
    void sessionStatesShowOnCardsAndHeader();
    void sessionListFollowsThePrototype_data();
    void sessionListFollowsThePrototype();
    void groupsPersistFilterAndRevealAttention();
    void workingCardsKeepTurnClockAcrossUpdates();
    void pagedArchiveSearchJoinsExactProjectBeforeFiltering();
    void retiredGraphHasNoNavigationOrContextActions();
    void multiSelectionKeepsConversationAndGroupMenu_data();
    void multiSelectionKeepsConversationAndGroupMenu();
    void selectionCommandsKeepPinnedTargets();
};

// U25 part 2 (spec 2.1-2.4) on the README workspace: the project's '+', the
// metadata chips with their counts, team members muted by default with a grey
// edge and a coloured badge, and the lead's strip with the team icon only.
// U30 (#508): the session states from the host's fields: each state's badge
// and line 3, deps always with its reason, a build with what runs, Cooling's
// countdown and heat turning into Cold by the clock, Keep cache warm's hours,
// the tooltips of §5, and an older host keeping today's presentation.
void TestSessionsWindow::sessionStatesAreComputedAndCountDown()
{
    const double now = 1'800'000'000;
    SessionInfo base; base.name = "claude/orbit/release"; base.cmd = "claude"; base.state = "running"; base.processState = "running";
    base.activity = "idle"; base.phase = "idle"; base.runId = "run"; base.conversationId = "conversation";
    const auto shown = [&](const SessionInfo &s, double at, double keepWarm = 0) { return SessionStates::present(SessionStates::facts(s, true, keepWarm), at); };
    // An older host: no state, nothing changes.
    QVERIFY(shown(base, now).key.isEmpty());
    QCOMPARE(SessionStates::present(SessionStates::facts(base, false), now).key, QString("offline"));
    // Working, and the user's four kinds.
    auto working = base; working.sessionState = "working"; working.activity = "busy";
    QCOMPARE(shown(working, now).key, QString("working"));
    QCOMPARE(shown(working, now).tooltip, QString("Working — the model is writing code or running tools"));
    auto approval = base; approval.sessionState = "user"; approval.phase = "approval";
    approval.stateReason = QJsonObject{{"kind", "approval"}, {"text", "Bash: cargo sqlx migrate run"}};
    QCOMPARE(shown(approval, now).badge, QString("Needs approval")); QCOMPARE(shown(approval, now).line, QString("Bash: cargo sqlx migrate run"));
    auto input = base; input.sessionState = "user"; input.phase = "input";
    QCOMPARE(shown(input, now).badge, QString("Needs input"));
    auto reply = base; reply.sessionState = "user"; reply.stateReason = QJsonObject{{"kind", "reply"}};
    QCOMPARE(shown(reply, now).badge, QString("New reply")); QCOMPARE(shown(reply, now).line, QString("Response ready to read"));
    auto lead = base; lead.sessionState = "user"; lead.stateReason = QJsonObject{{"kind", "team_review"}, {"text", "Waiting for you to approve phase 2 · Checkout"}};
    QCOMPARE(shown(lead, now).badge, QString("Needs you")); QCOMPARE(shown(lead, now).line, QString("Waiting for you to approve phase 2 · Checkout"));
    // Deps: always the reason; the tooltip of §5.
    auto deps = base; deps.sessionState = "deps";
    deps.stateReason = QJsonObject{{"kind", "long_operation"}, {"text", "Waiting for cargo test --workspace · long operation, 6m so far"}, {"command", "cargo test --workspace"}};
    QCOMPARE(shown(deps, now).badge, QString("Waiting")); QCOMPARE(shown(deps, now).icon, QString("hourglass"));
    QCOMPARE(shown(deps, now).line, QString("Waiting for cargo test --workspace · long operation, 6m so far"));
    QCOMPARE(shown(deps, now).tooltip, QString("Waiting for dependencies — subagents, a long operation, other members or the lead"));
    auto bare = deps; bare.stateReason = {};
    QVERIFY(!shown(bare, now).line.isEmpty());
    // A build: what runs and for how long.
    auto build = base; build.sessionState = "build";
    build.build = QJsonObject{{"source", "hgs_run"}, {"label", "cargo test"}, {"command", "cargo test --workspace"}, {"since", now - 6 * 60}};
    QCOMPARE(shown(build, now).badge, QString("Building")); QCOMPARE(shown(build, now).line, QString("Running cargo test · 6m so far"));
    build.stateReason = QJsonObject{{"kind", "build"}, {"text", "Running cargo test · 6m so far"}};
    QCOMPARE(shown(build, now).line, QString("Running cargo test · 6m so far"));
    // Cooling counts down with its heat; at the TTL it is Cold by itself.
    auto idle = base; idle.sessionState = "cooling"; idle.lastActive = now - 19 * 60; idle.cacheTtl = QJsonObject{{"seconds", 3600}};
    auto cooling = shown(idle, now);
    QCOMPARE(cooling.key, QString("cooling")); QCOMPARE(cooling.badge, QString("Cooling 41m")); QCOMPARE(cooling.icon, QString("thermo"));
    QCOMPARE(cooling.line, QString("Done · waiting for a new task · cache warm for 41m"));
    QVERIFY(qAbs(cooling.heat - 41.0 / 60) < .01);
    QVERIFY(qAbs(shown(idle, now + 40 * 60 + 50).heat - .04) < .001);   // never below 4 %
    const auto later = shown(idle, now + 41 * 60);
    QCOMPARE(later.key, QString("cold")); QCOMPARE(later.badge, QString("Cold"));
    QCOMPARE(shown(idle, now + 41 * 60 + 2 * 3600 + 10 * 60).line, QString("Cache cold 2h 10m"));
    QVERIFY(later.coldActions); QVERIFY(!cooling.coldActions);
    // The host's cold, and Keep cache warm's hours as the TTL.
    auto cold = idle; cold.sessionState = "cold";
    QCOMPARE(shown(cold, now).key, QString("cooling"));   // the clock decides between the two
    QCOMPARE(shown(idle, now + 2 * 3600, 3 * 3600).key, QString("cooling"));
    QVERIFY(shown(idle, now + 2 * 3600, 3 * 3600).line.endsWith("(kept warm)"));
    // A five-minute cache with no last activity is cold, not cooling.
    auto unknown = base; unknown.sessionState = "cold";
    QCOMPARE(shown(unknown, now).key, QString("cold")); QCOMPARE(shown(unknown, now).line, QString("Cache cold"));
    // Stopped by you, or the agent exited.
    auto paused = base; paused.sessionState = "stopped"; paused.state = "paused";
    QCOMPARE(shown(paused, now).badge, QString("Stopped")); QCOMPARE(shown(paused, now).line, QString("Stopped by you · resume to continue"));
    auto exited = base; exited.sessionState = "stopped"; exited.processState = "exited";
    QCOMPARE(shown(exited, now).badge, QString("Agent exited"));
    // The minutes the design shows.
    QCOMPARE(SessionStates::minutes(41 * 60 + 30), QString("41m")); QCOMPARE(SessionStates::minutes(2 * 3600 + 5 * 60), QString("2h 05m"));
}

// U30 (#508): the cards, the avatar dots and the header pill for every
// state in both themes, the Background rules (a dev server after the turn is
// Cooling with its services chip and never defers the reply; a Zerus build
// does), and a cold card's + New (New session with the project, machine,
// agent and "<name>-2") and Resume (opens it, the toast), neither opening the
// card by its click.
void TestSessionsWindow::sessionStatesShowOnCardsAndHeader_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QStringLiteral("dark");
    QTest::newRow("light") << QStringLiteral("light");
}

void TestSessionsWindow::sessionStatesShowOnCardsAndHeader()
{
    QFETCH(QString, theme);
    const auto restore = qScopeGuard([] { for (const auto *key : {"workspace/skin", "workspace/theme"}) removeSetting(key); });
    QSettings().setValue("workspace/skin", "whitewh"); QSettings().setValue("workspace/theme", theme);
    const double now = QDateTime::currentSecsSinceEpoch();
    const auto make = [now](const QString &tag, const QString &state) {
        SessionInfo s; s.name = "claude/orbit/" + tag; s.cmd = "claude"; s.project = "orbit"; s.tag = tag; s.tracked = true; s.resumable = true;
        s.state = "running"; s.processState = "running"; s.runtimeState = "live"; s.activity = "idle"; s.phase = "idle";
        s.runId = "run-" + tag; s.conversationId = "conversation-" + tag; s.model = "claude-opus-4-6"; s.effort = "high";
        s.cwd = "D:\\work\\orbit"; s.gitBranch = "main"; s.created = qint64(now) - 5 * 3600; s.lastEventAt = now - 60;
        s.sessionState = state; s.lastActive = now - 60; s.cacheTtl = QJsonObject{{"seconds", 3600}, {"source", "native_usage"}};
        return s;
    };
    auto working = make("checkout", "working"); working.activity = "busy"; working.phase = "tool"; working.currentTool = "Edit";
    working.toolDetail = "web/checkout/address.tsx"; working.turnStarted = now - 65;
    auto approval = make("migrate", "user"); approval.activity = "waiting"; approval.phase = "approval"; approval.attentionId = "perm-1";
    approval.currentTool = "Bash"; approval.toolDetail = "cargo sqlx migrate run";
    approval.stateReason = QJsonObject{{"kind", "approval"}, {"text", "Bash: cargo sqlx migrate run"}};
    auto longOp = make("api-refactor", "deps"); longOp.activity = "busy"; longOp.phase = "tool"; longOp.currentTool = "Bash"; longOp.turnStarted = now - 400;
    longOp.stateReason = QJsonObject{{"kind", "long_operation"}, {"text", "Waiting for cargo test --workspace · long operation, 6m so far"}, {"command", "cargo test --workspace"}};
    auto build = make("ledger", "build");
    build.build = QJsonObject{{"source", "hgs_run"}, {"label", "cargo test"}, {"command", "cargo test --workspace"}, {"since", now - 360}};
    build.stateReason = QJsonObject{{"kind", "build"}, {"text", "Running cargo test · 6m so far"}};
    build.unreadReply = true; build.replyId = "r-build"; build.replyAt = now - 120;   // waits for the build
    auto devServer = make("release", "cooling"); devServer.lastActive = now - 18 * 60 - 30;   // 41m 30s left
    devServer.backgroundProcesses = 0;   // the host moved the dev server to the services
    devServer.services = {SessionInfo::Service{"vite", "vite --port 5173", now - 1200}};
    auto replied = make("docs", "user"); replied.stateReason = QJsonObject{{"kind", "reply"}};
    replied.unreadReply = true; replied.replyId = "r-docs"; replied.replyAt = now - 30; replied.backgroundProcesses = 0;
    replied.services = {SessionInfo::Service{"mkdocs", "mkdocs serve", now - 900}};
    auto cold = make("staging", "cold"); cold.lastActive = now - 3600 - (2 * 3600 + 10 * 60) - 20; cold.cmd = "codex"; cold.name = "codex/orbit/staging";
    auto stopped = make("archive-job", "stopped"); stopped.state = "paused";
    auto asking = make("onboarding", "user"); asking.activity = "waiting"; asking.phase = "input"; asking.attentionId = "question-1";
    asking.stateReason = QJsonObject{{"kind", "input"}, {"text", "Which payment provider should the checkout use?"}};
    auto subagents = make("search", "deps"); subagents.activity = "busy"; subagents.phase = "tool"; subagents.turnStarted = now - 90;
    subagents.stateReason = QJsonObject{{"kind", "subagents"}, {"text", "Waiting for 2 subagents: Explore, reviewer"}};
    auto member = make("checkout-ui", "deps");
    member.stateReason = QJsonObject{{"kind", "team_dependency"}, {"text", "Waiting for #2 Saved cards API (Backend)"}};
    auto fresh = make("notes", "cooling"); fresh.lastActive = now - 50;
    BoxState box; box.host = "workstation"; box.ok = true; box.metrics = QJsonObject{{"platform", "windows"}, {"sampled_at", now - 4}};
    box.sessions = {working, approval, longOp, build, devServer, replied, cold, stopped, asking, subagents, member, fresh};
    FleetState state; state.setLocal(box, qint64(now * 1000));
    SessionsWindow window(FakeCli::program(script())); window.resize(1440, 960); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    const auto rowOf = [&](const QString &name) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString() == "\n" + name) return i; return -1; };
    const auto keys = [&] { QStringList all; for (int i = 0; i < list->count(); ++i) all << list->item(i)->data(SessionRoles::Key).toString().replace(QLatin1Char(10), QLatin1Char(124)); return all.join(", "); };
    QTRY_VERIFY2(rowOf("codex/orbit/staging") >= 0 && rowOf("claude/orbit/ledger") >= 0, qPrintable(keys()));
    // The design's list width, once the window has laid out.
    if (auto *slot = window.findChild<QWidget *>("sessionListSlot"))
        if (auto *splitter = qobject_cast<QSplitter *>(slot->parentWidget())) splitter->setSizes({380, splitter->sizes().value(0) + splitter->sizes().value(1) - 380});
    QTest::qWait(100);
    const auto statusOf = [&](const QString &name) { return SessionDelegate::statusOf(list->model()->index(rowOf(name), 0), false); };
    // The badges and line 3.
    QCOMPARE(statusOf("claude/orbit/checkout").kind, SessionStatusBadge::Working);
    QCOMPARE(statusOf("claude/orbit/migrate").kind, SessionStatusBadge::Attention);
    QCOMPARE(statusOf("claude/orbit/migrate").caption, QString("Needs approval"));
    QCOMPARE(statusOf("claude/orbit/api-refactor").kind, SessionStatusBadge::Deps);
    QCOMPARE(statusOf("claude/orbit/api-refactor").caption, QString("Waiting"));
    QCOMPARE(statusOf("claude/orbit/api-refactor").shown.line, QString("Waiting for cargo test --workspace · long operation, 6m so far"));
    QCOMPARE(statusOf("claude/orbit/ledger").kind, SessionStatusBadge::Build);
    QCOMPARE(statusOf("claude/orbit/ledger").shown.line, QString("Running cargo test · 6m so far"));
    QCOMPARE(statusOf("claude/orbit/release").kind, SessionStatusBadge::Cooling);
    QCOMPARE(statusOf("claude/orbit/release").caption, QString("Cooling 41m"));
    QCOMPARE(statusOf("codex/orbit/staging").kind, SessionStatusBadge::Cold);
    QCOMPARE(statusOf("codex/orbit/staging").shown.line, QString("Cache cold 2h 10m"));
    QCOMPARE(statusOf("claude/orbit/archive-job").kind, SessionStatusBadge::Stopped);
    // The Background rules: a dev server after the turn is a service (its
    // chip), never deps, and the reply of a session with only services shows;
    // a Zerus build keeps its reply waiting.
    const auto services = [&](const QString &name) { return list->item(rowOf(name))->data(SessionRoles::Services).toList().size(); };
    QCOMPARE(services("claude/orbit/release"), 1);
    QVERIFY(list->item(rowOf("claude/orbit/docs"))->data(SessionRoles::Unread).toBool());
    QCOMPARE(statusOf("claude/orbit/docs").caption, QString("New reply"));
    // #590 (QA #512 findings 1-3, handoff Status 10, 12, 14): a reply to read
    // is the user state, so its avatar dot is warn like an approval's, not the
    // unread blue; the working badge reads "Working 1m 05s"; an approval's
    // line 3 asks "Run command? <command>".
    QCOMPARE(SessionDelegate::avatarDot(statusOf("claude/orbit/docs"), theme == "dark"), QColor("#e8a75a"));
    QCOMPARE(SessionDelegate::avatarDot(statusOf("claude/orbit/docs"), theme == "dark"), SessionDelegate::avatarDot(statusOf("claude/orbit/migrate"), theme == "dark"));
    QCOMPARE(SessionDelegate::avatarDot(statusOf("claude/orbit/checkout"), theme == "dark"), QColor("#7dd3b0"));
    QVERIFY2(QRegularExpression("^Working 1m [0-5][0-9]s$").match(statusOf("claude/orbit/checkout").caption).hasMatch(), qPrintable(statusOf("claude/orbit/checkout").caption));
    QCOMPARE(list->item(rowOf("claude/orbit/migrate"))->data(SessionRoles::Detail).toString(), QString("Needs approval: Run command? cargo sqlx migrate run"));
    QVERIFY(!list->item(rowOf("claude/orbit/ledger"))->data(SessionRoles::Unread).toBool());
    // Counters stay as they were: deps, cooling and cold are not attention.
    QVERIFY(!list->item(rowOf("claude/orbit/api-refactor"))->data(SessionRoles::Attention).toBool());
    QVERIFY(!list->item(rowOf("codex/orbit/staging"))->data(SessionRoles::Attention).toBool());
    // The badge's tooltip (§5), with the reason.
    const auto tip = [&](const QPoint &point) {
        QHelpEvent help(QEvent::ToolTip, point, list->viewport()->mapToGlobal(point)); QApplication::sendEvent(list->viewport(), &help);
        return QToolTip::text();
    };
    const auto optionFor = [&](const QString &name) {
        auto *item = list->item(rowOf(name)); list->scrollToItem(item);
        QStyleOptionViewItem option; option.initFrom(list->viewport()); option.rect = list->visualItemRect(item);
        option.font = list->font(); option.widget = list;
        return option;
    };
    {
        const auto option = optionFor("claude/orbit/api-refactor");
        const auto index = list->model()->index(rowOf("claude/orbit/api-refactor"), 0);
        const QRect badgeRect = SessionDelegate::titleLayout(option, index, SessionDelegate::statusForView(option, index)).status;
        // Plain text, as Qt's rich tooltip keeps it (its spaces may be non-breaking).
        const QString text = QTextDocumentFragment::fromHtml(tip(badgeRect.center())).toPlainText().replace(QChar(0xa0), QLatin1Char(' '));
        QVERIFY2(text.contains("Waiting for dependencies") && text.contains("cargo test --workspace"), qPrintable(text));
    }
    // Shots of each card, the way the design's are cut.
    const QString shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    const auto cardShot = [&](const QString &name, const QString &file) {
        if (shots.isEmpty()) return;
        auto *item = list->item(rowOf(name)); list->scrollToItem(item); QTest::qWait(60);
        QVERIFY(list->viewport()->grab(list->visualItemRect(item)).save(QDir(shots).filePath(file + "-" + theme + ".png")));
    };
    cardShot("claude/orbit/checkout", "u30-10-working");
    cardShot("claude/orbit/migrate", "u30-12-user-approval");
    cardShot("claude/orbit/docs", "u30-14-user-reply");
    cardShot("claude/orbit/api-refactor", "deps");
    cardShot("claude/orbit/ledger", "u30-25-build");
    cardShot("claude/orbit/release", "u30-30-cooling");
    cardShot("codex/orbit/staging", "u30-40-cold");
    cardShot("claude/orbit/archive-job", "u30-50-stopped");
    cardShot("claude/orbit/onboarding", "u30-13-user-input");
    cardShot("claude/orbit/search", "u30-21-deps-subagents");
    cardShot("claude/orbit/checkout-ui", "u30-22-deps-team");
    cardShot("claude/orbit/notes", "u30-31-cooling-fresh");
    QCOMPARE(statusOf("claude/orbit/onboarding").caption, QString("Needs input"));
    QCOMPARE(statusOf("claude/orbit/search").shown.line, QString("Waiting for 2 subagents: Explore, reviewer"));
    QCOMPARE(statusOf("claude/orbit/checkout-ui").shown.line, QString("Waiting for #2 Saved cards API (Backend)"));
    QVERIFY(statusOf("claude/orbit/notes").heat > .95);
    // The header pill: the state's text, its tone and its reason.
    window.showSession({}, "claude/orbit/api-refactor");
    auto *badge = window.findChild<QLabel *>("badge"); QVERIFY(badge);
    QTRY_COMPARE(badge->property("sessionState").toString(), QString("deps"));
    QCOMPARE(badge->text(), QString("Waiting"));
    QVERIFY(badge->toolTip().contains("cargo test --workspace"));
    if (!shots.isEmpty()) { QTest::qWait(60); QVERIFY(badge->grab().save(QDir(shots).filePath("u30-61-header-deps-" + theme + ".png"))); }
    window.showSession({}, "claude/orbit/release");
    QTRY_COMPARE(badge->property("sessionState").toString(), QString("cooling"));
    QCOMPARE(badge->text(), QString("Cooling 41m"));
    window.showSession({}, "codex/orbit/staging");
    QTRY_COMPARE(badge->property("sessionState").toString(), QString("cold"));
    QCOMPARE(badge->text(), QString("Cold"));
    if (!shots.isEmpty()) { QTest::qWait(60); QVERIFY(badge->grab().save(QDir(shots).filePath("u30-63-header-cold-" + theme + ".png"))); }
    // Cold: + New opens New session for the same project, machine and agent,
    // named "<name>-2"; Resume opens the session with its toast. Neither click
    // selects the card by itself.
    window.showSession({}, "claude/orbit/checkout");
    QTRY_COMPARE(list->currentItem()->data(SessionRoles::Key).toString(), QString("\nclaude/orbit/checkout"));
    const auto coldIndex = list->model()->index(rowOf("codex/orbit/staging"), 0);
    const auto option = optionFor("codex/orbit/staging"); QTest::qWait(60);
    const auto actions = SessionDelegate::coldActionRects(option, coldIndex);
    QVERIFY(!actions.first.isEmpty() && !actions.second.isEmpty());
    QVERIFY(SessionDelegate::coldActionRects(optionFor("claude/orbit/release"), list->model()->index(rowOf("claude/orbit/release"), 0)).first.isEmpty());
    list->scrollToItem(list->item(rowOf("codex/orbit/staging"))); QTest::qWait(60);
    QVERIFY(tip(actions.first.center()).contains("Start a new session in the same folder and machine"));
    QString name;
    QTimer::singleShot(0, &window, [&] {
        if (auto *dialog = qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget())) {
            name = dialog->findChild<QLineEdit *>("launchName")->text();
            if (!shots.isEmpty()) { QTest::qWait(150); dialog->grab().save(QDir(shots).filePath("u30-41-cold-new-session-" + theme + ".png")); }
            dialog->reject();
        }
    });
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, actions.first.center());
    QCOMPARE(name, QString("staging-2"));
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), QString("\nclaude/orbit/checkout"));
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, actions.second.center());
    QTRY_COMPARE(list->currentItem()->data(SessionRoles::Key).toString(), QString("\ncodex/orbit/staging"));
    QTRY_COMPARE(window.findChild<QLabel *>("notice")->text(), QString("Continuing staging · the first message rebuilds the cache"));
}

void TestSessionsWindow::sessionListFollowsThePrototype_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QString("dark");
    QTest::newRow("light") << QString("light");
}

void TestSessionsWindow::sessionListFollowsThePrototype()
{
    QFETCH(QString, theme);
    const auto restore = qScopeGuard([] {
        for (const auto *key : {"workspace/skin", "workspace/theme", "workspace/organization", "workspace/inspectorVisible", "processes/enabled"}) removeSetting(key);
    });
    QSettings().setValue("workspace/skin", "whitewh"); QSettings().setValue("workspace/theme", theme);
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    QSettings().setValue("processes/enabled", true);
    const double now = QDateTime::currentSecsSinceEpoch();
    auto state = ReadmeShots::fleet(now);
    auto ws = state.local();
    for (auto &s : ws.sessions) {
        if (s.tag == "checkout") { s.backgroundProcesses = 1; s.backgroundStarted = now - 40; s.backgroundCommand = "pnpm dev --port 5173"; }
        if (s.tag == "checkout-api") {
            s.activity = "waiting"; s.phase = "approval"; s.attentionId = "perm-migrate"; s.currentTool = "Bash"; s.toolDetail = "cargo sqlx migrate run";
            s.services = {SessionInfo::Service{"rust-analyzer", "rust-analyzer", now - 3000}};
        }
    }
    state.setLocal(ws, qint64(now * 1000));
    auto mac = *state.peer("macbook");
    for (auto &s : mac.sessions) if (s.tag == "checkout-ui") s.services = {SessionInfo::Service{"vite", "vite --port 5173", now - 2000}};
    state.setPeer(mac, qint64(now * 1000));
    const auto git = [now](const QString &branch, int changed, int ahead, int behind) {
        return QJsonObject{{"state", "ok"}, {"branch", branch}, {"detached", false}, {"changed_files", changed}, {"conflicts", 0},
                           {"ahead", ahead}, {"behind", behind}, {"upstream", "origin/" + branch}, {"remote_state", "verified"},
                           {"remote_age_seconds", 5}, {"remote_checked_at", now - 120}};
    };
    const QJsonObject gitByPath{{"\nD:\\work\\orbit", git("feat/checkout-v2", 2, 1, 0)},
                                {"\nD:\\work\\orbit-team\\backend", git("team/checkout-v2/backend", 3, 0, 0)},
                                {"macbook\n/Users/alice/work/orbit-team/frontend", git("team/checkout-v2/frontend", 2, 2, 0)},
                                {"build-01\n/srv/work/orbit-team/tester", git("team/checkout-v2/tester", 0, 0, 0)}};
    const QJsonObject data{{"inspect", ReadmeShots::conversations(state, now)}, {"accounts", ReadmeShots::accounts(now)}, {"team", ReadmeShots::team(now)},
                           {"feed", ReadmeShots::feed(now)}, {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()},
                           {"git", gitByPath}};
    const QString fixture = m_dir->filePath("hgs-session-list-" + theme);
    QVERIFY(FakeCli::install(fixture, ReadmeShots::program));
    QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 960);
    QSignalSpy members(&window, &SessionsWindow::teamMembersChanged);
    window.setFleet(state); window.show();
    auto *controller = window.findChild<TeamController *>(); QVERIFY(controller);
    QTRY_COMPARE_WITH_TIMEOUT(controller->teams().size(), 1, 10000);
    auto *slot = window.findChild<QWidget *>("sessionListSlot"); QVERIFY(slot);
    auto *splitter = qobject_cast<QSplitter *>(slot->parentWidget()); QVERIFY(splitter);
    splitter->setSizes({350, splitter->sizes().value(0) + splitter->sizes().value(1) - 350});
    window.showSession({}, "claude/orbit/checkout");
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    const auto rowOf = [&](const QString &key) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString() == key) return i; return -1; };
    const auto lead = [&] { return rowOf("\nclaude/orbit/checkout"); };
    QTRY_VERIFY(lead() >= 0 && !list->item(lead())->data(SessionRoles::TeamLead).toMap().isEmpty());
    // 2.3: the members, not the lead, are muted by their team; the tray learns them.
    QTRY_VERIFY(!members.isEmpty());
    QCOMPARE(members.last().first().value<QSet<QString>>(),
             (QSet<QString>{"\ndsh/orbit/checkout-api", "macbook\ncodex/orbit/checkout-ui", "build-01\nkimi/orbit/checkout-tests"}));
    const int backend = rowOf("\ndsh/orbit/checkout-api");
    QTRY_VERIFY(backend > lead() && list->item(backend)->data(SessionRoles::Muted).toBool());
    QVERIFY(list->item(backend)->data(SessionRoles::TeamMuted).toBool());
    QVERIFY(!list->item(lead())->data(SessionRoles::Muted).toBool());
    const auto backendIndex = list->model()->index(backend, 0);
    const auto status = SessionDelegate::statusOf(backendIndex, false);
    QCOMPARE(status.kind, SessionStatusBadge::Attention);            // the badge keeps its colour
    QCOMPARE(status.emphasis, IdentityBadges::Muted);                 // the edge is grey
    const auto tip = [&](const QPoint &point) {
        QHelpEvent help(QEvent::ToolTip, point, list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(), &help);
        return QToolTip::text().replace(QChar(0xa0), QLatin1Char(' '));   // convertFromPlainText keeps spaces as nbsp
    };
    QStyleOptionViewItem option; option.initFrom(list); option.rect = list->visualItemRect(list->item(backend)); option.widget = list;
    const auto mute = SessionDelegate::muteRect(option, backendIndex); QVERIFY(!mute.isEmpty());
    // N36 of #499: the team default is this desktop's own; hgs, Android and the
    // other desktops keep notifying, so neither text says "all devices".
    QTRY_VERIFY(tip(mute.center()).contains("Muted on this computer: team members are muted by default; the lead reports for them. Right-click to unmute."));
    const auto spoken = list->item(backend)->data(Qt::AccessibleTextRole).toString();
    QVERIFY2(spoken.contains("Muted on this computer: team members are muted by default") && !spoken.contains("all devices"), qPrintable(spoken));
    // 2.4: the strip names its team in the tooltip only.
    const auto strip = SessionList::teamStripRect(list->visualItemRect(list->item(lead())));
    QTRY_VERIFY(tip(strip.center() + QPoint(-strip.width() / 4, 0)).contains("Open team Checkout v2"));
    // 2.2: the chips: background commands and git on the lead's card.
    const auto chipsOf = [&](int row) { return SessionDelegate::rowChips(list->visualItemRect(list->item(row)), list->model()->index(row, 0), list->font(), false); };
    const auto chipOf = [&](int row, SessionRowChips::Kind kind) {
        for (const auto &chip : chipsOf(row)) if (chip.kind == kind) return chip;
        return SessionRowChips::Chip{SessionRowChips::More, {}, {}, {}, {}};
    };
    QTRY_VERIFY(!chipOf(lead(), SessionRowChips::Changes).rect.isEmpty());
    QCOMPARE(chipOf(lead(), SessionRowChips::Background).count, QString("1"));
    QCOMPARE(chipOf(lead(), SessionRowChips::Changes).count, QString("2"));
    QCOMPARE(chipOf(lead(), SessionRowChips::Outgoing).count, QString("1"));
    QCOMPARE(chipOf(backend, SessionRowChips::Services).count, QString("1"));
    QVERIFY(tip(chipOf(lead(), SessionRowChips::Background).rect.center()).contains("1 running command: pnpm dev --port 5173"));
    QVERIFY(tip(chipOf(lead(), SessionRowChips::Changes).rect.center()).contains("2 changed files on feat/checkout-v2"));
    QVERIFY(tip(chipOf(lead(), SessionRowChips::Outgoing).rect.center()).contains("1 outgoing commit"));
    QVERIFY(tip(chipOf(backend, SessionRowChips::Services).rect.center()).contains("These live with the session and are not Background work."));
    const int tester = rowOf("build-01\nkimi/orbit/checkout-tests");
    QTRY_VERIFY(!chipOf(tester, SessionRowChips::Synced).rect.isEmpty());
    QVERIFY(tip(chipOf(tester, SessionRowChips::Synced).rect.center()).contains("Clean and in sync with the checked upstream · Remote checked: "));
    // The chips end where the metadata line does; the folder text stops before them.
    const auto chips = chipsOf(lead());
    QVERIFY(chips.last().rect.right() <= list->visualItemRect(list->item(lead())).right());
    // A click opens where each count lives: git in the inspector, commands in Processes.
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, chipOf(lead(), SessionRowChips::Changes).rect.center());
    QTRY_VERIFY(inspectorCardOpen(window, "changes"));
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs"); QVERIFY(tabs);
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, chipOf(lead(), SessionRowChips::Background).rect.center());
    QTRY_VERIFY(tabs->tabText(tabs->currentIndex()).startsWith("Processes"));
    // 2.1: the project's '+' names the project and its folder and opens New session for it.
    const int header = rowOf(list->item(0)->data(SessionRoles::Key).toString());
    QVERIFY(list->item(header)->data(SessionRoles::Header).toBool());
    const auto plus = SessionList::projectAddRect(list->visualItemRect(list->item(header)));
    QTRY_VERIFY(tip(plus.center()).contains(QString("New session in Orbit platform (%1)").arg(QDir::toNativeSeparators("D:/work/orbit")).toHtmlEscaped()));
    QString project;
    QTimer::singleShot(0, &window, [&] {
        if (auto *dialog = qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget())) {
            project = dialog->findChild<QComboBox *>("launchProject")->currentData().toString(); dialog->reject();
        }
    });
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, plus.center());
    QCOMPARE(project, QString("orbit"));
    QVERIFY(!list->item(header)->data(SessionRoles::Collapsed).toBool());   // '+' does not fold the project
    if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty()) {
        auto *panel = window.findChild<QWidget *>("sessionListPanel"); QVERIFY(panel);
        QTest::mouseMove(list->viewport(), chipOf(lead(), SessionRowChips::Changes).rect.center()); QTest::qWait(400);
        QVERIFY(panel->grab().save(QDir(shots).filePath("ux-2-session-list-" + theme + ".png")));
        // The Needs attention filter keeps the muted member, with its grey edge.
        auto *attention = window.findChild<QPushButton *>("attentionFilter");
        if (!attention) for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter") == "attention") attention = button;
        if (attention) { attention->click(); QTest::qWait(400); QVERIFY(panel->grab().save(QDir(shots).filePath("ux-2-session-list-attention-" + theme + ".png"))); }
    }
}

void TestSessionsWindow::groupsPersistFilterAndRevealAttention()
{
    SessionOrganization org;
    const auto work = org.createGroup("Release"), docs = org.createGroup("Research");
    const auto local = org.observe("arch", "codex/hgs/dashboard", ""), remote = org.observe("mac", "claude/infra/review", "");
    org.moveSession(local, work); org.moveSession(remote, work); org.setCollapsed(work, true);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    const auto header = [&](const QString &id) -> QListWidgetItem * {
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Header).toBool() && list->item(i)->data(SessionRoles::Group).toString() == id) return list->item(i);
        return nullptr;
    };
    QVERIFY(header(work)); QCOMPARE(header(work)->data(SessionRoles::Attention).toInt(), 1);
    window.showAttentionSession("mac", "claude/infra/review");
    QCOMPARE(list->currentItem()->data(SessionRoles::Identity).toString(), remote);
    QVERIFY(!list->currentItem()->isHidden()); QVERIFY(!header(work)->data(SessionRoles::Collapsed).toBool());
    QTest::mouseClick(list->viewport(), Qt::LeftButton, Qt::NoModifier, list->visualItemRect(header(work)).center());
    QVERIFY(header(work)->data(SessionRoles::Collapsed).toBool());
    QCOMPARE(list->currentItem()->data(SessionRoles::Identity).toString(), remote); // Keep the working pane when collapsing its group.
    auto *search = window.findChild<QLineEdit *>("search"); search->setText("review");
    auto *results = window.findChild<QListWidget *>("searchResults"); QVERIFY(results->isVisible()); QCOMPARE(results->count(), 1);
    search->clear(); QVERIFY(list->currentItem()->isHidden());
    list->sessionMoved(remote, docs, {}); list->groupMoved(docs, work);
    auto persisted = SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(persisted.groupFor(remote), docs); QCOMPARE(persisted.groups().at(0).id, docs);
    auto offline = fleet(); BoxState unavailable; unavailable.host = "mac"; offline.setPeer(unavailable, QDateTime::currentMSecsSinceEpoch()); window.setFleet(offline);
    QCOMPARE(header(docs)->data(SessionRoles::Attention).toInt(), 0); QCOMPARE(list->currentItem()->data(SessionRoles::Identity).toString(), remote);
    window.close(); SessionsWindow reopened(program()); reopened.setFleet(fleet()); reopened.show();
    reopened.showAttentionSession("mac", "claude/infra/review");
    auto *newList = reopened.findChild<SessionList *>("sessionList"); QCOMPARE(newList->currentItem()->data(SessionRoles::Group).toString(), docs);
    QVERIFY(!newList->currentItem()->isHidden());
}

void TestSessionsWindow::workingCardsKeepTurnClockAcrossUpdates()
{
    auto state=fleet(); auto box=state.local();
    const double started=QDateTime::currentSecsSinceEpoch()-1076;
    box.sessions[0].turnStarted=started; state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({},"codex/hgs/dashboard");
    auto *list=window.findChild<SessionList *>("sessionList"); auto *row=list->currentItem(); QVERIFY(row);
    QCOMPARE(row->data(SessionRoles::WorkingSince).toDouble(),started);
    QVERIFY(row->data(SessionRoles::Working).toBool()); QVERIFY(list->elapsedTimerRunning());
    list->setActivityAnimationEnabled(false); QVERIFY(list->elapsedTimerRunning());
    QSignalSpy reset(list->model(),&QAbstractItemModel::modelReset);
    QTest::qWait(1100); QCOMPARE(list->currentItem(),row); QCOMPARE(reset.size(),0);
    QCOMPARE(row->data(SessionRoles::WorkingSince).toDouble(),started);
    box.sessions[0].phase="compacting"; box.sessions[0].lastEventAt=QDateTime::currentSecsSinceEpoch();
    const double compactStarted=QDateTime::currentSecsSinceEpoch()-83;
    box.sessions[0].compactionStarted=compactStarted;
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->currentItem(),row); QCOMPARE(row->data(SessionRoles::WorkingSince).toDouble(),compactStarted);
    QCOMPARE(row->data(SessionRoles::Status).toString(),QString("Compacting"));
    const auto preview=qEnvironmentVariable("HGS_ELAPSED_PREVIEW");
    if(!preview.isEmpty()) { QDir().mkpath(preview); QVERIFY(list->grab().save(preview+"/working-timer.png")); }
    box.sessions[0].phase="working";
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(row->data(SessionRoles::WorkingSince).toDouble(),started);
    box.sessions[0].activity="idle"; box.sessions[0].phase="idle";
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(!row->data(SessionRoles::Working).toBool()); QVERIFY(!list->elapsedTimerRunning());
}

void TestSessionsWindow::pagedArchiveSearchJoinsExactProjectBeforeFiltering()
{
    const QString name="codex/hgs/dashboard", a="10000000-0000-4000-8000-000000000001",b="10000000-0000-4000-8000-000000000002";
    SessionOrganization org;const auto first=org.createGroup("Focus"),second=org.createGroup("Other");
    org.moveSession(org.observe("arch",name,"archive-run",a),first);org.moveSession(org.observe("arch",name,"archive-run",b),second);
    setJsonSetting("workspace/organization",QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    QTemporaryDir fixture;const auto cli=fixture.filePath("hgs");
    QVERIFY(FakeCli::install(cli,R"PY(#!/usr/bin/env python3
import json,sys
args=sys.argv[1:]
remote=bool(args and args[0].startswith('@'))
if remote:args=args[1:]
if args[0]=='search':
 rows=[]
 if not remote:
  for n in (1,2):
   ident='10000000-0000-4000-8000-00000000000'+str(n)
   session={'name':'codex/hgs/dashboard','cmd':'codex','run_id':'archive-run','conversation_id':'archived-conversation','state':'archived','archive_id':ident,'archived_at':n,'tracked':True,'resumable':True}
   rows.append(dict(name=session['name'],run_id=session['run_id'],conversation_id=session['conversation_id'],archive_id=ident,session=session,message_id='message'+str(n),at=n,role='assistant',snippet='Needle archived '+str(n),event={'detail':'Needle archived '+str(n)}))
 print(json.dumps({'query':'needle','results':rows,'truncated':False}))
elif args[0]=='inspect':print(json.dumps({'name':args[1],'run_id':'archive-run','conversation_id':'archived-conversation','archive_id':args[args.index('--archive')+1] if '--archive' in args else '', 'tracked':True,'state':'archived','events':[],'cursor':0}))
else:print('{}')
)PY"));
    auto state=fleet();auto local=state.local();local.archivePaging=true;local.sessions[0].runId="live-run";local.sessions[0].prompt.clear();state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(cli));keepDetailsPollsOut(window);window.setFleet(state);window.resize(1100,760);window.show();
    auto *projects=window.findChild<ProjectFilter *>("projectFilterChips");QVERIFY(projects);projects->setSelection({first});
    auto *query=window.findChild<QLineEdit*>("search");auto *results=window.findChild<QListWidget*>("searchResults");QVERIFY(query&&results);query->setText("needle");
    QTRY_COMPARE(results->count(),1);QVERIFY(SessionSearch::sessionKey(results->item(0)).endsWith(a));
    for(auto *filter:window.findChildren<QPushButton*>("sessionFilter"))if(filter->property("filter")=="archived"){QCOMPARE(filter->property("count").toInt(),1);QVERIFY(filter->text().startsWith("1"));}
    projects->setSelection({second});QTRY_COMPARE(results->count(),1);QVERIFY(SessionSearch::sessionKey(results->item(0)).endsWith(b));
    projects->setSelection({});QTRY_COMPARE(results->count(),2);
}

void TestSessionsWindow::retiredGraphHasNoNavigationOrContextActions()
{
    SessionsWindow window(script()); keepDetailsPollsOut(window);
    window.setFleet(fleet()); window.resize(1100,760); window.show();
    const auto name=fleet().local().sessions[0].name; window.showSession({},name);
    auto *list=window.findChild<SessionList *>("sessionList"); QVERIFY(list && list->currentItem());
    auto *composer=window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    setComposerText(composer,"Keep the current conversation draft");
    for(auto *button:window.findChildren<QPushButton *>("railButton"))
        QVERIFY(button->property("glyph")!="graph");
    auto *dashboard=window.findChild<DashboardPage *>(); QVERIFY(dashboard);
    auto *pages=qobject_cast<QStackedWidget *>(dashboard->parentWidget()); QVERIFY(pages);
    QCOMPARE(pages->count(),6);
    list->customContextMenuRequested(list->visualItemRect(list->currentItem()).center());
    auto *menu=window.findChild<QMenu *>("sessionContextMenu"); QVERIFY(menu);
    QVERIFY(!menu->findChild<QAction *>("contextShowInGraph"));
    QVERIFY(menu->findChild<QAction *>("contextCopyCommand")); menu->close();
    auto *projects=window.findChild<ProjectsDialog *>(); QVERIFY(projects);
    for(auto *button:window.findChildren<QPushButton *>("railButton"))
        if(button->property("glyph")=="projects") button->click();
    QTRY_VERIFY(projects->isVisible());
    window.showSession({},name);
    QTRY_COMPARE(composer->editor()->toPlainText(),QString("Keep the current conversation draft"));
}

void TestSessionsWindow::multiSelectionKeepsConversationAndGroupMenu_data()
{
    QTest::addColumn<QString>("theme");QTest::addColumn<int>("width");
    QTest::newRow("dark")<<QString("dark")<<1100;QTest::newRow("light-narrow")<<QString("light")<<840;
}

void TestSessionsWindow::multiSelectionKeepsConversationAndGroupMenu()
{
    QFETCH(QString,theme);QFETCH(int,width);QSettings().setValue("workspace/theme",theme);
    auto state=fleet();SessionsWindow window(program());window.resize(width,760);window.setFleet(state);window.show();
    if(width<900)window.findChild<QSplitter *>()->setSizes({270,width-270});
    auto *list=window.findChild<SessionList *>("sessionList");auto *composer=window.findChild<MessageComposer *>();
    const auto open=list->currentItem()->data(SessionRoles::Key).toString();
    setComposerText(composer, "Keep this draft");
    const auto select=[&](int row){QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(list->item(row)).center());};
    select(1);QCOMPARE(list->selectedItems().size(),2);QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(),open);
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    auto *bar=window.findChild<QFrame *>("sessionSelectionBar");QVERIFY(bar);QVERIFY(bar->isVisible());
    for(auto *button:bar->findChildren<QToolButton *>())if(button->isVisible()) {
        QVERIFY(button->width()>=24);QVERIFY(bar->rect().contains(QRect(button->mapTo(bar,QPoint()),button->size())));
    }
    QSet<QString> keys;for(auto *item:list->selectedItems())keys.insert(item->data(SessionRoles::Key).toString());
    // Reordering the actual rows must preserve the chosen identities.
    auto local=state.local();std::swap(local.sessions[1],local.sessions[2]);state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QSet<QString> after;for(auto *item:list->selectedItems())after.insert(item->data(SessionRoles::Key).toString());QCOMPARE(after,keys);
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(),open);QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    const auto point=list->visualItemRect(list->item(2)).center();
    QTest::mouseClick(list->viewport(),Qt::RightButton,{},point);
    QContextMenuEvent event(QContextMenuEvent::Mouse,point,list->viewport()->mapToGlobal(point));QApplication::sendEvent(list->viewport(),&event);
    auto *menu=window.findChild<QMenu *>("sessionContextMenu");QVERIFY(menu->isVisible());
    QVERIFY(!menu->findChild<QAction *>("contextOpenTerminal"));QVERIFY(menu->findChild<QMenu *>("moveSelectionToProject"));
    QVERIFY(menu->findChild<QAction *>("selectionattention"));QCOMPARE(list->selectedItems().size(),2);menu->hide();
    const auto preview=qEnvironmentVariable("HGS_BULK_PREVIEW");if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+"/selection-"+theme+".png"));}
    bar->findChild<QPushButton *>("clearSessionSelection")->click();QVERIFY(list->selectedItems().isEmpty());QVERIFY(!bar->isVisible());
    window.setFleet(state);QVERIFY(list->selectedItems().isEmpty());QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    QTest::mouseClick(list->viewport(),Qt::LeftButton,{},list->visualItemRect(list->item(0)).center());QCOMPARE(list->selectedItems().size(),1);
    select(1);QCOMPARE(list->selectedItems().size(),2);QTest::keyClick(list,Qt::Key_Escape);QVERIFY(list->selectedItems().isEmpty());
}

void TestSessionsWindow::selectionCommandsKeepPinnedTargets()
{
    auto state=fleet();auto box=state.local();box.sessions[0].activity="idle";box.sessions[0].phase="idle";
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());auto remote=*state.peer("mac");remote.sessions[0].activity="idle";remote.sessions[0].phase="idle";
    remote.sessions[0].runId="original";state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.resize(1100,850);window.setFleet(state);window.show();
    auto *list=window.findChild<SessionList *>("sessionList");
    QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(list->item(4)).center());
    QCOMPARE(list->selectedItems().size(),2);
    QTest::keyClick(list,Qt::Key_Menu);auto *menu=window.findChild<QMenu *>("sessionContextMenu");
    auto *pause=menu->findChild<QAction *>("selectionpause");QVERIFY(pause);QCOMPARE(pause->text(),QString("Pause (2)"));
    auto *client=window.findChild<HgsClient *>();QSignalSpy writes(client,&HgsClient::writeDone);
    // A native restart after opening the group menu must not retarget its action.
    remote.sessions[0].runId="replacement";state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    pause->trigger();menu->hide();QTRY_COMPARE(writes.size(),1);
    QCOMPARE(writes[0][2].toString(),QString("pause\ncodex/hgs/dashboard"));
    QTRY_VERIFY(window.findChild<QLabel *>("notice")->text().contains("1 failed or changed"));
    // A fresh selection is allowed to act on both current instances.
    auto *control=window.findChild<QAction *>("selectionpause");QVERIFY(control);QTRY_VERIFY(control->isEnabled());control->trigger();
    QTRY_COMPARE(writes.size(),3);QCOMPARE(writes[2][2].toString(),QString("@mac\npause\nclaude/infra/review"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_list1.moc"
