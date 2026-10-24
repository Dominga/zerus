#pragma once
#include <QJsonArray>
#include <QJsonObject>
#include <QTimer>
#include <QWidget>
#include <functional>

class ActivityView;
class HgsClient;
class QHBoxLayout;
class QLabel;
class QPlainTextEdit;
class QPushButton;

// One session's conversation shown inside another
// view: the Team designer's chat (D1) and the coordinator (D3). It reads the
// session's inspection while shown, renders it with ActivityView and sends
// with the guarded send (the run and conversation it last read), never twice.
class TeamConversation : public QWidget {
public:
    explicit TeamConversation(HgsClient *client, QWidget *parent = nullptr);
    // Empty clears the view.
    void setSession(const QString &host, const QString &name);
    QString host() const { return m_host; }
    QString session() const { return m_name; }
    QJsonObject details() const { return m_details; }
    void setTheme(bool dark);
    void setPlaceholder(const QString &text);
    // Chips above the composer; a click sends the text as written.
    void setQuickAsks(const QStringList &asks);
    // Called after a send or a failed send; error is empty on success.
    std::function<void(const QString &error)> sent;
    static void setPollInterval(int milliseconds);
    // The guarded send of `text`; one at a time.
    void send(const QString &text);
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void poll();
    void received(const QString &host, const QString &name, const QJsonObject &data, const QString &archive);
    HgsClient *m_client;
    QString m_host, m_name, m_pendingText;
    qint64 m_cursor = 0;
    QJsonArray m_events;
    QJsonObject m_details;
    ActivityView *m_view;
    QWidget *m_asks;
    QStringList m_askTexts;
    QHBoxLayout *m_asksLayout;
    QPlainTextEdit *m_input;
    QPushButton *m_send;
    QLabel *m_status;
    QTimer m_timer;
    quint64 m_sendRequest = 0;
    bool m_dark = true;
};
