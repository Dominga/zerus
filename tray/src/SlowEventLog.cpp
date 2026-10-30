#include "SlowEventLog.h"

#include <QCoreApplication>
#include <QEvent>
#include <QMetaEnum>
#include <QObject>
#include <QThread>

#include <algorithm>
#include <atomic>

namespace SlowEventLog {

int thresholdFromEnvironment()
{
    bool ok = false;
    const int value = qEnvironmentVariableIntValue("ZERUS_SLOW_EVENTS", &ok);
    if (!ok || value <= 0) return 0;
    return value == 1 ? Config().thresholdMs : value;
}

namespace {

QString named(const char *type, const QString &name)
{
    const QString text = QString::fromLatin1(type);
    return name.isEmpty() ? text : text + QStringLiteral(" \"") + name + QLatin1Char('"');
}

QString eventType(QEvent::Type type)
{
    static const QMetaEnum types = QMetaEnum::fromType<QEvent::Type>();
    const char *key = types.valueToKey(type);
    return key ? QString::fromLatin1(key) : QString::number(int(type));
}

}

Receiver::Receiver(const QObject *object)
{
    if (!object) return;
    type = object->metaObject()->className();
    name = object->objectName();
    if (const QObject *parent = object->parent()) {
        parentType = parent->metaObject()->className();
        parentName = parent->objectName();
    }
}

QString handlerName(const Receiver &receiver, QEvent::Type event)
{
    QString text = named(receiver.type, receiver.name) + QLatin1Char(' ') + eventType(event);
    // A timer or a queued call says little by itself: its owner names it.
    const bool anonymous = event == QEvent::Timer || event == QEvent::MetaCall || receiver.name.isEmpty();
    if (anonymous && receiver.parentType) text += QStringLiteral(" [in ") + named(receiver.parentType, receiver.parentName) + QLatin1Char(']');
    return text;
}

QString handlerName(const QObject *receiver, const QEvent *event)
{
    return handlerName(Receiver(receiver), event->type());
}

Meter::Meter(Config config, Log log) : m_config(config), m_log(std::move(log)) { m_clock.start(); }

bool Meter::onGuiThread() const
{
    const auto *app = QCoreApplication::instance();
    return app && QThread::currentThread() == app->thread();
}

void Meter::enter() { m_frames.append({m_clock.nsecsElapsed(), 0}); }

double Meter::pop()
{
    const Frame frame = m_frames.takeLast();
    const qint64 elapsed = m_clock.nsecsElapsed() - frame.start;
    // Its own time: what the events delivered inside it took is theirs.
    const double ms = double(std::max<qint64>(0, elapsed - frame.children)) / 1e6;
    if (!m_frames.isEmpty()) m_frames.last().children += elapsed;
    m_totalMs += ms;
    return ms;
}

void Meter::finish(const QString &name, double ms)
{
    if (m_statistics) record(name, ms);
    if (ms >= m_config.thresholdMs) report(name, ms);
}

void Meter::leave(const Receiver &receiver, QEvent::Type type)
{
    const double ms = pop();
    if (m_statistics || ms >= m_config.thresholdMs) finish(handlerName(receiver, type), ms);
}

void Meter::leaveScope(const Receiver &owner, const char *what)
{
    if (!onGuiThread() || m_frames.isEmpty()) return;
    const double ms = pop();
    if (m_statistics || ms >= m_config.thresholdMs)
        finish(QString::fromLatin1(what) + QStringLiteral(" for ") + named(owner.type, owner.name), ms);
}

namespace {
std::atomic<Meter *> s_active{nullptr};
}
Meter *Meter::active() { return s_active.load(); }
void Meter::setActive(Meter *meter) { s_active.store(meter); }

void Meter::record(const QString &name, double ms)
{
    auto found = m_handlers.find(name);
    if (found == m_handlers.end()) {
        if (m_handlers.size() >= MaxHandlers) return;
        found = m_handlers.insert(name, Handler{name});
    }
    ++found->count;
    found->totalMs += ms;
    found->maxMs = std::max(found->maxMs, ms);
}

void Meter::report(const QString &name, double ms)
{
    const qint64 now = m_clock.elapsed();
    if (now - m_windowStart >= m_config.windowMs) {
        m_windowStart = now;
        m_written = 0;
    }
    if (m_written >= m_config.burst) {
        ++m_suppressed;
        return;
    }
    ++m_written;
    QString line = QStringLiteral("Slow GUI event: %1 ms in %2").arg(qRound(ms)).arg(name);
    if (m_suppressed)
        line += QStringLiteral(" (%1 more slow event(s) were not logged)").arg(m_suppressed);
    m_suppressed = 0;
    if (m_log) m_log(line);
}

QList<Handler> Meter::top(int count) const
{
    QList<Handler> all = m_handlers.values();
    std::sort(all.begin(), all.end(), [](const Handler &a, const Handler &b) {
        return a.totalMs != b.totalMs ? a.totalMs > b.totalMs : a.name < b.name;
    });
    return all.mid(0, count);
}

void Meter::reset()
{
    m_handlers.clear();
    m_totalMs = 0;
}

}
