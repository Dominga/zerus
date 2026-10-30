#pragma once

#include <QElapsedTimer>
#include <QHash>
#include <QList>
#include <QEvent>
#include <QObject>
#include <QString>

#include <functional>


// WhiteWh fork (owner, 2026-10-30: "the UI lags"): where the GUI thread spends
// its time. The application's notify() runs every event through a Meter
// (main.cpp: ZerusApplication; the profile scenario: tests/test_guiprofile).
//
// A handler's time is its own: the time other events took inside it - a modal
// dialog's loop, processEvents() - is subtracted, so a dialog open for a
// minute is not one slow click. An event that takes threshold ms or more is
// logged with its receiver's class and object name, the event type and, for a
// timer or a queued call, the class and name of the receiver's parent, at most
// burst lines per window; the lines past that are counted in the next one.
// With statistics on, every handler's count, total and longest time are kept
// by the same name, at most MaxHandlers names, for the profile's ranking.
//
// Off unless asked: ZERUS_SLOW_EVENTS=1 (50 ms) or a threshold in ms, or the
// setting debug/slowEventsMs (main.cpp).
namespace SlowEventLog {

struct Config {
    int thresholdMs = 50;
    int burst = 20;
    int windowMs = 10000;
};

// ZERUS_SLOW_EVENTS: unset or 0 is off, 1 the default threshold, more a
// threshold in ms. Returns the threshold, 0 when off.
int thresholdFromEnvironment();

// "SessionsWindow \"sessionsWindow\" Timer [in TeamWorkspace \"teamWorkspace\"]".
QString handlerName(const QObject *receiver, const QEvent *event);

// What names a receiver, read before its event: the receiver may be deleted by
// it, or be in its destructor already, so nothing touches it afterwards.
struct Receiver {
    explicit Receiver(const QObject *object);
    const char *type = "";
    QString name;
    const char *parentType = nullptr;
    QString parentName;
};
QString handlerName(const Receiver &receiver, QEvent::Type event);

struct Handler {
    QString name;
    qint64 count = 0;
    double totalMs = 0;
    double maxMs = 0;
};

class Meter {
public:
    using Log = std::function<void(const QString &line)>;
    Meter(Config config, Log log);

    // Delivers the event through deliver() and times it on the GUI thread.
    template <class Deliver>
    bool measure(QObject *receiver, QEvent *event, Deliver &&deliver)
    {
        if (!onGuiThread()) return deliver();
        const Receiver who(receiver);
        const QEvent::Type type = event->type();
        enter();
        const bool result = deliver();
        leave(who, type);
        return result;
    }

    void setStatistics(bool on) { m_statistics = on; }
    // A named part of the event being handled (Scope): its own time is its own.
    void enterScope() { if (onGuiThread()) enter(); }
    void leaveScope(const Receiver &owner, const char *what);
    // The handlers by total time, the most first, at most count of them.
    QList<Handler> top(int count) const;
    // The GUI thread's measured time since the last reset.
    double totalMs() const { return m_totalMs; }
    void reset();
    // Lines the rate bound held back since the last line.
    qint64 suppressed() const { return m_suppressed; }

    static constexpr int MaxHandlers = 2000;

    // The meter the application measures with, for Scope (null: none).
    static Meter *active();
    static void setActive(Meter *meter);

private:
    bool onGuiThread() const;
    void enter();
    void leave(const Receiver &receiver, QEvent::Type type);
    void finish(const QString &name, double ms);
    double pop();
    void record(const QString &name, double ms);
    void report(const QString &name, double ms);

    Config m_config;
    Log m_log;
    bool m_statistics = false;
    // One frame per nested delivery: its start and the time its children took.
    struct Frame {
        qint64 start;
        qint64 children;
    };
    QList<Frame> m_frames;
    QElapsedTimer m_clock;
    QHash<QString, Handler> m_handlers;
    double m_totalMs = 0;
    // The rate bound: lines written in the current window.
    qint64 m_windowStart = 0;
    int m_written = 0;
    qint64 m_suppressed = 0;
};

// Names a part of the current event that would otherwise rank under an
// anonymous receiver: a process reply delivered through ProcessRunner is "result
// for <owner>", not one "QObject MetaCall" for every reply in the tray.
class Scope {
public:
    Scope(const QObject *owner, const char *what) : m_owner(owner), m_what(what), m_meter(Meter::active())
    {
        if (m_meter) m_meter->enterScope();
    }
    ~Scope()
    {
        if (m_meter) m_meter->leaveScope(m_owner, m_what);
    }
    Scope(const Scope &) = delete;
    Scope &operator=(const Scope &) = delete;

private:
    Receiver m_owner;
    const char *m_what;
    Meter *m_meter;
};

}
