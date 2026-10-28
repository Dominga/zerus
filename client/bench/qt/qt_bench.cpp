// Qt comparison harness for the Tauri client measurement (zerus-5wmh.4).
//
// Runs the tray's own SessionList (with SessionDelegate) and ActivityView on
// synthetic data in one window and measures, per scenario, after a warm-up,
// several timed runs, keeping the worst:
//   list-scroll      scroll position advanced each frame, synchronous repaint, events drained
//   activity-scroll  the same over a long Activity
//   session-open     setActivity() of another session until the first paint after the render
//   input-latency    a posted mouse press to the end of the paint that shows its change
//   idle             process CPU and RSS of this window with nothing happening
// Frame work is wall time and GUI-thread CPU time per frame (the method of the
// Activity scroll numbers in zerus-tllp and zerus-pgo9: wheel or scroll step,
// viewport()->repaint(), processEvents(), CLOCK_THREAD_CPUTIME_ID).
// Output: one JSON object (schema zerus-perf-v1) on stdout or in --out.
// Synthetic content only (example.com, /home/example).

#include "ActivityView.h"
#include "SessionCardDelegate.h"
#include "SessionList.h"

#include <QApplication>
#include <QElapsedTimer>
#include <QFile>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QListWidgetItem>
#include <QMouseEvent>
#include <QPainter>
#include <QRandomGenerator>
#include <QScreen>
#include <QWindow>
#include <QScrollBar>
#include <QSettings>
#include <QTemporaryDir>
#include <QTextBrowser>
#include <QThread>
#include <QTimer>

#ifdef Q_OS_MACOS
#include <mach/mach.h>
#endif

#include <algorithm>
#include <cmath>
#include <ctime>
#include <functional>
#include <vector>

namespace {

struct Options {
    qint64 warmupMs = 2000, runMs = 10000;
    int runs = 3, listRows = 2000, activityEvents = 1000, stepPx = 16, opens = 20, idleMs = 10000;
    QStringList scenarios{"list-scroll", "activity-scroll", "session-open", "input-latency", "idle"};
    QString out, label;
};

double threadCpuMs()
{
    timespec now{};
    clock_gettime(CLOCK_THREAD_CPUTIME_ID, &now);
    return now.tv_sec * 1e3 + now.tv_nsec / 1e6;
}

double processCpuMs()
{
    timespec now{};
    clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &now);
    return now.tv_sec * 1e3 + now.tv_nsec / 1e6;
}

qint64 rssBytes()
{
#ifdef Q_OS_LINUX
    QFile status("/proc/self/status");
    if (status.open(QIODevice::ReadOnly))
        for (const auto &line : status.readAll().split('\n'))
            if (line.startsWith("VmRSS:")) return line.mid(6).trimmed().split(' ').value(0).toLongLong() * 1024;
#elif defined(Q_OS_MACOS)
    mach_task_basic_info_data_t info{};
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO, task_info_t(&info), &count) == KERN_SUCCESS) return qint64(info.resident_size);
#endif
    return -1;
}

// macOS: the physical footprint Activity Monitor shows; -1 elsewhere.
qint64 footprintBytes()
{
#ifdef Q_OS_MACOS
    task_vm_info_data_t info{};
    mach_msg_type_number_t count = TASK_VM_INFO_COUNT;
    if (task_info(mach_task_self(), TASK_VM_INFO, task_info_t(&info), &count) == KERN_SUCCESS) return qint64(info.phys_footprint);
#endif
    return -1;
}

double nearestRank(const std::vector<double> &sorted, double p)
{
    if (sorted.empty()) return NAN;
    const auto rank = std::max<size_t>(1, size_t(std::ceil(p / 100.0 * sorted.size())));
    return sorted[std::min(rank, sorted.size()) - 1];
}

double round3(double value) { return std::round(value * 1000.0) / 1000.0; }

QJsonObject summary(std::vector<double> values)
{
    std::sort(values.begin(), values.end());
    double sum = 0;
    for (double v : values) sum += v;
    const size_t n = values.size();
    const double median = n == 0 ? NAN : n % 2 ? values[n / 2] : (values[n / 2 - 1] + values[n / 2]) / 2;
    return {{"count", qint64(n)}, {"mean", round3(n ? sum / n : NAN)}, {"median", round3(median)},
            {"p95", round3(nearestRank(values, 95))}, {"p99", round3(nearestRank(values, 99))},
            {"max", round3(n ? values.back() : NAN)}};
}

QJsonObject journalEvent(int seq, const QString &type, const QString &detail, const QString &tool = {})
{
    return {{"seq", seq}, {"at", 1791018000 + seq}, {"type", type}, {"detail", detail}, {"tool", tool}};
}

// A long synthetic Activity: prompts, rich replies (headings, lists, inline code
// pills, a fenced block) and tool results, like a real working session.
QJsonArray activityEvents(int count, const QString &salt)
{
    QJsonArray events;
    for (int i = 1; i <= count; ++i) {
        switch (i % 4) {
        case 1:
            events.append(journalEvent(i, "UserPromptSubmit",
                QString("Request %1 (%2): review `client/src/store/catalog.ts` and keep the list stable.").arg(i).arg(salt)));
            break;
        case 2:
            events.append(journalEvent(i, "PostToolUse", QString("rg -n \"applyBatch\" /home/example/project/client/src --stats %1").arg(i), "Bash"));
            break;
        case 3:
            events.append(journalEvent(i, "PostToolUse", QString("/home/example/project/client/src/ui/SessionCard%1.tsx").arg(i), "Read"));
            break;
        default: {
            QString body = QString("## Result %1\n\nThe change keeps **object identity** of unchanged rows and `reconcile` by key.\n\n").arg(i);
            for (int k = 0; k < 4; ++k)
                body += QString("- Preserve `field_%1_%2` and `example/cache/catalog.json` beside wrapped prose that continues across lines.\n").arg(i).arg(k);
            body += "\n```sh\ncargo +1.99.1 test --manifest-path client/core/Cargo.toml\n```\n\nSee https://example.com/docs for details.";
            events.append(journalEvent(i, "Stop", body));
        }
        }
    }
    return events;
}

void fillSessions(SessionList &list, int rows)
{
    const QStringList agents{"claude", "codex", "opencode", "deepseek"};
    const QStringList statuses{"Ready", "Working", "Needs approval", "Done", "Paused"};
    const int perProject = 20;
    for (int i = 0, project = 0; i < rows; ++project) {
        const QString group = QString("sample-project-%1").arg(project);
        auto *header = new QListWidgetItem(&list);
        header->setData(SessionRoles::Header, true); header->setData(SessionRoles::Group, group);
        header->setData(SessionRoles::Title, group); header->setData(SessionRoles::Total, perProject);
        header->setFlags(header->flags() & ~Qt::ItemIsSelectable);
        for (int k = 0; k < perProject && i < rows; ++k, ++i) {
            auto *row = new QListWidgetItem(&list);
            const QString agent = agents[i % agents.size()];
            row->setData(SessionRoles::Key, QString("key-%1").arg(i));
            row->setData(SessionRoles::Identity, QString("%1/%2/task-%3").arg(agent, group).arg(i));
            row->setData(SessionRoles::Group, group);
            row->setData(SessionRoles::Title, QString("Synthetic session %1 with a longer title").arg(i));
            row->setData(SessionRoles::Agent, agent);
            row->setData(SessionRoles::Status, statuses[i % statuses.size()]);
            row->setData(SessionRoles::Working, i % 5 == 1 ? 1 : 0);
            row->setData(SessionRoles::WorkingSince, 1791018000 + i);
            row->setData(SessionRoles::Attention, i % 17 == 0);
            row->setData(SessionRoles::Unread, i % 7 == 0 ? 2 : 0);
            row->setData(SessionRoles::Detail, QString("Reading client/src/ui/SessionCard%1.tsx").arg(i));
            row->setData(SessionRoles::Meta, QString("project-%1 / feature-branch-%2").arg(project).arg(i));
            row->setData(SessionRoles::MetaIcon, "git-branch");
            row->setData(SessionRoles::Model, i % 2 ? "model-large" : "model-fast");
            row->setData(SessionRoles::Effort, "high");
            row->setData(SessionRoles::Host, i % 3 ? "" : "example-host");
            row->setData(SessionRoles::MachineName, i % 3 ? "this computer" : "example-host");
        }
    }
}

// Debug aid (ZERUS_BENCH_DEBUG): prints the window's exposure changes.
class ExposeTrace : public QObject {
public:
    QElapsedTimer clock;
    ExposeTrace() { clock.start(); }
protected:
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        if (event->type() == QEvent::Expose || event->type() == QEvent::Hide || event->type() == QEvent::Show
            || event->type() == QEvent::WindowStateChange || event->type() == QEvent::ActivationChange)
            if (auto *window = qobject_cast<QWindow *>(watched))
                fprintf(stderr, "%lld ms: event %d exposed %d visible %d state %d\n", clock.elapsed(), int(event->type()),
                        window->isExposed(), window->isVisible(), int(window->windowStates()));
        return false;
    }
};

// A label that records when its paint after a change ends.
class Probe : public QLabel {
public:
    std::function<void()> painted;
protected:
    void mousePressEvent(QMouseEvent *event) override { setText(QString::number(++m_presses)); event->accept(); }
    void paintEvent(QPaintEvent *event) override { QLabel::paintEvent(event); if (painted) painted(); }
private:
    int m_presses = 0;
};

struct Bench {
    Options options;
    QWidget window;
    SessionList *list = nullptr;
    ActivityView *activity = nullptr;
    Probe *probe = nullptr;
    QJsonArray sessionsA, sessionsB;
    bool showingA = true;

    void build()
    {
        auto *layout = new QHBoxLayout(&window);
        layout->setContentsMargins(0, 0, 0, 0);
        list = new SessionList; list->setObjectName("sessionList");
        list->setItemDelegate(new SessionDelegate(list)); list->setMouseTracking(true);
        list->setVerticalScrollMode(QAbstractItemView::ScrollPerPixel);
        list->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        list->setProperty("compact", true);
        list->setFixedWidth(310);
        fillSessions(*list, options.listRows);
        activity = new ActivityView;
        activity->setTheme(true);
        probe = new Probe; probe->setText("0"); probe->setFixedWidth(60);
        layout->addWidget(list); layout->addWidget(activity, 1); layout->addWidget(probe);
        // On a real display the window says what it is. It must stay fully visible:
        // a compositor suspends an occluded window (Qt then reports it unexposed and
        // paints nothing), so it is raised and activated like a normal new window.
        window.setWindowTitle("Zerus Qt measurement (closes itself)");
        window.resize(1440, 900);
        window.show();
        window.raise();
        window.activateWindow();
        // A Wayland compositor exposes the window asynchronously (after its configure);
        // nothing paints before that.
        QElapsedTimer exposure; exposure.start();
        while (!(window.windowHandle() && window.windowHandle()->isExposed()) && exposure.elapsed() < 10000) {
            drain();
            QThread::usleep(1000);
        }
        if (qEnvironmentVariableIsSet("ZERUS_BENCH_DEBUG") && window.windowHandle())
            window.windowHandle()->installEventFilter(new ExposeTrace);
        if (qEnvironmentVariableIsSet("ZERUS_BENCH_DEBUG"))
            fprintf(stderr, "exposed %d after %lld ms, visible %d, handle %p\n", window.windowHandle() && window.windowHandle()->isExposed(),
                  exposure.elapsed(), window.isVisible(), (void *)window.windowHandle());
        sessionsA = activityEvents(options.activityEvents, "a");
        sessionsB = activityEvents(options.activityEvents, "b");
        openActivity("session-a", sessionsA);
    }

    // Like QTest::qWait: drain events and deferred deletes. A replaced Activity
    // document is deleted later, and only then does its render worker finish.
    static void drain()
    {
        QCoreApplication::processEvents(QEventLoop::AllEvents);
        QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
    }

    void settle()
    {
        QElapsedTimer timeout; timeout.start();
        while (activity->renderPending() && timeout.elapsed() < 30000) {
            drain();
            QThread::usleep(200);
        }
        if (activity->renderPending()) qFatal("Activity render did not complete");
    }

    // Wall time from switching to a session until the paint after its render.
    double openActivity(const QString &key, const QJsonArray &events)
    {
        QElapsedTimer clock; clock.start();
        activity->setSessionKey(key);
        activity->setActivity({{"conversation_id", key}}, events);
        settle();
        activity->browser()->viewport()->repaint();
        drain();
        return clock.nsecsElapsed() / 1e6;
    }

    QJsonObject scrollRun(QAbstractScrollArea *area, qint64 ms)
    {
        auto *bar = area->verticalScrollBar();
        std::vector<double> wall, cpu;
        int direction = 1;
        QElapsedTimer total; total.start();
        while (total.elapsed() < ms) {
            QElapsedTimer frame; frame.start();
            const double cpu0 = threadCpuMs();
            int next = bar->value() + direction * options.stepPx;
            if (next >= bar->maximum() || next <= bar->minimum()) { direction = -direction; next = std::clamp(next, bar->minimum(), bar->maximum()); }
            bar->setValue(next);
            area->viewport()->repaint();
            QCoreApplication::processEvents();
            wall.push_back(frame.nsecsElapsed() / 1e6);
            cpu.push_back(threadCpuMs() - cpu0);
        }
        return {{"frames", qint64(wall.size())}, {"wall_ms", summary(wall)}, {"gui_cpu_ms", summary(cpu)}};
    }

    QJsonObject openRun(qint64 ms)
    {
        std::vector<double> times;
        QElapsedTimer total; total.start();
        while (total.elapsed() < ms && int(times.size()) < options.opens) {
            // Always switch to the other session, as a click on another row does.
            showingA = !showingA;
            times.push_back(openActivity(showingA ? "session-a" : "session-b", showingA ? sessionsA : sessionsB));
        }
        return {{"opens", qint64(times.size())}, {"wall_ms", summary(times)}};
    }

    QJsonObject inputRun(qint64 ms)
    {
        std::vector<double> latencies;
        QElapsedTimer total; total.start();
        QElapsedTimer sent;
        bool waiting = false;
        probe->painted = [&] {
            if (!waiting) return;
            waiting = false;
            latencies.push_back(sent.nsecsElapsed() / 1e6);
        };
        while (total.elapsed() < ms) {
            // Random phase relative to Qt's update cadence, like real input.
            const int gap = 40 + int(QRandomGenerator::global()->bounded(70));
            QElapsedTimer pause; pause.start();
            while (pause.elapsed() < gap) { drain(); QThread::usleep(500); }
            const QPointF at(10, 10);
            waiting = true;
            sent.start();
            QCoreApplication::postEvent(probe, new QMouseEvent(QEvent::MouseButtonPress, at, probe->mapToGlobal(at),
                                                               Qt::LeftButton, Qt::LeftButton, Qt::NoModifier));
            QElapsedTimer timeout; timeout.start();
            while (waiting && timeout.elapsed() < 1000) { drain(); QThread::usleep(50); }
            waiting = false;
            QCoreApplication::postEvent(probe, new QMouseEvent(QEvent::MouseButtonRelease, at, probe->mapToGlobal(at),
                                                               Qt::LeftButton, Qt::NoButton, Qt::NoModifier));
        }
        probe->painted = nullptr;
        return {{"inputs", qint64(latencies.size())}, {"latency_ms", summary(latencies)}};
    }

    QJsonObject idleRun(qint64 ms)
    {
        // A real event loop sleeps between events; timers and animations still run.
        const double cpu0 = processCpuMs();
        QElapsedTimer clock; clock.start();
        QEventLoop loop;
        QTimer::singleShot(ms, Qt::PreciseTimer, &loop, &QEventLoop::quit);
        loop.exec();
        const double elapsed = clock.nsecsElapsed() / 1e6;
        return {{"window_ms", qint64(elapsed)}, {"cpu_percent", round3((processCpuMs() - cpu0) / elapsed * 100.0)},
                {"rss_bytes", rssBytes()}, {"footprint_bytes", footprintBytes()}};
    }

    bool exposed() const { return window.windowHandle() && window.windowHandle()->isExposed(); }

    // Warm-up, then the timed runs; the run with the highest p95 of `metric` counts.
    QJsonObject repeat(const QString &id, const QString &kind, const QString &metric, const std::function<QJsonObject(qint64)> &run)
    {
        const bool exposedBefore = exposed();
        run(options.warmupMs);
        QJsonArray runs;
        int worst = 0;
        double worstP95 = -1;
        for (int i = 0; i < options.runs; ++i) {
            const auto result = run(options.runMs);
            const double p95 = result[metric].toObject()["p95"].toDouble();
            if (p95 > worstP95) { worstP95 = p95; worst = i; }
            runs.append(result);
        }
        // A window that was hidden or suspended painted nothing: such a scenario is invalid.
        return {{"id", id}, {"kind", kind}, {"metric", metric}, {"runs", runs}, {"worst_index", worst}, {"worst", runs[worst]},
                {"window_exposed", exposedBefore && exposed()}};
    }
};

Options parse(const QStringList &args)
{
    Options o;
    for (int i = 1; i < args.size(); ++i) {
        const auto next = [&] { return i + 1 < args.size() ? args[++i] : QString(); };
        const auto &arg = args[i];
        if (arg == "--warmup-ms") o.warmupMs = next().toLongLong();
        else if (arg == "--run-ms") o.runMs = next().toLongLong();
        else if (arg == "--runs") o.runs = next().toInt();
        else if (arg == "--rows") o.listRows = next().toInt();
        else if (arg == "--events") o.activityEvents = next().toInt();
        else if (arg == "--step") o.stepPx = next().toInt();
        else if (arg == "--idle-ms") o.idleMs = next().toInt();
        else if (arg == "--scenarios") o.scenarios = next().split(',', Qt::SkipEmptyParts);
        else if (arg == "--out") o.out = next();
        else if (arg == "--label") o.label = next();
    }
    return o;
}

} // namespace

int main(int argc, char **argv)
{
    // Settings of this run stay in a temporary directory, never the user's.
    QTemporaryDir settings;
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, settings.path());
    QApplication app(argc, argv);
    QCoreApplication::setOrganizationName("zerus-qt-bench");
    QCoreApplication::setApplicationName("bench");

    Bench bench;
    bench.options = parse(app.arguments());
    const auto &o = bench.options;
    const double startupCpu = processCpuMs();
    QElapsedTimer startup; startup.start();
    bench.build();
    bench.settle();
    const double startupMs = startup.nsecsElapsed() / 1e6;
    const double startupCpuMs = processCpuMs() - startupCpu;
    // Evidence that the window was on screen (a real display or Xvfb).
    const bool exposed = bench.window.windowHandle() && bench.window.windowHandle()->isExposed();

    QJsonArray scenarios;
    for (const auto &id : o.scenarios) {
        if (id == "list-scroll")
            scenarios.append(bench.repeat(id, "frame-work", "wall_ms", [&](qint64 ms) { return bench.scrollRun(bench.list, ms); }));
        else if (id == "activity-scroll")
            scenarios.append(bench.repeat(id, "frame-work", "wall_ms", [&](qint64 ms) { return bench.scrollRun(bench.activity->browser(), ms); }));
        else if (id == "session-open")
            scenarios.append(bench.repeat(id, "timing", "wall_ms", [&](qint64 ms) { return bench.openRun(ms); }));
        else if (id == "input-latency")
            scenarios.append(bench.repeat(id, "latency", "latency_ms", [&](qint64 ms) { return bench.inputRun(ms); }));
        else if (id == "idle")
            scenarios.append(QJsonObject{{"id", id}, {"kind", "resources"}, {"worst", bench.idleRun(o.idleMs)}});
    }

    auto *screen = bench.window.screen() ? bench.window.screen() : QGuiApplication::primaryScreen();
    const QString platform = QGuiApplication::platformName();
    const QJsonObject result{
        {"schema", "zerus-perf-v1"},
        {"client", "qt"},
        {"harness", "client/bench/qt/qt_bench.cpp"},
        {"created_at", QDateTime::currentDateTimeUtc().toString(Qt::ISODate)},
        {"platform", QJsonObject{
            {"os", QSysInfo::productType()}, {"os_version", QSysInfo::productVersion()},
            {"label", o.label}, {"qpa", platform}, {"qt", qVersion()},
            {"screen_hz", screen ? round3(screen->refreshRate()) : 0.0},
            {"dpr", screen ? screen->devicePixelRatio() : 1.0},
            {"screen", screen ? QString("%1x%2").arg(screen->size().width()).arg(screen->size().height()) : QString()},
            {"cpu_threads", QThread::idealThreadCount()}, {"window_exposed", exposed},
            {"representative", platform != "offscreen" && qEnvironmentVariable("ZERUS_BENCH_XVFB") != "1"}}},
        {"settings", QJsonObject{
            {"warmup_ms", o.warmupMs}, {"run_ms", o.runMs}, {"runs", o.runs}, {"list_rows", o.listRows},
            {"activity_events", o.activityEvents}, {"step_px", o.stepPx}, {"window", "1440x900"}}},
        // Building the window and the first render; CPU of all threads (render workers included).
        {"startup", QJsonObject{{"wall_ms", round3(startupMs)}, {"process_cpu_ms", round3(startupCpuMs)}}},
        {"scenarios", scenarios},
    };
    const auto json = QJsonDocument(result).toJson(QJsonDocument::Indented);
    if (o.out.isEmpty()) {
        fwrite(json.constData(), 1, size_t(json.size()), stdout);
    } else {
        QFile file(o.out);
        if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) return 1;
        file.write(json);
    }
    return 0;
}
