#pragma once
#include <QHash>
#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QObject>
#include <QSet>
#include <QStringList>
#include <QTimer>
#include <functional>

class HgsClient;

// WhiteWh fork, U60 (#656): model servers - computers on the LAN, or this one,
// that serve models through an OpenAI- or Anthropic-compatible API. The
// configuration is this computer's own (`hgs model-server`, docs/reference.md
// "Model servers"); the desktop never reads or writes the private file and
// never sees a key. Nothing here retries or restarts anything on its own.
namespace ModelServers {

struct Server {
    QString name, baseUrl, kind, ssh;
    bool hasKey = false;
    int upMinutes = 0;
    QStringList status, up, down, logs;   // argv templates
    // Without control commands the server is read only: its models come from
    // the API's model list, with no Up, Down or Logs.
    bool readOnly() const { return status.isEmpty() && up.isEmpty() && down.isEmpty() && logs.isEmpty(); }
    QJsonObject json() const;   // as `save --json` takes it, without a key
};

struct Model { QString name, state; };

struct Status {
    bool known = false, reading = false;
    // unreachable, refused, timed_out, unauthorized, or failed (hgs itself)
    QString error, detail;
    QString source;                       // api or command
    QList<Model> models;
    double free = -1, total = -1;         // as the status command reports them
    QStringList raw;                      // lines neither JSON nor NAME: STATE
    qint64 readAt = 0;                    // ms since the epoch
    bool hasMemory() const { return free >= 0 && total > 0; }
};

QList<Server> parseList(const QJsonObject &list);
Status parseStatus(const QJsonObject &status);
// "Running", "Loading", "Stopped", "Listed" (from the API), else the state.
QString stateLabel(const QString &state, const QString &source);
// "38.2 of 48.0 GB free".
QString memoryText(double free, double total);
// The argv a field shows ("serve up {model}"), and back (QProcess::splitCommand).
QString argvText(const QStringList &argv);
QStringList argvFromText(const QString &text);

// The servers of this computer and their status. While the page shows, each
// server is read in the background every `interval` (60 s), at most two
// reads at a time and never two of one server; a read is bounded by hgs's own
// limits. Up, Down and Logs run once when asked.
class Controller : public QObject {
    Q_OBJECT
public:
    using Reply = std::function<void(const QJsonObject &result, const QString &error)>;
    explicit Controller(HgsClient *client, QObject *parent = nullptr);
    HgsClient *client() const { return m_client; }
    // Reads the list (`model-server list --json`); `changed` follows.
    void refreshList();
    bool listed() const { return m_listed; }
    bool available() const { return !m_servers.isEmpty(); }
    const QList<Server> &servers() const { return m_servers; }
    const Server *server(const QString &name) const;
    Status status(const QString &name) const { return m_status.value(name); }
    void refreshStatus(const QString &name);
    void refreshAll();
    // The page shows: background reads run; hidden, they stop.
    void setActive(bool active);
    bool active() const { return m_active; }
    void setInterval(int ms) { m_timer.setInterval(ms); }
    static constexpr int MaxReads = 2;
    // Up detaches (`up --detach`): the receipt names its run; the status is
    // read once when it returns. Down was confirmed by the caller (`--yes`).
    void up(const QString &server, const QString &model, const Reply &done);
    void down(const QString &server, const QString &model, const Reply &done);
    void logs(const QString &server, const QString &model, const Reply &done);
    // Settings: the whole list (`save --json`, by you) and one server's status
    // without saving it (`test --json`); the key goes only in the stdin JSON.
    void save(const QJsonArray &servers, const Reply &done);
    void test(const QJsonObject &server, const Reply &done);
signals:
    void changed();
private:
    void call(const QStringList &args, const QByteArray &input, int timeoutMs, const Reply &done);
    void pump();
    HgsClient *m_client;
    QList<Server> m_servers;
    QHash<QString, Status> m_status;
    QStringList m_queue;
    QSet<QString> m_reading;
    QHash<quint64, Reply> m_calls;
    QTimer m_timer;
    quint64 m_listRequest = 0;
    bool m_listed = false, m_active = false;
};

}   // namespace ModelServers
