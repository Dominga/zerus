#pragma once
#include "Settings.h"
#include "HgsClient.h"
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonObject>
#include <QLabel>
#include <QLocale>
#include <QMessageBox>
#include <QPushButton>
#include <QSettings>
#include <QShowEvent>
#include <QTimer>
#include <QVBoxLayout>
#include <utility>

// WhiteWh fork: the SocratiCode index of a session's folder (hgs socraticode).
// It shows whether the plugin, Qdrant and the index are there and whether the
// folder is watched, and offers the one next step: install the plugin, install
// the native Qdrant, or start: bring up Qdrant and Ollama, then index and watch
// the folder with a SocratiCode server that Zerus keeps running. Without
// Docker, "Connect agents" points the folder's agents at the same Qdrant.
// Reads are on demand and polled only while work runs.
class SocratiCodePanel : public QWidget {
public:
    explicit SocratiCodePanel(HgsClient *client, QWidget *parent = nullptr) : QWidget(parent), m_client(client)
    {
        setObjectName("socratiCodePanel");
        // A skin may give the panel a surface of its own.
        setAttribute(Qt::WA_StyledBackground, true);
        auto *layout = new QVBoxLayout(this);
        layout->setContentsMargins(8, 6, 8, 6);
        layout->setSpacing(4);
        // The title and the state, then the buttons on a row of their own: the
        // inspector can be narrow, and buttons must keep their whole labels.
        auto *head = new QHBoxLayout;
        head->setSpacing(6);
        auto *title = new QLabel(tr("SocratiCode"));
        title->setObjectName("inspectorPanelTitle");
        title->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Preferred);
        head->addWidget(title, 0, Qt::AlignTop);
        m_state = new QLabel;
        m_state->setObjectName("socratiCodeState");
        m_state->setTextFormat(Qt::PlainText);
        m_state->setWordWrap(true);
        m_state->setMinimumWidth(0);
        m_state->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        head->addWidget(m_state, 1);
        m_refresh = new QPushButton(QStringLiteral("↻"));
        m_refresh->setObjectName("socratiCodeRefresh");
        m_refresh->setAutoDefault(false);
        m_refresh->setFixedSize(24, 24);
        m_refresh->setToolTip(tr("Refresh")); m_refresh->setAccessibleName(tr("Refresh SocratiCode status"));
        head->addWidget(m_refresh, 0, Qt::AlignTop);
        layout->addLayout(head);
        auto *row = new QHBoxLayout;
        row->setSpacing(6);
        m_action = new QPushButton;
        m_action->setObjectName("socratiCodeAction");
        m_action->setAutoDefault(false);
        row->addWidget(m_action);
        m_connect = new QPushButton(tr("Connect agents"));
        m_connect->setObjectName("socratiCodeConnect");
        m_connect->setAutoDefault(false);
        m_connect->setToolTip(tr("Let this folder's agents use the same Qdrant and Ollama (.claude/settings.local.json)"));
        row->addWidget(m_connect);
        m_stop = new QPushButton(tr("Stop"));
        m_stop->setObjectName("socratiCodeStop");
        m_stop->setAutoDefault(false);
        m_stop->setToolTip(tr("Stop the SocratiCode server that Zerus started for this folder"));
        row->addWidget(m_stop);
        // At most two buttons show at once (start or stop, and connect); none
        // shrinks below its label.
        for (auto *button : {m_action, m_connect, m_stop}) button->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
        row->addStretch();
        layout->addLayout(row);
        m_details = new QLabel;
        m_details->setObjectName("socratiCodeDetails");
        m_details->setTextFormat(Qt::PlainText);
        m_details->setWordWrap(true);
        m_details->setTextInteractionFlags(Qt::TextSelectableByMouse);
        layout->addWidget(m_details);
        m_poll.setInterval(10000);
        connect(&m_poll, &QTimer::timeout, this, [this] { request(QStringLiteral("status")); });
        connect(m_refresh, &QPushButton::clicked, this, [this] { request(QStringLiteral("status")); });
        connect(m_action, &QPushButton::clicked, this, &SocratiCodePanel::act);
        connect(m_stop, &QPushButton::clicked, this, [this] { request(QStringLiteral("stop")); });
        connect(m_connect, &QPushButton::clicked, this, [this] {
            if (QMessageBox::question(this, tr("Connect this folder's agents?"),
                    tr("Agents use SocratiCode's Docker Qdrant unless the folder says otherwise. Zerus adds QDRANT_MODE, QDRANT_URL "
                       "and the Ollama settings to %1, which stays out of Git; variables the folder already sets are kept. "
                       "Sessions started after this use the index.").arg(m_path + QStringLiteral("/.claude/settings.local.json")))
                == QMessageBox::Yes) request(QStringLiteral("connect"));
        });
        connect(m_client, &HgsClient::socratiCodeReady, this, [this](quint64 id, const QString &host, const QString &path, const QString &action, const QJsonObject &data) {
            if (id != m_request || host != m_host || path != m_path) return;
            m_request = 0;
            if (action == QLatin1String("status")) { m_data = data; m_error.clear(); }
            else if (action == QLatin1String("install") && !data["installed"].toBool()) m_error = tr("The plugin was not installed. Run: claude plugin install socraticode@socraticode");
            else if (action == QLatin1String("install-qdrant") && !data["installed"].toBool()) m_error = tr("Qdrant was not installed.");
            render();
            if (action != QLatin1String("status")) QTimer::singleShot(action == QLatin1String("start") ? 1500 : 500, this, [this] { request(QStringLiteral("status")); });
        });
        connect(m_client, &HgsClient::socratiCodeFailed, this, [this](quint64 id, const QString &host, const QString &path, const QString &, const QString &error) {
            if (id != m_request || host != m_host || path != m_path) return;
            m_request = 0;
            m_error = error;
            render();
        });
        render();
    }
    void setContext(const QString &host, const QString &path)
    {
        if (host == m_host && path == m_path) return;
        m_host = host; m_path = path; m_request = 0; m_data = {}; m_error.clear();
        render();
        if (isVisible()) request(QStringLiteral("status"));
    }

protected:
    void showEvent(QShowEvent *event) override
    {
        QWidget::showEvent(event);
        if (m_data.isEmpty() && !m_request) request(QStringLiteral("status"));
    }

private:
    void request(const QString &action)
    {
        if (m_path.isEmpty() || m_request) return;
        m_request = m_client->requestSocratiCode(m_host, action, m_path, nativeQdrant());
        render();
    }
    void act()
    {
        const auto step = m_action->property("step").toString();
        if (step == QLatin1String("install-qdrant")
            && QMessageBox::question(this, tr("Install Qdrant?"),
                   tr("Zerus downloads Qdrant %1 for Windows (about 30 MB) from its GitHub release, checks the archive and qdrant.exe "
                      "against pinned SHA-256 hashes, and puts it in %LOCALAPPDATA%\\Programs\\qdrant. Its data stays in "
                      "%LOCALAPPDATA%\\qdrant, and it listens only on 127.0.0.1:6333.").arg(std::as_const(m_data)["qdrant"].toObject()["native"].toObject()["version"].toString()))
                   != QMessageBox::Yes) return;
        if (step == QLatin1String("start")) {
            const bool fresh = std::as_const(m_data)["index"].toObject()["state"].toString() == QLatin1String("none");
            if (fresh && QMessageBox::question(this, tr("Index this folder?"),
                    tr("SocratiCode reads every source file of %1 and keeps its index in a local Qdrant container. "
                       "The first index can take several minutes; Zerus then keeps watching the folder until you stop it.").arg(m_path))
                != QMessageBox::Yes) return;
        }
        request(step);
    }
public:
    // Settings > Sessions: SocratiCode on a native Qdrant instead of its Docker one.
    static bool nativeQdrant() { return Settings::value(QStringLiteral("socraticode/nativeQdrant"), false).toBool(); }

private:
    static QString ago(qint64 seconds)
    {
        if (seconds < 90) return tr("just now");
        if (seconds < 5400) return tr("%1 min ago").arg(seconds / 60);
        if (seconds < 129600) return tr("%1 h ago").arg(seconds / 3600);
        return tr("%1 days ago").arg(seconds / 86400);
    }
    void setAction(const QString &step, const QString &text, const QString &tip = {})
    {
        m_action->setProperty("step", step);
        m_action->setText(text);
        m_action->setToolTip(tip);
        m_action->setVisible(!step.isEmpty());
    }
    void render()
    {
        // A read-only copy: operator[] of a writable object adds what it looks up.
        const QJsonObject data = m_data;
        const bool busy = m_request != 0;
        m_refresh->setEnabled(!busy && !m_path.isEmpty());
        m_action->setEnabled(!busy);
        m_stop->setEnabled(!busy);
        m_connect->setEnabled(!busy);
        setVisible(!m_path.isEmpty());
        const auto runner = data["runner"].toObject();
        const bool running = runner["running"].toBool();
        m_stop->setVisible(running);
        if (data.isEmpty()) {
            m_state->setText(busy ? tr("Checking…") : m_error.isEmpty() ? tr("Not checked") : tr("Unavailable"));
            m_details->setText(m_error);
            setAction({}, {});
            m_connect->hide();
            m_poll.stop();
            return;
        }
        const auto index = data["index"].toObject();
        const auto state = index["state"].toString();
        const bool watching = data["watching"].toBool();
        const auto store = data["qdrant"].toObject();
        const bool qdrant = store["reachable"].toBool();
        const auto mode = store["mode"].toString();
        const bool native = store["native"].toObject()["installed"].toBool();
        // Zerus can bring up a Qdrant on this computer: the native one, or the
        // one the folder's settings name here.
        const bool startable = native && (mode == QLatin1String("native") || (mode == QLatin1String("external") && store["url"].toString().startsWith(QLatin1String("http://127.0.0.1"))));
        QStringList details;
        QString text;
        if (state == QLatin1String("indexed")) {
            text = tr("Indexed · %1 chunks").arg(QLocale().toString(index["chunks"].toInteger()));
            if (index["files_indexed"].toInteger() > 0) text += tr(" · %1 files").arg(QLocale().toString(index["files_indexed"].toInteger()));
            const auto at = QDateTime::fromString(index["last_indexed_at"].toString(), Qt::ISODateWithMs);
            if (at.isValid()) text += QStringLiteral(" · ") + ago(at.secsTo(QDateTime::currentDateTimeUtc()));
            text += watching ? tr(" · watching") : tr(" · not watched");
        } else if (state == QLatin1String("indexing") || runner["phase"].toString() == QLatin1String("indexing")) {
            text = tr("Indexing…");
            if (index["files_total"].toInteger() > 0)
                text += tr(" %1 of %2 files").arg(index["files_indexed"].toInteger()).arg(index["files_total"].toInteger());
        } else text = tr("Not indexed");
        if (!qdrant) text += startable ? tr(" · Qdrant is not running") : tr(" · no Qdrant");
        if (!data["plugin"].toObject()["installed"].toBool()) details << tr("The Claude Code plugin is not installed, so agents do not use this index.");
        if (mode == QLatin1String("managed") && !data["docker"].toBool())
            details << tr("SocratiCode keeps its index in a Qdrant that runs in Docker. Install Docker, or turn on a native Qdrant in Settings > Sessions.");
        if (mode == QLatin1String("native") && !native && !qdrant)
            details << (store["native"].toObject()["installable"].toBool() ? tr("The native Qdrant is not installed yet.")
                                                                         : tr("Install qdrant on PATH for a native Qdrant."));
        if (!store["agents"].toBool() && mode == QLatin1String("native"))
            details << tr("Zerus uses the native Qdrant at %1; this folder's agents still look for SocratiCode's Docker one.").arg(store["url"].toString());
        if (!data["node"].toBool()) details << tr("SocratiCode needs Node.js with npx on this computer.");
        const auto ollama = data["ollama"].toObject();
        if (ollama["reachable"].toBool() && !ollama["model_present"].toBool())
            details << tr("Ollama has no %1 yet; SocratiCode downloads it with the first index.").arg(ollama["model"].toString());
        if (runner["phase"].toString() == QLatin1String("failed") && !runner["error"].toString().isEmpty())
            details << runner["error"].toString().section('\n', 0, 2).trimmed();
        if (!m_error.isEmpty()) details << m_error;
        m_state->setText(text);
        m_details->setText(details.join('\n'));
        m_details->setVisible(!details.isEmpty());
        // The one next step.
        const bool needsQdrant = !qdrant && !startable && !(mode == QLatin1String("managed") && data["docker"].toBool());
        if (!data["plugin"].toObject()["installed"].toBool()) setAction(QStringLiteral("install"), tr("Install plugin"), tr("claude plugin install socraticode@socraticode"));
        else if (needsQdrant && mode == QLatin1String("native") && store["native"].toObject()["installable"].toBool())
            setAction(QStringLiteral("install-qdrant"), tr("Install Qdrant…"), tr("Download the pinned native Qdrant for SocratiCode"));
        else if (needsQdrant) setAction({}, {});
        else if (!running && !watching) setAction(QStringLiteral("start"), state == QLatin1String("none") ? tr("Index and watch") : qdrant ? tr("Watch") : tr("Start"),
                                                    tr("Bring up Qdrant and Ollama when they are down, then run a SocratiCode server that keeps this folder's index current"));
        else setAction({}, {});
        m_connect->setVisible(mode == QLatin1String("native") && !store["agents"].toBool());
        if (running || state == QLatin1String("indexing")) { if (!m_poll.isActive()) m_poll.start(); }
        else m_poll.stop();
    }
    HgsClient *m_client;
    QString m_host, m_path, m_error;
    QJsonObject m_data;
    quint64 m_request = 0;
    QLabel *m_state, *m_details;
    QPushButton *m_action, *m_connect, *m_stop, *m_refresh;
    QTimer m_poll;
};
