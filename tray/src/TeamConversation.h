#pragma once
#include <QHash>
#include <QJsonArray>
#include <QJsonObject>
#include <QPointer>
#include <QTextDocument>
#include <QTimer>
#include "HeldInputBar.h"
#include "HgsClient.h"
#include <QWidget>
#include <functional>

class ActivityView;
class HgsClient;
class QHBoxLayout;
class QLabel;
class QPlainTextEdit;
class QPushButton;
class QuestionCard;
class HeldInputBar;
class TerminalDraftBar;

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
    // U57: cards the owner answers (the lead's proposals), right above the
    // question card and the composer; the conversation takes ownership.
    void setCards(QWidget *cards);
    QuestionCard *questionCard() const { return m_question; }
    // U25 (spec 11.1): the Team designer's chat. Before the first message the
    // intro and the composer stand in the middle; then the messages and the
    // composer stay centred and grow until they fill the column, when the
    // composer keeps to the bottom. The composer is the session's: the field
    // grows (2-8 lines) with Send inside, and the row under it carries
    // Attach, Cache, the context, Copy session reference and the model.
    // `above` (proposals, the DRAFT bar) sits right above the composer.
    void useDesignerLayout(QWidget *intro, QWidget *above);
    // The model button of the designer's row; the page fills its menu.
    QPushButton *modelButton() const { return m_model; }
    // With no session, Send asks for one with the text; the text stays until
    // startFinished says whether it started.
    std::function<void(const QString &text)> startRequested;
    // Voice and video record in the designer's own session (Activity).
    std::function<void()> openSession;
    void startFinished(const QString &error);
    // U35: who takes the owner's messages, in "Queued until %1 takes your
    // previous message" ("the lead"); the agent's name when empty.
    void setAgentName(const QString &name);
    // U53: the account chip beside Attach (the designer's row: beside the
    // model). A team member's account changes only with swap-member: its
    // owner names why here and the menu only lists the accounts.
    void setAccountLocked(const QString &reason);
    class AccountSwitchChip *accountChip() const { return m_accountChip; }
    // Opens the session's terminal when a send is refused at a dialog or draft.
    std::function<void(const QString &host, const QString &name)> openTerminal;
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
private:
    void poll();
    void received(const QString &host, const QString &name, const QJsonObject &data, const QString &archive);
    void renderQuestion(int navigation = 0);
    void answerQuestion(const QString &key, const QString &questionId, const QJsonArray &answers);
    QJsonObject actionableQuestion(const QString &key, const QString &questionId, bool skipping) const;
    void finishAnswer(quint64 request, bool ok, const QString &error = {}, bool uncertain = false, bool submitted = false);
    void renderHeld();
    void updateAccountChip();
    void showRefusal(const QString &error, bool uncertain);
    QString sessionKey() const { return m_name.isEmpty() ? QString() : m_host + '\n' + m_name; }
    void fitDesigner();
    void fitInput();
    void updateSend();
    void showFiles();
    // Adds a file or a pasted image; a pasted one also puts its reference
    // at the caret, as the session's own composer does.
    bool addFile(const QString &path, bool reference);
    void addImage(const QImage &image);
    bool m_designer = false, m_starting = false;
    QWidget *m_intro = nullptr, *m_above = nullptr, *m_bottom = nullptr, *m_filesRow = nullptr;
    class QFrame *m_card = nullptr;
    class ComposerToolbar *m_row = nullptr;
    class AccountSwitchChip *m_accountChip = nullptr;
    QJsonArray m_accountProfiles;
    QString m_accountLock;
    QString m_accountHost = QStringLiteral("\x1f");   // the computer whose accounts were read; none yet
    quint64 m_accountsRequest = 0, m_switchRequest = 0;
    QPushButton *m_attach = nullptr, *m_voice = nullptr, *m_video = nullptr, *m_reference = nullptr, *m_model = nullptr;
    class ToolbarChip *m_cache = nullptr;
    QPushButton *m_context = nullptr;
    QList<MessageAttachment> m_files, m_sentFiles;
    QPointer<QTextDocument> m_watched;
    HgsClient *m_client;
    QString m_host, m_name, m_pendingText;
    qint64 m_cursor = 0;
    QJsonArray m_events;
    QJsonObject m_details;
    ActivityView *m_view;
    QWidget *m_asks, *m_composerRow;
    QuestionCard *m_question = nullptr;
    // U35: the messages hgs holds, and the one just queued until the next read.
    HeldInputBar *m_held = nullptr;
    TerminalDraftBar *m_terminalDraft = nullptr;
    QJsonArray m_justHeld;
    // #511 N5: the held messages this composer queued, until hgs settles them.
    HeldSent m_sentHeld;
    QString m_agentName;
    QPushButton *m_openTerminal = nullptr;
    QHBoxLayout *m_statusRow = nullptr;   // the status and Open terminal
    QHash<quint64, QString> m_cancels;
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
