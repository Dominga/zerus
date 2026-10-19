#pragma once
#include <QByteArray>
#include <QCryptographicHash>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QUrl>

class QFile;
class QNetworkAccessManager;
class QNetworkReply;
class QProcess;

// WhiteWh/zerus only; upstream Zerus has no such updater. A portable Windows
// build of this fork installs the newer builds that .github/workflows/
// fork-windows.yml publishes as releases of the fork (docs/fork-updates.md).
// It downloads and verifies a newer build in the background, next to the
// installation; Restart to finish update then replaces the portable folder once
// Zerus has quit, keeping one copy to roll back to. Native agents and the session
// host run from %USERPROFILE%\.local and are never stopped.
class ForkUpdater : public QObject {
    Q_OBJECT
public:
    static ForkUpdater *instance();
    // The fork workflow's run number, or 0 for a build made elsewhere.
    static qint64 installedBuild();
    // Built with a feed and running from a portable folder.
    bool enabled() const { return m_enabled; }
    // A verified newer build waits next to the installation.
    bool ready() const { return m_readyBuild > 0; }
    qint64 readyBuild() const { return m_readyBuild; }
    QString status() const { return m_status; }
    void check();
    // Starts the helper that swaps the folders once this process has exited.
    // The caller quits Zerus when it returns true.
    bool startInstall();
signals:
    void changed();
    void buildReady(qint64 build);

private:
    explicit ForkUpdater(QObject *parent);
    void feedFinished(QNetworkReply *reply);
    void download();
    void downloadFinished(QNetworkReply *reply);
    void extracted();
    void finish(const QString &status);
    QString unpacked() const;

    bool m_enabled = false, m_busy = false;
    QString m_install, m_stage, m_status;
    QNetworkAccessManager *m_network = nullptr;
    QPointer<QFile> m_file;
    QPointer<QProcess> m_extract;
    QCryptographicHash m_hash{QCryptographicHash::Sha256};
    qint64 m_received = 0, m_readyBuild = 0;
    // The build the feed offers.
    qint64 m_build = 0, m_size = 0;
    QString m_commit;
    QUrl m_url;
    QByteArray m_sha256;
};
