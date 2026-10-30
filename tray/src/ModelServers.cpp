#include "ModelServers.h"
#include "HgsClient.h"
#include <QDateTime>
#include <QJsonDocument>
#include <QProcess>

namespace ModelServers {

namespace {
QStringList strings(const QJsonValue &value)
{
    QStringList out;
    for (const auto &item : value.toArray()) out << item.toString();
    return out;
}
}

QJsonObject Server::json() const
{
    QJsonObject out{{"name", name}, {"base_url", baseUrl}, {"kind", kind}};
    if (!ssh.isEmpty()) out.insert("ssh", ssh);
    if (upMinutes > 0) out.insert("up_minutes", upMinutes);
    const QList<std::pair<const char *, const QStringList *>> fields{{"status", &status}, {"up", &up}, {"down", &down}, {"logs", &logs}};
    for (const auto &[field, argv] : fields)
        if (!argv->isEmpty()) out.insert(QLatin1String(field), QJsonArray::fromStringList(*argv));
    return out;
}

QList<Server> parseList(const QJsonObject &list)
{
    QList<Server> out;
    for (const auto &value : list.value("servers").toArray()) {
        const auto row = value.toObject();
        Server server;
        server.name = row.value("name").toString(); server.baseUrl = row.value("base_url").toString();
        server.kind = row.value("kind").toString(); server.ssh = row.value("ssh").toString();
        server.hasKey = row.value("key").toBool(); server.upMinutes = row.value("up_minutes").toInt();
        const auto argv = row.value("argv").toObject();
        server.status = strings(argv.value("status")); server.up = strings(argv.value("up"));
        server.down = strings(argv.value("down")); server.logs = strings(argv.value("logs"));
        if (!server.name.isEmpty()) out << server;
    }
    return out;
}

Status parseStatus(const QJsonObject &status)
{
    Status out;
    out.known = true;
    out.readAt = QDateTime::currentMSecsSinceEpoch();
    if (status.value("status").toString() == "error") {
        out.error = status.value("error").toString(QStringLiteral("refused"));
        out.detail = status.value("detail").toString();
        return out;
    }
    out.source = status.value("source").toString();
    for (const auto &value : status.value("models").toArray()) {
        const auto row = value.toObject();
        if (const auto name = row.value("name").toString(); !name.isEmpty()) out.models << Model{name, row.value("state").toString().toLower()};
    }
    if (const auto memory = status.value("memory").toObject(); !memory.isEmpty()) {
        out.free = memory.value("free").toDouble(-1); out.total = memory.value("total").toDouble(-1);
    }
    out.raw = strings(status.value("raw"));
    return out;
}

QString stateLabel(const QString &state, const QString &source)
{
    if (state.isEmpty()) return source == "api" ? QObject::tr("Listed") : QObject::tr("Unknown");
    if (state == "listed") return QObject::tr("Listed");
    QString label = state;
    label.replace(QLatin1Char('_'), QLatin1Char(' '));
    return label.left(1).toUpper() + label.mid(1);
}

QString memoryText(double free, double total)
{
    return QObject::tr("%1 of %2 GB free").arg(free, 0, 'f', 1).arg(total, 0, 'f', 1);
}

QString argvText(const QStringList &argv)
{
    QStringList shown;
    for (const auto &arg : argv) shown << (arg.contains(QLatin1Char(' ')) || arg.isEmpty() ? QLatin1Char('"') + arg + QLatin1Char('"') : arg);
    return shown.join(QLatin1Char(' '));
}

QStringList argvFromText(const QString &text)
{
    return QProcess::splitCommand(text.trimmed());
}

Controller::Controller(HgsClient *client, QObject *parent) : QObject(parent), m_client(client)
{
    m_timer.setInterval(60000);
    connect(&m_timer, &QTimer::timeout, this, &Controller::refreshAll);
    connect(m_client, &HgsClient::modelServerReady, this, [this](quint64 request, const QJsonObject &result) {
        if (request == m_listRequest) {
            m_listRequest = 0; m_listed = true;
            m_servers = parseList(result);
            QSet<QString> names;
            for (const auto &server : std::as_const(m_servers)) names << server.name;
            for (auto it = m_status.begin(); it != m_status.end();) it = names.contains(it.key()) ? std::next(it) : m_status.erase(it);
            emit changed();
            // A page that shows reads a server it has not read yet, once.
            if (m_active) for (const auto &server : std::as_const(m_servers)) if (!m_status.value(server.name).known) refreshStatus(server.name);
            return;
        }
        if (const auto done = m_calls.take(request)) done(result, {});
    });
    connect(m_client, &HgsClient::modelServerFailed, this, [this](quint64 request, const QString &error) {
        if (request == m_listRequest) {
            // An hgs without model servers lists none: no tab, as before.
            m_listRequest = 0; m_listed = true; m_servers.clear(); m_status.clear();
            emit changed();
            return;
        }
        if (const auto done = m_calls.take(request)) done({}, error);
    });
}

const Server *Controller::server(const QString &name) const
{
    for (const auto &server : m_servers) if (server.name == name) return &server;
    return nullptr;
}

void Controller::refreshList()
{
    if (m_listRequest) return;
    m_listRequest = m_client->requestModelServer({QStringLiteral("list"), QStringLiteral("--json")}, {}, 30000);
}

void Controller::call(const QStringList &args, const QByteArray &input, int timeoutMs, const Reply &done)
{
    m_calls.insert(m_client->requestModelServer(args, input, timeoutMs), done);
}

void Controller::refreshStatus(const QString &name)
{
    if (!server(name) || m_reading.contains(name) || m_queue.contains(name)) return;
    m_queue << name;
    m_status[name].reading = true;
    emit changed();
    pump();
}

void Controller::refreshAll()
{
    for (const auto &server : std::as_const(m_servers)) refreshStatus(server.name);
}

void Controller::pump()
{
    while (m_reading.size() < MaxReads && !m_queue.isEmpty()) {
        const QString name = m_queue.takeFirst();
        if (!server(name)) continue;
        m_reading << name;
        call({QStringLiteral("status"), name, QStringLiteral("--json")}, {}, 60000, [this, name](const QJsonObject &result, const QString &error) {
            m_reading.remove(name);
            Status status = error.isEmpty() ? parseStatus(result) : Status{};
            if (!error.isEmpty()) { status.known = true; status.error = QStringLiteral("failed"); status.detail = error; status.readAt = QDateTime::currentMSecsSinceEpoch(); }
            if (server(name)) m_status.insert(name, status);
            emit changed();
            pump();
        });
    }
}

void Controller::setActive(bool active)
{
    if (m_active == active) return;
    m_active = active;
    if (!active) { m_timer.stop(); return; }
    m_timer.start();
    // The first look reads every server once; later ones follow the timer.
    for (const auto &server : std::as_const(m_servers)) if (!m_status.value(server.name).known) refreshStatus(server.name);
}

void Controller::up(const QString &server, const QString &model, const Reply &done)
{
    call({QStringLiteral("up"), server, model, QStringLiteral("--detach")}, {}, 60000, [this, server, done](const QJsonObject &result, const QString &error) {
        if (done) done(result, error);
        refreshStatus(server);
    });
}

void Controller::down(const QString &server, const QString &model, const Reply &done)
{
    call({QStringLiteral("down"), server, model, QStringLiteral("--yes")}, {}, 180000, [this, server, done](const QJsonObject &result, const QString &error) {
        if (done) done(result, error);
        refreshStatus(server);
    });
}

void Controller::logs(const QString &server, const QString &model, const Reply &done)
{
    QStringList args{QStringLiteral("logs"), server};
    if (!model.isEmpty()) args << model;
    args << QStringLiteral("--lines") << QStringLiteral("200");
    call(args, {}, 60000, done);
}

void Controller::save(const QJsonArray &servers, const Reply &done)
{
    const QJsonObject input{{"by", "human"}, {"servers", servers}};
    call({QStringLiteral("save"), QStringLiteral("--json")}, QJsonDocument(input).toJson(QJsonDocument::Compact), 30000,
         [this, done](const QJsonObject &result, const QString &error) {
             if (error.isEmpty() && result.value("status").toString() == "saved") {
                 m_servers = parseList(result);
                 m_listed = true;
                 emit changed();
             }
             if (done) done(result, error);
         });
}

void Controller::test(const QJsonObject &server, const Reply &done)
{
    call({QStringLiteral("test"), QStringLiteral("--json")}, QJsonDocument(server).toJson(QJsonDocument::Compact), 60000, done);
}

}   // namespace ModelServers
