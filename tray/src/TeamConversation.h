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
class QuestionCard;

// WhiteWh fork: agent teams. One session's conversation shown inside another
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
    // U16: the session's pending questions and permission requests as the
    // session view's card above the composer, answered here with the
    // question's id, hash, run and conversation (the Team designer).
    void setQuestionsShown(bool shown);
    QuestionCard *questionCard() const { return m_question; }
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void poll();
    void received(const QString &host, const QString &name, const QJsonObject &data, const QString &archive);
    void renderQuestion(int navigation = 0);
    void answerQuestion(const QString &key, const QString &questionId, const QJsonArray &answers);
    QJsonObject actionableQuestion(const QString &key, const QString &questionId, bool skipping) const;
    void finishAnswer(quint64 request, bool ok, const QString &error = {}, bool uncertain = false, bool submitted = false);
    QString sessionKey() const { return m_name.isEmpty() ? QString() : m_host + '\n' + m_name; }
    HgsClient *m_client;
    QString m_host, m_name, m_pendingText;
    qint64 m_cursor = 0;
    QJsonArray m_events;
    QJsonObject m_details;
    ActivityView *m_view;
    QWidget *m_asks, *m_composerRow;
    QuestionCard *m_question = nullptr;
    // The answer in flight: its request, card key and question.
    quint64 m_answerRequest = 0;
    QString m_answerKey, m_answerQuestion, m_questionChoice;
    bool m_questionsShown = false, m_confirmingCache = false;
    QStringList m_askTexts;
    QHBoxLayout *m_asksLayout;
    QPlainTextEdit *m_input;
    QPushButton *m_send;
    QLabel *m_status;
    QTimer m_timer;
    quint64 m_sendRequest = 0;
    bool m_dark = true;
};
