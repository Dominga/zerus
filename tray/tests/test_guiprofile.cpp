#include <QtTest>

#include <QApplication>
#include <QDir>
#include <QJsonArray>
#include <QJsonDocument>
#include <QListWidget>
#include <QPushButton>
#include <QScrollBar>
#include <QSettings>
#include <QTemporaryDir>

#include <algorithm>
#include <atomic>
#include <map>
#include <thread>

#include "FakeCli.h"
#include "JsonSettings.h"
#include "ReadmeShots.h"
#include "SessionsWindow.h"
#include "SlowEventLog.h"
#include "TeamController.h"
#include "WorkspaceSkin.h"

// WhiteWh fork (owner, 2026-10-30: "the UI lags"): a repeatable profile of the
// main screens with synthetic data - hundreds of sessions on four computers, a
// running team of 10 members with 60 tasks, an active chat - measured with the
// slow-event meter the tray uses (SlowEventLog.h) on every GUI-thread event.
//
// Each scene runs ZERUS_GUI_PROFILE_SECONDS (default 3) with the polls a live
// tray gets: fleet refreshes, the inspector's and the team's own polls of a fake
// hgs whose team revision moves and whose chat grows. Scene steps are queued
// calls on objects named "scenario:<step>", so they rank apart from the
// tray's own handlers. ZERUS_GUI_PROFILE_SESSIONS sets the session count
// (default 400). The ranking goes to ZERUS_GUI_PROFILE_OUT (default: the test
// folder's gui-profile) as profile.json and profile.txt, and to the log.
//
// Run: QT_QPA_PLATFORM=windows ZERUS_GUI_PROFILE_SECONDS=20 test_guiprofile (the
// window stays off the screen), or offscreen (the guiprofile_sessions ctest
// runs the sessions scene so). ZERUS_GUI_PROFILE_METER=0 runs unmeasured. The
// crashes #704 saw, FreeType's at exit among them, came from the sessions
// scene reading a destroyed temporary (#723).
namespace {

SlowEventLog::Meter *g_meter = nullptr;

class ProfileApplication : public QApplication {
public:
    using QApplication::QApplication;
    bool notify(QObject *receiver, QEvent *event) override
    {
        if (!g_meter) return QApplication::notify(receiver, event);
        return g_meter->measure(receiver, event, [&] { return QApplication::notify(receiver, event); });
    }
};

int seconds()
{
    bool ok = false;
    const int value = qEnvironmentVariableIntValue("ZERUS_GUI_PROFILE_SECONDS", &ok);
    return ok && value > 0 ? value : 3;
}

int sessionCount()
{
    bool ok = false;
    const int value = qEnvironmentVariableIntValue("ZERUS_GUI_PROFILE_SESSIONS", &ok);
    return ok && value > 0 ? value : 400;
}

const QStringList hosts{QString(), QStringLiteral("macbook"), QStringLiteral("build-01"), QStringLiteral("build-02")};
const QString activeChat = QStringLiteral("\nclaude/orbit/checkout");

// A paragraph of an agent's answer, with the markdown the Activity view renders.
QString answer(int n)
{
    return QStringLiteral("Step %1 of the checkout review:\n\n- The saved cards endpoint returns **%2** cards\n"
                          "- `web/checkout/CardForm.tsx` keeps its keyboard order\n\n"
                          "```bash\ncargo test -p payments saved_cards_%1\n```\n\nNext I check the contract tests.")
        .arg(n).arg(n % 7 + 1);
}

// The README workspace grown to `count` sessions, a tenth of them working.
FleetState fleet(double now, int count, int generation)
{
    FleetState state = ReadmeShots::fleet(now);
    const QStringList agents{"claude", "codex", "dsh", "kimi", "opencode"};
    const QStringList projects{"orbit", "infra", "docs", "mobile", "data", "billing"};
    QList<BoxState> boxes{state.local()};
    for (const auto &peer : state.peerNames()) boxes << *state.peer(peer);
    // The team's nine members work in sessions of their own.
    for (int m = 1; m < 10; ++m) {
        auto s = ReadmeShots::session(QStringLiteral("codex"), QStringLiteral("orbit"), QStringLiteral("member-%1").arg(m), QStringLiteral("gpt-6.1-sol"),
                                      QStringLiteral("/srv/work/orbit-team/m%1").arg(m), QStringLiteral("team/big/m%1").arg(m), QStringLiteral("#%1 of the team").arg(m), now);
        if ((m + generation) % 3) ReadmeShots::working(s, QStringLiteral("Bash"), QStringLiteral("cargo test"), 40 + m * 10, now);
        boxes[m % 4].sessions << s;
    }
    for (int i = 0; boxes[0].sessions.size() + boxes[1].sessions.size() + boxes[2].sessions.size() + boxes[3].sessions.size() < count; ++i) {
        auto &box = boxes[i % 4];
        auto s = ReadmeShots::session(agents[i % agents.size()], projects[i % projects.size()], QStringLiteral("work-%1").arg(i, 3, 10, QLatin1Char('0')),
                                      QStringLiteral("model-%1").arg(i % 5), QStringLiteral("/srv/work/%1").arg(projects[i % projects.size()]),
                                      QStringLiteral("feat/item-%1").arg(i), QStringLiteral("Work item %1 of the backlog.").arg(i), now);
        // The polls move a tenth of them between working and idle.
        if ((i + generation) % 10 == 0) ReadmeShots::working(s, QStringLiteral("Bash"), QStringLiteral("cargo test item_%1").arg(i), 30 + i % 300, now);
        if (i % 13 == 0) { s.unreadReply = true; s.replyId = QStringLiteral("reply-%1-%2").arg(i).arg(generation); s.replyAt = now - 60; }
        if (i % 29 == 0) { s.activity = "waiting"; s.phase = "input"; s.attentionId = QStringLiteral("attention-%1").arg(i); }
        box.sessions << s;
    }
    FleetState grown;
    const auto ms = qint64(now * 1000);
    grown.setLocal(boxes[0], ms);
    for (int i = 1; i < 4; ++i) grown.setPeer(boxes[i], ms);
    return grown;
}

// A running team: the README lead and nine members, 60 tasks in six waves.
QJsonObject team(double now)
{
    QJsonObject value = ReadmeShots::team(now);
    QJsonArray members{value["members"].toArray()[0]};
    const QStringList roles{"Backend", "Frontend", "Tester", "Reviewer", "Docs"};
    for (int m = 1; m < 10; ++m)
        members.append(QJsonObject{{"id", QStringLiteral("m%1").arg(m)}, {"role", roles[m % roles.size()]}, {"agent", "codex"}, {"model", "gpt-6.1-sol"},
                                   {"state", "active"}, {"computer", hosts[m % 4]},
                                   {"session", QJsonObject{{"host", hosts[m % 4]}, {"name", QStringLiteral("codex/orbit/member-%1").arg(m)}}}});
    QJsonArray tasks;
    QJsonObject layers;
    const QStringList statuses{"accepted", "accepted", "in_progress", "submitted", "pending", "pending"};
    for (int id = 1; id <= 60; ++id) {
        const int wave = (id - 1) / 10;
        const QString status = statuses[std::min(wave, int(statuses.size()) - 1)];
        const QString assignee = id == 1 ? QStringLiteral("lead") : QStringLiteral("m%1").arg((id % 9) + 1);
        QJsonObject task{{"id", id}, {"title", QStringLiteral("Task %1 of wave %2").arg(id).arg(wave + 1)}, {"assignee", assignee}, {"status", status},
                         {"phase", 1}, {"criteria", QJsonArray{"It works", "Its tests pass"}}};
        if (id > 10) task["deps"] = QJsonArray{QJsonObject{{"task", id - 10}}, QJsonObject{{"task", ((id + 3) % 10) + 1 + (wave - 1) * 10}, {"kind", "soft"}}};
        if (status != "pending")
            task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", QStringLiteral("a%1").arg(id)}, {"member", assignee},
                {"state", status == "in_progress" ? "delivered" : "reported"}, {"sent_at", now - 3000 + id * 20},
                {"report", status == "in_progress" ? QJsonValue() : QJsonValue(QJsonObject{{"text", "Done; tests pass."}, {"at", now - 1000 + id * 10}})}}};
        tasks.append(task);
        layers[QString::number(id)] = wave;
    }
    value["id"] = "team-big";
    value["name"] = "Checkout at scale";
    value["members"] = members;
    value["tasks"] = tasks;
    value["layers"] = layers;
    return value;
}

// The fake hgs of the README pictures, with a running team (its revision moves
// with every read) and a chat that grows while it runs.
QByteArray program()
{
    QByteArray text(ReadmeShots::program);
    text.replace(" elif op=='status':out(d['team'])",
                 " elif op=='status':\n  import time\n  t=dict(d['team']);t['revision']=int(time.time()*2);t['updated']=time.time();out(t)");
    // #706: so does its item in `team list`, as hgs lists a running team.
    text.replace("  out({'items':[t]})", "  import time\n  t['revision']=int(time.time()*2);t['updated']=time.time()\n  out({'items':[t]})");
    text.replace(" out({} if '--agent' in a else d['inspect'].get(host+'\\n'+name,{'tracked':True,'events':[],'cursor':0}))",
                 " r={} if '--agent' in a else dict(d['inspect'].get(host+'\\n'+name,{'tracked':True,'events':[],'cursor':0}))\n"
                 " if host+'\\n'+name==d['active']['key']:\n"
                 "  import time\n"
                 "  base=r['events'];n=min(2000,int((time.time()-d['active']['start'])*3))\n"
                 "  r['events']=base+[{'seq':len(base)+i+1,'at':d['active']['start']+i/3,'type':'AgentMessage' if i%3 else 'PostToolUse',"
                 "'detail':d['active']['text'].replace('#',str(i)),'tool':'' if i%3 else 'Bash'} for i in range(n)]\n"
                 "  r['cursor']=len(r['events'])\n"
                 " out(r)");
    return text;
}

struct Scene {
    QString name;
    double wallMs = 0;
    double guiMs = 0;
    QList<SlowEventLog::Handler> top;
};

}

class TestGuiProfile : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void sessionsListAndInspector();
    void activityChat();
    void teamGraph() { teamScene(QStringLiteral("graph")); }
    void teamBoard() { teamScene(QStringLiteral("board")); }
    void teamLayers() { teamScene(QStringLiteral("layers")); }
    void settingsPages();
    void cleanupTestCase();

private:
    // Runs the scene for seconds(): each period `step` is queued on an object
    // named "scenario:<step>", and the meter keeps what every handler took.
    void run(const QString &name, int periodMs, const std::function<void(int)> &step);
    void teamScene(const QString &view);
    QString fixture() const { return m_dir.filePath(QStringLiteral("hgs")); }
    QString outDir() const;
    void writeScene(const Scene &scene) const;

    QTemporaryDir m_dir;
    double m_now = 0;
    std::unique_ptr<SlowEventLog::Meter> m_meter;
    std::unique_ptr<SessionsWindow> m_window;
    QList<Scene> m_scenes;
    int m_generation = 0;
};

void TestGuiProfile::initTestCase()
{
    QVERIFY(m_dir.isValid());
    QCoreApplication::setOrganizationName(QStringLiteral("hgs-tests"));
    QCoreApplication::setApplicationName(QStringLiteral("gui-profile"));
    QCoreApplication::setApplicationVersion(QLatin1String(HGS_TRAY_VERSION));
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_dir.path());
    WorkspaceSkin::preload();
    m_now = QDateTime::currentSecsSinceEpoch();
    const auto state = fleet(m_now, sessionCount(), 0);
    QJsonObject inspect = ReadmeShots::conversations(state, m_now);
    // The active chat starts with 300 events and grows by three a second.
    QJsonObject lead = inspect[activeChat].toObject();
    QJsonArray events = lead["events"].toArray();
    for (int i = 0; i < 300; ++i)
        events.append(ReadmeShots::event(events.size() + 1, m_now - 3000 + i * 5, i % 3 ? "AgentMessage" : "PostToolUse", answer(i), i % 3 ? QString() : "Bash"));
    lead["events"] = events;
    lead["cursor"] = events.size();
    inspect[activeChat] = lead;
    const QJsonObject data{{"inspect", inspect}, {"accounts", ReadmeShots::accounts(m_now)}, {"team", team(m_now)}, {"feed", ReadmeShots::feed(m_now)},
                           {"catalog", ReadmeShots::catalog()}, {"organization", ReadmeShots::organization()},
                           {"active", QJsonObject{{"key", activeChat}, {"start", m_now}, {"text", answer(0).replace("0", "#")}}}};
    // Both changes of the README fake took: a running team and a growing chat.
    QVERIFY(program().contains("d['active']") && program().count("time.time()*2") == 2);
    QVERIFY(FakeCli::install(fixture(), program()));
    QVERIFY(FakeCli::replace(fixture() + ".json", QJsonDocument(data).toJson(QJsonDocument::Compact)));
    setJsonSetting("workspace/organization", QJsonDocument(ReadmeShots::organization()).toJson(QJsonDocument::Compact));
    m_meter = std::make_unique<SlowEventLog::Meter>(SlowEventLog::Config{}, [](const QString &line) { qInfo().noquote() << line; });
    m_meter->setStatistics(true);
    // ZERUS_GUI_PROFILE_METER=0 runs the scenes unmeasured, to compare.
    if (qEnvironmentVariable("ZERUS_GUI_PROFILE_METER") != QLatin1String("0")) {
        g_meter = m_meter.get();
        SlowEventLog::Meter::setActive(g_meter);
    }
    // The window's start counts as a scene of its own.
    m_meter->reset();
    QElapsedTimer opening;
    opening.start();
    m_window = std::make_unique<SessionsWindow>(FakeCli::program(fixture()));
    m_window->resize(1440, 900);
    // On a real desktop (QT_QPA_PLATFORM=windows, to measure the platform's own
    // painting and fonts) the window stays off the screen and takes no focus.
    const bool desktop = QGuiApplication::platformName() != QLatin1String("offscreen");
    if (desktop) { m_window->setAttribute(Qt::WA_ShowWithoutActivating); m_window->move(-20000, -20000); }
    m_window->setFleet(state);
    m_window->show();
    if (!desktop) QVERIFY(QTest::qWaitForWindowExposed(m_window.get()));
    QTest::qWait(1000);
    m_scenes.append({QStringLiteral("open"), double(opening.elapsed()), m_meter->totalMs(), m_meter->top(50)});
    writeScene(m_scenes.last());
}

void TestGuiProfile::run(const QString &name, int periodMs, const std::function<void(int)> &step)
{
    std::map<QString, std::unique_ptr<QObject>> steps;
    m_meter->reset();
    QElapsedTimer wall;
    wall.start();
    int tick = 0;
    QTimer driver;
    driver.setObjectName(QStringLiteral("scenario:clock"));
    connect(&driver, &QTimer::timeout, this, [&] {
        const int n = tick++;
        auto &context = steps[name];
        if (!context) { context = std::make_unique<QObject>(); context->setObjectName(QStringLiteral("scenario:") + name); }
        QMetaObject::invokeMethod(context.get(), [&step, n] { step(n); }, Qt::QueuedConnection);
    });
    driver.start(periodMs);
    while (wall.elapsed() < seconds() * 1000) QTest::qWait(50);
    driver.stop();
    QCoreApplication::processEvents();
    Scene scene{name, double(wall.elapsed()), m_meter->totalMs(), m_meter->top(50)};
    QVERIFY2(!g_meter || (scene.guiMs > 0 && !scene.top.isEmpty()), qPrintable(name));
    m_scenes.append(scene);
    writeScene(scene);
}

void TestGuiProfile::sessionsListAndInspector()
{
    // Fleet polls every second; the person moves through the list.
    // #723: the fleet is kept while its names are read. A range-for over
    // fleet(...).local().sessions read a temporary that ended before the loop
    // (local() returns into it): each copied name raised the refcount of a
    // freed string, which corrupted the heap seconds later (#704's crash).
    QStringList names;
    const auto state = fleet(m_now, sessionCount(), 0);
    for (const auto &s : state.local().sessions) names << s.name;
    run(QStringLiteral("sessions"), 500, [this, names](int n) {
        if (n % 2 == 0) {
            m_window->refreshFleet(fleet(m_now + n, sessionCount(), ++m_generation));
        } else {
            m_window->showSession(QString(), names.value((n / 2) % std::max<qsizetype>(1, names.size())));
            if (auto *list = m_window->findChild<QAbstractScrollArea *>(QStringLiteral("sessionList")))
                list->verticalScrollBar()->setValue((n * 120) % std::max(1, list->verticalScrollBar()->maximum() + 1));
        }
    });
}

void TestGuiProfile::activityChat()
{
    m_window->showSession(QString(), QStringLiteral("claude/orbit/checkout"));
    run(QStringLiteral("activity"), 1000, [this](int n) {
        if (n % 3 == 0) m_window->refreshFleet(fleet(m_now + n, sessionCount(), ++m_generation));
    });
}

void TestGuiProfile::teamScene(const QString &view)
{
    m_window->showTeamAttention(QStringLiteral("zerus-team-attention:team-big:report:Backend:1"));
    QTRY_VERIFY_WITH_TIMEOUT(m_window->findChild<QWidget *>(QStringLiteral("teamWorkspaceView")), 20000);
    for (auto *button : m_window->findChild<QWidget *>(QStringLiteral("teamWorkspaceView"))->findChildren<QPushButton *>())
        if (button->property("segment") == view) { button->click(); break; }
    run(QStringLiteral("team-") + view, 1000, [this](int n) {
        if (n % 2 == 0) m_window->refreshFleet(fleet(m_now + n, sessionCount(), ++m_generation));
        if (n == 2) m_window->resize(1280, 860);
        if (n == 4) m_window->resize(1440, 900);
    });
}

void TestGuiProfile::settingsPages()
{
    auto *settings = m_window->findChild<QPushButton *>(QStringLiteral("workspaceSettings"));
    QVERIFY(settings);
    settings->click();
    auto *sections = m_window->findChild<QListWidget *>(QStringLiteral("settingsSections"));
    QVERIFY(sections);
    run(QStringLiteral("settings"), 400, [sections](int n) { sections->setCurrentRow(n % sections->count()); });
}

QString TestGuiProfile::outDir() const
{
    return qEnvironmentVariable("ZERUS_GUI_PROFILE_OUT", QDir::current().filePath(QStringLiteral("gui-profile")));
}

// A finished scene's numbers, at once: a later scene that fails keeps them, and
// scenes run one per process add up in the same folder.
void TestGuiProfile::writeScene(const Scene &scene) const
{
    QJsonArray top;
    for (const auto &h : scene.top)
        top.append(QJsonObject{{"handler", h.name}, {"count", h.count}, {"total_ms", h.totalMs}, {"max_ms", h.maxMs}});
    QDir().mkpath(outDir());
    QFile file(QDir(outDir()).filePath(QStringLiteral("scene-%1.json").arg(scene.name)));
    if (file.open(QIODevice::WriteOnly))
        file.write(QJsonDocument(QJsonObject{{"scene", scene.name}, {"wall_ms", scene.wallMs}, {"gui_ms", scene.guiMs},
                                             {"sessions", sessionCount()}, {"seconds", seconds()}, {"top", top}}).toJson());
}

void TestGuiProfile::cleanupTestCase()
{
    g_meter = nullptr;
    SlowEventLog::Meter::setActive(nullptr);
    m_window.reset();
    // The ranking across every scene in the folder: each handler's time in all.
    std::map<QString, SlowEventLog::Handler> all;
    double gui = 0;
    QJsonArray scenes;
    QString text = QStringLiteral("GUI profile: %1 sessions, 10 members and 60 tasks, %2 s a scene\n\n").arg(sessionCount()).arg(seconds());
    for (const auto &entry : QDir(outDir()).entryInfoList({QStringLiteral("scene-*.json")}, QDir::Files, QDir::Name)) {
        QFile file(entry.filePath());
        if (!file.open(QIODevice::ReadOnly)) continue;
        const QJsonObject scene = QJsonDocument::fromJson(file.readAll()).object();
        const double guiMs = scene["gui_ms"].toDouble(), wallMs = scene["wall_ms"].toDouble();
        gui += guiMs;
        QJsonArray top;
        for (const auto &value : scene["top"].toArray()) {
            const QJsonObject h = value.toObject();
            auto &found = all[h["handler"].toString()];
            found.name = h["handler"].toString();
            found.count += h["count"].toInteger();
            found.totalMs += h["total_ms"].toDouble();
            found.maxMs = std::max(found.maxMs, h["max_ms"].toDouble());
            if (top.size() < 10)
                top.append(QJsonObject{{"handler", found.name}, {"count", h["count"]}, {"total_ms", qRound(h["total_ms"].toDouble())},
                                       {"max_ms", qRound(h["max_ms"].toDouble())}, {"share", guiMs > 0 ? h["total_ms"].toDouble() / guiMs : 0}});
        }
        scenes.append(QJsonObject{{"scene", scene["scene"]}, {"wall_ms", qRound(wallMs)}, {"gui_ms", qRound(guiMs)},
                                  {"busy", wallMs > 0 ? guiMs / wallMs : 0}, {"top", top}});
        text += QStringLiteral("%1: %2 ms of GUI time in %3 ms (%4% busy)\n").arg(scene["scene"].toString(), -12).arg(qRound(guiMs), 6)
                    .arg(qRound(wallMs), 6).arg(wallMs > 0 ? qRound(100 * guiMs / wallMs) : 0);
    }
    QList<SlowEventLog::Handler> ranked;
    for (const auto &[name, handler] : all) ranked << handler;
    std::sort(ranked.begin(), ranked.end(), [](const auto &a, const auto &b) { return a.totalMs > b.totalMs; });
    QJsonArray overall;
    text += QStringLiteral("\nThe ten slowest handlers (total, share of all GUI time, count, longest):\n");
    for (int i = 0; i < std::min<qsizetype>(10, ranked.size()); ++i) {
        const auto &h = ranked[i];
        overall.append(QJsonObject{{"handler", h.name}, {"count", h.count}, {"total_ms", qRound(h.totalMs)}, {"max_ms", qRound(h.maxMs)},
                                   {"share", gui > 0 ? h.totalMs / gui : 0}});
        text += QStringLiteral("%1. %2 ms  %3%  x%4  max %5 ms  %6\n").arg(i + 1, 2).arg(qRound(h.totalMs), 6)
                    .arg(gui > 0 ? 100 * h.totalMs / gui : 0, 4, 'f', 1).arg(h.count).arg(qRound(h.maxMs)).arg(h.name);
    }
    QFile json(QDir(outDir()).filePath(QStringLiteral("profile.json"))), plain(QDir(outDir()).filePath(QStringLiteral("profile.txt")));
    QVERIFY(json.open(QIODevice::WriteOnly) && plain.open(QIODevice::WriteOnly));
    json.write(QJsonDocument(QJsonObject{{"sessions", sessionCount()}, {"seconds", seconds()}, {"gui_ms", qRound(gui)},
                                         {"scenes", scenes}, {"top", overall}}).toJson());
    plain.write(text.toUtf8());
    qInfo().noquote() << text;
    QVERIFY(!g_meter && (!qEnvironmentVariable("ZERUS_GUI_PROFILE_METER").isEmpty() || !overall.isEmpty()));
}
int main(int argc, char **argv)
{
    ProfileApplication app(argc, argv);
    QTEST_SET_MAIN_SOURCE_PATH
    TestGuiProfile test;
    return QTest::qExec(&test, argc, argv);
}

#include "test_guiprofile.moc"
