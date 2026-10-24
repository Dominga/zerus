#include "TeamConversation.h"
#include "ActivityView.h"
#include "HgsClient.h"
#include "TeamWidgets.h"
#include <QKeyEvent>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QSet>
#include <QTextBrowser>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
int pollInterval = 2000;

// Enter sends; Shift+Enter starts a new line.
class ComposerFilter : public QObject {
public:
    ComposerFilter(std::function<void()> send, QObject *parent) : QObject(parent), m_send(std::move(send)) {}
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        if (event->type() == QEvent::KeyPress) {
            const auto *key = static_cast<QKeyEvent *>(event);
            if ((key->key() == Qt::Key_Return || key->key() == Qt::Key_Enter) && !(key->modifiers() & Qt::ShiftModifier)) { m_send(); return true; }
        }
        return QObject::eventFilter(watched, event);
    }
private:
    std::function<void()> m_send;
};
}

void TeamConversation::setPollInterval(int milliseconds) { pollInterval = milliseconds; }

TeamConversation::TeamConversation(HgsClient *client, QWidget *parent) : QWidget(parent), m_client(client)
{
    setObjectName("teamConversation");
    auto *v = new QVBoxLayout(this); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(0);
    m_view = new ActivityView; m_view->setObjectName("teamConversationActivity");
    // The window's own conversation is the one named "activity".
    if (auto *text = m_view->findChild<QTextBrowser *>("activity")) text->setObjectName("teamConversationText");
    v->addWidget(m_view, 1);
    auto *bottom = new QWidget; bottom->setObjectName("teamConversationBottom");
    auto *bv = new QVBoxLayout(bottom); bv->setContentsMargins(10, 6, 10, 10); bv->setSpacing(6);
    m_asks = new QWidget; m_asksLayout = new QHBoxLayout(m_asks); m_asksLayout->setContentsMargins(0, 0, 0, 0); m_asksLayout->setSpacing(6);
    m_asks->hide();
    bv->addWidget(m_asks);
    m_status = new QLabel; m_status->setObjectName("teamConversationStatus"); m_status->setWordWrap(true); m_status->hide();
    bv->addWidget(m_status);
    auto *row = new QHBoxLayout; row->setSpacing(6);
    m_input = new QPlainTextEdit; m_input->setObjectName("teamConversationInput"); m_input->setFixedHeight(56);
    m_send = button(tr("Send"), "teamConversationSend", true);
    row->addWidget(m_input, 1); row->addWidget(m_send, 0, Qt::AlignBottom);
    bv->addLayout(row);
    v->addWidget(bottom);
    m_input->installEventFilter(new ComposerFilter([this] { send(m_input->toPlainText()); }, this));
    QObject::connect(m_send, &QPushButton::clicked, this, [this] { send(m_input->toPlainText()); });
    QObject::connect(m_client, &HgsClient::inspectionReady, this, [this](const QString &host, const QString &name, const QJsonObject &data, const QString &archive) {
        received(host, name, data, archive);
    });
    QObject::connect(m_client, &HgsClient::messageSent, this, [this](quint64 request, const QString &, const QString &, const QJsonObject &) {
        if (request != m_sendRequest) return;
        m_sendRequest = 0; m_send->setEnabled(true); m_status->hide();
        if (m_input->toPlainText() == m_pendingText) m_input->clear();
        if (sent) sent({});
        poll();
    });
    QObject::connect(m_client, &HgsClient::messageFailed, this, [this](quint64 request, const QString &, const QString &, const QString &error, bool uncertain) {
        if (request != m_sendRequest) return;
        m_sendRequest = 0; m_send->setEnabled(true);
        // An uncertain delivery may have arrived: the text stays, nothing is resent.
        m_status->setText(uncertain ? tr("Delivery unconfirmed — check the conversation before sending again. %1").arg(error) : error);
        m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).danger.name()));
        m_status->show();
        if (sent) sent(error.isEmpty() ? tr("Not sent") : error);
    });
    m_timer.setInterval(pollInterval);
    QObject::connect(&m_timer, &QTimer::timeout, this, [this] { poll(); });
    setTheme(true);
    setPlaceholder(tr("Message…"));
}

void TeamConversation::setSession(const QString &host, const QString &name)
{
    if (host == m_host && name == m_name) return;
    m_host = host; m_name = name; m_cursor = 0; m_events = {}; m_details = {};
    m_view->setSessionKey(name.isEmpty() ? QString() : host + '\n' + name);
    m_view->setTimeline({}, {}, {});
    m_input->setEnabled(!name.isEmpty()); m_send->setEnabled(!name.isEmpty() && !m_sendRequest);
    m_status->hide();
    if (isVisible()) poll();
}

void TeamConversation::setTheme(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    setStyleSheet(QStringLiteral("QWidget#teamConversationBottom{border-top:1px solid %1;} QPlainTextEdit#teamConversationInput{background:%2;color:%3;border:1px solid %1;border-radius:8px;padding:4px 6px;}")
                      .arg(c.border.name(), c.surface.name(), c.fg.name()));
    // After the sheet: a repolish would reset the Activity canvas palette.
    m_view->setTheme(dark);
}

void TeamConversation::setPlaceholder(const QString &text) { m_input->setPlaceholderText(text); }

void TeamConversation::setQuickAsks(const QStringList &asks)
{
    // Polls set the same asks again; rebuilding them would make them flicker.
    if (asks == m_askTexts && !m_asksLayout->isEmpty()) return;
    m_askTexts = asks;
    while (auto *item = m_asksLayout->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    for (const auto &ask : asks) {
        auto *chip = button(ask, "teamConversationAsk");
        QObject::connect(chip, &QPushButton::clicked, this, [this, ask] { send(ask); });
        m_asksLayout->addWidget(chip);
    }
    m_asksLayout->addStretch();
    m_asks->setVisible(!asks.isEmpty());
}

void TeamConversation::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    m_timer.start(pollInterval);
    poll();
}

void TeamConversation::hideEvent(QHideEvent *event)
{
    QWidget::hideEvent(event);
    m_timer.stop();
}

void TeamConversation::poll()
{
    if (m_name.isEmpty() || m_client->inspectionInFlight(m_host, m_name)) return;
    m_client->requestInspection(m_host, m_name, m_cursor, {}, false);
}

void TeamConversation::received(const QString &host, const QString &name, const QJsonObject &data, const QString &archive)
{
    if (host != m_host || name != m_name || !archive.isEmpty()) return;
    // Another conversation of the session starts its own window of events.
    if (!m_details.isEmpty() && m_details.value("conversation_id") != data.value("conversation_id")
        && !data.value("cleared_conversations").toArray().contains(m_details.value("conversation_id"))) {
        m_events = data.value("events").toArray(); m_cursor = 0; m_details = data;
        m_view->setTimeline(m_details, m_events, {});
        poll();
        return;
    }
    m_details = data;
    if (data.value("replace_events").toBool()) m_events = data.value("events").toArray();
    else {
        QSet<qint64> known;
        for (const auto &event : m_events) known.insert(event.toObject().value("seq").toInteger());
        for (const auto &event : data.value("events").toArray()) {
            const auto seq = event.toObject().value("seq").toInteger();
            if (seq <= m_cursor || known.contains(seq)) continue;
            known.insert(seq); m_events.append(event);
        }
    }
    while (m_events.size() > 500) m_events.removeFirst();
    m_cursor = data.value("cursor").toInteger();
    m_view->setTimeline(m_details, m_events, {});
}

void TeamConversation::send(const QString &raw)
{
    const QString text = raw.trimmed();
    if (text.isEmpty() || m_name.isEmpty() || m_sendRequest) return;
    if (m_details.value("run_id").toString().isEmpty()) {
        m_status->setText(tr("The session is not read yet; try again in a moment."));
        m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).warn.name()));
        m_status->show();
        return;
    }
    m_sendRequest = m_client->requestSendMessage(m_host, m_name, text, {}, m_details.value("run_id").toString(), m_details.value("conversation_id").toString());
    if (!m_sendRequest) return;
    m_send->setEnabled(false);
    m_pendingText = raw;
}
