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
    // #511 N10: what the feed's build is to the running one. The running
    // commit is never offered again; a numbered build takes a higher number;
    // an unnumbered build (made locally or experimental) has no order, so the
    // feed's build can only replace it, once the owner confirms.
    enum class Offer { Current, Newer, Replacement };
    static Offer offerFor(qint64 installed, const QString &runningCommit, qint64 build, const QString &commit);
    // The source_commit in a portable folder's build-info.json, or empty.
    static QString sourceCommit(const QString &folder);
    // Built with a feed and running from a portable folder.
    bool enabled() const { return m_enabled; }
    // A portable Windows folder of this fork: the owner can check the fork's
    // releases by hand (Settings > About), also from a build made locally,
    // which has no feed of its own and never checks by itself.
    bool manualAvailable() const { return m_portable; }
    // Checks the fork's latest release now; a newer build downloads and waits
    // for Restart to finish update, as an automatic check does.
    void checkNow();
    // A build the running unnumbered build can only be replaced with.
    qint64 offeredBuild() const { return m_offeredBuild; }
    QString offeredCommit() const { return m_offeredBuild > 0 ? m_commit : QString(); }
    QString runningCommit() const { return m_runningCommit; }
    // The owner confirmed the replacement: it downloads and waits for Restart.
    void confirmReplacement();
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
    void fetch();
    void feedFinished(QNetworkReply *reply);
    void download();
    void downloadFinished(QNetworkReply *reply);
    void extracted();
    void finish(const QString &status);
    QString unpacked() const;
    QString readyStatus() const;

    bool m_enabled = false, m_portable = false, m_busy = false;
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
    // build-info.json's commit of the running build, and the replacement offered.
    QString m_runningCommit;
    qint64 m_offeredBuild = 0;
    QByteArray m_sha256;
};
