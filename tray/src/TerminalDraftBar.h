#pragma once
#include "HgsClient.h"
#include <QColor>
#include <QElapsedTimer>
#include <QFontMetrics>
#include <QFrame>
#include <QHBoxLayout>
#include <QJsonObject>
#include <QLabel>
#include <QPushButton>
#include <functional>

// WhiteWh fork (owner, 2026-10-28): text left in the agent's own Terminal input
// blocked every send from Activity and the team windows, with no way out but
// Terminal. Above a composer this bar shows that draft with Send it (Enter in
// Terminal, only while the draft is exactly as shown) and Open Terminal. Zerus
// never types into, edits or clears the draft.
class TerminalDraftBar : public QFrame {
public:
    explicit TerminalDraftBar(HgsClient *client, QWidget *parent = nullptr) : QFrame(parent), m_client(client)
    {
        setObjectName("terminalDraftBar");
        auto *h = new QHBoxLayout(this); h->setContentsMargins(10, 6, 10, 6); h->setSpacing(8);
        m_label = new QLabel; m_label->setObjectName("terminalDraftText"); m_label->setWordWrap(true);
        m_label->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        m_send = new QPushButton(tr("Send it")); m_send->setObjectName("terminalDraftSend"); m_send->setAutoDefault(false);
        m_send->setToolTip(tr("Press Enter in Terminal: the agent gets this text as your message"));
        m_open = new QPushButton(tr("Open Terminal")); m_open->setObjectName("terminalDraftOpen"); m_open->setAutoDefault(false);
        h->addWidget(m_label, 1); h->addWidget(m_send, 0, Qt::AlignVCenter); h->addWidget(m_open, 0, Qt::AlignVCenter);
        QObject::connect(m_open, &QPushButton::clicked, this, [this] { if (openTerminal) openTerminal(); });
        QObject::connect(m_send, &QPushButton::clicked, this, [this] {
            if (m_hash.isEmpty() || m_request) return;
            m_submitting = true; m_send->setEnabled(false); m_send->setText(tr("Sending…"));
            m_request = m_client->requestTerminalDraft(m_host, m_name, m_run, m_conversation, m_hash);
        });
        QObject::connect(m_client, &HgsClient::terminalDraftReady, this, [this](quint64 request, const QJsonObject &result) {
            if (request != m_request) return;
            m_request = 0;
            if (std::exchange(m_submitting, false)) {
                clear();
                if (submitted) submitted();
                return;
            }
            const QString draft = result.value("draft").toString();
            if (draft.isEmpty()) { clear(); return; }
            m_hash = result.value("draft_hash").toString();
            show(draft, {});
        });
        QObject::connect(m_client, &HgsClient::terminalDraftFailed, this, [this](quint64 request, const QString &error) {
            if (request != m_request) return;
            m_request = 0;
            // A refused Enter shows why; a failed read keeps the bar as it was.
            if (std::exchange(m_submitting, false)) { m_hash.clear(); show(m_draft, error); check(); }
        });
        hide();
    }
    std::function<void()> openTerminal;
    // After Enter reached Terminal: the host reads the session again.
    std::function<void()> submitted;
    // The exact run; another session or run forgets the draft shown.
    void setSession(const QString &host, const QString &name, const QString &run, const QString &conversation)
    {
        if (host == m_host && name == m_name && run == m_run && conversation == m_conversation) return;
        m_host = host; m_name = name; m_run = run; m_conversation = conversation;
        m_request = 0; m_submitting = false; m_checked.invalidate();
        clear();
    }
    // Reads the draft: at once after a send was refused for it, or at most
    // every few seconds while messages wait for an idle agent (`waiting`).
    void check(bool waiting = false)
    {
        if (m_name.isEmpty() || m_run.isEmpty() || m_request) return;
        if (waiting && m_checked.isValid() && m_checked.elapsed() < 4000) return;
        m_checked.start();
        m_request = m_client->requestTerminalDraft(m_host, m_name, m_run, m_conversation);
    }
    QString draft() const { return m_draft; }
    void setColors(const QColor &text, const QColor &muted, const QColor &border, const QColor &background)
    {
        m_text = text; m_muted = muted; m_border = border; m_background = background;
        applyColors();
    }

private:
    void show(const QString &draft, const QString &error)
    {
        m_draft = draft;
        const QString preview = QFontMetrics(m_label->font()).elidedText(draft.simplified(), Qt::ElideRight, 360);
        m_label->setText(error.isEmpty() ? tr("Terminal has unsent text: “%1”").arg(preview) : error);
        m_label->setToolTip(draft);
        m_send->setText(tr("Send it")); m_send->setEnabled(!m_hash.isEmpty());
        setVisible(true);
        applyColors();
    }
    void clear() { m_draft.clear(); m_hash.clear(); m_label->clear(); m_send->setText(tr("Send it")); setVisible(false); }
    void applyColors()
    {
        if (!m_text.isValid()) return;
        setStyleSheet(QStringLiteral(
            "QFrame#terminalDraftBar{background:%4;border-top:1px solid %3;}"
            "QLabel#terminalDraftText{color:%1;font-size:12px;}"
            "QPushButton{color:%1;background:transparent;border:1px solid %3;border-radius:6px;padding:2px 10px;font-size:12px;}"
            "QPushButton:disabled{color:%2;}")
            .arg(m_text.name(), m_muted.name(), m_border.name(), m_background.name()));
    }
    HgsClient *m_client;
    QLabel *m_label;
    QPushButton *m_send, *m_open;
    QString m_host, m_name, m_run, m_conversation, m_draft, m_hash;
    quint64 m_request = 0;
    bool m_submitting = false;
    QElapsedTimer m_checked;
    QColor m_text, m_muted, m_border, m_background;
};
