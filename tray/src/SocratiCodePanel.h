#pragma once
#include "HgsClient.h"
#include <QDateTime>
#include <QDesktopServices>
#include <QHBoxLayout>
#include <QJsonObject>
#include <QLabel>
#include <QLocale>
#include <QMessageBox>
#include <QPushButton>
#include <QShowEvent>
#include <QTimer>
#include <QUrl>
#include <QVBoxLayout>

// WhiteWh fork: the SocratiCode index of a session's folder (hgs socraticode).
// It shows whether the plugin, Docker and the index are there and whether the
// folder is watched, and offers the one next step: install the plugin, get
// Docker, or index and watch the folder with a SocratiCode server that Zerus
// keeps running. Reads are on demand and polled only while work runs.
class SocratiCodePanel : public QWidget {
public:
    explicit SocratiCodePanel(HgsClient *client, QWidget *parent = nullptr) : QWidget(parent), m_client(client)
    {
        setObjectName("socratiCodePanel");
        auto *layout = new QVBoxLayout(this);
        layout->setContentsMargins(4, 6, 4, 2);
        layout->setSpacing(4);
        auto *row = new QHBoxLayout;
        row->setSpacing(6);
        auto *title = new QLabel(tr("SocratiCode"));
        title->setObjectName("detailsIdentityTitle");
        row->addWidget(title);
        m_state = new QLabel;
        m_state->setObjectName("socratiCodeState");
        m_state->setTextFormat(Qt::PlainText);
        row->addWidget(m_state, 1);
        m_action = new QPushButton;
        m_action->setObjectName("socratiCodeAction");
        m_action->setAutoDefault(false);
        row->addWidget(m_action);
        m_stop = new QPushButton(tr("Stop"));
        m_stop->setObjectName("socratiCodeStop");
        m_stop->setAutoDefault(false);
        m_stop->setToolTip(tr("Stop the SocratiCode server that Zerus started for this folder"));
        row->addWidget(m_stop);
        m_refresh = new QPushButton(tr("Refresh"));
        m_refresh->setObjectName("socratiCodeRefresh");
        m_refresh->setAutoDefault(false);
        row->addWidget(m_refresh);
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
        connect(m_client, &HgsClient::socratiCodeReady, this, [this](quint64 id, const QString &host, const QString &path, const QString &action, const QJsonObject &data) {
            if (id != m_request || host != m_host || path != m_path) return;
            m_request = 0;
            if (action == QLatin1String("status")) { m_data = data; m_error.clear(); }
            else if (action == QLatin1String("install") && !data["installed"].toBool()) m_error = tr("The plugin was not installed. Run: claude plugin install socraticode@socraticode");
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
        m_request = m_client->requestSocratiCode(m_host, action, m_path);
        render();
    }
    void act()
    {
        const auto step = m_action->property("step").toString();
        if (step == QLatin1String("docker")) {
            QDesktopServices::openUrl(QUrl(QStringLiteral("https://www.docker.com/products/docker-desktop/")));
            return;
        }
        if (step == QLatin1String("start")) {
            const bool fresh = m_data["index"].toObject()["state"].toString() == QLatin1String("none");
            if (fresh && QMessageBox::question(this, tr("Index this folder?"),
                    tr("SocratiCode reads every source file of %1 and keeps its index in a local Qdrant container. "
                       "The first index can take several minutes; Zerus then keeps watching the folder until you stop it.").arg(m_path))
                != QMessageBox::Yes) return;
        }
        request(step);
    }
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
        const bool busy = m_request != 0;
        m_refresh->setEnabled(!busy && !m_path.isEmpty());
        m_action->setEnabled(!busy);
        m_stop->setEnabled(!busy);
        setVisible(!m_path.isEmpty());
        const auto runner = m_data["runner"].toObject();
        const bool running = runner["running"].toBool();
        m_stop->setVisible(running);
        if (m_data.isEmpty()) {
            m_state->setText(busy ? tr("Checking…") : m_error.isEmpty() ? tr("Not checked") : tr("Unavailable"));
            m_details->setText(m_error);
            setAction({}, {});
            m_poll.stop();
            return;
        }
        const auto index = m_data["index"].toObject();
        const auto state = index["state"].toString();
        const bool watching = m_data["watching"].toBool();
        const bool qdrant = m_data["qdrant"].toObject()["reachable"].toBool();
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
        } else text = qdrant ? tr("Not indexed") : tr("Not indexed · Qdrant is not running");
        if (!m_data["plugin"].toObject()["installed"].toBool()) details << tr("The Claude Code plugin is not installed, so agents do not use this index.");
        if (!m_data["docker"].toBool() && !qdrant) details << tr("SocratiCode keeps its index in Qdrant, which runs in Docker. Install Docker Desktop and start it.");
        if (!m_data["node"].toBool()) details << tr("SocratiCode needs Node.js with npx on this computer.");
        const auto ollama = m_data["ollama"].toObject();
        if (ollama["reachable"].toBool() && !ollama["model_present"].toBool())
            details << tr("Ollama has no %1 yet; SocratiCode downloads it with the first index.").arg(ollama["model"].toString());
        if (runner["phase"].toString() == QLatin1String("failed") && !runner["error"].toString().isEmpty())
            details << runner["error"].toString().section('\n', 0, 2).trimmed();
        if (!m_error.isEmpty()) details << m_error;
        m_state->setText(text);
        m_details->setText(details.join('\n'));
        m_details->setVisible(!details.isEmpty());
        // The one next step.
        const bool local = m_host.isEmpty();
        if (!m_data["plugin"].toObject()["installed"].toBool()) setAction(QStringLiteral("install"), tr("Install plugin"), tr("claude plugin install socraticode@socraticode"));
        else if (!m_data["docker"].toBool() && !qdrant) {
            if (local) setAction(QStringLiteral("docker"), tr("Get Docker…"), tr("Open the Docker Desktop download page"));
            else setAction({}, {});
        } else if (!running && !watching) setAction(QStringLiteral("start"), state == QLatin1String("none") ? tr("Index and watch") : tr("Watch"),
                                                    tr("Run a SocratiCode server for this folder that indexes it and follows its changes"));
        else setAction({}, {});
        if (running || state == QLatin1String("indexing")) { if (!m_poll.isActive()) m_poll.start(); }
        else m_poll.stop();
    }
    HgsClient *m_client;
    QString m_host, m_path, m_error;
    QJsonObject m_data;
    quint64 m_request = 0;
    QLabel *m_state, *m_details;
    QPushButton *m_action, *m_stop, *m_refresh;
    QTimer m_poll;
};
