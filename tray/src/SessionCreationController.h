#pragma once
#include "HgsClient.h"
#include <QJsonObject>
#include <QSet>
#include <QThread>
#include <functional>

// Owns preparation independently of its form and views. Every mutation crosses
// an ordered durable barrier. Recovery observes original requests before resuming.
class SessionCreationController : public QObject {
    Q_OBJECT
public:
    explicit SessionCreationController(const QString &hgs, QObject *parent = nullptr);
    ~SessionCreationController() override;
    QString submit(QJsonObject plan);
    QString admissionError() const;
    QJsonObject operation(const QString &id) const { return m_operations.value(id).toObject(); }
    QJsonObject operations() const { return m_operations; }
    bool owns(const QString &id) const { return m_operations.contains(id); }
    QString forLaunch(const QString &launch) const;
    void resume(const QString &id);
    void check(const QString &id);
    void stop(const QString &id);
    void hide(const QString &id);
    void show(const QString &id);
    void complete(const QString &id);
    void launchFinished(const QString &launch, bool okay, const QString &error);
    void ended(const QString &id, const QString &launch, const QString &error);
    static QString caption(const QJsonObject &operation);
    static QString details(const QJsonObject &operation);
    static bool canResume(const QJsonObject &operation);
    bool canEdit(const QString &id) const;
    void setWriteObserver(std::function<void()> observer) { m_writeObserver=std::move(observer); }
    // Runs on the UI thread against the current project catalog. Acquisition
    // evidence is supplied only after the original native receipt is verified.
    std::function<bool(QJsonObject &, const QJsonObject &, QString &)> resolvePlacement;
signals:
    void removed(const QString &id);
    void clockTick();
    void changed(const QString &id, const QJsonObject &operation);
    void launchRequested(const QString &id, const QJsonObject &operation);
private:
    QString take(const char *kind, quint64 request);
    void flight(const char *kind, quint64 request, const QString &id);
    void put(const QString &id, QJsonObject value);
    void stage(const QString &id, const QString &state, const QString &error = {});
    void persist(const QString &id, std::function<void()> after = {});
    void pump();
    void begin(const QString &id);
    void directory(const QString &id);
    void repositoryCheck(const QString &id);
    void checkout(const QString &id);
    void launch(const QString &id);
    void failed(const QString &id, const QString &error, bool uncertain = false);
    void prepared(const QString &id, const QJsonObject &result, bool recovered);
    void worktreeFailed(const QString &id, const QJsonObject &result, const QString &error);
    void release(const QString &id);
    void flush();
    HgsClient m_client;
    QString m_path, m_storageError;
    QJsonObject m_operations;
    QHash<QString, QString> m_flights;
    QSet<QString> m_running;
    QThread m_thread;
    QObject *m_writer;
    std::function<void()> m_writeObserver;
    QSet<QString> m_rememberChoices;
    QHash<QString,QString> m_retiredLaunches;
    QStringList m_retiredOrder;
    QString m_admissionMessage;
    bool m_loaded = false, m_quitting = false;
};
