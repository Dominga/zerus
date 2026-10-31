// U62 (#712): Agent teams in the list, leads and their subordinates, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void subordinateSessionsFollowTheirLead();
    void leadPanelKeepsItsRowsWhenNothingChanges();
    void aRowUpdateReadsLeadLinksAndKeepWarmHoursOnce();
    void agentTeamsGroupTheirSessions();
    void waitingBannersShowInTheChatAndTheTeamView();
    void teamTreeShowsSubteamsAndRollsUp();
    void teamMembersFollowTheirSubLead_data();
    void teamMembersFollowTheirSubLead();
    void teamMemberUnmuteRecordsItsChoice();
    void teamMemberUnmuteOnAPeerReadsTheReplacedFleet();
    void teamRowBarsCountThePhaseInWork();
    void teamLeftoverStopAsksTheRealAction();
    void leadSessionHeaderCarriesTheTeam();
    void leadSessionAnswersAProposalOnItsCard();
    void modelServersRailButtonOnlyWithAServer();
    void teamAttentionReachesTheNotifications();
    void designerSessionDropsTheTeamHeader();
};

// WhiteWh fork: the skin's workspace view. The rail holds the list toggle and
// hides New session while the list is docked, cards make room for the agent
// tile, the composer is compact and SocratiCode leads Details. The classic
// skin restores all of it.
// WhiteWh fork: a subordinate session is listed under its lead, Details
// links and unlinks it, and its report becomes the lead's draft.
void TestSessionsWindow::subordinateSessionsFollowTheirLead()
{
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/sessionLeads"); });
    const auto state = fleet();
    SessionsWindow window(FakeCli::program(script())); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList");
    QTRY_VERIFY(list->count() >= 3);
    QStringList identities, names;
    for (int i = 0; i < list->count(); ++i) if (!list->item(i)->data(SessionRoles::Header).toBool() && list->item(i)->data(SessionRoles::ChildId).toString().isEmpty()) {
        identities << list->item(i)->data(SessionRoles::Identity).toString(); names << list->item(i)->data(SessionRoles::Title).toString(); }
    QVERIFY(identities.size() >= 3);
    // The last session reports to the first one.
    QVERIFY(SessionLeads::setLead(identities.last(), identities.first()));
    QVERIFY(!SessionLeads::setLead(identities.first(), identities[1]));   // a lead reports to no one
    window.setFleet(state);
    const auto rowOf = [&](const QString &identity) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Identity) == identity && list->item(i)->data(SessionRoles::ChildId).toString().isEmpty()) return i; return -1; };
    QTRY_COMPARE(rowOf(identities.last()), rowOf(identities.first()) + 1);
    QVERIFY(list->item(rowOf(identities.last()))->data(SessionRoles::Subordinate).toBool());
    QVERIFY(!list->item(rowOf(identities.first()))->data(SessionRoles::Subordinate).toBool());
    // Details of the subordinate name its lead.
    QString host, name;
    for (const auto &box : {state.local()}) for (const auto &session : box.sessions) if (identities.last().contains(session.name)) { host = {}; name = session.name; }
    list->setCurrentRow(rowOf(identities.last()));
    auto *panel = window.findChild<QWidget *>("sessionLeadPanel"); QVERIFY(panel);
    QTRY_VERIFY(panel->findChild<QLabel *>("sessionLeadName"));
    QVERIFY(panel->findChild<QLabel *>("sessionLeadName")->text().contains(names.first().toHtmlEscaped()));
    panel->findChild<QPushButton *>("detachSessionLead")->click();
    QTRY_VERIFY(SessionLeads::lead(identities.last()).isEmpty());
    QTRY_VERIFY(!list->item(rowOf(identities.last()))->data(SessionRoles::Subordinate).toBool());
    QTRY_VERIFY(panel->findChild<QComboBox *>("sessionLeadChoice"));
    // #648: one Reports to select, "nobody" first; choosing a session links it.
    auto *choice = panel->findChild<QComboBox *>("sessionLeadChoice");
    QCOMPARE(choice->itemText(0), QString("nobody")); QVERIFY(!panel->findChild<QPushButton *>("linkSessionLead"));
    emit choice->activated(0); QVERIFY(SessionLeads::lead(identities.last()).isEmpty());
    QVERIFY(choice->count() > 1); const auto lead = choice->itemData(1).toString(); QVERIFY(!lead.isEmpty());
    emit choice->activated(1);
    QTRY_COMPARE(SessionLeads::lead(identities.last()), lead);
    QTRY_VERIFY(panel->findChild<QLabel *>("sessionLeadName"));
}

// #704: Details sets the lead panel's context on every rebuild of the list. The
// same context keeps its select (and its popup's style); a changed one builds
// the rows again; a refused choice shows nobody again.
void TestSessionsWindow::leadPanelKeepsItsRowsWhenNothingChanges()
{
    SessionLeadPanel panel;
    const QList<SessionLeadPanel::Session> candidates{{"arch\nrun-1", "api"}, {"arch\nrun-2", "web"}};
    panel.setContext("arch\nrun-3", {}, {}, candidates);
    auto *choice = panel.findChild<QComboBox *>("sessionLeadChoice");
    QVERIFY(choice);
    QCOMPARE(choice->count(), 3);
    choice->setCurrentIndex(2);
    panel.setContext("arch\nrun-3", {}, {}, candidates);
    QCOMPARE(panel.findChild<QComboBox *>("sessionLeadChoice"), choice);
    QCOMPARE(choice->currentIndex(), 0);
    // Another candidate, or another session, builds the rows again.
    panel.setContext("arch\nrun-3", {}, {}, candidates.mid(0, 1));
    QTRY_VERIFY(!choice || panel.findChildren<QComboBox *>("sessionLeadChoice").size() == 1);
    auto *rebuilt = panel.findChildren<QComboBox *>("sessionLeadChoice").last();
    QCOMPARE(rebuilt->count(), 2);
    panel.setContext("arch\nrun-1", {"arch\nrun-2", "web"}, {}, {});
    QTRY_VERIFY(panel.findChild<QLabel *>("sessionLeadName"));
}

// #705: a row update and the details it renders read the lead links and the
// keep-warm hours a few times, not once or twice for every session (each read
// goes through QSettings: about 90 ms and 40 ms of the GUI thread with 400
// sessions in the GUI profile). The lead panel still offers the project's
// other unlinked sessions.
void TestSessionsWindow::aRowUpdateReadsLeadLinksAndKeepWarmHoursOnce()
{
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/sessionLeads"); QSettings().remove("workspace/cacheKeepWarm"); });
    auto state = fleet();
    auto box = state.local();
    const auto base = box.sessions.first();
    for (int i = 0; i < 200; ++i) {
        auto s = base; s.tag = QStringLiteral("s%1").arg(i, 3, 10, QLatin1Char('0')); s.name = "codex/hgs/" + s.tag;
        s.runId = QStringLiteral("run-%1").arg(i); s.conversationId = QStringLiteral("conversation-%1").arg(i);
        s.activity = "idle"; s.phase = "idle"; s.currentTool.clear();
        box.sessions << s;
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(script())); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList");
    QTRY_VERIFY(list->count() >= 200);
    QHash<int, QString> identities;   // by the session's number
    int selectedRow = -1;
    for (int i = 0; i < list->count(); ++i) {
        const auto *item = list->item(i);
        if (item->data(SessionRoles::Header).toBool() || !item->data(SessionRoles::ChildId).toString().isEmpty()) continue;
        if (item->data(SessionRoles::Key).toString().endsWith("codex/hgs/s002")) selectedRow = i;
        const auto key = item->data(SessionRoles::Key).toString();
        if (const auto at = key.indexOf("codex/hgs/s"); at >= 0) identities.insert(key.mid(at + 11).toInt(), item->data(SessionRoles::Identity).toString());
    }
    QCOMPARE(identities.size(), 200);
    QVERIFY(selectedRow >= 0);
    // One subordinate of s000, and one session kept warm: both loops run.
    QVERIFY(SessionLeads::setLead(identities[1], identities[0]));
    CacheKeepalive::setHours({}, box.sessions.last(), 2);
    list->setCurrentRow(selectedRow);
    auto *panel = window.findChild<QWidget *>("sessionLeadPanel"); QVERIFY(panel);
    QTRY_VERIFY(panel->findChild<QComboBox *>("sessionLeadChoice"));
    QTest::qWait(50);
    const auto leads = SettingsJson::readCount("workspace/sessionLeads"), warm = SettingsJson::readCount("workspace/cacheKeepWarm");
    QSignalSpy applied(&window, &SessionsWindow::rowsApplied);
    window.setFleet(state);
    QTRY_VERIFY(applied.size() >= 1);
    QTest::qWait(50);
    const auto leadReads = SettingsJson::readCount("workspace/sessionLeads") - leads, warmReads = SettingsJson::readCount("workspace/cacheKeepWarm") - warm;
    // A few per render (an update renders the details several times), never
    // one per session: reading per session made it 400 or more here.
    QVERIFY2(leadReads <= 30, qPrintable(QStringLiteral("%1 reads of the lead links for 200 sessions").arg(leadReads)));
    QVERIFY2(warmReads <= 30, qPrintable(QStringLiteral("%1 reads of the keep-warm hours for 200 sessions").arg(warmReads)));
    QVERIFY(leadReads >= 1 && warmReads >= 1);
    // s000 leads s001. s002's select offers nobody and the project's other
    // unlinked sessions: the lead s000, not the subordinate s001, not itself.
    QTRY_VERIFY(panel->findChild<QComboBox *>("sessionLeadChoice"));
    const auto *choice = panel->findChild<QComboBox *>("sessionLeadChoice");
    QStringList offered;
    for (int i = 1; i < choice->count(); ++i) offered << choice->itemData(i).toString();
    QVERIFY(offered.contains(identities[0]) && offered.contains(identities[199]));
    QVERIFY(!offered.contains(identities[1]) && !offered.contains(identities[2]));
    QCOMPARE(CacheKeepalive::hours({}, box.sessions.last()), 2);
}

// WhiteWh fork: agent teams. A team's header stands above its lead, its
// members follow indented, the header opens the Team tab and its chevron keeps
// only members with a problem.
void TestSessionsWindow::agentTeamsGroupTheirSessions()
{
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/collapsedTeams"); QSettings().remove("workspace/inspectorVisible"); });
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray teams = R"({"items":[{"id":"team-1","name":"Checkout","status":"running","lead":"lead","revision":3,
        "members":[{"id":"lead","role":"Lead","agent":"codex","state":"active","session":{"host":"","name":"codex/hgs/dashboard"}},
                   {"id":"qa","role":"Tester","agent":"kimi","state":"active","session":{"host":"","name":"kimi/hgs/tester"}}],
        "tasks":[{"id":1,"title":"API","status":"in_progress","assignee":"lead","attempt":{"n":1,"state":"delivered"}},
                 {"id":2,"title":"Tests","status":"pending","assignee":"qa","deps":[{"task":1}]}]}]})";
    const QByteArray answer = "elif c=='team' and a[1:2]==['list']:print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument::fromJson(teams).toJson(QJsonDocument::Compact))) + ")\n"
                              "elif c=='team':print('{}')\n";
    const QString fixture = m_dir->filePath("hgs-teams");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    auto state = fleet(); auto box = state.local();
    SessionInfo tester = box.sessions.value(1); tester.name = "kimi/hgs/tester"; tester.project = "hgs"; tester.tag = "tester";
    box.sessions.append(tester); state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto rowOf = [&](const QString &key) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key) == key) return i; return -1; };
    auto *controller = window.findChild<TeamController *>();
    QVERIFY(controller);
    QTRY_COMPARE(controller->teams().size(), 1);
    // The team lives in its lead's card: no row of its own above it.
    QTRY_VERIFY(rowOf("\ncodex/hgs/dashboard") >= 0
                && !list->item(rowOf("\ncodex/hgs/dashboard"))->data(SessionRoles::TeamLead).toMap().isEmpty());
    QCOMPARE(rowOf("#team\nteam-1"), -1);
    const int leadRow = rowOf("\ncodex/hgs/dashboard");
    QCOMPARE(rowOf("\nkimi/hgs/tester"), leadRow + 1);
    auto *member = list->item(leadRow + 1);
    QVERIFY(member->data(SessionRoles::TeamMember).toBool());
    // A member's card names its role, not the team's session name; the lead keeps its own.
    QTRY_COMPARE(member->data(SessionRoles::Title).toString(), QStringLiteral("Tester"));
    QVERIFY(list->item(leadRow)->data(SessionRoles::Title).toString() != QStringLiteral("Lead"));
    QVERIFY(!list->item(leadRow)->data(SessionRoles::TeamMember).toBool());
    // The last member ends the connector; the lead folds its members as a
    // session folds its subagents.
    QCOMPARE(member->data(SessionRoles::TeamMember).toInt(), 2);
    QVERIFY(list->item(leadRow)->data(SessionRoles::HasChildren).toBool() && list->item(leadRow)->data(SessionRoles::Expanded).toBool());
    emit list->childrenToggled(QStringLiteral("\ncodex/hgs/dashboard"));
    QTRY_VERIFY(list->item(rowOf("\nkimi/hgs/tester"))->isHidden());
    QVERIFY(!list->item(rowOf("\ncodex/hgs/dashboard"))->data(SessionRoles::Expanded).toBool());
    emit list->childrenToggled(QStringLiteral("\ncodex/hgs/dashboard"));
    QTRY_VERIFY(!list->item(rowOf("\nkimi/hgs/tester"))->isHidden());
    const auto strip = list->item(leadRow)->data(SessionRoles::TeamLead).toMap();
    QCOMPARE(strip.value("id").toString(), QStringLiteral("team-1"));
    QCOMPARE(strip.value("name").toString(), QStringLiteral("Checkout"));
    // G6: two compact bars — the phase in work (or the next draft) and the
    // phases — instead of one count over every task.
    QCOMPARE(strip.value("phaseText").toString(), QStringLiteral("0/2"));
    QCOMPARE(strip.value("phaseNumber").toInt(), 1);
    QCOMPARE(strip.value("phaseDraft").toBool(), false);
    QCOMPARE(strip.value("phasesText").toString(), QStringLiteral("0/1"));
    QVERIFY(strip.value("progressTooltip").toString().contains(QStringLiteral("Phase 1")));
    QVERIFY(strip.value("progressTooltip").toString().contains(QStringLiteral("0 of 1 phases")));
    QCOMPARE(strip.value("members").toString(), QStringLiteral("2"));
    QCOMPARE(SessionList::childrenLabel(list->model()->index(leadRow, 0)), QStringLiteral("Team"));
    // The lead's card grows by the strip, and the strip opens the Team tab.
    const QRect rect = list->visualItemRect(list->item(leadRow));
    QVERIFY(rect.height() >= SessionList::TeamStripHeight + 64);
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, SessionList::teamStripRect(rect).center());
    auto *inspector = window.findChild<QWidget *>("sessionInspector");
    QTRY_VERIFY(inspectorCardOpen(window, "team"));
    QTRY_VERIFY(inspector->findChild<QLabel *>("teamName"));
    QCOMPARE(inspector->findChild<QLabel *>("teamName")->text(), QStringLiteral("Checkout"));
    // Optional: ZERUS_TEAM_SHOTS=<dir> keeps a picture of the window for review.
    if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty()) {
        QTest::qWait(300); window.grab().save(QDir(shots).filePath("team-window.png"));
    }
    // A team's session shows Team next to Activity; a session without one
    // offers New team, which it would lead.
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    const auto tabNamed = [tabs](const QString &text) { for (int i = 0; i < tabs->count(); ++i) if (tabs->tabText(i) == text) return i; return -1; };
    window.showSession({}, "codex/hgs/dashboard");
    QTRY_VERIFY(tabNamed("Team") >= 0);
    QCOMPARE(tabNamed("New team"), -1);
    QString loner;
    for (const auto &s : state.local().sessions) if (s.name != "codex/hgs/dashboard" && s.name != "kimi/hgs/tester" && s.state != "archived") { loner = s.name; break; }
    QVERIFY(!loner.isEmpty());
    window.showSession({}, loner);
    QTRY_VERIFY(tabNamed("New team") >= 0);
    QCOMPARE(tabNamed("Team"), -1);
    tabs->setCurrentIndex(tabNamed("New team"));
    QTRY_VERIFY(tabs->currentWidget()->findChild<QWidget *>("teamNewDialog"));
    auto *form = dynamic_cast<TeamNewTab *>(tabs->currentWidget()->findChild<QWidget *>("teamNewDialog"));
    QVERIFY(form); QCOMPARE(form->leadSession(), loner);
    // The rail's Team designer opens its own mode.
    QPushButton *designerNav = nullptr;
    for (auto *b : window.findChildren<QPushButton *>("railButton")) if (b->property("teamDesignerNav").toBool()) designerNav = b;
    QVERIFY(designerNav && !designerNav->isChecked());   // only while its page shows
    QVERIFY(designerNav);
    designerNav->click();
    QTRY_VERIFY(window.findChild<QWidget *>("teamDesignerPage") && window.findChild<QWidget *>("teamDesignerPage")->isVisible());
    QVERIFY(designerNav->isChecked());
    // U25 part 9 (spec 11.2): Settings > Teams holds the same catalog widget
    // with Open Team designer; the designer's page holds it without.
    auto *designerPage = window.findChild<QWidget *>("teamDesignerPage");
    QCOMPARE(designerPage->findChildren<QWidget *>("teamCatalogEditor").size(), 1);
    QVERIFY(designerPage->findChild<QPushButton *>("teamCatalogAskDesigner")->isHidden());
    QWidget *settingsCatalog = nullptr;
    for (auto *w : window.findChildren<QWidget *>("teamCatalogEditor")) if (!designerPage->isAncestorOf(w)) settingsCatalog = w;
    QVERIFY(settingsCatalog);
    auto *openDesigner = settingsCatalog->findChild<QPushButton *>("teamCatalogAskDesigner");
    QCOMPARE(openDesigner->text(), QStringLiteral("Open Team designer"));
    QVERIFY(!openDesigner->isHidden());
    window.showSession({}, loner);
    QTRY_VERIFY(!designerPage->isVisible());
    openDesigner->click();
    QTRY_VERIFY(designerPage->isVisible());
    // Team views show the agents' own logos: DeepSeek in its blue, and Kimi's
    // letter in the text color it is given.
    const auto painted = [](const QIcon &icon, const std::function<bool(const QColor &)> &match) {
        const QImage image = icon.pixmap(24, 24).toImage();
        for (int y = 0; y < image.height(); ++y) for (int x = 0; x < image.width(); ++x) if (const QColor px = image.pixelColor(x, y); px.alpha() > 200 && match(px)) return true;
        return false;
    };
    QVERIFY(painted(TeamUi::agentLogo("dsh", Qt::white), [](const QColor &px) { return px.blue() > 200 && px.red() < 120; }));
    QVERIFY(painted(TeamUi::agentLogo("kimi", QColor("#ff0000")), [](const QColor &px) { return px.red() > 200 && px.green() < 60 && px.blue() < 60; }));
    QVERIFY(painted(TeamUi::agentLogo("opencode", QColor("#00ff00")), [](const QColor &px) { return px.green() > 200 && px.red() < 60; }));
}

// U55: what waits for the owner shows as a banner at the top of the session's
// chat and Activity and above the lead's conversation in the Team tab; each
// thing raises one notification, whether the attention list or the waiting
// list sees it first, and a read again raises nothing.
void TestSessionsWindow::waitingBannersShowInTheChatAndTheTeamView()
{
    const auto clean = [] { for (const char *key : {"teams/waitingNotified", "teams/attentionNotified"}) QSettings().remove(QLatin1String(key)); };
    clean();
    const auto restore = qScopeGuard(clean);
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const QJsonObject team{{"id", "team-1"}, {"name", "Checkout"}, {"status", "running"}, {"lead", "lead"}, {"revision", 3},
        {"members", QJsonArray{QJsonObject{{"id", "lead"}, {"role", "Lead"}, {"agent", "codex"}, {"state", "active"},
                                           {"session", QJsonObject{{"host", ""}, {"name", "codex/hgs/dashboard"}}}},
                               QJsonObject{{"id", "qa"}, {"role", "Tester"}, {"agent", "kimi"}, {"state", "active"}}}},
        {"tasks", QJsonArray{QJsonObject{{"id", 2}, {"title", "Tests"}, {"status", "submitted"}, {"assignee", "qa"}}}},
        {"attention", QJsonArray{QJsonObject{{"kind", "report"}, {"task", 2}, {"member", "Tester"}, {"since", now - 600}, {"minutes", 10}}}}};
    const QJsonArray waiting{
        QJsonObject{{"id", "team-1/report_escalated/2"}, {"team", "team-1"}, {"team_name", "Checkout"}, {"kind", "report_escalated"},
                    {"title", "Report on #2 Tests from Tester waits for your decision"}, {"since", now - 600}, {"action", "decide_report"}, {"task", 2}},
        QJsonObject{{"id", "team-9/plan_draft/plan"}, {"team", "team-9"}, {"team_name", "Billing"}, {"kind", "plan_draft"},
                    {"title", "Plan of 4 tasks waits for Approve & Run"}, {"since", now - 60}, {"action", "approve_plan"}}};
    const auto compact = [](const QJsonValue &value) {
        return FakeCli::pythonString(QString::fromUtf8(value.isArray() ? QJsonDocument(value.toArray()).toJson(QJsonDocument::Compact)
                                                                        : QJsonDocument(value.toObject()).toJson(QJsonDocument::Compact)));
    };
    const QByteArray answer = "elif c=='team' and a[1:2]==['list']:print(" + compact(QJsonObject{{"items", QJsonArray{team}}}) + ")\n"
                              "elif c=='team' and a[1:2]==['waiting']:print(json.dumps({'items':json.loads(" + compact(waiting) + "),'next':None,'total':2}))\n"
                              "elif c=='team':print('{}')\n";
    const QString fixture = m_dir->filePath("hgs-waiting");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1280, 760);
    QSignalSpy raised(&window, &SessionsWindow::teamAttentionRaised);
    window.setFleet(fleet()); window.show();
    auto *chat = dynamic_cast<TeamWaitingBanner *>(window.findChild<QWidget *>("sessionWaitingBanner"));
    auto *teamView = dynamic_cast<TeamWaitingBanner *>(window.findChild<QWidget *>("workspaceWaitingBanner"));
    QVERIFY(chat && teamView);
    // At the top of the chat: above the team strip and the Activity view.
    auto *activity = window.findChild<QWidget *>("mainActivity");
    QVERIFY(activity);
    auto *page = activity->parentWidget();
    QVERIFY(page->isAncestorOf(chat));
    auto *pageLayout = static_cast<QVBoxLayout *>(page->layout());
    QCOMPARE(pageLayout->indexOf(chat->parentWidget()), 0);
    QVERIFY(pageLayout->indexOf(chat->parentWidget()) < pageLayout->indexOf(activity));
    // Above the lead's conversation in the Team tab.
    auto *conversation = dynamic_cast<TeamConversation *>(teamView->parentWidget());
    QVERIFY(conversation);
    QCOMPARE(static_cast<QVBoxLayout *>(conversation->layout())->indexOf(teamView), 0);
    // Both list the two items, folded, with the count.
    QTRY_VERIFY(!chat->isHidden() && chat->property("count").toInt() == 2);
    QCOMPARE(teamView->property("count").toInt(), 2);
    QCOMPARE(chat->findChild<QLabel *>("teamWaitingBannerCount")->text(), QStringLiteral("2 wait for you"));
    QVERIFY(teamView->findChild<QLabel *>("teamWaitingBannerCount"));
    // One notification per thing once both lists are read: the report (from
    // whichever list saw it first) and the plan; never both for the report.
    auto *controller = window.findChild<TeamController *>();
    QTRY_VERIFY(controller->teams().size() == 1);
    QTRY_COMPARE(raised.size(), 2);
    const auto tokens = [&] { QStringList found; for (const auto &args : raised) found << args.at(0).toString(); return found; };
    QCOMPARE(tokens().filter("report").size(), 1);   // ":report:" (attention) or "report_escalated" (waiting)
    QCOMPARE(tokens().filter("plan_draft").size(), 1);
    // Read again: nothing repeats.
    window.findChild<TeamWaitingFeed *>()->refresh();
    controller->refresh();
    QTest::qWait(600);
    QCOMPARE(raised.size(), 2);
    // Show all lists both, newest first.
    chat->findChild<QPushButton *>("teamWaitingBannerToggle")->click();
    QTRY_COMPARE(chat->findChildren<QWidget *>("teamWaitingBannerRow").size(), 2);
}

void TestSessionsWindow::teamTreeShowsSubteamsAndRollsUp()
{
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/collapsedTeams"); });
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    // G1: the child's lead session is also the parent's delegate, so the child
    // lead row must sit indented under the parent lead and fold with it.
    const QByteArray teams = R"({"items":[
      {"id":"team-1","name":"Parent","status":"running","lead":"lead","revision":3,
       "children":[{"team":"team-2","name":"Child","status":"running","done":0,"total":1}],
       "members":[{"id":"lead","role":"Lead","agent":"codex","state":"active","session":{"host":"","name":"codex/hgs/dashboard"}},
                  {"id":"childlead","role":"Tester","agent":"kimi","state":"active","session":{"host":"","name":"kimi/hgs/tester"}}],
       "tasks":[{"id":1,"title":"API","status":"in_progress","assignee":"lead","attempt":{"n":1,"state":"delivered"}},
                {"id":2,"title":"Tests","status":"pending","assignee":"childlead","subteam":{"team":"team-2","status":"running","done":0,"total":1}}]},
      {"id":"team-2","name":"Child","status":"running","lead":"childlead","revision":1,
       "parent":{"team":"team-1","task":2,"lead":"lead","member":"childlead"},
       "members":[{"id":"childlead","role":"Tester","agent":"kimi","state":"active","session":{"host":"","name":"kimi/hgs/tester"}}],
       "tasks":[{"id":1,"title":"Child work","status":"pending","assignee":"childlead"}]}]})";
    const QByteArray answer = "elif c=='team' and a[1:2]==['list']:print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument::fromJson(teams).toJson(QJsonDocument::Compact))) + ")\n"
                              "elif c=='team':print('{}')\n";
    const QString fixture = m_dir->filePath("hgs-tree");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    auto state = fleet(); auto box = state.local();
    SessionInfo tester = box.sessions.value(1); tester.name = "kimi/hgs/tester"; tester.project = "hgs"; tester.tag = "tester";
    box.sessions.append(tester); state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto rowOf = [&](const QString &key) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key) == key) return i; return -1; };
    auto *controller = window.findChild<TeamController *>();
    QVERIFY(controller);
    QTRY_COMPARE(controller->teams().size(), 2);
    QTRY_VERIFY(rowOf("\ncodex/hgs/dashboard") >= 0 && rowOf("\nkimi/hgs/tester") >= 0);
    const int parentRow = rowOf("\ncodex/hgs/dashboard");
    const int childRow = rowOf("\nkimi/hgs/tester");
    // The sub-team lead is indented under its parent's lead and carries no team
    // strip of the parent; the parent row rolls up the whole subtree.
    QCOMPARE(childRow, parentRow + 1);
    QVERIFY(list->item(childRow)->data(SessionRoles::Subordinate).toBool());
    const auto strip = list->item(parentRow)->data(SessionRoles::TeamLead).toMap();
    QCOMPARE(strip.value("rollup").toString(), QStringLiteral("2 sessions \u00b7 3 open"));
    QCOMPARE(strip.value("openTasks").toInt(), 3);
    if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty())
        { QTest::qWait(200); window.grab().save(QDir(shots).filePath("team-tree.png")); }
    // Folding the parent folds the whole subtree, including the child's lead.
    emit list->childrenToggled(QStringLiteral("\ncodex/hgs/dashboard"));
    QTRY_VERIFY(list->item(childRow)->isHidden());
}

void TestSessionsWindow::teamMembersFollowTheirSubLead_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QString("dark");
    QTest::newRow("light") << QString("light");
}

// U25 part 10 (spec 2.5): the member tree in the session list - the lead's
// direct reports one level in, a sub-lead's reports one more, and a line that
// ends at the last child of its level.
void TestSessionsWindow::teamMembersFollowTheirSubLead()
{
    QFETCH(QString, theme);
    QSettings().setValue("workspace/theme", theme);
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/collapsedTeams"); QSettings().remove("workspace/inspectorVisible"); });
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray teams = R"({"items":[{"id":"team-1","name":"Checkout","status":"running","lead":"lead","revision":3,
        "members":[{"id":"lead","role":"Lead","agent":"codex","state":"active","session":{"host":"","name":"codex/hgs/dashboard"}},
                   {"id":"qa","role":"Tester","agent":"kimi","state":"active","parent":"qalead","session":{"host":"","name":"kimi/hgs/tester"}},
                   {"id":"backend","role":"Backend","agent":"kimi","state":"active","session":{"host":"","name":"kimi/hgs/backend"}},
                   {"id":"qalead","role":"QA lead","agent":"kimi","state":"active","sub":true,"session":{"host":"","name":"kimi/hgs/qalead"}}],
        "tasks":[{"id":1,"title":"API","status":"in_progress","assignee":"backend","attempt":{"n":1,"state":"delivered"}},
                 {"id":2,"title":"Tests","status":"pending","assignee":"qa","deps":[{"task":1}]}]}]})";
    const QByteArray answer = "elif c=='team' and a[1:2]==['list']:print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument::fromJson(teams).toJson(QJsonDocument::Compact))) + ")\n"
                              "elif c=='team':print('{}')\n";
    const QString fixture = m_dir->filePath("hgs-subleads");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    auto state = fleet(); auto box = state.local();
    for (const QString tag : {"tester", "backend", "qalead"}) {
        SessionInfo member = box.sessions.value(1); member.name = "kimi/hgs/" + tag; member.project = "hgs"; member.tag = tag;
        box.sessions.append(member);
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1280, 860); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto rowOf = [&](const QString &key) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key) == key) return i; return -1; };
    QTRY_VERIFY(rowOf("\ncodex/hgs/dashboard") >= 0 && !list->item(rowOf("\ncodex/hgs/dashboard"))->data(SessionRoles::TeamLead).toMap().isEmpty());
    const int lead = rowOf("\ncodex/hgs/dashboard");
    // The tree order: Backend, then the QA lead followed by its Tester.
    QTRY_COMPARE(rowOf("\nkimi/hgs/backend"), lead + 1);
    QCOMPARE(rowOf("\nkimi/hgs/qalead"), lead + 2);
    QCOMPARE(rowOf("\nkimi/hgs/tester"), lead + 3);
    const auto data = [&](int row, int role) { return list->item(row)->data(role); };
    QCOMPARE(data(lead + 1, SessionRoles::TeamDepth).toInt(), 1);
    QCOMPARE(data(lead + 1, SessionRoles::TeamMember).toInt(), 1);   // a sibling follows
    QCOMPARE(data(lead + 2, SessionRoles::TeamDepth).toInt(), 1);
    QCOMPARE(data(lead + 2, SessionRoles::TeamMember).toInt(), 2);   // the last direct report ends the lead's line
    QCOMPARE(data(lead + 3, SessionRoles::TeamDepth).toInt(), 2);
    QCOMPARE(data(lead + 3, SessionRoles::TeamMember).toInt(), 2);
    // No tail: the lead's line does not run past the QA lead into its Tester.
    QCOMPARE(data(lead + 3, SessionRoles::TeamLines).toList(), QVariantList{false});
    // Level 1 is 22 px in, each level below 26 more.
    QCOMPARE(sessionCardIndent(list->model()->index(lead + 1, 0)), 22);
    QCOMPARE(sessionCardIndent(list->model()->index(lead + 3, 0)), 48);
    QCOMPARE(sessionCardIndent(list->model()->index(lead, 0)), 0);
    if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty()) {
        QTest::qWait(300);
        // QA #399 finding 4: the list is transparent; the panel paints the theme behind it.
        QVERIFY(window.findChild<QWidget *>("sessionListPanel")->grab().save(QDir(shots).filePath("ux-10-session-list-" + theme + ".png")));
    }
}

// U25 (spec 2.3): a member muted by its team records the owner's Unmute: hgs
// keeps only changes and revision 0 means "never chosen", so the desktop
// writes the mute it shows, then the unmute; the choice then wins over the team.
void TestSessionsWindow::teamMemberUnmuteRecordsItsChoice()
{
    const auto preferences = [](bool muted, qint64 revision) { return QJsonObject{{"conversation_id", "member-conversation"}, {"muted", muted},
        {"revision", revision}, {"suppressed", QJsonObject{{"reply_id", QJsonValue()}, {"attention_signature", QJsonValue()},
        {"optional_question_ids", QJsonArray()}, {"main_attention", QJsonValue()}, {"child_attention", QJsonObject()}}}}; };
    const QByteArray teams = R"({"items":[{"id":"team-1","name":"Docs","status":"running","lead":"lead","revision":1,
        "members":[{"id":"lead","role":"Lead","agent":"codex","state":"active","session":{"host":"","name":"codex/hgs/dashboard"}},
                   {"id":"writer","role":"Writer","agent":"kimi","state":"active","session":{"host":"","name":"kimi/docs/research"}}],
        "tasks":[]}]})";
    const QByteArray behavior = "elif c=='--help':print('session-notifications-v1')\n"
        "elif c=='notifications':\n"
        " payload=json.load(sys.stdin)\n"
        " with open(__file__+'.calls','a') as output:output.write(json.dumps({'argv':sys.argv[1:],'payload':payload})+'\\n')\n"
        " n=sum(1 for _ in open(__file__+'.calls'))\n"
        " print(open(__file__+'.reply%d'%n).read())\n"
        "elif c=='team' and a[1:2]==['list']:print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument::fromJson(teams).toJson(QJsonDocument::Compact))) + ")\n"
        "elif c=='team':print('{}')\n";
    const auto fixture = m_dir->filePath("team-member-mute"); QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + behavior)));
    const auto reply = [&](int call, bool muted, qint64 revision) {
        return FakeCli::replace(fixture + QString(".reply%1").arg(call), QJsonDocument(QJsonObject{{"ok", true}, {"applied", true}, {"run_id", "member-run"},
            {"conversation_id", "member-conversation"}, {"preferences", preferences(muted, revision)}}).toJson(QJsonDocument::Compact));
    };
    QVERIFY(reply(1, true, 1)); QVERIFY(reply(2, false, 2));
    const auto payloads = [&] {
        QList<QJsonObject> found; QFile file(fixture + ".calls");
        if (file.open(QIODevice::ReadOnly)) for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) found << QJsonDocument::fromJson(line).object()["payload"].toObject();
        return found;
    };
    auto state = fleet(); auto box = state.local();
    auto &member = box.sessions[1];
    member.runId = "member-run"; member.conversationId = "member-conversation";
    member.sharedNotifications = true; member.notificationPreferences = preferences(false, 0);
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window);
    connect(&window, &SessionsWindow::notificationPreferencesChanged, &window, [&](const QString &host, const QString &, const QString &, const QString &, const QJsonObject &value) {
        state.applyNotificationPreferences(host, state.local().sessions[1], value);
    });
    window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    const auto rowOf = [&](const QString &key) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString() == key) return i; return -1; };
    QTRY_VERIFY(rowOf("\nkimi/docs/research") >= 0 && list->item(rowOf("\nkimi/docs/research"))->data(SessionRoles::Muted).toBool());
    QVERIFY(list->item(rowOf("\nkimi/docs/research"))->data(SessionRoles::TeamMuted).toBool());
    QVERIFY(!list->item(rowOf("\ncodex/hgs/dashboard"))->data(SessionRoles::Muted).toBool());
    auto *menu = window.findChild<QMenu *>("sessionContextMenu"); QVERIFY(menu);
    const auto open = [&] {
        menu->hide(); const auto point = list->visualItemRect(list->item(rowOf("\nkimi/docs/research"))).center();
        QContextMenuEvent event(QContextMenuEvent::Mouse, point, list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(), &event);
    };
    open(); auto *toggle = menu->findChild<QAction *>("contextToggleMute"); QVERIFY(toggle);
    QCOMPARE(toggle->text(), QString("Unmute notifications"));
    toggle->trigger(); menu->hide();
    QTRY_COMPARE(payloads().size(), 2);
    QCOMPARE(payloads()[0], (QJsonObject{{"run_id", "member-run"}, {"conversation_id", "member-conversation"}, {"muted", true}, {"expected_revision", 0}}));
    QCOMPARE(payloads()[1], (QJsonObject{{"run_id", "member-run"}, {"conversation_id", "member-conversation"}, {"muted", false}, {"expected_revision", 1}}));
    window.setFleet(state);
    QTRY_VERIFY(!list->item(rowOf("\nkimi/docs/research"))->data(SessionRoles::Muted).toBool());
    QVERIFY(!list->item(rowOf("\nkimi/docs/research"))->data(SessionRoles::TeamMuted).toBool());
    open(); toggle = menu->findChild<QAction *>("contextToggleMute"); QCOMPARE(toggle->text(), QString("Mute notifications")); menu->hide();
    // The choice (revision 2) wins over the team default in any fleet.
    FleetState other = state; other.setTeamMembers({"\nkimi/docs/research"});
    QVERIFY(!other.local().sessions[1].muted);
    // A session that never chose follows its team.
    FleetState unchosen; auto fresh = state.local(); fresh.sessions[1].notificationPreferences = preferences(false, 0); unchosen.setLocal(fresh, 0);
    QVERIFY(!unchosen.local().sessions[1].muted);
    QVERIFY(unchosen.setTeamMembers({"\nkimi/docs/research"})); QVERIFY(unchosen.local().sessions[1].muted && unchosen.local().sessions[1].teamMuted);
}

// C2 of #499: the mute answer's signal reaches TrayAgent, which calls setFleet
// at once and frees the window's copy of the peer's box. The unmute is built
// from the fleet as it is after that, here with the revision another device
// wrote meanwhile, never from the freed box.
void TestSessionsWindow::teamMemberUnmuteOnAPeerReadsTheReplacedFleet()
{
    const auto preferences = [](bool muted, qint64 revision) { return QJsonObject{{"conversation_id", "member-conversation"}, {"muted", muted},
        {"revision", revision}, {"suppressed", QJsonObject{{"reply_id", QJsonValue()}, {"attention_signature", QJsonValue()},
        {"optional_question_ids", QJsonArray()}, {"main_attention", QJsonValue()}, {"child_attention", QJsonObject()}}}}; };
    const QByteArray teams = R"({"items":[{"id":"team-1","name":"Infra","status":"running","lead":"lead","revision":1,
        "members":[{"id":"lead","role":"Lead","agent":"codex","state":"active","session":{"host":"","name":"codex/hgs/dashboard"}},
                   {"id":"reviewer","role":"Reviewer","agent":"claude","state":"active","session":{"host":"mac","name":"claude/infra/review"}}],
        "tasks":[]}]})";
    const QByteArray behavior = "elif c=='--help':print('session-notifications-v1')\n"
        "elif c=='notifications':\n"
        " payload=json.load(sys.stdin)\n"
        " with open(__file__+'.calls','a') as output:output.write(json.dumps({'argv':sys.argv[1:],'payload':payload})+'\\n')\n"
        " n=sum(1 for _ in open(__file__+'.calls'))\n"
        " print(open(__file__+'.reply%d'%n).read())\n"
        "elif c=='team' and a[1:2]==['list']:print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument::fromJson(teams).toJson(QJsonDocument::Compact))) + ")\n"
        "elif c=='team':print('{}')\n";
    const auto fixture = m_dir->filePath("team-member-peer-mute"); QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + behavior)));
    const auto reply = [&](int call, bool muted, qint64 revision) {
        return FakeCli::replace(fixture + QString(".reply%1").arg(call), QJsonDocument(QJsonObject{{"ok", true}, {"applied", true}, {"run_id", "member-run"},
            {"conversation_id", "member-conversation"}, {"preferences", preferences(muted, revision)}}).toJson(QJsonDocument::Compact));
    };
    QVERIFY(reply(1, true, 1)); QVERIFY(reply(2, false, 3));
    const auto calls = [&] {
        QList<QJsonObject> found; QFile file(fixture + ".calls");
        if (file.open(QIODevice::ReadOnly)) for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) found << QJsonDocument::fromJson(line).object();
        return found;
    };
    auto state = fleet(); auto mac = *state.peer("mac");
    auto &member = mac.sessions[0];
    member.runId = "member-run"; member.conversationId = "member-conversation";
    member.sharedNotifications = true; member.notificationPreferences = preferences(false, 0);
    state.setPeer(mac, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window);
    const auto memberOf = [&] { return state.peer("mac")->sessions.constLast(); };
    // What TrayAgent does with the signal: its fleet takes the answer and
    // replaces the window's at once. Since then another device has muted it
    // again and the host lists more sessions, so a read of the freed box
    // cannot happen to find the new one in its place.
    connect(&window, &SessionsWindow::notificationPreferencesChanged, &window, [&](const QString &host, const QString &, const QString &, const QString &, const QJsonObject &value) {
        QVERIFY(state.applyNotificationPreferences(host, memberOf(), value));
        if (value.value("revision").toInteger() == 1) {
            QVERIFY(state.applyNotificationPreferences(host, memberOf(), preferences(true, 2)));
            auto listed = *state.peer(host);
            for (int i = 0; i < 40; ++i) {
                auto other = listed.sessions.constLast(); other.name = QString("claude/infra/other-%1").arg(i);
                other.runId = QString("other-run-%1").arg(i); other.conversationId = QString("other-conversation-%1").arg(i);
                listed.sessions.prepend(other);
            }
            state.setPeer(listed, QDateTime::currentMSecsSinceEpoch());
        }
        window.setFleet(state);
    });
    window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    const QString key = "mac\nclaude/infra/review";
    const auto rowOf = [&](const QString &wanted) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString() == wanted) return i; return -1; };
    WAIT_VERIFY(rowOf(key) >= 0 && list->item(rowOf(key))->data(SessionRoles::TeamMuted).toBool());
    auto *menu = window.findChild<QMenu *>("sessionContextMenu"); QVERIFY(menu);
    const auto point = list->visualItemRect(list->item(rowOf(key))).center();
    QContextMenuEvent event(QContextMenuEvent::Mouse, point, list->viewport()->mapToGlobal(point));
    QApplication::sendEvent(list->viewport(), &event);
    auto *toggle = menu->findChild<QAction *>("contextToggleMute"); QVERIFY(toggle);
    QCOMPARE(toggle->text(), QString("Unmute notifications"));
    toggle->trigger(); menu->hide();
    WAIT_COMPARE(calls().size(), 2);
    QCOMPARE(calls()[0]["argv"].toArray().first().toString(), QString("@mac"));
    QCOMPARE(calls()[0]["payload"].toObject(), (QJsonObject{{"run_id", "member-run"}, {"conversation_id", "member-conversation"}, {"muted", true}, {"expected_revision", 0}}));
    QCOMPARE(calls()[1]["argv"].toArray().first().toString(), QString("@mac"));
    QCOMPARE(calls()[1]["payload"].toObject(), (QJsonObject{{"run_id", "member-run"}, {"conversation_id", "member-conversation"}, {"muted", false}, {"expected_revision", 2}}));
    WAIT_VERIFY(!list->item(rowOf(key))->data(SessionRoles::Muted).toBool());
    QVERIFY(!memberOf().muted && !memberOf().teamMuted);
}

void TestSessionsWindow::teamRowBarsCountThePhaseInWork()
{
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    QJsonObject summary{{"id", "team-1"}, {"name", "Teams v2"}, {"status", "running"}, {"lead", "lead"},
                        {"pending_phase", 33}, {"revision", 40}};
    QJsonArray tasks;
    const auto addTask = [&](int id, int phase, const QString &status) {
        tasks.append(QJsonObject{{"id", id}, {"phase", phase}, {"title", QStringLiteral("Task %1").arg(id)},
                                 {"status", status}, {"assignee", "backend"}});
    };
    QJsonArray phase1Tasks, phase31, phase32, phase33, phase34;
    for (int id = 1; id <= 30; ++id) { addTask(id, 1, "accepted"); phase1Tasks.append(id); }
    for (int id = 271; id <= 286; ++id) { addTask(id, 31, "accepted"); phase31.append(id); }
    for (int id = 287; id <= 297; ++id) { addTask(id, 32, id == 292 ? "in_progress" : "accepted"); phase32.append(id); }
    for (int id = 298; id <= 308; ++id) { addTask(id, 33, "draft"); phase33.append(id); }
    for (int id = 309; id <= 319; ++id) { addTask(id, 34, "draft"); phase34.append(id); }
    summary["tasks"] = tasks;
    summary["phases"] = QJsonArray{QJsonObject{{"n", 1}, {"state", "approved"}, {"tasks", phase1Tasks}},
        QJsonObject{{"n", 31}, {"title", "Phase 1 fixes and urgent"}, {"state", "approved"}, {"tasks", phase31}},
        QJsonObject{{"n", 32}, {"title", "Permissions and lead load"}, {"state", "approved"}, {"tasks", phase32}},
        QJsonObject{{"n", 33}, {"title", "Team creation"}, {"state", "draft"}, {"tasks", phase33}},
        QJsonObject{{"n", 34}, {"title", "Reports and findings"}, {"state", "draft"}, {"tasks", phase34}}};
    summary["members"] = QJsonArray{QJsonObject{{"id", "lead"}, {"role", "Lead"}, {"agent", "codex"}, {"state", "active"},
        {"session", QJsonObject{{"host", ""}, {"name", "codex/hgs/dashboard"}}}}};
    const auto teams = QJsonDocument(QJsonObject{{"items", QJsonArray{summary}}}).toJson(QJsonDocument::Compact);
    const QByteArray answer = "elif c=='team' and a[1:2]==['list']:print(" + FakeCli::pythonString(QString::fromUtf8(teams)) + ")\n"
                              "elif c=='team':print('{}')\n";
    const QString fixture = m_dir->filePath("hgs-team-bars");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1280, 760); window.setFleet(fleet()); window.show();
    auto *list = window.findChild<SessionList *>("sessionList");
    auto *controller = window.findChild<TeamController *>();
    QVERIFY(list && controller);
    QTRY_COMPARE(controller->teams().size(), 1);
    const auto rowOf = [&](const QString &key) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key) == key) return i; return -1; };
    const int leadRow = rowOf("\ncodex/hgs/dashboard");
    QTRY_VERIFY(leadRow >= 0 && !list->item(leadRow)->data(SessionRoles::TeamLead).toMap().isEmpty());
    const auto strip = list->item(leadRow)->data(SessionRoles::TeamLead).toMap();
    QCOMPARE(strip.value("phaseText").toString(), QStringLiteral("10/11"));
    QCOMPARE(strip.value("phaseNumber").toInt(), 32);
    QCOMPARE(strip.value("phasesText").toString(), QStringLiteral("2/5"));
    QVERIFY(strip.value("progressTooltip").toString().contains(QStringLiteral("Phase 32 Permissions and lead load: 10/11")));
    QTest::qWait(200);
    if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty())
        window.grab().save(QDir(shots).filePath("team-row-phases.png"));
}

// N7: the Team tab's leftover Stop names a session, not the action. The window
// selects it, loads the details its decision needs, and shows the session's own
// stop confirmation with the real action; Cancel sends nothing and Yes stops
// exactly that session.
void TestSessionsWindow::teamLeftoverStopAsksTheRealAction()
{
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QJsonObject team = QJsonDocument::fromJson(R"({
     "id":"team-done","name":"Checkout","status":"done","lead":"lead","revision":4,
     "members":[{"id":"lead","role":"Lead","agent":"kimi","state":"active","session":{"host":"","name":"kimi/docs/research"}},
                {"id":"backend","role":"Backend","agent":"codex","state":"active","session":{"host":"","name":"codex/hgs/dashboard"}}],
     "tasks":[{"id":1,"title":"API","status":"accepted","assignee":"backend"}],
     "left_running":[{"member":"backend","role":"Backend",
                      "session":{"host":"","name":"codex/hgs/dashboard","run_id":"terminated-origin-run","conversation_id":"conversation-one"}}],
     "finish":{"by":"Lead","summary":"All done.","at":1800}})").object();
    const QByteArray listed = QJsonDocument(QJsonObject{{"items", QJsonArray{team}}}).toJson(QJsonDocument::Compact);
    const QByteArray status = QJsonDocument(team).toJson(QJsonDocument::Compact);
    const QByteArray answer =
        "elif c=='team' and a[1:2]==['list']:print(" + FakeCli::pythonString(QString::fromUtf8(listed)) + ")\n"
        "elif c=='team' and a[1:2]==['status']:print(" + FakeCli::pythonString(QString::fromUtf8(status)) + ")\n"
        "elif c=='team' and a[1:2]==['events']:print('{\"items\":[],\"next_cursor\":null}')\n"
        "elif c in ('kill','terminate'):open(__file__+'.ended','a').write(json.dumps(sys.argv[1:])+'\\n')\n"
        "elif c=='team':print('{}')\n";
    const QString fixture = m_dir->filePath("hgs-team-stop");
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    QFile::remove(fixture + ".ended");
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) if (session.name == "codex/hgs/dashboard") {
        session.state = "running"; session.runId = "terminated-origin-run"; session.conversationId = "conversation-one";
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.resize(1280, 820);
    window.setFleet(state); window.show();
    auto *inspector = window.findChild<QWidget *>("sessionInspector");
    QVERIFY(inspector);
    auto *panel = dynamic_cast<TeamPanel *>(window.findChild<QWidget *>("teamPanel"));
    QVERIFY(panel);
    // The window keeps its selected team only once the team list has arrived.
    auto *teams = window.findChild<TeamController *>();
    QVERIFY(teams);
    QTRY_VERIFY(teams->team("team-done"));
    auto *list = window.findChild<SessionList *>("sessionList");
    QVERIFY(list);
    emit list->teamActivated("team-done");   // as the list's team header does
    QTRY_VERIFY(inspectorCardOpen(window, "team"));
    QTRY_VERIFY(panel->findChild<QPushButton *>("teamLeftoverStop"));
    auto *client = window.findChild<HgsClient *>();
    QVERIFY(client);
    QSignalSpy writes(client, &HgsClient::writeDone);
    // The Stop handler loads the session's details first, so its confirmation
    // appears a little later: answer it from the event loop that dialog runs in.
    const auto answerDialog = [&](const std::function<void(QMessageBox *)> &action) {
        auto *timer = new QTimer(&window);
        timer->setInterval(20);
        QObject::connect(timer, &QTimer::timeout, &window, [&, timer, action] {
            auto *confirm = window.findChild<QMessageBox *>("terminateSessionConfirm");
            if (!confirm) return;   // the details are still loading
            timer->stop();
            action(confirm);
        });
        timer->start();
    };
    bool asked = false;
    answerDialog([&](QMessageBox *confirm) {
        asked = true;
        // The real action for this tracked codex member: stop and archive. The
        // old Team text promised the history stays; this names what happens.
        QVERIFY(confirm->text().contains(QStringLiteral("move its recorded conversation to Archive")));
        QString action;
        for (auto *button : confirm->buttons()) if (button->text().startsWith(QStringLiteral("Stop and archive"))) action = button->text();
        QCOMPARE(action, QStringLiteral("Stop and archive"));
        if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty())
            confirm->grab().save(QDir(shots).filePath("leftover-stop-confirm-dark.png"));
        confirm->done(QMessageBox::Cancel);
    });
    panel->findChild<QPushButton *>("teamLeftoverStop")->click();
    QTRY_VERIFY(asked);
    QTest::qWait(100);
    // Cancel sends nothing.
    QVERIFY(writes.isEmpty());
    QVERIFY(!QFileInfo::exists(fixture + ".ended"));
    // Yes stops exactly that session.
    bool confirmed = false;
    answerDialog([&](QMessageBox *confirm) {
        confirmed = true;
        confirm->done(QMessageBox::Yes);
    });
    panel->findChild<QPushButton *>("teamLeftoverStop")->click();
    QTRY_VERIFY(confirmed);
    QTRY_COMPARE(writes.size(), 1);
    QFile ended(fixture + ".ended");
    QVERIFY(ended.open(QIODevice::ReadOnly));
    QVERIFY(ended.readAll().contains("codex/hgs/dashboard"));
}

void TestSessionsWindow::leadSessionHeaderCarriesTheTeam()
{
    const QStringList keys{"workspace/skin", "workspace/theme", "workspace/organization", "workspace/inspectorVisible", "workspace/collapsedTeams"};
    const auto restore = qScopeGuard([keys] { for (const auto &key : keys) removeSetting(key); sessionCardTileInset() = 0; });
    QSettings().setValue("workspace/skin", "whitewh");
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    QSettings().setValue("workspace/inspectorVisible", false);
    const double now = QDateTime::currentSecsSinceEpoch();
    const auto plain = ReadmeShots::fleet(now);
    const QJsonObject data{{"inspect", ReadmeShots::conversations(plain, now)}, {"accounts", ReadmeShots::accounts(now)}, {"team", ReadmeShots::team(now)},
                           {"feed", ReadmeShots::feed(now)}, {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()}};
    const QString fixture = m_dir->filePath("hgs-lead-header");
    QVERIFY(FakeCli::install(fixture, ReadmeShots::program));
    QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
    const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    for (const QString theme : {"dark", "light"}) {
        QSettings().setValue("workspace/theme", theme);
        SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 900); window.setFleet(plain); window.show();
        auto *controller = window.findChild<TeamController *>(); QVERIFY(controller);
        QTRY_COMPARE_WITH_TIMEOUT(controller->teams().size(), 1, 10000);
        window.showSession({}, "claude/orbit/checkout");
        auto *details = window.findChild<QWidget *>("sessionDetailPanel"); QVERIFY(details);
        QWidget *status = nullptr;
        QTRY_VERIFY_WITH_TIMEOUT((status = details->findChild<QWidget *>("teamHeaderStatus")) && status->isVisible(), 10000);
        QCOMPARE(status->property("status").toString(), QStringLiteral("running"));
        // After the title, before the session's own state pill.
        auto *title = window.findChild<QLabel *>("detailTitle"); auto *badge = window.findChild<QLabel *>("badge");
        const auto x = [details](QWidget *w) { return w->mapTo(details, QPoint()).x(); };
        QVERIFY(x(status) > x(title)); QVERIFY(x(status) + status->width() < x(badge));
        QVERIFY(qAbs(status->mapTo(details, QPoint()).y() - title->mapTo(details, QPoint()).y()) < title->height());
        // Phase 1 is the whole plan here: 1 of 7 decided, one phase, an hour in.
        auto *tasks = details->findChild<QWidget *>("teamHeaderTasks"); QVERIFY(tasks && tasks->isVisible());
        QCOMPARE(tasks->property("done").toInt(), 1); QCOMPARE(tasks->property("total").toInt(), 7);
        QVERIFY(details->findChild<QWidget *>("teamHeaderPhase")); QVERIFY(details->findChild<QWidget *>("teamHeaderPhases"));
        auto *elapsed = details->findChild<QLabel *>("teamHeaderElapsedText"); QVERIFY(elapsed);
        QVERIFY(elapsed->text().startsWith("1h") || elapsed->text() == "60m");
        // Running: Pause and Stop… at the right of the chips row; nothing decided yet, so no Finish.
        auto *pause = details->findChild<QPushButton *>("teamWorkspacePause"); auto *stop = details->findChild<QPushButton *>("teamWorkspaceStop");
        QVERIFY(pause && pause->isVisible() && stop && stop->isVisible());
        QVERIFY(!details->findChild<QPushButton *>("teamWorkspaceFinish"));
        auto *provider = window.findChild<QWidget *>("sessionProviderBadge");
        QVERIFY(qAbs(pause->mapTo(details, QPoint()).y() + pause->height() / 2 - provider->mapTo(details, QPoint()).y() - provider->height() / 2) <= 4);
        QVERIFY(x(stop) + stop->width() > details->width() - 40);
        // The Team tab has no name bar of its own.
        auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
        int teamTab = -1; for (int i = 0; i < tabs->count(); ++i) if (tabs->tabText(i) == "Team") teamTab = i;
        QVERIFY(teamTab >= 0);
        tabs->setCurrentIndex(teamTab);
        QTRY_VERIFY_WITH_TIMEOUT(window.findChild<QWidget *>("teamPhaseGraph"), 10000);
        QVERIFY(!window.findChild<QWidget *>("teamWorkspaceBar"));
        QVERIFY(!window.findChild<QWidget *>("teamWorkspaceHeader")->isVisible());
        // The header keeps the team whichever tab is open (it is rebuilt on a refresh).
        auto *progress = window.findChild<QWidget *>("teamHeaderProgress"); auto *actions = window.findChild<QWidget *>("teamHeaderActions");
        QVERIFY(details->isAncestorOf(progress) && details->isAncestorOf(actions));
        QVERIFY(progress->isVisible() && actions->isVisible());
        tabs->setCurrentIndex(0);
        // The composer row: three zones, the centre under the middle of the field.
        auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
        auto *toolbar = composer->toolbar(); QVERIFY(toolbar->threeZones());
        auto *card = composer->findChild<QFrame *>("composerCard"); QVERIFY(card && card->isVisible());
        auto *reference = composer->findChild<QPushButton *>("copySessionReference");
        auto *attach = composer->findChild<QPushButton *>("attachMessageFile");
        auto *model = composer->findChild<QPushButton *>("sessionModelSettings");
        auto *context = composer->findChild<QWidget *>("activityContext");
        QTRY_VERIFY(reference->isVisible());
        QVERIFY(toolbar->isAncestorOf(reference) && toolbar->isAncestorOf(attach) && toolbar->isAncestorOf(model));
        QVERIFY(toolbar->mapTo(composer, QPoint()).y() >= card->mapTo(composer, QPoint()).y() + card->height());
        const auto cx = [composer](QWidget *w) { return w->mapTo(composer, QPoint()).x(); };
        QVERIFY(cx(attach) < cx(context)); QVERIFY(cx(context) < cx(reference)); QVERIFY(cx(reference) + reference->width() < cx(model));
        QVERIFY(cx(attach) < composer->width() / 3); QVERIFY(cx(model) > composer->width() * 2 / 3);
        auto *stopInCard = composer->findChild<QPushButton *>("interruptAgent"); QVERIFY(card->isAncestorOf(stopInCard));
        if (!shots.isEmpty()) {
            QTest::qWait(300);
            const int headerBottom = tabs->tabBar()->mapTo(details, QPoint(0, tabs->tabBar()->height())).y();
            // From the window, so the panels' own backgrounds show.
            const auto picture = window.grab();
            const QPoint at = details->mapTo(&window, QPoint());
            QVERIFY(picture.copy(QRect(at, QSize(details->width(), headerBottom + 4))).save(QDir(shots).filePath(QStringLiteral("ux-3-lead-header-%1.png").arg(theme))));
            const QPoint composerAt = composer->mapTo(&window, QPoint(-12, -8));
            QVERIFY(picture.copy(QRect(composerAt, composer->size() + QSize(24, 16))).save(QDir(shots).filePath(QStringLiteral("ux-3-composer-%1.png").arg(theme))));
        }
        // A member's header names no team actions (its Team tab stays).
        window.showSession({}, "dsh/orbit/checkout-api");
        QTRY_VERIFY_WITH_TIMEOUT(!progress->isVisible() && !actions->isVisible(), 10000);
        QVERIFY(!window.findChild<QWidget *>("teamHeaderStatus"));
        QTRY_VERIFY(window.findChild<QLabel *>("detailTitle")->maximumWidth() == QWIDGETSIZE_MAX);
    }
}

// U57: the lead's own session shows its team's open proposals as cards above
// the composer, read from the polled team list; a member's session shows
// none. Allow sends decide-proposal with the proposal's id and its hash.
void TestSessionsWindow::leadSessionAnswersAProposalOnItsCard()
{
    const QStringList keys{"workspace/skin", "workspace/theme", "workspace/organization", "workspace/inspectorVisible", "workspace/collapsedTeams"};
    const auto restore = qScopeGuard([keys] { for (const auto &key : keys) removeSetting(key); });
    QSettings().setValue("workspace/skin", "whitewh");
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    QSettings().setValue("workspace/inspectorVisible", false);
    const double now = QDateTime::currentSecsSinceEpoch();
    const auto plain = ReadmeShots::fleet(now);
    auto team = ReadmeShots::team(now);
    const QString hash = QStringLiteral("5d1c0e9a7b3f4c2d8e6a1b0c9f8e7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c");
    const QJsonObject designer{{"id", "designer"}, {"role", "Designer"}, {"agent", "claude"}, {"model", "sonnet"}};
    auto second = designer; second["id"] = "designer-2";
    team["proposals"] = QJsonArray{
        QJsonObject{{"id", "p1"}, {"kind", "add-member"}, {"by", "Lead"}, {"at", now - 120}, {"state", "open"}, {"hash", hash},
                    {"reason", "The checkout form and the receipt page need a designer each"},
                    {"change", QJsonObject{{"members", QJsonArray{designer, second}}}}},
        QJsonObject{{"id", "p0"}, {"kind", "remove-member"}, {"by", "Lead"}, {"at", now - 900}, {"state", "rejected"}, {"member", "tester"},
                    {"reason", "Later"}}};
    const QJsonObject data{{"inspect", ReadmeShots::conversations(plain, now)}, {"accounts", ReadmeShots::accounts(now)}, {"team", team},
                           {"feed", ReadmeShots::feed(now)}, {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()}};
    const QString fixture = m_dir->filePath("hgs-lead-proposal");
    QVERIFY(FakeCli::install(fixture, ReadmeShots::program));
    QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
    const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    for (const QString theme : {"dark", "light"}) {
        QSettings().setValue("workspace/theme", theme);
        QFile::remove(fixture + ".calls");
        SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 900); window.setFleet(plain); window.show();
        auto *controller = window.findChild<TeamController *>(); QVERIFY(controller);
        QTRY_COMPARE_WITH_TIMEOUT(controller->teams().size(), 1, 10000);
        // A member's session has no cards.
        window.showSession({}, "dsh/orbit/checkout-api");
        auto *cards = window.findChild<QWidget *>("mainProposalCards"); QVERIFY(cards);
        QTest::qWait(100);
        QVERIFY(!cards->isVisible());
        window.showSession({}, "claude/orbit/checkout");
        QFrame *card = nullptr;
        QTRY_VERIFY_WITH_TIMEOUT((card = cards->findChild<QFrame *>("teamProposalCard")) && card->isVisible(), 10000);
        QCOMPARE(cards->findChildren<QFrame *>("teamProposalCard").size(), 1);   // the decided one is gone
        QCOMPARE(card->property("proposal").toString(), QStringLiteral("p1"));
        QCOMPARE(card->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Add 2 members"));
        QStringList lines; for (auto *line : card->findChildren<QLabel *>("teamProposalChange")) lines << line->text();
        QCOMPARE(lines, (QStringList{"Designer (claude, sonnet)", "Designer (claude, sonnet)"}));
        QVERIFY(card->findChild<QLabel *>("teamProposalBy")->text().startsWith("Lead asks, "));
        QVERIFY(card->findChild<QLabel *>("teamProposalReason")->text().contains("need a designer each"));
        // Above the composer, in the Activity column.
        auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
        QVERIFY(card->mapTo(&window, QPoint()).y() < composer->mapTo(&window, QPoint()).y());
        if (!shots.isEmpty()) { TestWait::settle(); QVERIFY(window.grab().save(QDir(shots).filePath(QStringLiteral("u57-session-card-%1.png").arg(theme)))); }
        card->findChild<QPushButton *>("teamProposalAllow")->click();
        QJsonObject input;
        QTRY_VERIFY_WITH_TIMEOUT(([&] {
            QFile file(fixture + ".calls");
            if (!file.open(QIODevice::ReadOnly)) return false;
            for (const auto &line : file.readAll().split('\n')) {
                const auto call = QJsonDocument::fromJson(line).object();
                if (call["args"].toArray().at(1).toString() == "decide-proposal") input = call["input"].toObject();
            }
            return !input.isEmpty();
        })(), 10000);
        QCOMPARE(input["team"].toString(), QStringLiteral("team-checkout"));
        QCOMPARE(input["proposal"].toString(), QStringLiteral("p1"));
        QVERIFY(input["accept"].toBool());
        QCOMPARE(input["expected_hash"].toString(), hash);
        QCOMPARE(input["by"].toString(), QStringLiteral("human"));
        QVERIFY(!input.contains("reason"));
    }
}

// U60 (#656): a computer without a model server shows no Model servers
// button and behaves as before; with one, the rail's button opens the tab.
void TestSessionsWindow::modelServersRailButtonOnlyWithAServer()
{
    const QStringList keys{"workspace/skin", "workspace/theme", "workspace/organization", "workspace/inspectorVisible"};
    const auto restore = qScopeGuard([keys] { for (const auto &key : keys) removeSetting(key); });
    QSettings().setValue("workspace/skin", "whitewh");
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    const double now = QDateTime::currentSecsSinceEpoch();
    const auto plain = ReadmeShots::fleet(now);
    for (const bool configured : {false, true}) {
        QJsonObject data{{"inspect", ReadmeShots::conversations(plain, now)}, {"accounts", ReadmeShots::accounts(now)}, {"team", ReadmeShots::team(now)},
                         {"feed", ReadmeShots::feed(now)}, {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()}};
        if (configured)
            data["model_servers"] = QJsonObject{{"servers", QJsonArray{QJsonObject{{"name", "lab"}, {"base_url", "http://192.0.2.10:8000/v1"}, {"kind", "openai"},
                                                                                   {"ssh", ""}, {"key", false}, {"argv", QJsonObject{}}}}}};
        const QString fixture = m_dir->filePath(configured ? "hgs-model-servers" : "hgs-no-model-servers");
        QVERIFY(FakeCli::install(fixture, ReadmeShots::program));
        QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
        SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 900); window.setFleet(plain); window.show();
        QPushButton *nav = nullptr;
        for (auto *button : window.findChildren<QPushButton *>()) if (button->property("modelServersNav").toBool()) nav = button;
        QVERIFY(nav);
        if (!configured) {
            QTest::qWait(1500);
            QVERIFY(nav->isHidden());
            QVERIFY(!window.findChild<QWidget *>("modelServersPage"));
            continue;
        }
        QTRY_VERIFY_WITH_TIMEOUT(!nav->isHidden(), 10000);
        QCOMPARE(nav->toolTip(), QStringLiteral("Model servers"));
        nav->click();
        QWidget *page = nullptr;
        QTRY_VERIFY((page = window.findChild<QWidget *>("modelServersPage")) && page->isVisible());
        QTRY_VERIFY(nav->isChecked());
        QFrame *card = nullptr;
        QTRY_VERIFY((card = page->findChild<QFrame *>("modelServerCard")) && card->property("server") == "lab");
    }
}

// U25 (spec 3, 4): the lead session's header carries its team after the title
// (status, the three bars, the time) and the team's actions at the right of
// the chips; a member's header does not, and the Team tab has no name bar.
// The compact composer's row under the field has three zones, the centre one
// centred under the field. ZERUS_TEAM_SHOTS=<dir> keeps both, dark and light.
// N7 (#364, U18): a team's attention item reaches the desktop notifications
// once (TrayAgent posts teamAttentionRaised to its AttentionNotifier), is
// withdrawn when it is gone, and its click opens the team.
void TestSessionsWindow::teamAttentionReachesTheNotifications()
{
    const QStringList keys{"workspace/organization", "workspace/inspectorVisible", "teams/attentionNotified"};
    const auto restore = qScopeGuard([keys] { for (const auto &key : keys) removeSetting(key); });
    QSettings().remove("teams/attentionNotified");
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    const double now = QDateTime::currentSecsSinceEpoch();
    const auto plain = ReadmeShots::fleet(now);
    auto team = ReadmeShots::team(now);
    team["attention"] = QJsonArray{QJsonObject{{"kind", "permission"}, {"member", "Backend"}, {"since", now - 1800}, {"minutes", 30},
        {"summary", "Backend waits for your answer: Bash cargo test -p payments"},
        {"command", R"({"request_id":"<new request id>","team":"team-checkout","member":"backend"} | hgs team answer --json)"}}};
    QJsonObject data{{"inspect", ReadmeShots::conversations(plain, now)}, {"accounts", ReadmeShots::accounts(now)}, {"team", team},
                     {"feed", ReadmeShots::feed(now)}, {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()}};
    const QString fixture = m_dir->filePath("hgs-team-attention");
    QVERIFY(FakeCli::install(fixture, ReadmeShots::program));
    QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 900); window.setFleet(plain);
    QSignalSpy raised(&window, &SessionsWindow::teamAttentionRaised), cleared(&window, &SessionsWindow::teamAttentionCleared);
    window.show();
    auto *controller = window.findChild<TeamController *>(); QVERIFY(controller);
    QTRY_COMPARE_WITH_TIMEOUT(raised.size(), 1, 10000);
    QCOMPARE(raised[0][1].toString(), QStringLiteral("Team Checkout v2 needs you"));
    QCOMPARE(raised[0][2].toString(), QStringLiteral("Backend waits for your answer: Bash cargo test -p payments · 30 min"));
    QVERIFY(raised[0][0].toString().startsWith(QStringLiteral("zerus-team-attention:team-checkout:permission:Backend:")));
    // Later polls: the same item, no second notification.
    for (int poll = 0; poll < 3; ++poll) { controller->refresh(); QTest::qWait(300); }
    QCOMPARE(raised.size(), 1); QCOMPARE(cleared.size(), 0);
    // A click opens the team.
    window.showTeamAttention(raised[0][0].toString());
    QTRY_VERIFY(window.findChild<QWidget *>("teamWorkspace") && window.findChild<QWidget *>("teamWorkspace")->isVisible());
    // Answered: the notification is withdrawn.
    team.remove("attention"); data["team"] = team;
    QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
    QTRY_VERIFY_WITH_TIMEOUT(([&] { controller->refresh(); return cleared.size() == 1; })(), 10000);
    QCOMPARE(cleared[0][0].toString(), raised[0][0].toString());
    QCOMPARE(raised.size(), 1);
    // N32 of #499: the tray raised a notice before this window existed; a click
    // creates the window, whose teams are not read yet: the team opens once they are.
    SessionsWindow opened(FakeCli::program(fixture)); opened.resize(1440, 900); opened.setFleet(plain); opened.show();
    auto *fresh = opened.findChild<TeamController *>(); QVERIFY(fresh && fresh->teams().isEmpty());
    opened.showTeamAttention(raised[0][0].toString());
    WAIT_VERIFY(opened.findChild<QWidget *>("teamWorkspace") && opened.findChild<QWidget *>("teamWorkspace")->isVisible());
}

// N12 (#382): from the lead's session (its team's pill, Pause and Stop in the
// header) to the Team designer's session: no team header and no team action
// stays behind to act on that team.
void TestSessionsWindow::designerSessionDropsTheTeamHeader()
{
    const QStringList keys{"workspace/skin", "workspace/organization", "workspace/inspectorVisible", "teams/attentionNotified"};
    const auto restore = qScopeGuard([keys] { for (const auto &key : keys) removeSetting(key); sessionCardTileInset() = 0; });
    QSettings().setValue("workspace/skin", "whitewh");
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    QSettings().setValue("workspace/inspectorVisible", false);
    const double now = QDateTime::currentSecsSinceEpoch();
    QTemporaryDir git; QVERIFY(git.isValid()); QVERIFY(QDir(git.path()).mkdir(".git"));
    const auto fleet = ReadmeShots::fleet(now, git.path());
    const QJsonObject data{{"inspect", ReadmeShots::conversations(fleet, now)}, {"accounts", ReadmeShots::accounts(now)}, {"team", ReadmeShots::team(now)},
                           {"feed", ReadmeShots::feed(now)}, {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()}};
    const QString fixture = m_dir->filePath("hgs-designer-header");
    QVERIFY(FakeCli::install(fixture, ReadmeShots::program));
    QVERIFY(FakeCli::replace(fixture + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
    SessionsWindow window(FakeCli::program(fixture)); window.resize(1440, 900); window.setFleet(fleet); window.show();
    auto *controller = window.findChild<TeamController *>(); QVERIFY(controller);
    QTRY_COMPARE_WITH_TIMEOUT(controller->teams().size(), 1, 10000);
    auto *details = window.findChild<QWidget *>("sessionDetailPanel"); QVERIFY(details);
    const auto shown = [details](const char *name) { auto *w = details->findChild<QWidget *>(name); return w && w->isVisible(); };
    window.showSession({}, "claude/orbit/checkout");
    QTRY_VERIFY_WITH_TIMEOUT(shown("teamHeaderStatus") && shown("teamWorkspacePause"), 10000);
    window.showSession({}, "claude/team-designer/team-designer");
    QTRY_VERIFY(!shown("teamHeaderStatus"));
    QVERIFY(!shown("teamWorkspacePause")); QVERIFY(!shown("teamWorkspaceStop"));
    // And back: the lead's header returns.
    window.showSession({}, "claude/orbit/checkout");
    QTRY_VERIFY(shown("teamHeaderStatus") && shown("teamWorkspacePause"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_teams.moc"
