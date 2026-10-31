// WhiteWh fork, #713: the agent-team UI tests of Graph, Board and Layers,
// their toolbar, header and skin, and polls that redraw nothing.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "Settings.h"
#include "ActivityView.h"
#include "AgentWorking.h"
#include "CollapsibleBlock.h"
#include "HgsClient.h"
#include "TeamHeat.h"
#include "TeamTaskCard.h"
#include "TeamWorkspace.h"
#include "WorkspaceStyle.h"

namespace {

// U25 part 5 (owner, 2026-10-27): one task per activity-label rule, plus a
// problem that must win over the label.
QJsonObject labelTeam()
{
    return QJsonDocument::fromJson(R"({
 "id":"team-1","name":"Labels","status":"running","project":"Checkout","folder":"/work/labels","lead":"lead",
 "revision":5,"created":1000,"updated":5000,"settings":{},"layers":{},"cycle":[],"ready":[],"validation":{"errors":[],"warnings":[]},
 "members":[
  {"id":"lead","role":"Lead","agent":"claude","state":"active","session":{"host":"","name":"claude/labels/lead"}},
  {"id":"backend","role":"Backend","agent":"dsh","state":"active","session":{"host":"","name":"dsh/labels/backend"}},
  {"id":"blocked","role":"Tester","agent":"codex","state":"active","session":{"host":"","name":"codex/labels/blocked"}}],
 "tasks":[
  {"id":1,"title":"Review me","assignee":"backend","status":"submitted","attempts":[{"n":1,"id":"a1","member":"backend","state":"reported"}]},
  {"id":2,"title":"Second try","assignee":"backend","status":"in_progress","attempts":[{"n":2,"id":"a2","member":"backend","state":"delivered","started_at":1200}]},
  {"id":3,"title":"First try","assignee":"backend","status":"in_progress","attempts":[{"n":1,"id":"a3","member":"backend","state":"delivered","started_at":1300}]},
  {"id":4,"title":"Held back","assignee":"backend","status":"pending","hold":{"by":"Lead","at":1400,"reason":"ask first"}},
  {"id":5,"title":"Delegated","assignee":"backend","status":"in_progress","subteam":{"team":"team-2","done":1,"total":2},"attempts":[{"n":1,"id":"a5","member":"backend","state":"delivered","started_at":1500}]},
  {"id":6,"title":"Queued","assignee":"backend","status":"pending","waits":{"for":"member","on":["Backend"],"unblock":"wait for Backend"},"attempts":[{"n":1,"id":"a6","member":"backend","state":"rejected"}]},
  {"id":7,"title":"Still a draft","assignee":"backend","status":"draft"},
  {"id":8,"title":"Done","assignee":"backend","status":"accepted"},
  {"id":9,"title":"Offline problem","assignee":"blocked","status":"in_progress","attempts":[{"n":2,"id":"a9","member":"blocked","state":"delivered","started_at":1600}]},
  {"id":10,"title":"Handed over","assignee":"lead","status":"in_progress","attempts":[{"n":1,"id":"a10","member":"lead","state":"delivered","delivered_at":1700}]},
  {"id":11,"title":"Turn ended","assignee":"lead","status":"in_progress","attempts":[{"n":1,"id":"a11","member":"lead","state":"delivered","started_at":1800,"turn_ended_at":1900}]},
  {"id":12,"title":"Not needed","assignee":"lead","status":"cancelled"}],
 "layers":{"1":0,"2":1,"3":1,"4":2,"5":2,"6":3,"7":3,"8":4,"9":1}})").object();
}
}

// U37: the labels' team with each task's timing as hgs status computes it:
// #2 runs its second attempt, #3 its first, #8 is done (its decision was not
// recorded), #9 runs behind a problem, #11's turn ended. The clock is 9000.
static QJsonObject timingTeam()
{
    auto t = labelTeam();
    const auto span = [](int n, const char *state, double from, double to, int worked, int member, int lead) {
        return QJsonObject{{"n", n}, {"member", "backend"}, {"state", state}, {"from", from}, {"to", to > 0 ? QJsonValue(to) : QJsonValue()},
                           {"worked_s", worked}, {"member_s", member}, {"lead_s", lead}};
    };
    const auto timing = [](int worked, double since, int deps, int member, int lead, const QJsonArray &attempts, const QJsonArray &unknown = {}) {
        QJsonObject o{{"worked_s", worked}, {"running_since", since > 0 ? QJsonValue(since) : QJsonValue()},
                      {"waits", QJsonObject{{"deps_s", deps}, {"member_s", member}, {"lead_s", lead}}}, {"attempts", attempts}};
        if (!unknown.isEmpty()) o["unknown"] = unknown;
        return o;
    };
    const QHash<int, QJsonObject> timings{
        {1, timing(600, 0, 0, 60, 300, {span(1, "reported", 1000, 1600, 600, 60, 300)})},
        {2, timing(5400, 5100, 2400, 180, 600, {span(1, "rejected", 2000, 3500, 1500, 120, 600), span(2, "delivered", 5100, 0, 3900, 60, 0)})},
        {3, timing(720, 8280, 0, 30, 0, {span(1, "delivered", 8280, 0, 720, 30, 0)})},
        {4, timing(0, 0, 0, 0, 300, {})},
        {8, timing(4500, 0, 0, 0, 0, {span(1, "reported", 1000, 5500, 4500, 0, 0)}, {"lead"})},
        {9, timing(30, 8970, 0, 0, 0, {span(2, "delivered", 8970, 0, 30, 0, 0)})},
        {11, timing(100, 0, 0, 0, 0, {span(1, "delivered", 1800, 1900, 100, 0, 0)})},
    };
    auto tasks = t["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (timings.contains(task["id"].toInt())) task["timing"] = timings.value(task["id"].toInt());
        tasks[i] = task;
    }
    t["tasks"] = tasks;
    return t;
}

// Owner 2026-10-31 (#746): the timing team with #745's estimates: #1 done
// within its estimate, #2 running past it, #3 running within it (until the
// clock passes 45 minutes), #4 not started, #5 with none yet and #8 done past
// it. The other tasks come from an older hgs: no estimate fields.
static QJsonObject estimateTeam()
{
    auto t = timingTeam();
    const QHash<int, std::pair<double, QString>> estimates{
        {1, {15, "history"}}, {2, {40, "role"}}, {3, {45, "lead"}}, {4, {30, "history"}}, {5, {-1, "none"}}, {8, {60, "history"}}};
    auto tasks = t["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        const int id = task["id"].toInt();
        if (!estimates.contains(id)) continue;
        const auto [minutes, source] = estimates.value(id);
        task["estimate_minutes"] = minutes > 0 ? QJsonValue(minutes) : QJsonValue();
        task["estimate_source"] = source;
        task["estimate_text"] = minutes > 0 ? QStringLiteral("%1 min").arg(minutes) : QStringLiteral("no estimate yet");
        tasks[i] = task;
    }
    t["tasks"] = tasks;
    return t;
}

// U58: the phased team with a cancelled task in phase 2 and an approved phase 4
// whose two tasks were cancelled (copies the lead made under new ids).
static QJsonObject cancelledTeam()
{
    auto t = phasedTeam();
    auto phases = t["phases"].toArray();
    auto two = phases[0].toObject(); two["tasks"] = QJsonArray{3, 4, 6}; phases[0] = two;
    phases.append(QJsonObject{{"n", 4}, {"title", "Dropped copies"}, {"barrier", false}, {"approval", "review"}, {"state", "approved"},
                              {"tasks", QJsonArray{7, 8}}, {"added_by", "Lead"}, {"added_at", 1800.0}, {"decided_by", "You"}, {"decided_at", 1900.0}});
    t["phases"] = phases;
    auto tasks = t["tasks"].toArray();
    tasks.append(QJsonObject{{"id", 6}, {"phase", 2}, {"title", "Old fix"}, {"assignee", "backend"}, {"status", "cancelled"}});
    tasks.append(QJsonObject{{"id", 7}, {"phase", 4}, {"title", "Copy of the review"}, {"assignee", "backend"}, {"status", "cancelled"}});
    tasks.append(QJsonObject{{"id", 8}, {"phase", 4}, {"title", "Copy of the tests"}, {"assignee", "tester"}, {"status", "cancelled"}});
    t["tasks"] = tasks;
    auto layers = t["layers"].toObject(); layers["6"] = 1; layers["7"] = 0; layers["8"] = 0; t["layers"] = layers;
    return t;
}
namespace {
// The part-1 pieces together, as the shots show them in either theme.
QWidget *foundationShowcase(bool dark)
{
    const auto &skin = WorkspaceSkin::active();
    const auto t = [&skin, dark](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)); };
    const auto c = TeamUi::colorsFor(dark);
    auto *page = new QWidget; page->setObjectName("uxFoundation");
    page->setStyleSheet(QStringLiteral("QWidget#uxFoundation{background:%1;}QLabel{color:%2;background:transparent;}").arg(t("bg").name(), t("fg").name())
                        + TeamUi::teamSheet(c) + workspaceScrollbars(dark));
    auto *v = new QVBoxLayout(page); v->setContentsMargins(16, 14, 16, 14); v->setSpacing(12);
    const auto caption = [&](const QString &text) { auto *l = TeamUi::label(text, t("label"), 11); v->addWidget(l); };
    caption(QStringLiteral("Role palette"));
    auto *roles = new TeamUi::FlowLayout(6);
    for (const auto &role : WorkspaceSkin::paletteRoles()) roles->addWidget(new TeamUi::Pill(role, TeamUi::roleColor(role, dark)));
    v->addLayout(roles);
    caption(QStringLiteral("Icons"));
    auto *icons = TeamUi::row(10);
    auto *gear = new QPushButton; gear->setProperty("ghost", true); gear->setIcon(workspaceIcon("settings", t("muted")));
    gear->setIconSize(QSize(19, 19)); gear->setFixedSize(30, 28); gear->setToolTip("Settings");
    icons->addWidget(gear);
    auto *chip = new QPushButton(QStringLiteral("2")); chip->setProperty("chip", true); chip->setIcon(workspaceIcon("settings", t("muted")));
    chip->setIconSize(QSize(12, 12)); icons->addWidget(chip);
    for (const char *name : {"bell", "bell-off"}) {
        auto *icon = new QLabel; icon->setPixmap(workspaceIcon(QString::fromLatin1(name), t(name == QByteArray("bell") ? "muted" : "dim")).pixmap(16, 16));
        icons->addWidget(icon);
    }
    icons->addStretch(1);
    v->addLayout(icons);
    caption(QStringLiteral("Agent works"));
    auto *tiles = TeamUi::row(14);
    for (const auto &[agent, working, size] : QList<std::tuple<QString, bool, int>>{{"claude", false, 17}, {"codex", true, 17}, {"kimi", true, 22}, {"dsh", false, 22}}) {
        auto *tile = new AgentTile(size); tile->setAgent(agent); tile->setDark(dark); tile->setWorking(working);
        tiles->addWidget(tile, 0, Qt::AlignTop);
    }
    tiles->addStretch(1);
    v->addLayout(tiles);
    auto *folders = new CollapsibleBlock({}, QStringLiteral("Folders"), true);
    folders->setDark(dark); folders->setNote(QStringLiteral("3"));
    auto *add = new QPushButton(QStringLiteral("Folder")); add->setProperty("ghost", true); add->setIcon(workspaceIcon("add", t("text2")));
    folders->trailing()->addWidget(add);
    for (const char *row : {"zerus  ·  D:/Work/zerus", "docs  ·  D:/Work/docs", "mobile  ·  D:/Work/mobile"})
        folders->body()->addWidget(TeamUi::label(QString::fromUtf8(row), t("text2"), 12));
    v->addWidget(folders);
    auto *templates = new CollapsibleBlock({}, QStringLiteral("Templates"), false);
    templates->setDark(dark); templates->setNote(QStringLiteral("3"));
    templates->setSummary(QStringLiteral("Feature delivery → Bugfix → Dev Team"));
    templates->body()->addWidget(TeamUi::label(QStringLiteral("hidden"), t("text2"), 12));
    v->addWidget(templates);
    caption(QStringLiteral("Scroll bars"));
    auto *list = new QListWidget; list->setFixedHeight(96);
    list->setStyleSheet(QStringLiteral("QListWidget{background:%1;border:1px solid %2;border-radius:8px;color:%3;}").arg(t("surface").name(), t("cardBorder").name(), t("text2").name()));
    for (int i = 1; i <= 30; ++i) list->addItem(QStringLiteral("Session %1").arg(i));
    v->addWidget(list);
    v->addStretch(1);
    return page;
}
// The lead session's header as the window lays it out (spec 3): the title, the
// team after it and the session's pill; the chips, then the team's actions.
QWidget *leadHeaderMock(TeamWorkspace &workspace, bool dark)
{
    const auto &skin = WorkspaceSkin::active();
    const auto t = [&skin, dark](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)).name(); };
    auto *frame = new QFrame; frame->setObjectName("leadHeaderMock");
    frame->setStyleSheet(QStringLiteral("QFrame#leadHeaderMock{background:%1;} QLabel#mockTitle{color:%2;font-size:16px;font-weight:600;}"
                                        "QLabel[chip=\"true\"]{background:%3;color:%4;border-radius:6px;padding:2px 7px;font-size:11px;}"
                                        "QLabel#mockMeta{color:%5;font-size:11px;} QLabel#mockPill{color:%4;border:1px solid %6;border-radius:12px;padding:3px 10px;font-size:11.5px;}")
                             .arg(t("bg"), t("fg"), t("hover"), t("text3"), t("dim"), t("border")));
    auto *v = new QVBoxLayout(frame); v->setContentsMargins(14, 10, 14, 10); v->setSpacing(6);
    auto *top = new QHBoxLayout; top->setSpacing(4);
    auto *title = new QLabel(QStringLiteral("Orbit platform / checkout")); title->setObjectName("mockTitle");
    top->addWidget(title); top->addSpacing(10); top->addWidget(workspace.headerStatus()); top->addStretch(1);
    auto *pill = new QLabel(QStringLiteral("Ready")); pill->setObjectName("mockPill"); top->addWidget(pill);
    v->addLayout(top);
    auto *chips = new QHBoxLayout; chips->setSpacing(5);
    for (const auto &text : {QStringLiteral("Claude"), QStringLiteral("workstation"), QStringLiteral("feat/checkout-v2")}) {
        auto *chip = new QLabel(text); chip->setProperty("chip", true); chips->addWidget(chip);
    }
    auto *meta = new QLabel(QStringLiteral("orbit / feat/checkout-v2")); meta->setObjectName("mockMeta"); chips->addWidget(meta);
    chips->addStretch(1); chips->addWidget(workspace.headerActions());
    v->addLayout(chips);
    return frame;
}
}

class TestTeamsUiWorkspace : public QObject, public TeamsUiFixture {
    Q_OBJECT
private slots:
    void initTestCase()
    {
        setUpCase();
        // #525: a pending Activity render is work in flight for the waits. A hidden
        // view stays dirty until it is shown, so only a visible one counts.
        TestWait::addProbe([] {
            for (auto *widget : QApplication::allWidgets())
                if (auto *activity = qobject_cast<ActivityView *>(widget); activity && activity->isVisible() && activity->renderPending()) return true;
            return false;
        });
    }
    void init() { setUp(); }
    void cleanup() { tearDown(); }
    void workspaceShowsGraphBoardAndFocus();
    void workspaceGroupsTasksByPhase();
    void lanesGroupACollapseInEveryView();
    void lightThemeShots();
    void hideFinishedLeavesOutDecidedPhases();
    void graphRoutesEdgesAroundNodes();
    void graphEdgesAreEditedByHandleAndSelection();
    void graphHoverLightSurvivesPolls();
    void graphRoutingStaysFastForTheLargestPlan();
    void aTallLayerWrapsIntoColumns();
    void graphNodesInTwoLinesWithDraftEdits();
    void boardPhaseHeaderKeepsItsActionsInView();
    void graphNodesCarryTheActivityLabel();
    void cancelledWorkIsHiddenUntilShown();
    void workingTimeFollowsTheActivityLabel();
    void everyNodeShowsItsEstimateAndTheTimeAgainstIt();
    void boardScrollsInsteadOfSqueezingColumns();
    void planningBoardScrollsInsteadOfSqueezingColumns();
    void boardColumnsKeepTheirScroll();
    void skinTokensMatchThePrototype();
    void rolePaletteReadsTheCatalog();
    void scrollBarsAreThin();
    void gearAndBellOffIcons();
    void collapsibleBlockKeepsItsState();
    void agentWorkingIndicatorFollowsTheSetting();
    void foundationShots();
    void leadHeaderFollowsTheTeamState();
    void leadHeaderShots();
    void aNarrowToolbarKeepsOnlyItsIcons();
    void theNarrowToolbarSettlesOnOneStep();
    void stageColumnsAndChipsOnTheBoard();
    void heldByFromTheStatusOnTheBoard();
    void aPollThatChangesNothingDrawnRebuildsNothing();
    void aPhaseGraphThatFitsShowsNoSidewaysBar();
    void aMembersLiveStateChangesInPlace();
    void aListPollThatMovesOnlyTheRevisionChangesNothing();
};

void TestTeamsUiWorkspace::workspaceShowsGraphBoardAndFocus()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    // #719: Tests is a fifth view; its words need 100 px more.
    workspace.resize(1400, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamPhaseGraph"));
    // The coordinator shares the side column with the members, split by a divider.
    auto *side = workspace.findChild<QSplitter *>("teamWorkspaceSideSplit");
    QVERIFY(side && side->count() == 2);
    QVERIFY(side->widget(1)->findChild<QWidget *>("teamConversation"));
    // U25: nothing waits, so the right column has no Needs your answer list.
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceAnswers"));
    // Owner, 2026-10-29: a one-phase plan is drawn as its phase scroll too,
    // so the graph sits in that scroll under the phase header. A render
    // (every poll) builds the canvas again, so each check finds it anew.
    const auto graph = [&workspace] { return workspace.findChild<QWidget *>("teamPhaseGraph"); };
    QTRY_VERIFY(graph() && graph()->height() > 0 && workspace.findChild<QWidget *>("teamPhaseHeader"));
    // ...and holds still: no scroll bar comes and goes under it.
    QTest::qWait(150);
    QTRY_VERIFY(graph());
    const QSize settled = graph()->size();
    for (int i = 0; i < 5; ++i) { QTest::qWait(40); QTRY_VERIFY(graph()); QCOMPARE(graph()->size(), settled); }
    // A divider the owner moves stands between the canvas and the side column.
    auto *split = workspace.findChild<QSplitter *>("teamWorkspaceSplit");
    QVERIFY(split && split->handleWidth() >= 5);
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    shot(&workspace, "d3-running");
    // U35: the coordinator offers no quick-ask chips, in any team state or filter.
    QVERIFY(workspace.findChildren<QPushButton *>("teamConversationAsk").isEmpty());
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceFilter")->findChildren<QPushButton *>())
        if (b->property("segment") == "active") { b->click(); break; }
    QTest::qWait(150);
    QVERIFY(workspace.findChildren<QPushButton *>("teamConversationAsk").isEmpty());
    // U25, #755: the views carry their icons, and their names in tooltips and accessible names.
    auto *views = workspace.findChild<QWidget *>("teamWorkspaceView");
    QVERIFY(views);
    for (auto *b : views->findChildren<QPushButton *>()) QVERIFY(b->text().isEmpty() && !b->icon().isNull() && !b->accessibleName().isEmpty() && !b->toolTip().isEmpty());
    for (auto *b : views->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QCOMPARE(workspace.findChildren<QFrame *>("teamBoardCard").size(), 5);
    // The role and the state stay on the card at any width (a phase card has
    // the state on its first line, the role on its second). U33: a narrow
    // window no longer squeezes the card; the Board scrolls sideways instead.
    {
        // Reads the first card afresh: a resize may render the board again.
        const auto oneLine = [&workspace] {
            auto *boardCard = workspace.findChildren<QFrame *>("teamBoardCard").value(0);
            auto *pill = boardCard ? boardCard->findChild<QWidget *>("teamBoardRole") : nullptr;
            auto *mark = boardCard ? boardCard->findChild<QWidget *>("teamBoardState") : nullptr;
            return pill && mark && pill->isVisible() && mark->isVisible() ? 1 : pill && mark ? 0 : -1;
        };
        const QSize before = workspace.size();
        workspace.resize(700, before.height());
        QTRY_COMPARE(oneLine(), 1);
        QTRY_VERIFY(workspace.findChild<QScrollArea *>("teamWorkspaceBoard")->horizontalScrollBar()->isVisible());
        workspace.resize(1800, before.height());
        QTRY_COMPARE(oneLine(), 1);
        workspace.resize(before);
    }
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceFilter"));   // the board's columns sort by state
    shot(&workspace, "d3-board");
    // Focusing a member keeps only its tasks.
    for (auto *m : workspace.findChildren<QFrame *>("teamWorkspaceMember")) if (m->property("teamMember") == "tester") { QTest::mouseClick(m, Qt::LeftButton, {}, QPoint(4, 4)); break; }
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceFocus"));
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamBoardCard").size(), 1);
    QVERIFY(workspace.findChild<QLabel *>("teamBoardEmpty"));   // empty columns say so
    // A card opens the drawer.
    // A phase card is itself the button.
    auto *boardCard = workspace.findChildren<QFrame *>("teamBoardCard").value(0);
    QVERIFY(boardCard);
    QTest::mouseClick(boardCard, Qt::LeftButton, {}, QPoint(6, 6));
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceDrawer") && workspace.findChild<QWidget *>("teamWorkspaceDrawer")->isVisible());
}

void TestTeamsUiWorkspace::workspaceGroupsTasksByPhase()
{
    setTeam(phasedTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    // U25: every phase is a scroll, stacked top to bottom; the finished one
    // starts collapsed and builds no graph.
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 3);
    QCOMPARE(phaseScrollOf(&workspace, 1)->property("state").toString(), QStringLiteral("done"));
    QCOMPARE(phaseScrollOf(&workspace, 2)->property("state").toString(), QStringLiteral("running"));
    QCOMPARE(phaseScrollOf(&workspace, 3)->property("state").toString(), QStringLiteral("draft"));
    QCOMPARE(phaseScrollOf(&workspace, 2)->findChild<QLabel *>("teamPhaseTitle")->text(), QStringLiteral("Phase 2 · QA fixes"));
    QVERIFY(!phaseOpen(&workspace, 1) && phaseOpen(&workspace, 2) && phaseOpen(&workspace, 3));
    QCOMPARE(phaseNodeIds(&workspace), (QList<int>{3, 4, 5}));
    QVERIFY(phaseScrollOf(&workspace, 1)->findChild<QLabel *>("teamPhaseSummary")->text().contains("#1 Requirements"));
    shot(&workspace, "d3-phases-graph-collapsed");
    togglePhase(&workspace, 1);   // the header opens the finished phase, its tasks come back
    QTRY_VERIFY(phaseOpen(&workspace, 1));
    QTRY_COMPARE(phaseNodeIds(&workspace).size(), 5);
    togglePhase(&workspace, 2);   // the current phase closes; its tasks go, the others stay
    QTRY_VERIFY(!phaseOpen(&workspace, 2));
    QTRY_COMPARE(phaseNodeIds(&workspace), (QList<int>{1, 2, 5}));
    togglePhase(&workspace, 2);
    QTRY_VERIFY(phaseOpen(&workspace, 2));
    QTRY_COMPARE(phaseNodeIds(&workspace).size(), 5);
    QCOMPARE(TeamUi::phaseSummaryText(2, 2), QStringLiteral("2/2 done · click to open"));
    QCOMPARE(TeamUi::phaseSummaryText(1, 2), QStringLiteral("1/2 · click to open"));
    togglePhase(&workspace, 2);   // closed again for the views below
    QTRY_VERIFY(!phaseOpen(&workspace, 2));
    shot(&workspace, "d3-phases-graph-open");
    const auto showView = [&workspace](const QString &view) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == view) { b->click(); break; }
    };
    const auto rowsOf = [&workspace](const char *name, const char *rowName) {
        QList<int> ids;
        for (auto *row : workspace.findChildren<QFrame *>(rowName)) ids.append(row->property("task").toInt());
        std::sort(ids.begin(), ids.end());
        Q_UNUSED(name);
        return ids;
    };
    // Layers keep the same scrolls, a row per task in the open ones.
    showView("layers");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_COMPARE(rowsOf("teamWorkspaceLayers", "teamLayerRow"), (QList<int>{1, 2, 5}));
    QVERIFY(!phaseOpen(&workspace, 2));
    shot(&workspace, "d3-phases-layers");
    // The board too; a collapsed phase shows only its header.
    showView("board");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QTRY_COMPARE(rowsOf("teamWorkspaceBoard", "teamBoardCard"), (QList<int>{1, 2, 5}));
    QCOMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 3);
    shot(&workspace, "d3-phases-board-collapsed");
    togglePhase(&workspace, 2);
    QTRY_COMPARE(rowsOf("teamWorkspaceBoard", "teamBoardCard"), (QList<int>{1, 2, 3, 4, 5}));
    QTRY_VERIFY(phaseScrollOf(&workspace, 2) && !phaseScrollOf(&workspace, 2)->findChild<QToolButton *>("teamPhaseToggle")->property("collapsed").toBool());
    shot(&workspace, "d3-phases-board");
}

// G9: the workspace graph stacks phase bands top to bottom; each band's header
// carries the phase status, progress, health and a draft's decisions (spec
// 10.2). Shots: every status, collapsed and expanded, light and dark.
void TestTeamsUiWorkspace::lanesGroupACollapseInEveryView()
{
    // U27: a phase's tasks group by lane, in lane order, each lane a labelled
    // row that collapses; the tasks without a lane fall back to their layers.
    // Lanes show in Graph, Board and Layers.
    auto t = phasedTeam();
    auto tasks = t["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        // Phase 3 keeps #5; give it and two new tasks lanes, and leave one alone.
        if (task["id"].toInt() == 5) task["lane"] = QJsonObject{{"n", 1}, {"label", "Checkpoint 1"}};
        tasks[i] = task;
    }
    tasks.append(QJsonObject{{"id", 6}, {"phase", 3}, {"title", "Screenshots"}, {"assignee", "tester"}, {"status", "draft"},
                             {"lane", QJsonObject{{"n", 1}, {"label", "Checkpoint 1"}}}});
    tasks.append(QJsonObject{{"id", 7}, {"phase", 3}, {"title", "Review"}, {"assignee", "backend"}, {"status", "draft"},
                             {"lane", QJsonObject{{"n", 2}, {"label", "Review"}}}});
    tasks.append(QJsonObject{{"id", 8}, {"phase", 3}, {"title", "No lane work"}, {"assignee", "backend"}, {"status", "draft"}});
    t["tasks"] = tasks;
    auto phases = t["phases"].toArray();
    auto polish = phases[1].toObject();
    polish["tasks"] = QJsonArray{5, 6, 7, 8};
    phases[1] = polish;
    t["phases"] = phases;
    t["pending_phases"] = QJsonArray{3};
    setTeam(t);
    Settings::remove("ui/collapsed/teamLane/team-1/3/1");
    Settings::remove("ui/collapsed/teamLane/team-1/3/2");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 1000);
    workspace.show();
    controller.refresh();
    workspace.showTeam("team-1");
    const auto laneHeaders = [&workspace](int phase) {
        QList<QToolButton *> found;
        for (auto *header : workspace.findChildren<QToolButton *>("teamLaneHeader"))
            if (header->property("phase").toInt() == phase && header->isVisible()) found << header;
        return found;
    };
    const auto laneBody = [&workspace](int phase, int lane) -> QWidget * {
        for (auto *body : workspace.findChildren<QWidget *>("teamLaneBody"))
            if (body->property("phase").toInt() == phase && body->property("lane").toInt() == lane && body->isVisible()) return body;
        return nullptr;
    };
    QTRY_VERIFY_WITH_TIMEOUT(laneHeaders(3).size() == 3, 20000);
    QCOMPARE(laneHeaders(3)[0]->property("lane").toInt(), 1);
    QVERIFY2(laneHeaders(3)[0]->text().contains(QStringLiteral("Checkpoint 1")), qPrintable(laneHeaders(3)[0]->text()));
    QCOMPARE(laneHeaders(3)[2]->property("lane").toInt(), 0);
    QVERIFY2(laneHeaders(3)[2]->text().contains(QStringLiteral("No lane")), qPrintable(laneHeaders(3)[2]->text()));
    QVERIFY(laneBody(3, 1));
    // Graph: the lane's graph shows only its own tasks.
    auto *graph = laneBody(3, 1)->findChild<QWidget *>("teamPhaseGraph");
    QVERIFY(graph);
    // Collapsing a lane hides its body and remembers the choice.
    laneHeaders(3)[0]->click();
    QTRY_VERIFY(!laneBody(3, 1));
    QCOMPARE(Settings::value("ui/collapsed/teamLane/team-1/3/1").toBool(), true);
    workspace.render();
    QTest::qWait(150);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u27-lanes-collapsed-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "u27-lanes-collapsed-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    QTRY_VERIFY(laneHeaders(3).size() == 3);
    laneHeaders(3)[0]->click();
    QTRY_VERIFY(laneBody(3, 1));
    workspace.render();
    QTest::qWait(150);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u27-lanes-expanded-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "u27-lanes-expanded-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    // Board and Layers group the phase the same way.
    QPointer<QWidget> views = workspace.findChild<QWidget *>("teamWorkspaceView");
    QVERIFY(views);
    for (auto *button : views->findChildren<QPushButton *>())
        if (button->property("segment") == "board") { button->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    // Rendering another view replaces the toolbar too. Track its destruction
    // and fetch the current toolbar before the next click.
    QTRY_VERIFY(views.isNull());
    QTRY_VERIFY_WITH_TIMEOUT(([&] { const auto headers = laneHeaders(3); return headers.size() == 3 && laneBody(3, 2); })(), 20000);
    const auto cards = [&workspace](const QList<int> &ids) {
        int n = 0;
        for (auto *card : workspace.findChildren<QFrame *>("teamBoardCard"))
            if (ids.contains(card->property("task").toInt())) ++n;
        return n;
    };
    QCOMPARE(cards({5, 6}), 2);
    QCOMPARE(cards({7}), 1);
    QCOMPARE(cards({8}), 1);
    views = workspace.findChild<QWidget *>("teamWorkspaceView");
    QVERIFY(views);
    for (auto *button : views->findChildren<QPushButton *>())
        if (button->property("segment") == "layers") { button->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_VERIFY(views.isNull());
    QTRY_VERIFY_WITH_TIMEOUT(([&] { const auto headers = laneHeaders(3); return headers.size() == 3 && laneBody(3, 2); })(), 20000);
    // The new layers view groups the same lanes after replacing the board.
    QTRY_VERIFY_WITH_TIMEOUT(([&] { const auto headers = laneHeaders(3); return headers.size() == 3 && laneBody(3, 1) && laneBody(3, 2); })(), 20000);
}

void TestTeamsUiWorkspace::lightThemeShots()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) QSKIP("Set ZERUS_TEAM_SHOTS to render the light shots.");
    const QPalette dark = QApplication::palette();
    QPalette light;
    light.setColor(QPalette::Window, QColor("#f5f7f9")); light.setColor(QPalette::WindowText, QColor("#1a2733"));
    light.setColor(QPalette::Base, QColor("#ffffff")); light.setColor(QPalette::Button, QColor("#edf2f5"));
    light.setColor(QPalette::ButtonText, QColor("#1a2733")); light.setColor(QPalette::Text, QColor("#1a2733"));
    light.setColor(QPalette::PlaceholderText, QColor("#627082"));
    QApplication::setPalette(light);
    setTeam(phasedTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setTheme(false); panel.setTheme(false);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    WAIT_VERIFY(workspace.findChildren<QFrame *>("teamPhaseScroll").size() > 2);
    // The coordinator's empty Activity text is light-theme text, not a dark
    // document left over from before the theme switch.
    auto *conversation = workspace.findChild<QWidget *>("teamConversationActivity");
    auto *browser = conversation ? conversation->findChild<QTextBrowser *>() : nullptr;
    QVERIFY(browser);
    const auto lightText = [&] {
        QColor color;
        for (auto block = browser->document()->begin(); block.isValid(); block = block.next())
            if (block.text().contains(QStringLiteral("Messages, tools and approvals"))) {
                QTextCursor cursor(block); cursor.movePosition(QTextCursor::StartOfBlock);
                cursor.movePosition(QTextCursor::Right, QTextCursor::KeepAnchor, 1);
                color = cursor.charFormat().foreground().color();
            }
        return color.isValid() && color.lightness() < 150;
    };
    // The Activity loads in the background: a loaded machine needs over 5 s.
    WAIT_VERIFY(lightText());
    shot(&workspace, "d3-phases-draft-light");
    auto waiting = phasedTeam();
    auto tasks = waiting["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto task = tasks[i].toObject(); task["status"] = "accepted"; tasks[i] = task; }
    waiting["tasks"] = tasks;
    waiting["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}}};
    waiting["pending_phase"] = QJsonValue();
    waiting["waiting_for_lead"] = true;
    setTeam(waiting);
    WAIT_VERIFY(([&] { controller.refresh(); return workspace.findChild<QPushButton *>("teamWorkspaceFinish") != nullptr; })());
    shot(&workspace, "d3-phases-waiting-light");
    QApplication::setPalette(dark);
}

// Owner, 2026-10-30: Hide finished leaves out a phase whose tasks are all
// decided, with its tasks, in Graph, Board and Layers, and is remembered.
void TestTeamsUiWorkspace::hideFinishedLeavesOutDecidedPhases()
{
    Settings::remove("teams/hideFinished");
    const auto team = Teams::parse(phasedTeam());
    QVERIFY(phaseFinished(team, 1) && !phaseFinished(team, 2) && !phaseFinished(team, 3));
    QCOMPARE(finishedPhases(team), 1);
    const auto shown = withoutFinished(team);
    QVERIFY(shown.cancelledHidden);
    QVERIFY(!shown.task(1) && !shown.task(2) && shown.task(3) && shown.task(4) && shown.task(5));
    setTeam(phasedTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished"));
    QTRY_VERIFY(phaseScrollOf(&workspace, 2));
    QVERIFY(!workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->isChecked());
    QVERIFY(phaseScrollOf(&workspace, 1));
    workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->click();
    QTRY_VERIFY(!phaseScrollOf(&workspace, 1));
    QVERIFY(phaseScrollOf(&workspace, 2) && phaseScrollOf(&workspace, 3));
    QVERIFY(Settings::value("teams/hideFinished").toBool());
    QTRY_VERIFY(workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->isChecked());
    shot(&workspace, "hide-finished-dark");
    workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->click();
    QTRY_VERIFY(phaseScrollOf(&workspace, 1));
    Settings::remove("teams/hideFinished");
}

// Spec 5.4: the Wide graph's geometry and routes - nodes 196x54, columns 236
// apart and centered, rows 74 apart, orthogonal edges that never cross a node
// of an intermediate column, 6 px lanes, back edges under every node.
void TestTeamsUiWorkspace::graphRoutesEdgesAroundNodes()
{
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    const auto node = [](int id, int layer) { TeamUi::TaskView::Node n; n.id = id; n.layer = layer; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend"; return n; };
    // Layer 0: #1; layer 1: #2 #3 #4; layer 2: #5 #6; layer 3: #7.
    view.nodes = {node(1, 0), node(2, 1), node(3, 1), node(4, 1), node(5, 2), node(6, 2), node(7, 3)};
    view.edges[2] = {1}; view.edges[3] = {1}; view.edges[4] = {1};
    view.edges[5] = {2, 3}; view.edges[6] = {4};
    view.edges[7] = {1, 2, 5, 6};   // #1 -> #7 and #2 -> #7 pass the columns between
    view.edges[3].append(7);        // a back edge (a cycle)
    view.cycle = {3, 7};
    view.area = QSize(1400, 700);
    view.layout();
    const auto r = [&](int id) { return view.nodeRect(id); };
    QCOMPARE(r(1).size(), QSizeF(196, 54));
    QCOMPARE(r(2).left() - r(1).left(), 236.0);
    QCOMPARE(r(3).top() - r(2).top(), 74.0);
    // A shorter column is centered on the tallest.
    QCOMPARE(r(1).center().y(), r(3).center().y());
    QCOMPARE(r(5).center().y(), (r(2).center().y() + r(3).center().y()) / 2);
    QMap<int, int> layerOf; for (const auto &n : view.nodes) layerOf.insert(n.id, n.layer);
    double lowest = 0; for (const auto &n : view.nodes) lowest = qMax(lowest, r(n.id).bottom());
    QList<double> gapLanes;
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) for (int dep : it.value()) {
        const int task = it.key();
        QVERIFY2(view.routed(dep, task), qPrintable(QStringLiteral("%1->%2").arg(dep).arg(task)));
        const auto route = view.route(dep, task).points;
        // It leaves the dependency's right side and ends at the arrow before the task.
        QCOMPARE(route.first(), QPointF(r(dep).right(), r(dep).center().y()));
        QCOMPARE(route.last(), QPointF(r(task).left() - 2, r(task).center().y()));
        for (int i = 1; i < route.size(); ++i) {
            const QPointF a = route.at(i - 1), b = route.at(i);
            QVERIFY2(qFuzzyCompare(a.x() + 1, b.x() + 1) || qFuzzyCompare(a.y() + 1, b.y() + 1), "every leg is horizontal or vertical");
            // No leg crosses a node of a column between the two ends.
            const QRectF leg = QRectF(a, b).normalized().adjusted(-.01, -.01, .01, .01);
            for (const auto &n : view.nodes) {
                const int l = n.layer;
                const bool between = layerOf[task] > layerOf[dep] ? (l > layerOf[dep] && l < layerOf[task]) : false;
                if (between) QVERIFY2(!leg.intersects(r(n.id).adjusted(1, 1, -1, -1)),
                                      qPrintable(QStringLiteral("edge #%1->#%2 crosses #%3").arg(dep).arg(task).arg(n.id)));
            }
        }
        if (layerOf[task] <= layerOf[dep]) {
            // A back edge runs under every node.
            double deepest = 0; for (const QPointF &point : route) deepest = qMax(deepest, point.y());
            QVERIFY(deepest > lowest);
        }
        if (dep == 1 && layerOf[task] == 1) gapLanes << route.at(1).x();
    }
    // The three edges from #1 through one gap spread over lanes 6 px apart.
    std::sort(gapLanes.begin(), gapLanes.end());
    QCOMPARE(gapLanes.size(), 3);
    QCOMPARE(gapLanes.at(1) - gapLanes.at(0), 6.0);
    QCOMPARE(gapLanes.at(2) - gapLanes.at(1), 6.0);
}

void TestTeamsUiWorkspace::graphEdgesAreEditedByHandleAndSelection()
{
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    const auto node = [](int id, int layer) {
        TeamUi::TaskView::Node n; n.id = id; n.layer = layer; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend";
        n.tone = TeamUi::colorsFor(true).muted; n.roleTone = TeamUi::roleColor(n.role, true); n.kind = "pending"; n.agent = "codex";
        return n;
    };
    view.nodes = {node(1, 0), node(2, 1), node(3, 1)};
    view.edges[2] = {1};
    view.editable = true;
    QList<QPair<int, int>> linked, unlinked;
    view.linked = [&](int dep, int task) { linked << qMakePair(dep, task); };
    view.unlinked = [&](int dep, int task) { unlinked << qMakePair(dep, task); };
    view.area = QSize(900, 400);
    view.resize(900, 400); view.show();
    view.layout();
    QVERIFY(QTest::qWaitForWindowExposed(&view));
    const QRectF one = view.nodeRect(1), three = view.nodeRect(3);
    QTest::mouseMove(&view, one.center().toPoint());
    const QPoint port(int(one.right() - 4), int(one.center().y()));
    QTest::mouseClick(&view, Qt::LeftButton, {}, port);
    QCOMPARE(view.connecting(), 1);
    QTest::mouseMove(&view, three.center().toPoint());
    shot(&view, "ux-5-graph-connect-dark");
    QTest::mouseClick(&view, Qt::LeftButton, {}, three.center().toPoint());
    QCOMPARE(linked, (QList<QPair<int, int>>{{1, 3}}));
    QCOMPARE(view.connecting(), 0);
    // Escape leaves the connect mode without a link.
    QTest::mouseClick(&view, Qt::LeftButton, {}, port);
    QCOMPARE(view.connecting(), 1);
    QTest::keyClick(&view, Qt::Key_Escape);
    QCOMPARE(view.connecting(), 0);
    // Click the edge, then Delete.
    const auto route = view.route(1, 2).points;
    const QPointF onLeg = (route.at(0) + route.at(1)) / 2;
    QTest::mouseClick(&view, Qt::LeftButton, {}, onLeg.toPoint());
    QCOMPARE(view.selectedEdge(), qMakePair(1, 2));
    QTest::keyClick(&view, Qt::Key_Delete);
    QCOMPARE(unlinked, (QList<QPair<int, int>>{{1, 2}}));
    // Outside planning only the tasks of a draft take edits.
    view.editable = false;
    view.editableTasks = {3};
    QVERIFY(!view.canEdit(2) && view.canEdit(3));

    // #461: in a running team the draft phase's scroll takes the same handle
    // and selection edits through edit-draft, never `plan` (which the server
    // takes only in Planning); the running phase's scroll takes none.
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1"); workspace.showTeam("team-1");
    TeamUi::TaskView *draft = nullptr;
    QTRY_VERIFY(([&] { draft = phaseGraphOf(&workspace, 3); return draft && draft->nodeRect(8).width() > 0 && draft->isVisible(); })());
    QCOMPARE(draft->property("phase").toInt(), 3);
    auto *build = phaseGraphOf(&workspace, 2); QVERIFY(build);
    QVERIFY(!build->editable && !build->canEdit(5) && !build->linked);
    QVERIFY(!draft->editable && draft->canEdit(7) && draft->canEdit(8) && draft->canEdit(9));
    const auto at = [&](const QPointF &point) { return QPoint(int(std::round(point.x() * draft->zoom)), int(std::round(point.y() * draft->zoom))); };
    // #465 N18: a poll that changes the team rebuilds every view; the owner's
    // half-made link and selected edge come back in the new one.
    int revision = 21;
    const auto poll = [&] {
        QPointer<TeamUi::TaskView> old = draft;
        auto next = graphTeam(); next["revision"] = ++revision;
        // #693: a change, so the views are rebuilt.
        next["description"] = QStringLiteral("poll %1").arg(revision);
        setTeam(next);
        controller.refresh();
        QTRY_VERIFY(!old && (draft = phaseGraphOf(&workspace, 3)) && draft->nodeRect(8).width() > 0);
    };
    // #9's handle, then a poll, then #8: #8 gains #9.
    const QRectF nine = draft->nodeRect(9);
    QTest::mouseMove(draft, at(nine.center()));
    QTRY_VERIFY((draft = phaseGraphOf(&workspace, 3)) && draft->nodeRect(9).width() > 0);
    QTest::mouseClick(draft, Qt::LeftButton, {}, at(QPointF(nine.right() - 4, nine.center().y())));
    QCOMPARE(draft->connecting(), 9);
    poll();
    QCOMPARE(draft->connecting(), 9);
    QTest::mouseClick(draft, Qt::LeftButton, {}, at(draft->nodeRect(8).center()));
    QCOMPARE(draft->connecting(), 0);
    QTRY_COMPARE(calls(script(), "edit-draft").size(), 1);
    auto input = calls(script(), "edit-draft").first();
    QCOMPARE(input["phase"].toInt(), 3);
    auto update = input["tasks_update"].toArray().first().toObject();
    QCOMPARE(update["id"].toInt(), 8);
    QCOMPARE(update["deps"].toArray().last().toObject()["task"].toInt(), 9);
    // The edge #8 -> #7 selected, then Delete: #7 keeps only #9.
    QTRY_VERIFY(([&] { draft = phaseGraphOf(&workspace, 3); return draft && draft->routed(8, 7); })());
    const auto legs = draft->route(8, 7).points;
    QTest::mouseClick(draft, Qt::LeftButton, {}, at((legs.at(0) + legs.at(1)) / 2));
    QCOMPARE(draft->selectedEdge(), qMakePair(8, 7));
    poll();
    QCOMPARE(draft->selectedEdge(), qMakePair(8, 7));
    QTest::keyClick(draft, Qt::Key_Delete);
    QTRY_COMPARE(calls(script(), "edit-draft").size(), 2);
    update = calls(script(), "edit-draft").last()["tasks_update"].toArray().first().toObject();
    QCOMPARE(update["id"].toInt(), 7);
    QCOMPARE(update["deps"].toArray().size(), 1);
    QCOMPARE(update["deps"].toArray().first().toObject()["task"].toInt(), 9);
    QVERIFY(calls(script(), "plan").isEmpty());
    // Escape ends a link for good: the next view starts clear. The reply to
    // edit-draft may rebuild the views, so the click goes to the one shown.
    QTRY_VERIFY((draft = phaseGraphOf(&workspace, 3)) && draft->nodeRect(9).width() > 0);
    QTest::mouseClick(draft, Qt::LeftButton, {}, at(QPointF(draft->nodeRect(9).right() - 4, draft->nodeRect(9).center().y())));
    QTRY_VERIFY((draft = phaseGraphOf(&workspace, 3)) && draft->connecting() == 9);
    QTest::keyClick(draft, Qt::Key_Escape);
    poll();
    QCOMPARE(draft->connecting(), 0);
    QCOMPARE(draft->selectedEdge(), qMakePair(0, 0));
    QCOMPARE(calls(script(), "edit-draft").size(), 2);
}

// #570 (owner): hovering a task lights its linked tasks; every poll rebuilt
// the phase graphs and the light went out under a resting pointer.
void TestTeamsUiWorkspace::graphHoverLightSurvivesPolls()
{
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1"); workspace.showTeam("team-1");
    // Every render replaces the phase graphs: always ask for the current one.
    const auto graph = [&] { return phaseGraphOf(&workspace, 2); };
    QTRY_VERIFY(graph() && graph()->nodeRect(3).width() > 0 && graph()->isVisible());
    const auto at = [&](const QPointF &point) { return QPoint(int(std::round(point.x() * graph()->zoom)), int(std::round(point.y() * graph()->zoom))); };
    // #4 is not linked to #3: it dims while #3 is lit.
    const auto pixels = [&](int task) {
        const QRectF r = graph()->nodeRect(task);
        return graph()->grab(QRect(at(r.topLeft()), at(r.bottomRight()))).toImage();
    };
    TestWait::settle();
    const QImage plain = pixels(4);
    QTest::mouseMove(graph(), at(graph()->nodeRect(3).center()));
    QTRY_COMPARE(graph()->hovered(), 3);
    QVERIFY(graph()->chain(3).contains(5) && graph()->chain(3).contains(6) && !graph()->chain(3).contains(4));
    TestWait::settle();
    const QImage lit = pixels(4);
    QVERIFY(lit != plain);
    int revision = 21;
    const auto poll = [&](QJsonObject next) {
        QPointer<TeamUi::TaskView> old = graph();
        next["revision"] = ++revision;
        // #693: a poll that changes nothing drawn rebuilds nothing; this one
        // carries a change, so the views are rebuilt.
        next["description"] = QStringLiteral("poll %1").arg(revision);
        setTeam(next);
        controller.refresh();
        QTRY_VERIFY(!old && graph() && graph()->nodeRect(4).width() > 0);
        TestWait::settle();
    };
    // Several polls with new revisions; the pointer does not move.
    for (int i = 0; i < 3; ++i) {
        poll(graphTeam());
        QCOMPARE(graph()->hovered(), 3);
        QCOMPARE(pixels(4), lit);
        QVERIFY(graph()->toolTip().startsWith(QStringLiteral("#3 Card form UI")));
    }
    // Another phase's graph never takes the light.
    QCOMPARE(phaseGraphOf(&workspace, 3)->hovered(), 0);
    // The pointer moves to the gutter right of #3: the light goes, and a poll
    // keeps it off.
    const QRectF three = graph()->nodeRect(3);
    QTest::mouseMove(graph(), at(QPointF(three.right() + 12, three.center().y())));
    QTRY_COMPARE(graph()->hovered(), 0);
    poll(graphTeam());
    QCOMPARE(graph()->hovered(), 0);
    QCOMPARE(pixels(4), plain);
    // Back on #3, then the pointer leaves the view: off, also after a poll.
    QTest::mouseMove(graph(), at(graph()->nodeRect(3).center()));
    QTRY_COMPARE(graph()->hovered(), 3);
    QEvent leave(QEvent::Leave);
    QCoreApplication::sendEvent(graph(), &leave);
    QCOMPARE(graph()->hovered(), 0);
    poll(graphTeam());
    QCOMPARE(graph()->hovered(), 0);
    // A task that is gone takes its light with it. (A move to where the
    // pointer already is sends nothing: go by the gutter.)
    QTest::mouseMove(graph(), at(QPointF(graph()->nodeRect(3).right() + 12, graph()->nodeRect(3).center().y())));
    QTest::mouseMove(graph(), at(graph()->nodeRect(3).center()));
    QTRY_COMPARE(graph()->hovered(), 3);
    auto gone = graphTeam();
    QJsonArray tasks;
    for (const auto &value : gone["tasks"].toArray()) {
        auto task = value.toObject();
        if (task["id"].toInt() == 3) continue;
        QJsonArray deps;
        for (const auto &d : task["deps"].toArray()) if (d.toObject()["task"].toInt() != 3) deps.append(d);
        task["deps"] = deps;
        tasks.append(task);
    }
    gone["tasks"] = tasks;
    auto phases = gone["phases"].toArray();
    auto second = phases[0].toObject(); second["tasks"] = QJsonArray{2, 4, 5, 6}; phases[0] = second;
    gone["phases"] = phases;
    poll(gone);
    QCOMPARE(graph()->hovered(), 0);
    poll(graphTeam());
    QCOMPARE(graph()->hovered(), 0);
}

// Spec 5.4: in planning a click on a node's handle starts a link and a click
// on the dependent task makes it; a selected edge goes with its × or Delete.
// #465 N19: a 200-task plan (the plan limit) in one Wide graph, with 180 long
// edges, routes in well under a second per layout, as a resize lays it out
// again; no long edge crosses a node of the columns it passes.
void TestTeamsUiWorkspace::graphRoutingStaysFastForTheLargestPlan()
{
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    for (int id = 1; id <= 200; ++id) {
        TeamUi::TaskView::Node n; n.id = id; n.layer = (id - 1) / 20; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend";
        n.tone = TeamUi::colorsFor(true).muted; n.roleTone = TeamUi::roleColor(n.role, true); n.kind = "pending"; n.agent = "codex";
        view.nodes.append(n);
        // Two layers back, and three layers back for every other task.
        if (n.layer >= 2) view.edges[id].append(id - 40 + (id % 7 == 0 ? 3 : 0));
        if (n.layer >= 3 && id % 2) view.edges[id].append(id - 60);
    }
    int long_ = 0;
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) long_ += it.value().size();
    QVERIFY(long_ >= 80);
    view.area = QSize(1200, 800);
    QElapsedTimer timer; timer.start();
    for (int i = 0; i < 5; ++i) { view.zoom = 1 - i * .05; view.layout(); }
    const qint64 elapsed = timer.elapsed();
    QVERIFY2(elapsed < 2500, qPrintable(QStringLiteral("5 layouts took %1 ms").arg(elapsed)));
    const auto layerOf = [&](int id) { return (id - 1) / 20; };
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) for (int dep : it.value()) {
        const int task = it.key();
        QVERIFY(view.routed(dep, task));
        const auto route = view.route(dep, task).points;
        for (int other = 1; other <= 200; ++other) {
            if (layerOf(other) <= layerOf(dep) || layerOf(other) >= layerOf(task)) continue;
            const QRectF box = view.nodeRect(other).adjusted(1, 1, -1, -1);
            for (int i = 1; i < route.size(); ++i)
                QVERIFY2(!QRectF(route.at(i - 1), route.at(i)).normalized().adjusted(-.01, -.01, .01, .01).intersects(box),
                         qPrintable(QStringLiteral("#%1 -> #%2 crosses #%3").arg(dep).arg(task).arg(other)));
        }
    }
}

// Owner, 2026-10-28: a layer is not always one column. Phase 39's plan, 13
// tasks into one integration, then review and QA, then the build: the first
// layer wraps into 3 columns of at most 5, the next layers move right of
// them, and no edge crosses a node on its way.
void TestTeamsUiWorkspace::aTallLayerWrapsIntoColumns()
{
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 13}, {1, 1}, {2, 2}, {3, 1}}).value(0).span, 3);
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 13}, {1, 1}, {2, 2}, {3, 1}}).value(1).start, 3);
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 6}, {1, 1}}).value(0).span, 1);
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 20}, {1, 20}}).value(0).span, 1);
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    const auto node = [](int id, int layer) {
        TeamUi::TaskView::Node n; n.id = id; n.layer = layer; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend";
        n.tone = TeamUi::colorsFor(true).muted; n.roleTone = TeamUi::roleColor(n.role, true); n.kind = "accepted"; n.agent = "claude";
        return n;
    };
    for (int id = 1; id <= 13; ++id) view.nodes.append(node(id, 0));
    view.nodes.append(node(14, 1)); view.nodes.append(node(15, 2)); view.nodes.append(node(16, 2)); view.nodes.append(node(17, 3));
    for (int id = 1; id <= 13; ++id) view.edges[14].append(id);
    view.edges[15] = {14}; view.edges[16] = {14}; view.edges[17] = {15, 16};
    view.area = QSize(1400, 700);
    view.layout();
    QSet<double> firstColumns;
    QMap<double, int> perColumn;
    for (int id = 1; id <= 13; ++id) { firstColumns.insert(view.nodeRect(id).left()); ++perColumn[view.nodeRect(id).left()]; }
    QCOMPARE(firstColumns.size(), 3);
    for (int count : perColumn) QVERIFY(count <= 5);
    double right = 0;
    for (int id = 1; id <= 13; ++id) right = qMax(right, view.nodeRect(id).right());
    QVERIFY(view.nodeRect(14).left() > right);
    QVERIFY(view.nodeRect(15).left() > view.nodeRect(14).right());
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) for (int dep : it.value()) {
        QVERIFY(view.routed(dep, it.key()));
        const auto route = view.route(dep, it.key()).points;
        for (const auto &other : view.nodes) {
            if (other.id == dep || other.id == it.key()) continue;
            const QRectF box = view.nodeRect(other.id).adjusted(1, 1, -1, -1);
            for (int i = 1; i < route.size(); ++i)
                QVERIFY2(!QRectF(route.at(i - 1), route.at(i)).normalized().adjusted(-.01, -.01, .01, .01).intersects(box),
                         qPrintable(QStringLiteral("#%1 -> #%2 crosses #%3").arg(dep).arg(it.key()).arg(other.id)));
        }
    }
    view.resize(view.size()); shot(&view, "graph-wrapped-layer");
}

void TestTeamsUiWorkspace::graphNodesInTwoLinesWithDraftEdits()
{
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    const LiveLookup live = [](const Member &m) { Live l; l.working = m.id == "frontend"; return l; };
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // #461: a team with phases draws each phase in its own scroll; each one
    // draws the spec 5.4 nodes.
    TeamUi::TaskView *graph = nullptr, *draft = nullptr;
    QTRY_VERIFY(([&] { graph = phaseGraphOf(&workspace, 2); draft = phaseGraphOf(&workspace, 3); return graph && draft && graph->nodeRect(6).width() > 0; })());
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    QCOMPARE(graph->property("phase").toInt(), 2);
    // #465 (spec 1.2): the phase scrolls sideways under the thin 8 px bar.
    auto *sideways = qobject_cast<QScrollArea *>(phaseScrollOf(&workspace, 2)->findChild<QWidget *>("teamPhaseGraphArea"));
    QVERIFY(sideways);
    // #693: at 1500 px the graph fits, so no bar (one used to flash on each
    // rebuild); the narrow window below shows it.
    QCOMPARE(sideways->horizontalScrollBar()->sizeHint().height(), 8);
    // Owner, 2026-10-28: the place survives the rebuild of a poll. A render
    // rebuilds the phase, so the area is looked up each time.
    {
        const auto area = [&]() -> QScrollArea * {
            auto *phase = phaseScrollOf(&workspace, 2);
            return phase ? qobject_cast<QScrollArea *>(phase->findChild<QWidget *>("teamPhaseGraphArea")) : nullptr;
        };
        // A narrow window, so the phase graph needs its sideways bar.
        workspace.resize(760, 900);
        QTRY_VERIFY(area() && area()->horizontalScrollBar()->maximum() > 0 && area()->horizontalScrollBar()->isVisible());
        const int place = qMin(120, area()->horizontalScrollBar()->maximum());
        area()->horizontalScrollBar()->setValue(place);
        QCOMPARE(area()->horizontalScrollBar()->value(), place);
        workspace.render();
        QTest::qWait(200);
        QTRY_COMPARE(area()->horizontalScrollBar()->value(), place);
        workspace.resize(1500, 900);
        QTRY_VERIFY(([&] { graph = phaseGraphOf(&workspace, 2); draft = phaseGraphOf(&workspace, 3); return graph && draft && graph->nodeRect(6).width() > 0; })());
        sideways = area();
    }
    const auto nodeIn = [](TeamUi::TaskView *view, int id) { for (const auto &n : view->nodes) if (n.id == id) return n; return TeamUi::TaskView::Node{}; };
    const auto nodeOf = [&](int id) { return nodeIn(graph, id); };
    // Only the dependencies from other phases show on the node; the edge
    // from #1 (phase 1) is not drawn inside phase 2.
    QCOMPARE(nodeOf(2).ext, QStringLiteral("↤#1"));
    QVERIFY(nodeOf(5).ext.isEmpty());
    QVERIFY(nodeOf(2).deps.isEmpty());
    QVERIFY(!graph->routed(1, 2));
    QVERIFY(graph->routed(3, 6) && graph->routed(2, 5));
    // The second line: the problem as Board and Layers name it, never a
    // status word; the full status in the tooltip.
    QCOMPARE(nodeOf(2).problemText, QStringLiteral("Waiting for your answer"));
    QVERIFY(nodeOf(2).problem);
    QVERIFY(nodeOf(5).problemText.isEmpty());
    QVERIFY2(nodeOf(2).tooltip.startsWith(QStringLiteral("#2 Saved cards API — ")), qPrintable(nodeOf(2).tooltip));
    QVERIFY(nodeOf(2).tooltip.contains(QStringLiteral(" · after #1 (earlier phase) · Backend · deepseek-v4-pro")));
    QVERIFY(nodeOf(3).working);
    QVERIFY(!nodeOf(2).working);
    // The draft phase's nodes too: its other-phase dependencies and tooltip.
    QCOMPARE(nodeIn(draft, 8).ext, QStringLiteral("↤#2"));
    QCOMPARE(nodeIn(draft, 9).ext, QStringLiteral("↤#6"));
    QVERIFY(nodeIn(draft, 7).ext.isEmpty());
    QVERIFY(nodeIn(draft, 8).tooltip.contains(QStringLiteral(" · after #2 (earlier phase) · Backend · deepseek-v4-pro")));
    QVERIFY(draft->routed(8, 7) && draft->routed(9, 7));
    // The long edge #3 -> #6 passes #5's column without crossing it.
    const auto route = graph->route(3, 6).points;
    for (int i = 1; i < route.size(); ++i)
        QVERIFY(!QRectF(route.at(i - 1), route.at(i)).normalized().adjusted(-.01, -.01, .01, .01).intersects(graph->nodeRect(5).adjusted(1, 1, -1, -1)));
    // The draft phase's tasks take edge edits through edit-draft; the running
    // phase's tasks take none.
    QVERIFY(draft->canEdit(7) && !graph->canEdit(5));
    QVERIFY(draft->linked && !graph->linked);
    draft->linked(9, 8);
    QTRY_COMPARE(calls(script(), "edit-draft").size(), 1);
    const auto input = calls(script(), "edit-draft").first();
    QCOMPARE(input["phase"].toInt(), 3);
    const auto update = input["tasks_update"].toArray().first().toObject();
    QCOMPARE(update["id"].toInt(), 8);
    QCOMPARE(update["deps"].toArray().size(), 2);
    QCOMPARE(update["deps"].toArray().last().toObject()["task"].toInt(), 9);
    QVERIFY(calls(script(), "plan").isEmpty());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "ux-5-graph-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "ux-5-graph-light");
    }
}

// QA #500, finding 1 (SPEC 5.3): on Board at 1440x900 a phase header keeps
// its actions in view while the five columns scroll sideways.
void TestTeamsUiWorkspace::boardPhaseHeaderKeepsItsActionsInView()
{
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1440, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    // A poll rebuilds the board: the area is looked up each time.
    const auto area = [&] { return qobject_cast<QScrollArea *>(workspace.findChild<QWidget *>("teamWorkspaceBoard")); };
    QTRY_VERIFY(area() && phaseScrollOf(&workspace, 3) && phaseScrollOf(&workspace, 3)->findChild<QPushButton *>("teamPhaseAccept"));
    // The columns need more room than the viewport: the page scrolls sideways.
    QTRY_VERIFY(area() && area()->horizontalScrollBar()->maximum() > 0);
    const auto inView = [&](const char *name, int place) {
        auto *board = area();
        auto *scroll = phaseScrollOf(&workspace, 3);
        auto *widget = scroll ? scroll->findChild<QWidget *>(name) : nullptr;
        if (!board || !widget || !widget->isVisible()) return false;
        // Placed at a fraction of the sideways range: 0, half or the end.
        auto *bar = board->horizontalScrollBar();
        const int value = bar->maximum() * place / 2;
        if (bar->value() != value) { bar->setValue(value); return false; }
        const QRect rect(widget->mapTo(board->viewport(), QPoint(0, 0)), widget->size());
        return board->viewport()->rect().contains(rect);
    };
    for (int place : {0, 1, 2})
        for (const char *name : {"teamPhaseAutoAccept", "teamPhaseReject", "teamPhaseAccept", "teamPhaseMore"})
            QTRY_VERIFY2(inView(name, place), qPrintable(QStringLiteral("%1 at %2/2 of the sideways range").arg(name).arg(place)));
}

void TestTeamsUiWorkspace::graphNodesCarryTheActivityLabel()
{
    setTeam(labelTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    // #9's member is offline: its problem text must win over the label.
    // Backend's agent runs; the lead's does not.
    const LiveLookup live = [](const Member &m) { Live l; l.known = true; if (m.id == "blocked") l.offline = true; l.working = m.id == "backend"; return l; };
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    TeamUi::TaskView *graph = nullptr;
    QTRY_VERIFY(([&] { graph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph")); return graph && graph->nodeRect(1).width() > 0; })());
    // U58: cancelled #12 is hidden until Show cancelled brings it back.
    QVERIFY(std::none_of(graph->nodes.cbegin(), graph->nodes.cend(), [](const TeamUi::TaskView::Node &n) { return n.id == 12; }));
    controller.setShowCancelled(true);
    QTRY_VERIFY(([&] { graph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
                       return graph && std::any_of(graph->nodes.cbegin(), graph->nodes.cend(), [](const TeamUi::TaskView::Node &n) { return n.id == 12; }); })());
    const auto node = [&](int id) { for (const auto &n : graph->nodes) if (n.id == id) return n; return TeamUi::TaskView::Node{}; };
    // Each rule of the owner's addition: Submitted, attempt 2, Held, a
    // sub-team, and (below) words for Pending, Draft, Accepted. Owner, 2026-10-28:
    // work in progress says where it is - working, starting or idle.
    QCOMPARE(node(1).activity, QStringLiteral("review"));
    QCOMPARE(node(2).activity, QStringLiteral("attempt 2 · working"));
    QCOMPARE(node(3).activity, QStringLiteral("working"));
    QCOMPARE(node(10).activity, QStringLiteral("starting"));
    QCOMPARE(node(11).activity, QStringLiteral("idle"));
    QCOMPARE(node(12).activity, QStringLiteral("cancelled"));
    QCOMPARE(node(4).activity, QStringLiteral("held"));
    QCOMPARE(node(5).activity, QStringLiteral("sub-team 1/2"));
    // Owner, 2026-10-29: every task says its state in words: a returned task
    // and what it waits for, a draft, an accepted one.
    QCOMPARE(node(6).activity, QStringLiteral("returned · waits for Backend"));
    QCOMPARE(node(7).activity, QStringLiteral("draft"));
    QCOMPARE(node(8).activity, QStringLiteral("accepted"));
    // A problem text wins; the label stays empty.
    QVERIFY(node(9).problem);
    QVERIFY(!node(9).problemText.isEmpty());
    QVERIFY(node(9).activity.isEmpty());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "ux-5-graph-label-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "ux-5-graph-label-light");
        darkPalette(); workspace.setTheme(true);
    }
    // The same label on Board cards, right of the role chip and model tile.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    const auto card = [&](int id) {
        for (auto *frame : workspace.findChildren<QFrame *>("teamBoardCard"))
            if (frame->property("task").toInt() == id) {
                auto *a = frame->findChild<QLabel *>("teamBoardActivity");
                return a ? a->text() : QString();
            }
        return QStringLiteral("?");
    };
    QCOMPARE(card(1), QStringLiteral("review"));
    QCOMPARE(card(2), QStringLiteral("attempt 2 · working"));
    QCOMPARE(card(3), QStringLiteral("working"));
    QCOMPARE(card(4), QStringLiteral("held"));
    QCOMPARE(card(6), QStringLiteral("returned · waits for Backend"));
    QVERIFY(card(9).isEmpty());
    // The same label on Layers rows: the list view carries it on its nodes.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "layers") { b->click(); break; }
    // Owner, 2026-10-29: a one-phase plan is a phase scroll too; its Layers
    // rows carry the label.
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers") && !workspace.findChildren<QFrame *>("teamLayerRow").isEmpty());
    const auto layerLabel = [&](int id) {
        for (auto *row : workspace.findChildren<QFrame *>("teamLayerRow"))
            if (row->property("task").toInt() == id) {
                auto *a = row->findChild<QLabel *>("teamLayerActivity");
                return a ? a->text() : QString();
            }
        return QStringLiteral("?");
    };
    QCOMPARE(layerLabel(1), QStringLiteral("review"));
    QCOMPARE(layerLabel(4), QStringLiteral("held"));
}

void TestTeamsUiWorkspace::cancelledWorkIsHiddenUntilShown()
{
    setTeam(cancelledTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The reads ask hgs for the cancelled work; the desktop hides it itself.
    QTRY_VERIFY(!calls(script(), "status").isEmpty());
    QVERIFY(calls(script(), "status").last()["include_cancelled"].toBool());
    QVERIFY(calls(script(), "list").last()["include_cancelled"].toBool());
    QVERIFY(!controller.showCancelled());
    // Graph: #6 is gone from phase 2, phase 4 is not shown at all.
    QTRY_VERIFY(phaseNodeIds(&workspace).contains(4));
    QVERIFY(!phaseNodeIds(&workspace).contains(6));
    QVERIFY(phaseScrollOf(&workspace, 2) && phaseScrollOf(&workspace, 3));
    QVERIFY(!phaseScrollOf(&workspace, 4));
    auto *show = workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled");
    QVERIFY(show && show->isCheckable() && !show->isChecked());
    QCOMPARE(show->text(), QStringLiteral("Show cancelled (3)"));
    shot(&workspace, "u58-cancelled-hidden-dark");
    const auto view = [&](const QString &value) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == value) { b->click(); break; }
    };
    const auto ids = [&](const char *name) {
        QList<int> found;
        for (auto *frame : workspace.findChildren<QFrame *>(name)) found << frame->property("task").toInt();
        std::sort(found.begin(), found.end());
        return found;
    };
    // Board and Layers leave them out too, and so does a fully cancelled phase.
    view("board");
    QTRY_VERIFY(ids("teamBoardCard").contains(4));
    QVERIFY(!ids("teamBoardCard").contains(6) && !ids("teamBoardCard").contains(7) && !ids("teamBoardCard").contains(8));
    QVERIFY(!phaseScrollOf(&workspace, 4));
    QVERIFY(workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled"));
    view("layers");
    QTRY_VERIFY(ids("teamLayerRow").contains(4));
    QVERIFY(!ids("teamLayerRow").contains(6) && !phaseScrollOf(&workspace, 4));
    // Show cancelled brings back the task and the phase, which reads cancelled.
    workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled")->click();
    QVERIFY(controller.showCancelled());
    QTRY_VERIFY(ids("teamLayerRow").contains(6));
    QVERIFY(phaseScrollOf(&workspace, 4));
    QCOMPARE(phaseScrollOf(&workspace, 4)->property("state").toString(), QStringLiteral("cancelled"));
    view("board");
    QTRY_VERIFY(ids("teamBoardCard").contains(6));
    view("graph");
    QTRY_VERIFY(phaseNodeIds(&workspace).contains(6));
    QCOMPARE(phaseScrollOf(&workspace, 4)->property("state").toString(), QStringLiteral("cancelled"));
    show = workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled");
    QVERIFY(show->isChecked());
    QCOMPARE(show->text(), QStringLiteral("Hide cancelled (3)"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u58-cancelled-shown-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "u58-cancelled-shown-light");
        darkPalette(); workspace.setTheme(true);
    }
    // And hides them again (the theme change rebuilt the toolbar).
    workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled")->click();
    QTRY_VERIFY(!phaseScrollOf(&workspace, 4));
    QVERIFY(!phaseNodeIds(&workspace).contains(6));
    if (shotsWanted()) {   // #713: the light half of the shots
        workspace.setTheme(false);
        lightShot(&workspace, "u58-cancelled-hidden-light");
        darkPalette(); workspace.setTheme(true);
    }
    // A team without cancelled work has no toggle and shows every task, as before.
    setTeam(phasedTeam());
    QTRY_VERIFY(([&] { controller.refresh(); return !workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled") && phaseNodeIds(&workspace).contains(4); })());
}

void TestTeamsUiWorkspace::workingTimeFollowsTheActivityLabel()
{
    // The test owns the clock; the minute ticks come by hand.
    double now = 9000;
    TeamUi::workedClock() = [&now] { return now; };
    const auto restore = qScopeGuard([] { TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); }; });
    const auto tick = [] { QVERIFY(QMetaObject::invokeMethod(&TeamUi::workedTicker(), "timeout")); };
    QCOMPARE(TeamUi::workedTicker().interval(), 60 * 1000);
    setTeam(timingTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    const LiveLookup live = [](const Member &m) { Live l; l.known = true; if (m.id == "blocked") l.offline = true; l.working = m.id == "backend"; return l; };
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    // Wide enough that the shots show the done task's layer and column.
    workspace.resize(1900, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The text itself: minutes, then hours with two-digit minutes.
    QCOMPARE(TeamUi::workedText(0), QStringLiteral("<1m"));
    QCOMPARE(TeamUi::workedText(12 * 60 + 59), QStringLiteral("12m"));
    QCOMPARE(TeamUi::workedText(3600 + 5 * 60), QStringLiteral("1h 05m"));
    QCOMPARE(TeamUi::workedText(26 * 3600 + 40 * 60), QStringLiteral("26h 40m"));
    const auto graph = [&] { return dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph")); };
    QTRY_VERIFY(graph() && graph()->nodeRect(1).width() > 0);
    const auto node = [&](TeamUi::TaskView *view, int id) { for (const auto &n : view->nodes) if (n.id == id) return n; return TeamUi::TaskView::Node{}; };
    // Running: since the current attempt started; done: the sum; never
    // worked: nothing. A problem text keeps the time beside it.
    QCOMPARE(node(graph(), 2).worked, QStringLiteral("1h 05m"));
    QCOMPARE(node(graph(), 2).activity, QStringLiteral("attempt 2 · working"));
    QCOMPARE(node(graph(), 3).worked, QStringLiteral("12m"));
    QCOMPARE(node(graph(), 1).worked, QStringLiteral("10m"));
    QCOMPARE(node(graph(), 8).worked, QStringLiteral("1h 15m"));
    QCOMPARE(node(graph(), 11).worked, QStringLiteral("1m"));
    QCOMPARE(node(graph(), 9).worked, QStringLiteral("<1m"));
    QVERIFY(node(graph(), 9).problem);
    QVERIFY(node(graph(), 4).worked.isEmpty());
    QVERIFY(node(graph(), 6).worked.isEmpty());
    QVERIFY(node(graph(), 7).worked.isEmpty());
    // The tooltip: each attempt with its waits, the task's waits named apart.
    const QString retried = node(graph(), 2).tooltip;
    QVERIFY2(retried.contains("Working 1h 05m, since "), qPrintable(retried));
    QVERIFY2(retried.contains("Attempt 1 (backend, rejected): worked 25m · waited 2m for a free member · 10m for the lead"), qPrintable(retried));
    QVERIFY2(retried.contains("Attempt 2 (backend): working since "), qPrintable(retried));
    QVERIFY2(retried.contains("Waited 40m for dependencies · 3m for a free member · 10m for the lead, not counted"), qPrintable(retried));
    QVERIFY2(node(graph(), 8).tooltip.contains("Worked 1h 15m\n"), qPrintable(node(graph(), 8).tooltip));
    QVERIFY(node(graph(), 8).tooltip.contains("Not recorded: the lead's decision"));
    QVERIFY(!node(graph(), 6).tooltip.contains("Worked"));
    const auto shoot = [&](const QString &name) {
        if (shotsWanted()) {   // #713: the light half of the shots
            shot(&workspace, name + "-dark");
            lightPalette(); workspace.setTheme(false); workspace.render(); QTest::qWait(200);
            lightShot(&workspace, name + "-light");
            darkPalette(); workspace.setTheme(true); workspace.render(); QTest::qWait(150);
        }
    };
    shoot("ux-u37-graph");
    // One tick a minute: the view keeps its nodes and only the running times
    // change.
    QPointer<TeamUi::TaskView> ticked = graph();
    now = 12000;
    tick();
    QVERIFY(ticked);
    QCOMPARE(node(ticked, 3).worked, QStringLiteral("1h 02m"));
    QCOMPARE(node(ticked, 2).worked, QStringLiteral("1h 55m"));
    QCOMPARE(node(ticked, 9).worked, QStringLiteral("50m"));
    QCOMPARE(node(ticked, 8).worked, QStringLiteral("1h 15m"));
    QCOMPARE(node(ticked, 1).worked, QStringLiteral("10m"));
    now = 9000; tick();
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == segment) { b->click(); break; }
    };
    // The Board card: right after the activity label, in its color and size.
    view("board");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    const auto card = [&](int id) -> QFrame * {
        for (auto *frame : workspace.findChildren<QFrame *>("teamBoardCard")) if (frame->property("task").toInt() == id) return frame;
        return nullptr;
    };
    const auto worked = [&](int id) -> QLabel * { auto *frame = card(id); return frame ? frame->findChild<QLabel *>("teamBoardWorked") : nullptr; };
    QTRY_VERIFY(worked(3));
    QCOMPARE(worked(3)->text(), QStringLiteral("12m"));
    QCOMPARE(worked(2)->text(), QStringLiteral("1h 05m"));
    QCOMPARE(worked(8)->text(), QStringLiteral("1h 15m"));
    QVERIFY(!worked(6));
    auto *activity = card(3)->findChild<QLabel *>("teamBoardActivity");
    QCOMPARE(activity->text(), QStringLiteral("working"));
    QCOMPARE(worked(3)->font().pixelSize(), activity->font().pixelSize());
    QCOMPARE(worked(3)->palette().color(QPalette::WindowText), activity->palette().color(QPalette::WindowText));
    QVERIFY(worked(3)->geometry().left() > activity->geometry().right());
    QVERIFY(worked(2)->toolTip().contains("Attempt 1 (backend, rejected): worked 25m"));
    // A running time keeps the room it grows to: "12m" becoming "1h 02m"
    // moves nothing after it.
    QVERIFY(worked(3)->minimumWidth() >= worked(3)->fontMetrics().horizontalAdvance(QStringLiteral("1h 02m")));
    auto *state = card(3)->findChild<QWidget *>("teamBoardState");
    TestWait::settle();
    const int stateLeft = state->geometry().left();
    QPointer<QLabel> label = worked(3);
    now = 12000;
    tick();
    QVERIFY(label);
    QCOMPARE(label->text(), QStringLiteral("1h 02m"));
    QCOMPARE(worked(8)->text(), QStringLiteral("1h 15m"));
    TestWait::settle();
    QCOMPARE(state->geometry().left(), stateLeft);
    now = 9000; tick();
    shoot("ux-u37-board");
    // The Layers row: the same text after the activity label (the phase
    // scroll's rows since the one-phase plan has its scroll too).
    view("layers");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers") && !workspace.findChildren<QFrame *>("teamLayerRow").isEmpty());
    const auto layer = [&](int id) -> QLabel * {
        for (auto *row : workspace.findChildren<QFrame *>("teamLayerRow"))
            if (row->property("task").toInt() == id) return row->findChild<QLabel *>("teamLayerWorked");
        return nullptr;
    };
    QTRY_VERIFY(layer(2));
    QCOMPARE(layer(2)->text(), QStringLiteral("1h 05m"));
    QCOMPARE(layer(8)->text(), QStringLiteral("1h 15m"));
    QCOMPARE(layer(11)->text(), QStringLiteral("1m"));
    QVERIFY(layer(2)->toolTip().contains("Attempt 1 (backend, rejected): worked 25m"));
    now = 12000; tick();
    QCOMPARE(layer(3)->text(), QStringLiteral("1h 02m"));
    now = 9000; tick();
    shoot("ux-u37-layers");
    // An older hgs sends no timing: nothing is shown.
    const auto old = parse(labelTeam());
    QVERIFY(!old.task(2)->timing.known);
    QVERIFY(TeamUi::workedLabel(*old.task(2), now).isEmpty());
    QVERIFY(TeamUi::workedTip(*old.task(2), now).isEmpty());
}

// Owner 2026-10-31 (#746): every task node in Graph, Board and Layers, and the
// task card, shows the estimate ("~30 min"), a running or done task its time
// against it ("12 of ~45 min"), past it in the warning color, and "no
// estimate yet" without one; an older hgs keeps the plain working time. The
// running times tick against the estimate.
void TestTeamsUiWorkspace::everyNodeShowsItsEstimateAndTheTimeAgainstIt()
{
    double now = 9000;
    TeamUi::workedClock() = [&now] { return now; };
    const auto restore = qScopeGuard([] { TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); }; });
    const auto tick = [] { QVERIFY(QMetaObject::invokeMethod(&TeamUi::workedTicker(), "timeout")); };
    setTeam(estimateTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    const LiveLookup live = [](const Member &m) { Live l; l.known = true; if (m.id == "blocked") l.offline = true; l.working = m.id == "backend"; return l; };
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1900, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The text itself.
    QCOMPARE(TeamUi::estimateTime(45, true, 12 * 60, true).text, QStringLiteral("12 of ~45 min"));
    QVERIFY(!TeamUi::estimateTime(45, true, 12 * 60, true).over);
    QVERIFY(TeamUi::estimateTime(45, true, 46 * 60, true).over);
    QCOMPARE(TeamUi::estimateTime(45, true, 0, false).text, QStringLiteral("~45 min"));
    QCOMPARE(TeamUi::estimateTime(-1, true, 0, false).text, QStringLiteral("no estimate yet"));
    QCOMPARE(TeamUi::estimateTime(-1, true, 5 * 60, true).text, QStringLiteral("5 min, no estimate yet"));
    QCOMPARE(TeamUi::estimateTime(-1, false, 12 * 60, true).text, QStringLiteral("12m"));   // an older hgs
    const auto graph = [&] { return dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph")); };
    QTRY_VERIFY(graph() && graph()->nodeRect(1).width() > 0);
    const auto node = [&](TeamUi::TaskView *view, int id) { for (const auto &n : view->nodes) if (n.id == id) return n; return TeamUi::TaskView::Node{}; };
    QCOMPARE(node(graph(), 1).worked, QStringLiteral("10 of ~15 min"));
    QCOMPARE(node(graph(), 2).worked, QStringLiteral("65 of ~40 min"));
    QVERIFY(node(graph(), 2).timeOver);
    QCOMPARE(node(graph(), 3).worked, QStringLiteral("12 of ~45 min"));
    QVERIFY(!node(graph(), 3).timeOver);
    QCOMPARE(node(graph(), 4).worked, QStringLiteral("~30 min"));
    QCOMPARE(node(graph(), 5).worked, QStringLiteral("no estimate yet"));
    QCOMPARE(node(graph(), 8).worked, QStringLiteral("75 of ~60 min"));
    QVERIFY(node(graph(), 8).timeOver);
    QCOMPARE(node(graph(), 11).worked, QStringLiteral("1m"));   // no estimate fields: as before
    QVERIFY2(node(graph(), 3).tooltip.contains("Estimate ~45 min, from the lead's plan"), qPrintable(node(graph(), 3).tooltip));
    QVERIFY(node(graph(), 2).tooltip.contains("Estimate ~40 min, from this role's solved tasks"));
    QVERIFY(node(graph(), 5).tooltip.contains("No estimate yet: it fills in after this role's first solved task"));
    // No dots as separators in the new texts.
    for (int id : {1, 2, 3, 4, 5, 8}) QVERIFY(!node(graph(), id).worked.contains(QStringLiteral(" · ")));
    const auto shoot = [&](const QString &name) {
        if (shotsWanted()) {
            shot(&workspace, name + "-dark");
            lightPalette(); workspace.setTheme(false); workspace.render(); QTest::qWait(200);
            lightShot(&workspace, name + "-light");
            darkPalette(); workspace.setTheme(true); workspace.render(); QTest::qWait(150);
        }
    };
    shoot("u746-graph");
    // A tick: #3 passes its 45 minutes and turns to the warning color in place.
    QPointer<TeamUi::TaskView> ticked = graph();
    now = 12000; tick();
    QVERIFY(ticked);
    QCOMPARE(node(ticked, 3).worked, QStringLiteral("62 of ~45 min"));
    QVERIFY(node(ticked, 3).timeOver);
    QCOMPARE(node(ticked, 4).worked, QStringLiteral("~30 min"));
    now = 9000; tick();
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == segment) { b->click(); break; }
    };
    const QColor warn = TeamUi::colorsFor(true).warn, muted = TeamUi::colorsFor(true).text3;
    // The Board.
    view("board");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    const auto worked = [&](int id) -> QLabel * {
        for (auto *frame : workspace.findChildren<QFrame *>("teamBoardCard"))
            if (frame->property("task").toInt() == id) return frame->findChild<QLabel *>("teamBoardWorked");
        return nullptr;
    };
    QTRY_VERIFY(worked(3));
    QCOMPARE(worked(3)->text(), QStringLiteral("12 of ~45 min"));
    QCOMPARE(worked(3)->palette().color(QPalette::WindowText), muted);
    QCOMPARE(worked(2)->text(), QStringLiteral("65 of ~40 min"));
    QCOMPARE(worked(2)->palette().color(QPalette::WindowText), warn);
    QCOMPARE(worked(4)->text(), QStringLiteral("~30 min"));
    QVERIFY(worked(2)->toolTip().contains("Estimate ~40 min"));
    // A running time keeps the room it grows to.
    // As many digits as the larger of its minutes and the estimate.
    QVERIFY(worked(3)->minimumWidth() >= worked(3)->fontMetrics().horizontalAdvance(QStringLiteral("88 of ~45 min")));
    QPointer<QLabel> running = worked(3);
    now = 12000; tick();
    QVERIFY(running);
    QCOMPARE(running->text(), QStringLiteral("62 of ~45 min"));
    QCOMPARE(running->palette().color(QPalette::WindowText), warn);
    now = 9000; tick();
    view("graph"); view("board");   // rebuilt at 9000
    QTRY_VERIFY(worked(3) && worked(3)->text() == QStringLiteral("12 of ~45 min"));
    shoot("u746-board");
    // The Layers.
    view("layers");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers") && !workspace.findChildren<QFrame *>("teamLayerRow").isEmpty());
    const auto layer = [&](int id) -> QLabel * {
        for (auto *row : workspace.findChildren<QFrame *>("teamLayerRow"))
            if (row->property("task").toInt() == id) return row->findChild<QLabel *>("teamLayerWorked");
        return nullptr;
    };
    QTRY_VERIFY(layer(2));
    QCOMPARE(layer(2)->text(), QStringLiteral("65 of ~40 min"));
    QCOMPARE(layer(5)->text(), QStringLiteral("no estimate yet"));
    QCOMPARE(layer(8)->text(), QStringLiteral("75 of ~60 min"));
    shoot("u746-layers");
    // The task card.
    TeamTaskCard card;
    card.resize(420, 640); card.show();
    const auto team = parse(estimateTeam());
    const auto estimate = [&] { auto *found = card.findChild<QLabel *>("teamTaskEstimate"); return found ? found->text() : QStringLiteral("<none>"); };
    card.setTask(team, 3, live, true);
    QCOMPARE(estimate(), QStringLiteral("12 of ~45 min"));
    QVERIFY(card.findChild<QLabel *>("teamTaskEstimate")->toolTip().contains("the lead's plan"));
    card.setTask(team, 4, live, true);
    QCOMPARE(estimate(), QStringLiteral("~30 min"));
    card.setTask(team, 5, live, true);
    QCOMPARE(estimate(), QStringLiteral("no estimate yet"));
    card.setTask(team, 11, live, true);
    QCOMPARE(estimate(), QStringLiteral("<none>"));   // an older hgs: no row
    card.setTask(team, 2, live, true);
    QCOMPARE(estimate(), QStringLiteral("65 of ~40 min"));
    QCOMPARE(card.findChild<QLabel *>("teamTaskEstimate")->palette().color(QPalette::WindowText), warn);
    if (shotsWanted()) {
        shot(&card, "u746-task-card-dark");
        lightPalette(); card.setTask(team, 2, live, false); QTest::qWait(150);
        lightShot(&card, "u746-task-card-light");
        darkPalette();
    }
    // The spent minutes change every minute; the views never rebuild for them.
    auto a = estimateTeam(), b = estimateTeam();
    auto tasks = b["tasks"].toArray(); auto three = tasks[2].toObject(); three["estimate_spent_minutes"] = 13; tasks[2] = three; b["tasks"] = tasks;
    QCOMPARE(Teams::visibleShape(a), Teams::visibleShape(b));
}

// U33: the Board scrolls sideways instead of squeezing its columns. At 700 px
// every column keeps its readable width and the thin bar shows; the columns of
// two open phases share their x positions; the horizontal place survives a
// poll with a new revision and a phase collapsed; at 1600 px there is no bar.
// Graph and Layers keep their columns without a horizontal bar.
void TestTeamsUiWorkspace::boardScrollsInsteadOfSqueezingColumns()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    setTeam(prototypeTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    workspace.resize(700, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamPhaseScrolls"));
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == segment) { b->click(); return; }
    };
    // Graph and Layers fit the column: no horizontal bar on their page.
    QCOMPARE(workspace.findChild<QScrollArea *>("teamWorkspacePhases")->horizontalScrollBarPolicy(), Qt::ScrollBarAlwaysOff);
    view("layers");
    QTRY_VERIFY(workspace.findChild<QScrollArea *>("teamWorkspaceLayers"));
    QCOMPARE(workspace.findChild<QScrollArea *>("teamWorkspaceLayers")->horizontalScrollBarPolicy(), Qt::ScrollBarAlwaysOff);
    view("board");
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && !area()->findChildren<QFrame *>("teamBoardColumn").isEmpty());
    // The bar is on exactly while the columns need more room (it never flickers).
    QVERIFY(area()->horizontalScrollBarPolicy() != Qt::ScrollBarAsNeeded);
    if (auto *all = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll"); all->text() == QStringLiteral("Expand all")) all->click();
    const auto columns = [&] { return area()->findChildren<QFrame *>("teamBoardColumn"); };
    const auto phasesOpen = [&] { QSet<int> open; for (auto *col : columns()) open.insert(col->property("phase").toInt()); return open.size(); };
    QTRY_VERIFY(phasesOpen() >= 2);
    // Narrow: every column keeps its readable width; the bar scrolls the Board.
    const auto readable = [&] {
        for (auto *col : columns()) if (col->width() < 200) return false;
        return true;
    };
    QTRY_VERIFY(readable());
    QTRY_VERIFY(area()->horizontalScrollBar()->isVisible() && area()->horizontalScrollBar()->maximum() > 0);
    // The columns of every open phase line up: the same set at the same x.
    const auto positions = [&] {
        QMap<int, QList<int>> xs;
        for (auto *col : columns()) xs[col->property("phase").toInt()] << col->mapTo(area()->widget(), QPoint(0, 0)).x();
        for (auto &list : xs) std::sort(list.begin(), list.end());
        return xs;
    };
    const auto xs = positions();
    QVERIFY(xs.size() >= 2);
    for (const auto &list : xs) QCOMPARE(list, xs.first());
    // The horizontal place survives a poll that rebuilds (#693: one with a
    // change; a new revision alone rebuilds nothing) and a phase collapsed.
    area()->horizontalScrollBar()->setValue(120);
    QCOMPARE(area()->horizontalScrollBar()->value(), 120);
    auto *before = area();
    auto next = prototypeTeam(); next["revision"] = 21; next["description"] = "changed"; setTeam(next);
    controller.refresh();
    QTRY_VERIFY(area() && area() != before);
    QTRY_COMPARE(area()->horizontalScrollBar()->value(), 120);
    shot(&workspace, "u33-board-narrow-dark");
    QFrame *firstOpen = nullptr;
    for (auto *frame : area()->findChildren<QFrame *>("teamPhaseScroll")) if (frame->findChild<QWidget *>("teamPhaseBody")) { firstOpen = frame; break; }
    QVERIFY(firstOpen);
    const int collapsed = firstOpen->property("phase").toInt();
    before = area();
    firstOpen->findChild<QToolButton *>("teamPhaseToggle")->click();
    QTRY_VERIFY(area() && area() != before);
    QTRY_VERIFY(!positions().contains(collapsed));
    QTRY_COMPARE(area()->horizontalScrollBar()->value(), 120);
    firstOpen = nullptr;
    for (auto *frame : area()->findChildren<QFrame *>("teamPhaseScroll")) if (frame->property("phase").toInt() == collapsed) firstOpen = frame;
    firstOpen->findChild<QToolButton *>("teamPhaseToggle")->click();
    QTRY_VERIFY(positions().contains(collapsed));
    workspace.setTheme(false);
    lightShot(&workspace, "u33-board-narrow-light");
    // Wide: the columns share the room, no bar.
    workspace.resize(1600, 900);
    QTRY_VERIFY2(!area()->horizontalScrollBar()->isVisible(), qPrintable(QStringLiteral("viewport %1, content min %2, columns %3")
        .arg(area()->viewport()->width()).arg(area()->widget()->minimumSizeHint().width()).arg(columns().size())));
    QVERIFY(readable());
    QSet<int> widths; for (auto *col : columns()) widths.insert(col->width());
    QVERIFY2(widths.size() <= 2, "the columns share the width");   // integer rounding at most
    lightShot(&workspace, "u33-board-wide-light");
    darkPalette(); workspace.setTheme(true);
    QTRY_VERIFY(area() && !area()->horizontalScrollBar()->isVisible());
    shot(&workspace, "u33-board-wide-dark");
}

// U33: the planning team's Board (no phases) gets the same: readable columns
// and one horizontal scroll at 700 px, none at 1600 px, the place kept.
void TestTeamsUiWorkspace::planningBoardScrollsInsteadOfSqueezingColumns()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    auto planning = team(); planning["status"] = "planning";
    setTeam(planning);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(700, 800); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY_WITH_TIMEOUT(workspace.findChild<QWidget *>("teamWorkspaceView"), 20000);
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    // Owner, 2026-10-29: a one-phase plan is a phase scroll too; its columns
    // are frames in the board that scrolls as a whole (U33).
    QTRY_VERIFY(area() && area()->findChildren<QWidget *>("teamBoardColumn").size() == 5);   // no Decided task, no Decided column
    const auto readable = [&] {
        for (auto *col : area()->findChildren<QWidget *>("teamBoardColumn")) if (col->width() < 200) return false;
        return true;
    };
    QTRY_VERIFY(readable());
    QTRY_VERIFY(area()->horizontalScrollBar()->isVisible());
    area()->horizontalScrollBar()->setValue(90);
    auto *before = area();
    // #693: a poll with a change rebuilds; the place stays.
    auto next = planning; next["revision"] = planning["revision"].toInt() + 1; next["description"] = "changed"; setTeam(next);
    controller.refresh();
    QTRY_VERIFY(area() && area() != before);
    QTRY_COMPARE(area()->horizontalScrollBar()->value(), 90);
    workspace.resize(1600, 800);
    QTRY_VERIFY(!area()->horizontalScrollBar()->isVisible());
    QVERIFY(readable());
}

void TestTeamsUiWorkspace::boardColumnsKeepTheirScroll()
{
    // Thirty pending tasks: more than a column shows.
    auto t = team();
    auto tasks = t["tasks"].toArray();
    for (int id = 6; id < 36; ++id) tasks.append(QJsonObject{{"id", id}, {"title", QStringLiteral("Bin %1").arg(id)}, {"assignee", "tester"}, {"status", "pending"}});
    t["tasks"] = tasks;
    QJsonObject layers; for (const auto &k : tasks) layers[QString::number(k.toObject()["id"].toInt())] = 0;
    t["layers"] = layers;
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The first team read took 6.6 s on a loaded computer.
    QTRY_VERIFY_WITH_TIMEOUT(workspace.findChild<QWidget *>("teamWorkspaceView"), 30000);
    const auto showBoard = [&workspace] {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    };
    showBoard();
    // Owner, 2026-10-29: a one-phase plan is a phase scroll too; the board
    // scrolls as a whole (U33), and a render keeps where it was.
    const auto pending = [&workspace]() -> QScrollBar * {
        auto *area = workspace.findChild<QScrollArea *>("teamWorkspaceBoard");
        return area ? area->verticalScrollBar() : nullptr;
    };
    QTRY_VERIFY(pending() && pending()->maximum() > 0);
    pending()->setValue(pending()->maximum() / 2);
    const int scrolled = pending()->value();
    QVERIFY(scrolled > 0);
    // A poll renders the board again; the column stays where it was.
    const QPointer<QScrollBar> old = pending();
    showBoard();
    QTRY_VERIFY(old.isNull());
    QTRY_VERIFY(pending());
    QTRY_COMPARE(pending()->value(), scrolled);
}

// U25 part 1: the Compact skin carries every prototype token for both themes,
// and any skin answers a token it does not name with the prototype's value.
void TestTeamsUiWorkspace::skinTokensMatchThePrototype()
{
    const auto skins = WorkspaceSkin::builtins();
    QCOMPARE(skins[1].id, QString("whitewh"));
    for (const bool dark : {true, false})
        for (const auto &[name, d, l] : WorkspaceSkin::designTokens())
            QVERIFY2(WorkspaceSkin::color(skins[1], dark, QString::fromLatin1(name)) == QColor(QString::fromLatin1(dark ? d : l)),
                     qPrintable(QStringLiteral("%1 (%2)").arg(QString::fromLatin1(name), dark ? "dark" : "light")));
    QCOMPARE(WorkspaceSkin::color(skins[1], true, "cardBorder"), QColor("#232932"));
    QCOMPARE(WorkspaceSkin::color(skins[1], false, "text3"), QColor("#415060"));
    // The plain Zerus skin keeps its own workspace colors and takes the rest.
    QCOMPARE(WorkspaceSkin::color(skins[0], true, "bg"), QColor(skins[0].dark.background));
    QCOMPARE(WorkspaceSkin::color(skins[0], true, "label"), QColor("#8f97a2"));
    // A skin's view colors override a token, roles included.
    auto own = skins[1];
    own.darkViewColors.insert("roleCoder", "#123456");
    QCOMPARE(WorkspaceSkin::color(own, true, "roleCoder"), QColor("#123456"));
    // The team surfaces read the tokens of the selected skin.
    Settings::set("workspace/skin", "whitewh");
    const auto c = TeamUi::colorsFor(false);
    QCOMPARE(c.text3, QColor("#415060")); QCOMPARE(c.accentMuted, QColor("#4f8f78")); QCOMPARE(c.label, QColor("#647181"));
    QCOMPARE(c.cardBorder, QColor("#e1e6eb")); QCOMPARE(c.segOn, QColor("#e0f0e9")); QCOMPARE(c.tile, QColor("#eef2f5"));
    Settings::remove("workspace/skin");
}

// U25 part 1: the new roles have the prototype's colors; a color the catalog
// gives a role wins over the palette.
void TestTeamsUiWorkspace::rolePaletteReadsTheCatalog()
{
    const auto saved = Teams::catalogRoleColors();
    Teams::catalogRoleColors().clear();
    Settings::set("workspace/skin", "whitewh");
    const QList<std::tuple<QString, QString, QString>> roles{
        {"secretary", "#7fcf9a", "#2f7d4a"}, {"coder", "#e8a75a", "#a35f10"}, {"qa", "#4fd1c5", "#0f7a72"},
        {"designer", "#ef7d8d", "#b8384b"}, {"leadgpt", "#9aa5ff", "#4652c4"}, {"expert", "#e0b070", "#8a5d16"},
        {"proexpert", "#ef7d6d", "#b8402f"}, {"lead", "#b79cff", "#6f4fd8"}, {"backend", "#6fa8ff", "#2f66c9"}};
    for (const auto &[role, dark, light] : roles) {
        QCOMPARE(TeamUi::roleColor(role, true), QColor(dark));
        QCOMPARE(TeamUi::roleColor(role, false), QColor(light));
    }
    // Names as the catalog writes them: the longest palette role they contain.
    QCOMPARE(TeamUi::roleColor("Lead (GPT)", true), QColor("#9aa5ff"));
    QCOMPARE(TeamUi::roleColor("ProExpert", true), QColor("#ef7d6d"));
    QCOMPARE(TeamUi::roleColor("backend-2", true), QColor("#6fa8ff"));
    // Any other role gets a stable palette color, never a lead's.
    const QColor other = TeamUi::roleColor("cartographer", true);
    QCOMPARE(TeamUi::roleColor("cartographer", true), other);
    QVERIFY(other != QColor("#b79cff") && other != QColor("#9aa5ff"));
    // The Team designer recolors a role: the catalog's color wins, by id and name.
    Catalog recolored;
    Role coder; coder.id = "coder"; coder.name = "Coder"; coder.color = "#22aa55"; recolored.roles << coder;
    Role broken; broken.id = "qa"; broken.color = "url(x)"; recolored.roles << broken;
    Teams::rememberRoleColors(recolored);
    QCOMPARE(TeamUi::roleColor("coder", true), QColor("#22aa55"));
    QCOMPARE(TeamUi::roleColor("Coder", false), QColor("#22aa55"));
    QCOMPARE(TeamUi::roleColor("qa", true), QColor("#4fd1c5"));
    Teams::catalogRoleColors() = saved;
    Settings::remove("workspace/skin");
}

// U25 part 1: thin 8 px scroll bars without arrows, the thumb text3 at 24 %.
void TestTeamsUiWorkspace::scrollBarsAreThin()
{
    // #729: through Settings, as the desktop sets it (a queued skin would hide
    // a direct write).
    Settings::set("workspace/skin", "whitewh");
    const QString sheet = workspaceScrollbars(true);
    QVERIFY(sheet.contains("QScrollBar:vertical { width:8px;"));
    QColor thumb("#aab1ba"); thumb.setAlphaF(.24);
    QVERIFY(sheet.contains(thumb.name(QColor::HexArgb)));
    QColor hover("#aab1ba"); hover.setAlphaF(.45);
    QVERIFY(sheet.contains(hover.name(QColor::HexArgb)));
    QWidget box; box.setStyleSheet(sheet); box.resize(240, 160);
    auto *list = new QListWidget(&box); list->resize(240, 160);
    for (int i = 0; i < 60; ++i) list->addItem(QStringLiteral("Row %1").arg(i));
    box.show(); QTest::qWait(20);
    QTRY_VERIFY(list->verticalScrollBar()->isVisible());
    QCOMPARE(list->verticalScrollBar()->width(), 8);
    Settings::remove("workspace/skin");
}

// U25 part 1: the toothed gear of the prototype (the title bar's 19 px and the
// services chip) and the crossed-out bell of mute.
void TestTeamsUiWorkspace::gearAndBellOffIcons()
{
    const auto painted = [](const QString &name) {
        const QImage image = workspaceIcon(name, QColor("#ffffff")).pixmap(48, 48).toImage();
        int count = 0;
        for (int y = 0; y < image.height(); ++y) for (int x = 0; x < image.width(); ++x) count += image.pixelColor(x, y).alpha() > 128;
        return std::pair{image, count};
    };
    QVERIFY(prototypeIconPath("settings")); QVERIFY(prototypeIconPath("bell-off"));
    QPixmap probe(24, 24); probe.fill(Qt::transparent);
    { QPainter p(&probe); QVERIFY2(paintPrototypeIcon(p, 24, prototypeIconPath("settings"), Qt::white), "the SVG image plugin is missing"); }
    const auto [gear, gearInk] = painted("settings");
    const auto [bell, bellInk] = painted("bell");
    const auto [off, offInk] = painted("bell-off");
    QVERIFY(gearInk > 100); QVERIFY(bellInk > 60); QVERIFY(offInk > 60);
    QVERIFY(bell != off);
    // Mute is the crossed-out bell now.
    QCOMPARE(painted("mute").first, off);
    // The gear's teeth: its outline reaches the corners' diagonals, which the old
    // sun of spokes never did, and its centre hole is clear.
    QVERIFY(gear.pixelColor(24, 24).alpha() < 60);
    QVERIFY(!workspaceIcon("settings", QColor("#9aa3ad")).pixmap(19, 19).isNull());
}

// U25 part 1: the one collapsible block: chevron header, aria-expanded, the
// collapsed summary and its state in QSettings under its key.
void TestTeamsUiWorkspace::collapsibleBlockKeepsItsState()
{
    Settings::remove(CollapsibleBlock::settingsKey("test/folders"));
    QList<bool> changes;
    {
        CollapsibleBlock block("test/folders", "Folders", false);
        block.setNote("3"); block.setSummary("zerus, docs, mobile");
        auto *add = new QPushButton("Folder"); block.trailing()->addWidget(add);
        auto *row = new QLabel("D:/Work/zerus"); block.body()->addWidget(row);
        block.expandedChanged = [&](bool open) { changes << open; };
        block.resize(360, 120); block.show(); QTest::qWait(20);
        QVERIFY(!block.isExpanded());
        QVERIFY(!row->isVisible()); QVERIFY(add->isVisible());
        auto *header = block.findChild<QPushButton *>("collapsibleHeader");
        auto *summary = block.findChild<QPushButton *>("collapsibleSummary");
        QVERIFY(header && summary);
        QVERIFY(!header->isChecked()); QCOMPARE(header->property("expanded").toBool(), false);
        QCOMPARE(header->accessibleDescription(), QString("Collapsed")); QCOMPARE(header->accessibleName(), QString("Folders"));
        QVERIFY(summary->isVisible()); QCOMPARE(summary->text(), QString("zerus, docs, mobile"));
        QCOMPARE(block.findChild<QLabel *>("collapsibleNote")->text(), QString("3"));
        header->click();
        QVERIFY(block.isExpanded()); QVERIFY(row->isVisible()); QVERIFY(!summary->isVisible()); QVERIFY(add->isVisible());
        QVERIFY(header->isChecked()); QCOMPARE(header->accessibleDescription(), QString("Expanded"));
        QCOMPARE(Settings::value(CollapsibleBlock::settingsKey("test/folders")).toBool(), true);
        // The keyboard toggles it as well.
        header->setFocus(); QTest::keyClick(header, Qt::Key_Space);
        QVERIFY(!block.isExpanded());
        summary->click();   // the summary expands the block
        QVERIFY(block.isExpanded());
    }
    QCOMPARE(changes, (QList<bool>{true, false, true}));
    // A new block under the same key starts as it was left.
    CollapsibleBlock again("test/folders", "Folders", false);
    QVERIFY(again.isExpanded());
    // No key keeps no state.
    CollapsibleBlock loose({}, "Loose", false);
    loose.setExpanded(true);
    QVERIFY(!Settings::contains(CollapsibleBlock::settingsKey({})));
    Settings::remove(CollapsibleBlock::settingsKey("test/folders"));
}

// U25 part 1: the agent-works indicator: an accent ring and three dots in a
// wave under the model tile; static when animations are off.
void TestTeamsUiWorkspace::agentWorkingIndicatorFollowsTheSetting()
{
    Settings::set("workspace/skin", "whitewh");
    const auto frame = [](bool working, double phase) {
        QImage image(17, 17 + AgentWorking::Below, QImage::Format_ARGB32_Premultiplied); image.fill(Qt::transparent);
        QPainter p(&image);
        AgentWorking::paintTile(p, QRectF(0, 0, 17, 17), workspaceIcon("terminal", QColor("#c4cad1")), true, working, phase);
        return image;
    };
    AgentWorking::setAnimated(true);
    // The risen dot walks left, middle, right in one period.
    QCOMPARE(AgentWorking::rise(0, 0), 1.0); QCOMPARE(AgentWorking::rise(1, 1 / 3.0), 1.0); QCOMPARE(AgentWorking::rise(2, 2 / 3.0), 1.0);
    QCOMPARE(AgentWorking::rise(1, 0), 0.0);
    QVERIFY(frame(true, 0) != frame(true, 1 / 3.0));
    // Idle: no ring, no dots under the tile.
    const QImage idle = frame(false, 0), busy = frame(true, 0);
    bool dotsIdle = false, dotsBusy = false;
    for (int y = 18; y < idle.height(); ++y) for (int x = 0; x < 17; ++x) {
        dotsIdle = dotsIdle || idle.pixelColor(x, y).alpha() > 0;
        dotsBusy = dotsBusy || busy.pixelColor(x, y).alpha() > 0;
    }
    QVERIFY(!dotsIdle); QVERIFY(dotsBusy);
    // The ring: the accent mixed into the card border, on the tile's edge.
    const QColor edgeIdle = idle.pixelColor(8, 0), edgeBusy = busy.pixelColor(8, 0);
    QVERIFY(edgeBusy.green() > edgeIdle.green() + 40);
    // Animations off: the dots stand still at every phase.
    AgentWorking::setAnimated(false);
    QCOMPARE(frame(true, 0), frame(true, 1 / 3.0));
    AgentTile tile; tile.setAgent("claude"); tile.setWorking(true); tile.show(); QTest::qWait(20);
    QVERIFY(!tile.isAnimating());
    AgentWorking::setAnimated(true); tile.refreshAnimation();
    QVERIFY(tile.isAnimating());
    QCOMPARE(tile.size(), QSize(17, 17 + AgentWorking::Below));
    tile.hide(); QVERIFY(!tile.isAnimating());
    tile.show(); tile.setWorking(false); QVERIFY(!tile.isAnimating());
    Settings::remove("workspace/skin");
}

// U25 part 1: the shots of the pieces, dark and light (ux-1-*).
void TestTeamsUiWorkspace::foundationShots()
{
    Settings::set("workspace/skin", "whitewh");
    darkPalette();
    std::unique_ptr<QWidget> dark(foundationShowcase(true));
    dark->resize(460, 600); dark->show(); QTest::qWait(60);
    shot(dark.get(), "ux-1-foundation-dark");
    dark.reset();
    std::unique_ptr<QWidget> light(foundationShowcase(false));
    light->resize(460, 600); light->show(); QTest::qWait(60);
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) lightShot(light.get(), "ux-1-foundation-light");
    darkPalette();
    Settings::remove("workspace/skin");
}

// U25 (spec 3): the team's part of the lead session's header, in every state:
// the status, the waiting links, the three bars and the actions by state.
void TestTeamsUiWorkspace::leadHeaderFollowsTheTeamState()
{
    setTeam(team("planning"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    int opened = 0; workspace.openWorkspace = [&opened] { ++opened; };
    workspace.resize(1500, 820); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    const auto find = [&workspace](const char *name) { return workspace.findChild<QWidget *>(name); };
    const auto status = [&] { auto *badge = find("teamHeaderStatus"); return badge ? badge->property("status").toString() : QString(); };
    const auto shows = [&](const char *name) { auto *w = find(name); return w && w->isVisible(); };
    // Until a window takes it, the header shows above the canvas; no name bar.
    QTRY_COMPARE(status(), QStringLiteral("planning"));
    QVERIFY(find("teamWorkspaceHeader")->isVisible());
    QVERIFY(!find("teamWorkspaceBar"));
    // Planning: Discard plan…, Send back, Approve & Run held by the plan error.
    QVERIFY(shows("teamWorkspaceDiscard") && shows("teamWorkspaceSendBack") && shows("teamWorkspaceApprove"));
    // Approve & Run reads the plan errors from `team status`: until it answers
    // it says it is checking, then it names the errors (#382 review: no fixed wait).
    const auto approveTip = [&] { auto *b = workspace.findChild<QPushButton *>("teamWorkspaceApprove"); return b && !b->isEnabled() ? b->toolTip() : QString(); };
    QTRY_COMPARE(approveTip(), QStringLiteral("Fix the plan errors first"));
    // Send back opens its dialog from the workspace: a poll that rebuilds the
    // header under the dialog deletes the button, never code that still runs.
    {
        QPointer<QWidget> sendBack = find("teamWorkspaceSendBack");
        bool rebuiltUnder = false, sawDialog = false;
        QTimer watch; watch.setInterval(5);
        QObject::connect(&watch, &QTimer::timeout, [&] {
            auto *dialog = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!dialog || sawDialog) return;
            sawDialog = true;
            controller.refresh(); QTest::qWait(150);
            workspace.render();
            // Rebuilt: the old button is retired (unnamed, hidden) or gone already.
            rebuiltUnder = sendBack.isNull() || (sendBack->objectName().isEmpty() && !sendBack->isVisible());
            dialog->reject();
        });
        watch.start();
        static_cast<QPushButton *>(sendBack.data())->click();
        QTRY_VERIFY(sawDialog);
        QTRY_VERIFY(rebuiltUnder);
        watch.stop();
        QTRY_VERIFY(sendBack.isNull());
        QVERIFY(find("teamWorkspaceSendBack"));
    }
    QVERIFY(!find("teamWorkspacePause") && !find("teamWorkspaceStop") && !find("teamBarPendingPhase"));
    // Running: Pause and Stop…; the bars count the team's tasks and phases.
    setTeam(team());
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "running"; })());
    QVERIFY(shows("teamWorkspacePause") && shows("teamWorkspaceStop"));
    QVERIFY(!find("teamWorkspaceApprove") && !find("teamWorkspaceFinish") && !find("teamHeaderFinishWaits"));
    QCOMPARE(find("teamHeaderTasks")->property("done").toInt(), 1);
    QCOMPARE(find("teamHeaderTasks")->property("total").toInt(), 5);
    QCOMPARE(find("teamHeaderPhases")->property("total").toInt(), 1);
    QVERIFY(shows("teamHeaderElapsed"));
    // Stop… asks first: a cancel sends nothing, a confirm sends stop once.
    armConfirm(&panel, true);
    workspace.findChild<QPushButton *>("teamWorkspaceStop")->click();
    QTest::qWait(50); QCOMPARE(calls(script(), "stop").size(), 0);
    armConfirm(&panel, false);
    workspace.findChild<QPushButton *>("teamWorkspaceStop")->click();
    QTRY_COMPARE(calls(script(), "stop").size(), 1);
    // A draft phase waits: "Phase 3 waits for you" opens the Team tab on it, unfolded.
    setTeam(phasedTeam());
    QTRY_VERIFY(([&] { controller.refresh(); return find("teamBarPendingPhase") != nullptr; })());
    auto *waits = workspace.findChild<QPushButton *>("teamBarPendingPhase");
    QCOMPARE(waits->text(), QStringLiteral("Phase 3 waits for you"));
    QCOMPARE(find("teamHeaderPhase")->property("caption").toString(), QStringLiteral("Phase 2 · QA fixes"));
    QCOMPARE(find("teamHeaderPhases")->property("total").toInt(), 3);
    // N16 (#382): the bars count accepted tasks, as the phase's scroll does: a
    // cancelled task in phase 2 is decided, not done.
    {
        auto counted = phasedTeam(); counted["revision"] = 40;
        auto tasks = counted["tasks"].toArray();
        tasks.append(QJsonObject{{"id", 6}, {"phase", 2}, {"title", "Old fix"}, {"assignee", "backend"}, {"status", "cancelled"}});
        tasks.append(QJsonObject{{"id", 7}, {"phase", 2}, {"title", "Retry"}, {"assignee", "tester"}, {"status", "rejected"}});
        counted["tasks"] = tasks;
        auto phases = counted["phases"].toArray(); auto two = phases[0].toObject(); two["tasks"] = QJsonArray{3, 4, 6, 7}; phases[0] = two; counted["phases"] = phases;
        setTeam(counted);
        QTRY_VERIFY(([&] { controller.refresh(); auto *bar = find("teamHeaderTasks"); return bar && bar->property("total").toInt() == 7; })());
        QCOMPARE(find("teamHeaderPhase")->property("done").toInt(), 1);
        QCOMPARE(find("teamHeaderPhase")->property("total").toInt(), 4);
        QCOMPARE(find("teamHeaderTasks")->property("done").toInt(), 3);
        QVERIFY(find("teamHeaderTasks")->toolTip().startsWith(QStringLiteral("Accepted")));
        setTeam(phasedTeam());
        QTRY_VERIFY(([&] { controller.refresh(); auto *bar = find("teamHeaderTasks"); return bar && bar->property("total").toInt() == 5; })());
        waits = workspace.findChild<QPushButton *>("teamBarPendingPhase"); QVERIFY(waits);
    }
    waits->click();
    QCOMPARE(opened, 1);
    QTRY_VERIFY(([&] { const auto *t = controller.team("team-1"); if (!t) return false;
                       for (const auto &g : phaseGroups(*t)) if (g.n == 3) return panel.phaseOpen(*t, g); return false; })());
    // Every task decided: "Waiting for you to finish" and Finish….
    auto decided = team();
    auto tasks = decided["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto t = tasks[i].toObject(); t["status"] = "accepted"; tasks[i] = t; }
    decided["tasks"] = tasks;
    setTeam(decided);
    QTRY_VERIFY(([&] { controller.refresh(); return find("teamHeaderFinishWaits") != nullptr; })());
    QCOMPARE(workspace.findChild<QPushButton *>("teamHeaderFinishWaits")->text(), QStringLiteral("Waiting for you to finish"));
    QVERIFY(shows("teamWorkspaceFinish") && shows("teamWorkspacePause") && shows("teamWorkspaceStop"));
    // Paused: Resume and Stop…; Done: Archive and Next goal…; Stopped: Archive.
    auto paused = team(); paused["status"] = "paused"; setTeam(paused);
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "paused"; })());
    QVERIFY(shows("teamWorkspaceResume") && shows("teamWorkspaceStop") && !find("teamWorkspacePause"));
    auto done = decided; done["status"] = "done"; setTeam(done);
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "done"; })());
    QVERIFY(shows("teamWorkspaceArchive") && shows("teamWorkspaceNextGoal"));
    QVERIFY(!find("teamWorkspaceStop") && !find("teamWorkspaceFinish") && !find("teamHeaderFinishWaits") && !find("teamWorkspaceResumeLimits"));
    auto stopped = team(); stopped["status"] = "stopped"; setTeam(stopped);
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "stopped"; })());
    QVERIFY(shows("teamWorkspaceArchive") && !find("teamWorkspaceNextGoal") && !find("teamWorkspaceStop"));
    // N17 (#382): Archive asks first; Cancel sends nothing, Archive sends it once for this team.
    armConfirm(&panel, true);
    workspace.findChild<QPushButton *>("teamWorkspaceArchive")->click();
    QTest::qWait(80); QCOMPARE(calls(script(), "archive").size(), 0);
    armConfirm(&panel, false);
    workspace.findChild<QPushButton *>("teamWorkspaceArchive")->click();
    QTRY_COMPARE(calls(script(), "archive").size(), 1);
    QCOMPARE(calls(script(), "archive").first()["team"].toString(), QStringLiteral("team-1"));
    // N17: Approve & Run always asks, and names the member sessions it starts.
    {
        auto clean = team("planning"); clean["revision"] = 50;
        clean["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}};
        auto members = clean["members"].toArray(); auto backend = members[1].toObject(); backend.remove("session"); members[1] = backend;
        auto tester = members[2].toObject(); tester.remove("session"); members[2] = tester; clean["members"] = members;
        setTeam(clean);
        QPushButton *approve = nullptr;
        QTRY_VERIFY(([&] { controller.refresh(); approve = workspace.findChild<QPushButton *>("teamWorkspaceApprove"); return approve && approve->isEnabled(); })());
        QString asked, dialogName, dialogTitle, confirmAction;
        bool cancelIsDefault = false;
        QTimer watch; watch.setInterval(10);
        QObject::connect(&watch, &QTimer::timeout, &panel, [&] {
            auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
            if (!box || !asked.isEmpty()) return;
            asked = box->text(); dialogName = box->objectName(); dialogTitle = box->windowTitle();
            cancelIsDefault = box->defaultButton() == box->button(QMessageBox::Cancel);
            for (auto *button : box->buttons())
                if (box->buttonRole(button) == QMessageBox::DestructiveRole) confirmAction = button->text();
            box->button(QMessageBox::Cancel)->click();
        });
        watch.start();
        approve->click();
        QTRY_VERIFY(!asked.isEmpty());
        watch.stop();
        QTest::qWait(50); QCOMPARE(calls(script(), "approve").size(), 0);
        QCOMPARE(dialogName, QStringLiteral("teamApproveConfirm"));
        // Native macOS message boxes omit the window title. The body and the
        // destructive action carry the approval there as on other platforms.
#ifndef Q_OS_MACOS
        QCOMPARE(dialogTitle, QStringLiteral("Approve plan and run?"));
#endif
        QVERIFY2(asked.startsWith(QStringLiteral("Approve the plan of Checkout redesign and run it?")), qPrintable(asked));
        QCOMPARE(confirmAction, QStringLiteral("Approve and run")); QVERIFY(cancelIsDefault);
        QVERIFY2(asked.contains(QStringLiteral("It starts 2 member sessions: Backend (codex), Tester (kimi on msi-old).")), qPrintable(asked));
        armConfirm(&panel, false);
        workspace.findChild<QPushButton *>("teamWorkspaceApprove")->click();
        QTRY_COMPARE(calls(script(), "approve").size(), 1);
        QVERIFY(!calls(script(), "approve").first().contains("confirm_widening"));
        setTeam(stopped);
        QTRY_VERIFY(([&] { controller.refresh(); return status() == "stopped"; })());
    }
    // Not the lead's session: nothing; an archived team: nothing.
    workspace.setHeaderShown(false);
    QVERIFY(!find("teamHeaderStatus") && !workspace.headerStatus()->isVisible() && !find("teamWorkspaceHeader")->isVisible());
    workspace.setHeaderShown(true);
    QVERIFY(find("teamHeaderStatus"));
    auto archived = team(); archived["status"] = "archived"; setTeam(archived);
    QTRY_VERIFY(([&] { controller.refresh(); return !find("teamHeaderStatus"); })());
    // A window takes the two parts into its own header.
    workspace.takeHeader();
    QVERIFY(!workspace.headerStatus()->parentWidget() && !workspace.headerActions()->parentWidget());
    QVERIFY(!find("teamWorkspaceHeader")->isVisible());
}

// U25: the lead header in the prototype's scenarios (shots 03, 24, 25), dark and light.
void TestTeamsUiWorkspace::leadHeaderShots()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) QSKIP("Set ZERUS_TEAM_SHOTS=<dir> to keep the pictures.");
    Settings::set("workspace/skin", "whitewh");
    auto phased = phasedTeam(); phased["created"] = QDateTime::currentSecsSinceEpoch() - 4140;
    auto decided = team(); decided["created"] = QDateTime::currentSecsSinceEpoch() - 8940;
    auto tasks = decided["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto t = tasks[i].toObject(); t["status"] = "accepted"; tasks[i] = t; }
    decided["tasks"] = tasks;
    auto planning = team("planning"); planning["created"] = QDateTime::currentSecsSinceEpoch() - 720;
    const QList<QPair<QString, QJsonObject>> scenarios{{"running", phased}, {"planning", planning}, {"done", decided}};
    for (const auto &[name, json] : scenarios) {
        for (const bool dark : {true, false}) {
            setTeam(json);
            HgsClient client(FakeCli::program(script()));
            TeamController controller(&client);
            TeamPanel panel(&controller);
            TeamWorkspace workspace(&controller, &panel);
            workspace.setLive(live);
            workspace.takeHeader();
            if (dark) darkPalette(); else lightPalette();
            workspace.setTheme(dark);
            std::unique_ptr<QWidget> header(leadHeaderMock(workspace, dark));
            header->resize(1180, 76); header->show();
            controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
            QTRY_VERIFY(workspace.headerStatus()->findChild<QWidget *>("teamHeaderStatus"));
            QTest::qWait(100);
            const QString file = QStringLiteral("ux-3-header-%1-%2").arg(name, dark ? "dark" : "light");
            if (dark) shot(header.get(), file); else lightShot(header.get(), file);
            header.reset();
        }
    }
    darkPalette();
    Settings::remove("workspace/skin");
}

// U25 (spec 5.1, 5.3, 5.5): every phase a collapsible scroll in Graph, Board
// and Layers, with its number, status badge, accepted/tasks bar, proposed by or
// health, the lead's note and the owner's actions in its header.
// Owner, 2026-10-28: on a small screen the workspace toolbar keeps its icons
// and drops their words (Graph / Board / Layers, All / Active / Problems,
// Auto-accept drafts, Expand all); the words come back when it fits again
// and stay in the tooltips and accessible names.
void TestTeamsUiWorkspace::aNarrowToolbarKeepsOnlyItsIcons()
{
    Settings::set("workspace/skin", "whitewh");
    setTeam(prototypeTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    darkPalette(); workspace.setTheme(true);
    // #719: Tests is a fifth view; its words need 100 px more.
    workspace.resize(1540, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    const auto segment = [&](const char *group, const char *value) -> QPushButton * {
        for (auto *b : workspace.findChild<QWidget *>(group)->findChildren<QPushButton *>()) if (b->property("segment") == value) return b;
        return nullptr;
    };
    // #755: the views are icons alone at every width; each names itself and
    // what it shows in its tooltip and accessible name.
    QTRY_VERIFY(segment("teamWorkspaceView", "graph") && segment("teamWorkspaceFilter", "problems")->text() == QStringLiteral("Problems"));
    for (const auto &[value, name] : QList<std::pair<const char *, QString>>{{"graph", "Graph"}, {"board", "Board"}, {"layers", "Layers"}, {"profile", "Profile"}, {"tests", "Tests"}}) {
        auto *view = segment("teamWorkspaceView", value);
        QVERIFY2(view && view->text().isEmpty() && !view->icon().isNull(), value);
        QVERIFY2(view->toolTip().startsWith(name + QLatin1Char('\n')) && view->toolTip().size() > name.size() + 10, qPrintable(view->toolTip()));
        QCOMPARE(view->accessibleName(), name);
        QVERIFY(!view->accessibleDescription().isEmpty());
        QCOMPARE(view->focusPolicy() & Qt::TabFocus, Qt::TabFocus);
    }
    QVERIFY(segment("teamWorkspaceView", "graph")->isChecked());
    QVERIFY(segment("teamWorkspaceFilter", "problems")->icon().isNull());
    // U38: with the switch labels gone, Heat is one flame button whose menu
    // holds the same three choices; a choice is kept for the team.
    auto *heatMenu = workspace.findChild<QPushButton *>("teamWorkspaceHeatMenu");
    QVERIFY(heatMenu && heatMenu->isVisible() && !heatMenu->icon().isNull());
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceHeatWide")->isVisible());
    QCOMPARE(heatMenu->toolTip(), QStringLiteral("Heat: Off"));
    QStringList choices; for (auto *action : heatMenu->menu()->actions()) choices << action->text();
    QCOMPARE(choices, (QStringList{"Off", "Time lost", "Tokens"}));
    Settings::remove("teams/heat/team-1");
    heatMenu->menu()->actions().at(1)->trigger();
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("time"));
    QTRY_COMPARE(workspace.findChild<QPushButton *>("teamWorkspaceHeatMenu")->toolTip(), QStringLiteral("Heat: Time lost"));
    workspace.findChild<QPushButton *>("teamWorkspaceHeatMenu")->menu()->actions().at(0)->trigger();
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("off"));
    QTRY_COMPARE(segment("teamWorkspaceFilter", "problems")->text(), QStringLiteral("Problems"));
    auto *split = workspace.findChild<QSplitter *>("teamWorkspaceSplit");
    QVERIFY(split);
    split->setSizes({430, 1110});
    QTRY_VERIFY(segment("teamWorkspaceFilter", "problems")->text().isEmpty());
    QVERIFY(segment("teamWorkspaceView", "graph")->text().isEmpty() && !segment("teamWorkspaceView", "graph")->icon().isNull());
    QVERIFY(segment("teamWorkspaceView", "graph")->toolTip().startsWith(QStringLiteral("Graph\n")));
    QCOMPARE(segment("teamWorkspaceView", "graph")->accessibleName(), QStringLiteral("Graph"));
    QVERIFY(!segment("teamWorkspaceFilter", "problems")->icon().isNull());
    if (auto *expand = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")) QVERIFY(expand->text().isEmpty());
    // U38: the icon-only step leaves the Heat switch out.
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceHeatBox")->isVisible());
    // A render rebuilds the toolbar: look it up each time.
    const auto bar = [&] { return workspace.findChild<QWidget *>("teamWorkspaceToolbar"); };
    QTest::qWait(200); shot(bar(), "team-toolbar-narrow-dark");
    QTRY_VERIFY2(bar()->sizeHint().width() <= bar()->width() + 1, qPrintable(QStringLiteral("%1 px for a %2 px row").arg(bar()->sizeHint().width()).arg(bar()->width())));
    split->setSizes({1100, 340});
    QTRY_COMPARE(segment("teamWorkspaceFilter", "problems")->text(), QStringLiteral("Problems"));
    QVERIFY(segment("teamWorkspaceView", "graph")->text().isEmpty());
    shot(bar(), "team-toolbar-wide-dark");
    lightPalette(); workspace.setTheme(false); QTest::qWait(200); shot(bar(), "team-toolbar-wide-light");
    darkPalette(); workspace.setTheme(true);
    Settings::remove("workspace/skin");
}

// whitewh-115: in the shortened mode the toolbar jumped between its fit steps
// at the boundaries. Each step is measured with its own controls now, and the
// choice only widens with slack; at a boundary it must settle on one step and
// keep it across refits and polls.
void TestTeamsUiWorkspace::theNarrowToolbarSettlesOnOneStep()
{
    Settings::set("workspace/skin", "whitewh");
    setTeam(prototypeTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1440, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    auto *split = workspace.findChild<QSplitter *>("teamWorkspaceSplit");
    QVERIFY(split);
    const auto bar = [&] { return workspace.findChild<QWidget *>("teamWorkspaceToolbar"); };
    const auto segment = [&](const char *group, const char *value) -> QPushButton * {
        auto *found = workspace.findChild<QWidget *>(group);
        if (!found) return nullptr;
        for (auto *b : found->findChildren<QPushButton *>()) if (b->property("segment") == value) return b;
        return nullptr;
    };
    // What the row settled on: the wide Heat switch, its one-button menu, the
    // words on the view and filter segments, and Hide finished tell the four
    // steps apart.
    const auto signature = [&]() -> QString {
        QString s;
        auto *graph = segment("teamWorkspaceView", "graph");
        auto *problems = segment("teamWorkspaceFilter", "problems");
        const auto shown = [&](const char *name) { auto *w = workspace.findChild<QWidget *>(name); return w && w->isVisible(); };
        s += graph && !graph->text().isEmpty() ? QLatin1Char('v') : QLatin1Char('V');
        s += problems && !problems->text().isEmpty() ? QLatin1Char('f') : QLatin1Char('F');
        s += shown("teamWorkspaceHeatWide") ? QLatin1Char('w') : QLatin1Char('-');
        s += shown("teamWorkspaceHeatBox") ? QLatin1Char('b') : QLatin1Char('-');
        s += shown("teamWorkspaceHeatMenu") ? QLatin1Char('m') : QLatin1Char('-');
        s += shown("teamWorkspaceHideFinished") ? QLatin1Char('h') : QLatin1Char('-');
        return s;
    };
    const auto settle = [&](int width) {
        split->setSizes({width, 1440 - width});
        QCoreApplication::processEvents();
        return signature();
    };
    // One refit at the same width: a hide/show (the row refits on show) and a
    // one-pixel nudge there and back (two resize refits).
    const auto refit = [&](int width) {
        auto *row = bar();
        row->hide(); row->show();
        split->setSizes({width - 1, 1440 - width + 1});
        QCoreApplication::processEvents();
        split->setSizes({width, 1440 - width});
        QCoreApplication::processEvents();
        return signature();
    };
    // The steps across a range of widths; a change between neighbours is a
    // boundary.
    QMap<int, QString> steps;
    for (int width = 400; width <= 1040; width += 4) steps.insert(width, settle(width));
    QList<int> boundaries;
    for (int width = 404; width <= 1040; width += 4)
        if (steps.value(width) != steps.value(width - 4)) boundaries << width;
    QVERIFY2(boundaries.size() >= 2, "the scan found the toolbar's steps");
    // Repeated refits at both sides of every boundary keep exactly the step the
    // width decided on; hiding a control never changes the step it started from.
    for (int boundary : boundaries) {
        for (int width : {boundary - 1, boundary, boundary + 1}) {
            const QString settled = settle(width);
            for (int pass = 0; pass < 5; ++pass) {
                QCOMPARE(refit(width), settled);
                QCOMPARE(settle(width), settled);
            }
        }
    }
    // A poll rebuilds the row; at the same width it settles on the same step.
    for (int boundary : boundaries) {
        const QString settled = settle(boundary);
        controller.refresh();
        QTRY_VERIFY(bar() && bar()->isVisible());
        QTest::qWait(50);
        QCOMPARE(signature(), settled);
    }
    // Shots: the icon-only toolbar in both themes (the existing narrow test's
    // 430 px height of the split).
    const int narrow = 430;
    settle(narrow);
    QTest::qWait(100);
    shot(bar(), "team-toolbar-settled-dark");
    lightPalette(); workspace.setTheme(false);
    settle(narrow);
    QTest::qWait(100);
    lightShot(bar(), "team-toolbar-settled-light");
    Settings::remove("workspace/skin");
}

// U36: the Board's In review and In test columns sit between Submitted and
// Accepted at U33's widths with the shared sideways scroll; each card's stage
// chip says its state (in review, in test, waiting for a stage member, returned,
// out of rounds, a held folder, detached, shared folder, skipped, accepted,
// auto) with the stage member's agent and the track; members show their stage
// and stage queue. Shots of the wide and the narrow Board in both themes.
void TestTeamsUiWorkspace::stageColumnsAndChipsOnTheBoard()
{
    Settings::set("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        Settings::remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    setTeam(pipelineTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(2000, 960); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 4));
    // The columns, in order, for phase 2: In review and In test between
    // Submitted and Accepted.
    QStringList order;
    QList<QFrame *> phase2;
    for (auto *col : area()->findChildren<QFrame *>("teamBoardColumn")) if (col->property("phase").toInt() == 2) phase2 << col;
    std::sort(phase2.begin(), phase2.end(), [&](QFrame *a, QFrame *b) { return a->mapTo(area()->widget(), QPoint()).x() < b->mapTo(area()->widget(), QPoint()).x(); });
    for (auto *col : phase2) order << col->property("column").toString();
    QCOMPARE(order, (QStringList{"pending", "active", "problems", "submitted", "in_review", "in_test", "accepted"}));
    for (auto *col : phase2) QVERIFY2(col->width() >= 200, qPrintable(col->property("column").toString()));   // U33: BoardColumnWidth
    QCOMPARE(columnOf(area(), 3), QString("in_review")); QCOMPARE(columnOf(area(), 11), QString("in_review")); QCOMPARE(columnOf(area(), 10), QString("in_review"));
    QCOMPARE(columnOf(area(), 4), QString("in_test")); QCOMPARE(columnOf(area(), 17), QString("in_test")); QCOMPARE(columnOf(area(), 18), QString("in_test"));
    QCOMPARE(columnOf(area(), 5), QString("submitted")); QCOMPARE(columnOf(area(), 2), QString("active")); QCOMPARE(columnOf(area(), 9), QString("pending"));
    // In review says one stage waits for a member (#10).
    QStringList waiting;
    for (auto *note : area()->findChildren<QLabel *>("teamBoardColumnWaiting")) waiting << note->text();
    QCOMPARE(waiting, (QStringList{QStringLiteral("· 1 waiting")}));
    // Every state's chip. The board builds a large view in bounded slices, so
    // a card's line is waited for, not assumed to exist at the first turn.
    const auto chips = [&] { return boardChips(area()); };
    QTRY_COMPARE(chips().value(3), QStringList{"Review r1/3"});
    QTRY_COMPARE(chips().value(11), QStringList{"Review r2/3"});
    QTRY_COMPARE(chips().value(10), QStringList{QStringLiteral("Review waits · 1st in queue")});
    QTRY_COMPARE(chips().value(4), QStringList{"Test r2/3"});
    QTRY_COMPARE(chips().value(17), QStringList{QStringLiteral("Test r1/3 · 4 files")});
    QTRY_COMPARE(chips().value(18), QStringList{QStringLiteral("Test r1/3 · detached")});
    QTRY_COMPARE(chips().value(9), QStringList{"Folder held by the test stage of #4"});
    QTRY_COMPARE(chips().value(2), QStringList{"Review returned r1"});
    QTRY_COMPARE(chips().value(5), QStringList{QStringLiteral("Lead decides · 7 findings")});
    QTRY_COMPARE(chips().value(16), (QStringList{"Review skipped", "Lead"}));
    QTRY_COMPARE(chips().value(12), (QStringList{QStringLiteral("Review ✓"), QStringLiteral("Test ✓")}));
    QTRY_COMPARE(chips().value(13), (QStringList{QStringLiteral("Review ✓"), "Auto"}));
    QVERIFY(!chips().contains(8) && !chips().contains(6));   // not reported yet: no line
    // The stage member's agent beside a running stage; the skipped chip is struck.
    QVERIFY(boardCard(area(), 3)->findChild<QLabel *>("teamStageAgent"));
    QVERIFY(!boardCard(area(), 10)->findChild<QLabel *>("teamStageAgent"));
    for (auto *chip : boardCard(area(), 16)->findChildren<QWidget *>("teamStageChip"))
        QCOMPARE(chip->font().strikeOut(), chip->property("state") == "skipped");
    QCOMPARE(boardCard(area(), 4)->findChild<QWidget *>("teamStageTrack")->property("segments").toInt(), 4);
    // The returned task names its new attempt and findings; the round limit
    // its rounds; both warn, as does the held folder's border.
    QCOMPARE(boardCard(area(), 2)->findChild<QLabel *>("teamBoardActivity")->text(), QStringLiteral("Attempt 2 · 3 findings"));
    QCOMPARE(boardCard(area(), 5)->findChild<QLabel *>("teamBoardActivity")->text(), QStringLiteral("3 test rounds used"));
    const auto warnEdge = [&](int task) { return boardCard(area(), task)->styleSheet().contains(TeamUi::colorsFor(true).warn.name()); };
    QVERIFY(warnEdge(5) && warnEdge(9) && !warnEdge(3) && !warnEdge(4));
    // The test's tooltip names its member, where it works and what is next.
    QString tip;
    for (auto *chip : boardCard(area(), 4)->findChildren<QWidget *>("teamStageChip")) tip = chip->toolTip();
    QVERIFY2(tip.contains("Test round 2 of 3") && tip.contains("Tester · kimi-k2.5") && tip.contains("/work/orbit-team/frontend")
             && tip.contains("Holds Frontend's folder") && tip.contains("Next: Lead accepts"), qPrintable(tip));
    // Members: the stage each has and its queue; the implementer's folder waits.
    const auto memberCard = [&](const QString &id) -> QWidget * {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember")) if (card->property("teamMember") == id) return card;
        return nullptr;
    };
    const auto queueTitle = [&](const QString &id) {
        auto *queue = memberCard(id) ? memberCard(id)->findChild<QFrame *>("teamMemberStageQueue") : nullptr;
        return queue ? queue->findChild<QLabel *>("teamMemberStageQueueTitle")->text() : QString();
    };
    QCOMPARE(queueTitle("reviewer"), QString("Stage queue 1"));
    QCOMPARE(queueTitle("reviewer2"), QString("Stage queue 1"));
    QCOMPARE(queueTitle("tester"), QStringLiteral("Stage queue empty · 12 min in this stage"));
    QVERIFY(queueTitle("frontend").isEmpty() && queueTitle("backend").isEmpty());
    auto *row = memberCard("reviewer")->findChild<QPushButton *>("teamMemberStageQueueRow");
    QVERIFY(row && row->property("task").toInt() == 10);
    QCOMPARE(memberStatus(*controller.team("team-1"), *controller.team("team-1")->member("reviewer"), Live{}).text, QStringLiteral("Reviewing #3 Card form UI · round 1"));
    QCOMPARE(memberStatus(*controller.team("team-1"), *controller.team("team-1")->member("tester"), Live{}).text, QStringLiteral("Testing #4 Address validation · round 2"));
    QCOMPARE(memberStatus(*controller.team("team-1"), *controller.team("team-1")->member("frontend"), Live{}).text, QStringLiteral("Waits: its folder is held by the test stage of #4"));
    // A queued stage opens its task.
    row->click();
    QTRY_VERIFY(static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->isVisible());
    QCOMPARE(static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->task(), 10);
    static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->back();
    QTRY_VERIFY(!static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->isVisible());
    if (auto *all = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll"); all && all->text() == QStringLiteral("Expand all")) all->click();
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u36-boardwide-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "u36-boardwide-light");
        // Narrow: the whole Board scrolls sideways to the new columns.
        darkPalette(); workspace.setTheme(true);
    }
    workspace.resize(1180, 960);
    QTRY_VERIFY(area() && area()->horizontalScrollBar()->isVisible() && area()->horizontalScrollBar()->maximum() > 0);
    area()->horizontalScrollBar()->setValue(area()->horizontalScrollBar()->maximum());
    QTRY_VERIFY(area()->horizontalScrollBar()->value() > 0);
    QVERIFY(boardCard(area(), 4)->visibleRegion().boundingRect().width() > 100);
    shot(&workspace, "u36-board-dark");
    workspace.setTheme(false);
    QTRY_VERIFY(area() && area()->horizontalScrollBar()->maximum() > 0);
    area()->horizontalScrollBar()->setValue(area()->horizontalScrollBar()->maximum());
    lightShot(&workspace, "u36-board-light");
    // The Layers rows carry the same chips.
    darkPalette(); workspace.setTheme(true);
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "layers") b->click();
    QTRY_VERIFY(workspace.findChild<QScrollArea *>("teamWorkspaceLayers"));
    QTRY_VERIFY(!boardChips(workspace.findChild<QScrollArea *>("teamWorkspaceLayers")).isEmpty());
    QTRY_COMPARE(boardChips(workspace.findChild<QScrollArea *>("teamWorkspaceLayers")).value(4), QStringList{"Test r2/3"});
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u36-layers-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "u36-layers-light");
    }
}

// #635 N4: the status's held_by names the stage that holds a member's folder;
// a review holds the implementer's folder here (the server's rule since #683),
// which the old test-only rule could not see. The held card's chip, the
// member's status and the phase note say "the review stage of #3". Shots in
// both themes.
void TestTeamsUiWorkspace::heldByFromTheStatusOnTheBoard()
{
    Settings::set("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        Settings::remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    auto json = pipelineTeam();
    auto members = json["members"].toArray();
    for (int i = 0; i < members.size(); ++i) {
        auto m = members[i].toObject();
        if (m["id"] == "frontend") { m["held_by"] = QJsonObject{{"task", 3}, {"stage", "review"}}; members[i] = m; }
    }
    json["members"] = members;
    auto tasks = json["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto t = tasks[i].toObject();
        if (t["id"].toInt() == 9) { t["waits"] = QJsonObject{{"for", "folder"}, {"on", QJsonArray{"#3"}}, {"unblock", "folder held by the stage of #3"}}; tasks[i] = t; }
    }
    json["tasks"] = tasks;
    setTeam(json);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(2000, 960); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 9));
    const auto *team = controller.team("team-1");
    QVERIFY(team);
    QCOMPARE(team->member("frontend")->heldBy.task, 3);
    // #4's test no longer counts: the status names the review as the hold.
    QTRY_COMPARE(boardChips(area()).value(9), QStringList{"Folder held by the review stage of #3"});
    QCOMPARE(memberStatus(*team, *team->member("frontend"), Live{}).text, QStringLiteral("Waits: its folder is held by the review stage of #3"));
    QString tip;
    QTRY_VERIFY(boardCard(area(), 3));
    for (auto *chip : boardCard(area(), 3)->findChildren<QWidget *>("teamStageChip")) tip = chip->toolTip();
    QVERIFY2(tip.contains("Holds Frontend's folder"), qPrintable(tip));
    tip.clear();
    QTRY_VERIFY(boardCard(area(), 4));
    for (auto *chip : boardCard(area(), 4)->findChildren<QWidget *>("teamStageChip")) tip = chip->toolTip();
    QVERIFY2(!tip.contains("Holds Frontend's folder"), qPrintable(tip));
    auto *held = workspace.findChild<QLabel *>("teamPhaseHeld");
    QVERIFY(held);
    QCOMPARE(held->text(), QStringLiteral("1 folder held"));
    QVERIFY2(held->toolTip().contains("Frontend's folder: held by the review stage of #3"), qPrintable(held->toolTip()));
    if (auto *all = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll"); all && all->text() == QStringLiteral("Expand all")) all->click();
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u687-held-by-review-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "u687-held-by-review-light");
    }
}

// #693: the Board stops shaking. A poll that changes only what the views do not
// draw (the revision, the updated time, a running task's growing seconds and
// waits, the minutes of a health warning) or the session list naming the same
// team again rebuilds nothing; the running time, the header's elapsed time and
// a stage's minutes tick in place; a real change rebuilds and keeps the
// scroll, the open phases and the open task.
void TestTeamsUiWorkspace::aPollThatChangesNothingDrawnRebuildsNothing()
{
    Settings::set("workspace/skin", "whitewh");
    double now = stageNow();
    TeamUi::workedClock() = [&now] { return now; };
    const auto restore = qScopeGuard([] {
        Settings::remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    const auto tick = [] { QVERIFY(QMetaObject::invokeMethod(&TeamUi::workedTicker(), "timeout")); };
    // #2 runs since 1800003000; a health warning names idle minutes.
    const auto poll = [&](int revision, double updated, int worked, int deps, int idle, const QString &secondStatus = QStringLiteral("in_progress")) {
        auto json = pipelineTeam();
        json["revision"] = revision; json["updated"] = updated;
        json["health"] = QJsonObject{{"starved", QJsonArray{QJsonObject{{"member", "secretary"}, {"minutes", idle}}}},
                                     {"warnings", QJsonArray{QStringLiteral("starved: Secretary idle %1 min without a ready task").arg(idle)}}};
        // #709 L4: the running phase's health names the same minutes.
        json["phase_summary"] = QJsonArray{
            QJsonObject{{"n", 1}, {"title", "Analysis"}, {"status", "done"}, {"done", 1}, {"total", 1}, {"health_line", ""}},
            QJsonObject{{"n", 2}, {"title", "Build"}, {"status", "in_work"}, {"done", 0}, {"total", 14}, {"health_line", "path 4 · par 2.0 · 1 warning"}}};
        json["phases_health"] = QJsonObject{{"2", QJsonObject{{"tasks", 14}, {"critical_path", 4}, {"parallelism", 2.0},
                                                             {"warnings", QJsonArray{QStringLiteral("starved: Secretary idle %1 min without a ready task").arg(idle)}}}}};
        auto tasks = json["tasks"].toArray();
        for (int i = 0; i < tasks.size(); ++i) {
            auto t = tasks[i].toObject();
            if (t["id"].toInt() == 2) {
                t["status"] = secondStatus;
                t["timing"] = QJsonObject{{"worked_s", worked}, {"running_since", 1800003000.0},
                                          {"waits", QJsonObject{{"deps_s", deps}, {"member_s", 0}, {"lead_s", 0}}},
                                          {"attempts", QJsonArray{QJsonObject{{"n", 2}, {"member", "backend"}, {"state", "delivered"}, {"from", 1800003000.0},
                                                                              {"to", QJsonValue()}, {"worked_s", worked}}}}};
            }
            tasks[i] = t;
        }
        json["tasks"] = tasks;
        return json;
    };
    setTeam(poll(40, 1800003900, 1000, 300, 44));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1700, 640); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 4) && controller.watched() && controller.watched()->revision == 40);
    TestWait::settle();
    const auto statusReads = [&] { return calls(script(), "status").size(); };
    QPointer<QScrollArea> canvas = area();
    QPointer<QFrame> card = boardCard(area(), 4);
    QPointer<QWidget> members = workspace.findChild<QWidget *>("teamWorkspaceSide");
    // Polls that move only the revision, the updated time, #2's seconds and
    // waits and the health minutes: the same widgets stay.
    for (int i = 1; i <= 3; ++i) {
        setTeam(poll(40 + i, 1800003900 + 60 * i, 1000 + 60 * i, 300 + 60 * i, 44 + i));
        const int before = statusReads();
        controller.refresh();
        QTRY_VERIFY(statusReads() > before && controller.watched()->revision == 40 + i);
        QTest::qWait(150);
        QVERIFY2(canvas && canvas == area(), qPrintable(QStringLiteral("poll %1 rebuilt the Board").arg(i)));
        QVERIFY(card && card == boardCard(area(), 4));
        QVERIFY(members && members == workspace.findChild<QWidget *>("teamWorkspaceSide"));
    }
    // The session list names the same team after its own polls: nothing.
    workspace.showTeam("team-1");
    QTest::qWait(100);
    QVERIFY(canvas && canvas == area());
    // Running times and ages tick in place: #2's time, the header's elapsed
    // time and the tester's minutes in its stage.
    auto *worked = boardCard(area(), 2)->findChild<QLabel *>("teamBoardWorked");
    QVERIFY(worked);
    QCOMPARE(worked->text(), QStringLiteral("16m"));
    auto *elapsed = workspace.findChild<QLabel *>("teamHeaderElapsedText");
    QVERIFY(elapsed);
    const QString elapsedBefore = elapsed->text();
    QLabel *stageTitle = nullptr;
    for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
        if (card->property("teamMember") == "tester") stageTitle = card->findChild<QLabel *>("teamMemberStageQueueTitle");
    QVERIFY(stageTitle);
    QCOMPARE(stageTitle->text(), QStringLiteral("Stage queue empty · 12 min in this stage"));
    // #709 L4: the phase health's idle minutes as drawn at 44, three polls on.
    QLabel *health = nullptr;
    for (auto *l : workspace.findChildren<QLabel *>("teamPhaseHealth")) if (l->toolTip().contains("idle")) health = l;
    QVERIFY(health);
    const QString healthText = health->text();
    QVERIFY2(health->toolTip().contains("starved: Secretary idle 44 min without a ready task"), qPrintable(health->toolTip()));
    now += 600;
    tick();
    QCOMPARE(worked->text(), QStringLiteral("26m"));
    QCOMPARE(stageTitle->text(), QStringLiteral("Stage queue empty · 22 min in this stage"));
    QVERIFY2(health->toolTip().contains("starved: Secretary idle 54 min without a ready task"), qPrintable(health->toolTip()));
    QCOMPARE(health->text(), healthText);
    // The fixture's team is weeks old: a day later its elapsed time moves.
    now += 86400;
    tick();
    QVERIFY2(elapsed->text() != elapsedBefore, qPrintable(elapsed->text()));
    now -= 86400;
    tick();
    QVERIFY(canvas && canvas == area() && card == boardCard(area(), 4));
    // A real change rebuilds, and keeps where the owner was: the Board's
    // scroll, the finished phase 1 the owner opened and the open task.
    if (!phaseOpen(&workspace, 1)) togglePhase(&workspace, 1);
    QTRY_VERIFY(phaseOpen(&workspace, 1));
    workspace.resize(1700, 420);
    QTRY_VERIFY(area() && area()->verticalScrollBar()->maximum() > 40);
    area()->verticalScrollBar()->setValue(40);
    QTest::mouseClick(boardCard(area(), 4), Qt::LeftButton, {}, QPoint(boardCard(area(), 4)->width() / 2, 8));
    auto *drawer = static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"));
    QTRY_VERIFY(drawer->isVisible() && drawer->task() == 4);
    TestWait::settle();
    canvas = area();
    setTeam(poll(50, 1800004500, 1000, 300, 50, QStringLiteral("submitted")));
    controller.refresh();
    QTRY_VERIFY(controller.watched()->revision == 50);
    QTRY_VERIFY(!canvas || canvas != area());   // rebuilt
    QTRY_COMPARE(columnOf(area(), 2), QString("submitted"));
    QTRY_COMPARE(area()->verticalScrollBar()->value(), 40);
    QVERIFY(phaseOpen(&workspace, 1));
    QVERIFY(drawer->isVisible() && drawer->task() == 4);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u693-board-after-change-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "u693-board-after-change-light");
    }
}

// #693 (the lead's note): a phase graph's sideways bar is decided after its
// layout, never in a rebuild's first pass, and not flipped by the other bar:
// a graph that fits shows none across rebuilds and resizes; one wider than
// its column keeps it.
void TestTeamsUiWorkspace::aPhaseGraphThatFitsShowsNoSidewaysBar()
{
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] { TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); }; });
    setTeam(pipelineTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(2000, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    // Phase 1 is done and folded: the open phase 2's graph.
    const auto graphArea = [&]() -> QScrollArea * {
        for (auto *area : workspace.findChildren<QScrollArea *>("teamPhaseGraphArea")) if (area->isVisible()) return area;
        return nullptr;
    };
    QTRY_VERIFY(graphArea());
    TestWait::settle();
    const auto fits = [&] { auto *a = graphArea(); return a->widget()->minimumWidth() <= a->width(); };
    QVERIFY2(fits(), "the phase graph fits a 2000 px window");
    // Rebuilds (a real change each time) and resizes: the bar never shows,
    // not even before the new graph's layout.
    for (int i = 0; i < 4; ++i) {
        workspace.render();
        QCOMPARE(graphArea()->horizontalScrollBarPolicy(), Qt::ScrollBarAlwaysOff);
        TestWait::settle();
        QVERIFY(!graphArea()->horizontalScrollBar()->isVisible());
        workspace.resize(2000 - 20 * i, 900 - 10 * i);
        TestWait::settle();
        QVERIFY(fits());
        QVERIFY2(!graphArea()->horizontalScrollBar()->isVisible(), qPrintable(QStringLiteral("resize %1").arg(i)));
    }
    // Narrow, at full zoom: wider than its column, the bar stays.
    workspace.resize(760, 900);
    for (int i = 0; i < 4; ++i) {
        QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceZoomIn"));
        workspace.findChild<QPushButton *>("teamWorkspaceZoomIn")->click();
        TestWait::settle();
    }
    QTRY_VERIFY(!fits());
    QTRY_VERIFY(graphArea()->horizontalScrollBar()->isVisible());
    for (int i = 0; i < 3; ++i) {
        workspace.render();
        TestWait::settle();
        QVERIFY(graphArea()->horizontalScrollBar()->isVisible());
    }
}

// #706: a member going from working to idle changes its activity label on
// the Board and its node on the Graph in place, and its mark in Members is
// set again where it is: the canvas, the cards and the column stay. A live change that makes a problem
// (the member goes offline) rebuilds as before.
void TestTeamsUiWorkspace::aMembersLiveStateChangesInPlace()
{
    setTeam(liveTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    bool working = true, offline = false;
    workspace.setLive([&](const Member &m) {
        Live l; l.known = true;
        if (m.id == "backend") { l.working = working; l.offline = offline; l.attentionText = working ? "cargo test" : "cargo build"; }
        return l;
    });
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1500, 640); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == segment) b->click();
    };
    view("board");
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 1) && controller.watched());
    TestWait::settle();
    const auto activity = [&] { auto *l = boardCard(area(), 1)->findChild<QLabel *>("teamBoardActivity"); return l ? l->text() : QString(); };
    const auto mark = [&]() -> TeamUi::StatusMark * {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (card->property("teamMember") == "backend") return dynamic_cast<TeamUi::StatusMark *>(card->findChild<QWidget *>("teamWorkspaceMemberState"));
        return nullptr;
    };
    const auto side = [&] { return workspace.findChild<QWidget *>("teamWorkspaceSide"); };
    QCOMPARE(activity(), QStringLiteral("working"));
    QVERIFY(mark());
    QPointer<QScrollArea> canvas = area();
    QPointer<QFrame> card = boardCard(area(), 1);
    QPointer<QWidget> column = side();
    QPointer<QWidget> state = mark();
    // The session list's poll names the same team: Backend is idle now.
    working = false;
    workspace.showTeam("team-1");
    QTRY_COMPARE(activity(), QStringLiteral("idle"));
    QVERIFY(canvas && canvas == area() && card && card == boardCard(area(), 1));
    QVERIFY(column && column == side() && state && state == mark());
    const Live idle = [] { Live l; l.known = true; return l; }();
    QCOMPARE(mark()->text(), Teams::memberStatus(*controller.watched(), *controller.watched()->member("backend"), idle).text);
    shot(&workspace, "u706-board-idle-in-place-dark");
    // The Graph's node: its activity, the view kept.
    view("graph");
    const auto graph = [&]() -> TeamUi::TaskView * {
        for (auto *w : workspace.findChildren<QWidget *>()) if (auto *v = dynamic_cast<TeamUi::TaskView *>(w); v && !v->nodes.isEmpty()) return v;
        return nullptr;
    };
    const auto node = [&](int id) { TeamUi::TaskView::Node found; if (auto *v = graph()) for (const auto &n : v->nodes) if (n.id == id) found = n; return found; };
    QTRY_COMPARE(node(1).activity, QStringLiteral("idle"));
    TestWait::settle();
    QPointer<QWidget> graphView = graph();
    working = true;
    workspace.showTeam("team-1");
    QTRY_COMPARE(node(1).activity, QStringLiteral("working"));
    QVERIFY(node(1).working);
    QVERIFY(graphView && graphView == graph());
    // Offline is a problem: the team's problems change, so it rebuilds.
    offline = true;
    workspace.showTeam("team-1");
    QTRY_VERIFY(!graphView || graphView != graph());
    QVERIFY(node(1).problem);
}

// #706: a `team list` read that moves only the revision and the updated time
// keeps the new values and tells nobody (the session list used to rebuild
// all its rows for it); a change it shows does. The team panel's refresh
// rebuilds only for a change it shows.
void TestTeamsUiWorkspace::aListPollThatMovesOnlyTheRevisionChangesNothing()
{
    auto team = liveTeam();
    setTeam(team);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    QSignalSpy changed(&controller, &TeamController::teamsChanged);
    const auto listReads = [&] { return calls(script(), "list").size(); };
    controller.refresh();
    QTRY_COMPARE(changed.count(), 1);
    for (int i = 1; i <= 2; ++i) {
        team["revision"] = 1 + i; team["updated"] = 5000 + 60 * i;
        setTeam(team);
        const auto before = listReads();
        controller.refresh();
        QTRY_VERIFY(listReads() > before && controller.team("team-1")->revision == 1 + i);
        QTest::qWait(50);
        QCOMPARE(changed.count(), 1);
    }
    team["name"] = "Live checkout";
    setTeam(team);
    controller.refresh();
    QTRY_COMPARE(changed.count(), 2);
    // The panel: a poll and the window's refresh with nothing new keep it.
    TeamPanel panel(&controller);
    panel.resize(360, 640); panel.show();
    panel.showTeam("team-1");
    QTRY_VERIFY(controller.watched() && panel.findChild<QWidget *>("teamPanelContent"));
    TestWait::settle();
    QPointer<QWidget> content = panel.findChild<QWidget *>("teamPanelContent");
    const auto statusReads = [&] { return calls(script(), "status").size(); };
    team["revision"] = 9; team["updated"] = 6000;
    setTeam(team);
    const auto before = statusReads();
    controller.refresh();
    QTRY_VERIFY(statusReads() > before && controller.watched()->revision == 9);
    panel.refresh();
    QTest::qWait(50);
    QVERIFY(content && content == panel.findChild<QWidget *>("teamPanelContent"));
    // A shown change: #1 is submitted.
    auto tasks = team["tasks"].toArray();
    auto first = tasks[0].toObject(); first["status"] = "submitted"; tasks[0] = first;
    team["tasks"] = tasks;
    setTeam(team);
    controller.refresh();
    QTRY_VERIFY(!content || content != panel.findChild<QWidget *>("teamPanelContent"));
}

QTEST_MAIN(TestTeamsUiWorkspace)
#include "test_teamsui_workspace.moc"
