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
class QJsonObject;
class QProcess;

// WhiteWh/zerus only; upstream Zerus has no such updater. A portable Windows
// build of this fork installs the newer builds that .github/workflows/
// fork-windows.yml publishes as releases of the fork (docs/fork-updates.md).
// It downloads and verifies a newer build in the background; Restart to finish
// update then installs it once Zerus has quit, keeping one copy to roll back to.
// The feed is read again while a build waits, and a newer build replaces it.
// A plain portable folder is swapped with its sibling copies; a versioned root
// (docs/fork-updates.md) keeps every build inside it and only re-points its
// `current` junction. Native agents and the session host run from
// %USERPROFILE%\.local and are never stopped.
class ForkUpdater : public QObject {
    Q_OBJECT
public:
    // Where the running build lives and where its updates go.
    struct Layout {
        // The folder of the running build, links resolved.
        QString install;
        // Versioned root: <root>\current is a junction to the running build, and
        // the builds, the staged update and the log all stay inside <root>.
        bool versioned = false;
        QString root, link;
        // Another build is current in this versioned root: this one stays put.
        bool superseded = false;
        // Where a newer build is downloaded and unpacked.
        QString stage;
    };
    static Layout layoutFor(const QString &executable);
    static ForkUpdater *instance();
    // The fork workflow's run number, or 0 for a build made elsewhere.
    static qint64 installedBuild();
    // Built with a feed and running from a portable folder.
    bool enabled() const { return m_enabled; }
    // Manual checks are available only when a verified feed is configured.
    bool manualAvailable() const { return m_enabled; }
    // Check the configured feed now.
    void checkNow();
    // A verified newer build waits next to the installation.
    bool ready() const { return m_readyBuild > 0; }
    qint64 readyBuild() const { return m_readyBuild; }
    QString status() const { return m_status; }
    void check();
    // Starts the helper that swaps the folders once this process has exited.
    // installStarted is emitted after any connector stop request completes.
    bool startInstall();
signals:
    void changed();
    void buildReady(qint64 build);
    void installStarted();

private:
    explicit ForkUpdater(QObject *parent);
    void fetch();
    void feedFinished(QNetworkReply *reply);
    void applyFeed(const QJsonObject &feed);
    void download();
    void downloadFinished(QNetworkReply *reply);
    void extracted();
    void applyExtracted(bool complete, const QString &output, const QJsonObject &info);
    void finish(const QString &status);
    QString unpacked() const;
    QString readyStatus() const;

    bool m_enabled = false, m_portable = false, m_busy = false, m_installing = false;
    Layout m_layout;
    QString m_install, m_stage, m_status;
    // Where a download goes: the stage, or beside it while a ready build waits
    // there, so that a newer build replaces it only once verified.
    QString m_download;
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
