#include "JsonParse.h"

#include <QCoreApplication>
#include <QJsonParseError>
#include <QMutex>
#include <QSet>
#include <QThread>

#include <atomic>

namespace JsonParse {
namespace {

std::atomic<int> s_violations{0};
QMutex s_mutex;
std::function<void(const char *, int)> s_handler;
QSet<QByteArray> s_reported;

bool fatalChecks()
{
    static const bool fatal = qEnvironmentVariable("ZERUS_GUI_THREAD_CHECKS") == QLatin1String("fatal");
    return fatal;
}

void violation(const char *file, int line, const char *what = "JSON parsed on the GUI thread")
{
    ++s_violations;
    QMutexLocker lock(&s_mutex);
    if (s_handler) { const auto handler = s_handler; lock.unlock(); handler(file, line); return; }
    if (fatalChecks()) qFatal("%s at %s:%d (JsonParse.h)", what, file, line);
    const QByteArray site = QByteArray(file) + ':' + QByteArray::number(line);
    if (s_reported.size() < 256 && !s_reported.contains(site)) {
        s_reported.insert(site);
        qWarning("%s at %s", what, site.constData());
    }
}

QJsonDocument decode(const QByteArray &bytes, QString *error)
{
    QJsonParseError parseError;
    QJsonDocument document = QJsonDocument::fromJson(bytes, &parseError);
    if (error) *error = parseError.error == QJsonParseError::NoError ? QString() : parseError.errorString();
    return document;
}

}

bool onGuiThread()
{
    const auto *application = QCoreApplication::instance();
    return application && QThread::currentThread() == application->thread() && application->inherits("QGuiApplication");
}

QJsonDocument parse(const QByteArray &bytes, QString *error, const char *file, int line)
{
    if (onGuiThread()) violation(file, line);
    return decode(bytes, error);
}

QJsonObject object(const QByteArray &bytes, QString *error, const char *file, int line)
{
    QString parseError; const auto document = parse(bytes, &parseError, file, line);
    if (parseError.isEmpty() && !document.isObject()) parseError = QStringLiteral("expected a JSON object");
    if (error) *error = parseError;
    return parseError.isEmpty() ? document.object() : QJsonObject();
}

QJsonArray array(const QByteArray &bytes, QString *error, const char *file, int line)
{
    QString parseError; const auto document = parse(bytes, &parseError, file, line);
    if (parseError.isEmpty() && !document.isArray()) parseError = QStringLiteral("expected a JSON array");
    if (error) *error = parseError;
    return parseError.isEmpty() ? document.array() : QJsonArray();
}

void reportUnprepared(const char *file, int line)
{
    if (onGuiThread()) violation(file, line, "JSON value not prepared for the GUI thread");
}

int violations() { return s_violations; }

void setViolationHandler(std::function<void(const char *, int)> handler)
{
    QMutexLocker lock(&s_mutex);
    s_handler = std::move(handler);
}

}
