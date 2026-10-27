#include "TeamConversation.h"
#include "ActivityView.h"
#include "CacheStatus.h"
#include "HeldInputBar.h"
#include "HgsClient.h"
#include "QuestionCard.h"
#include "TeamWidgets.h"
#include <QKeyEvent>
#include <QMessageBox>
#include <QScopedValueRollback>
#include <algorithm>
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
    // U16: a pending question takes the composer's place, as in the session view.
    m_question = new QuestionCard; m_question->setObjectName("teamConversationQuestion"); m_question->hide();
    v->addWidget(m_question);
    // U35: the owner's messages hgs holds until the agent takes the previous one.
    m_held = new HeldInputBar; m_held->setObjectName("teamConversationHeld");
    v->addWidget(m_held);
    m_held->cancel = [this](const QString &id, const QString &text) {
        m_cancels.insert(m_client->requestCancelHeld(m_host, m_name, id), text);
    };
    auto *bottom = new QWidget; bottom->setObjectName("teamConversationBottom");
    auto *bv = new QVBoxLayout(bottom); bv->setContentsMargins(10, 6, 10, 10); bv->setSpacing(6);
    m_asks = new QWidget; m_asksLayout = new QHBoxLayout(m_asks); m_asksLayout->setContentsMargins(0, 0, 0, 0); m_asksLayout->setSpacing(6);
    m_asks->hide();
    bv->addWidget(m_asks);
    m_status = new QLabel; m_status->setObjectName("teamConversationStatus"); m_status->setWordWrap(true); m_status->hide();
    // A dialog or draft in the agent's terminal: the owner finishes it there.
    m_openTerminal = button(tr("Open terminal"), "teamConversationOpenTerminal"); m_openTerminal->hide();
    QObject::connect(m_openTerminal, &QPushButton::clicked, this, [this] { if (openTerminal) openTerminal(m_host, m_name); });
    auto *statusRow = new QHBoxLayout; statusRow->setSpacing(8);
    statusRow->addWidget(m_status, 1); statusRow->addWidget(m_openTerminal, 0, Qt::AlignTop);
    bv->addLayout(statusRow);
    m_composerRow = new QWidget;
    auto *row = new QHBoxLayout(m_composerRow); row->setContentsMargins(0, 0, 0, 0); row->setSpacing(6);
    m_input = new QPlainTextEdit; m_input->setObjectName("teamConversationInput"); m_input->setFixedHeight(56);
    m_send = button(tr("Send"), "teamConversationSend", true);
    row->addWidget(m_input, 1); row->addWidget(m_send, 0, Qt::AlignBottom);
    bv->addWidget(m_composerRow);
    v->addWidget(bottom);
    m_input->installEventFilter(new ComposerFilter([this] { send(m_input->toPlainText()); }, this));
    QObject::connect(m_send, &QPushButton::clicked, this, [this] { send(m_input->toPlainText()); });
    QObject::connect(m_client, &HgsClient::inspectionReady, this, [this](const QString &host, const QString &name, const QJsonObject &data, const QString &archive) {
        received(host, name, data, archive);
    });
    QObject::connect(m_client, &HgsClient::messageSent, this, [this](quint64 request, const QString &, const QString &, const QJsonObject &receipt) {
        if (request != m_sendRequest) return;
        m_sendRequest = 0; m_send->setEnabled(true); m_status->hide(); m_openTerminal->hide();
        if (m_input->toPlainText() == m_pendingText) m_input->clear();
        // Held by hgs: its line shows at once, then as the next reads list it.
        if (receipt.value("status") == "queued") {
            m_justHeld.append(QJsonObject{{"request_id", receipt.value("request_id")}, {"text", receipt.value("text")}});
            renderHeld();
        }
        if (sent) sent({});
        poll();
    });
    QObject::connect(m_client, &HgsClient::messageFailed, this, [this](quint64 request, const QString &, const QString &, const QString &error, bool uncertain) {
        if (request != m_sendRequest) return;
        m_sendRequest = 0; m_send->setEnabled(true);
        showRefusal(error, uncertain);
        if (sent) sent(error.isEmpty() ? tr("Not sent") : error);
    });
    QObject::connect(m_client, &HgsClient::heldCancelFinished, this, [this](quint64 request, const QString &, const QString &, bool ok, const QJsonObject &receipt, const QString &error) {
        if (!m_cancels.contains(request)) return;
        const QString text = m_cancels.take(request);
        if (!ok) {
            m_held->cancelFinished(receipt.value("request_id").toString());
            m_status->setText(error); m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).danger.name())); m_status->show();
            poll(); return;
        }
        // The cancelled text comes back to an empty composer.
        if (m_input->toPlainText().trimmed().isEmpty()) m_input->setPlainText(text);
        for (int i = int(m_justHeld.size()); i-- > 0;)
            if (m_justHeld[i].toObject().value("request_id") == receipt.value("request_id")) m_justHeld.removeAt(i);
        renderHeld(); poll();
    });
    QObject::connect(m_question, &QuestionCard::answerRequested, this, [this](const QString &key, const QString &id, const QJsonArray &answers) {
        answerQuestion(key, id, answers);
    });
    QObject::connect(m_question, &QuestionCard::queueNavigationRequested, this, [this](int direction) { renderQuestion(direction); });
    QObject::connect(m_question, &QuestionCard::draftRestored, this, [this] { renderQuestion(); });
    QObject::connect(m_client, &HgsClient::questionAnswered, this, [this](quint64 request) { finishAnswer(request, true); });
    QObject::connect(m_client, &HgsClient::questionAnswerSubmitted, this, [this](quint64 request) { finishAnswer(request, true, {}, false, true); });
    QObject::connect(m_client, &HgsClient::questionAnswerFailed, this, [this](quint64 request, const QString &, const QString &, const QString &error, bool uncertain) {
        finishAnswer(request, false, error, uncertain);
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
    m_status->hide(); m_openTerminal->hide();
    m_questionChoice.clear(); renderQuestion();
    m_justHeld = {}; renderHeld();
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
    m_question->setTheme(dark);
    m_held->setColors(c.fg, c.muted, c.border, c.surface);
    m_view->setBubbles(WorkspaceSkin::current().view(QStringLiteral("activity")) == QLatin1String("bubbles"));
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
        renderQuestion();
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
    renderQuestion();
    // The reads list what hgs still holds. A message just queued shows until a
    // read lists it or a second read after it (the first may predate it) does not.
    QSet<QString> listed;
    for (const auto &value : m_details.value("held_inputs").toArray()) listed.insert(value.toObject().value("request_id").toString());
    for (int i = int(m_justHeld.size()); i-- > 0;) {
        auto item = m_justHeld[i].toObject();
        item["reads"] = item.value("reads").toInt() + 1;
        if (listed.contains(item.value("request_id").toString()) || item.value("reads").toInt() >= 2) m_justHeld.removeAt(i);
        else m_justHeld[i] = item;
    }
    renderHeld();
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
    // U35: an hgs that holds messages keeps this one until the agent takes the previous.
    m_sendRequest = m_client->requestSendMessage(m_host, m_name, text, {}, m_details.value("run_id").toString(), m_details.value("conversation_id").toString(),
                                                 {}, {}, m_details.value("input_hold_supported").toBool());
    if (!m_sendRequest) return;
    m_send->setEnabled(false);
    m_pendingText = raw;
}

void TeamConversation::setQuestionsShown(bool shown)
{
    m_questionsShown = shown;
    renderQuestion();
}

namespace {
// What the owner agreed to: an answer goes only to the very question shown.
QJsonObject questionIdentity(const QJsonObject &question)
{
    QJsonObject identity;
    for (const auto *field : {"question_id", "question_hash", "run_id", "conversation_id", "questions",
            "source", "tool", "tool_name", "tool_call_id", "native_question_id", "agent_id",
            "answer_transport", "approval", "approval_choices", "trust_request", "optional"}) {
        const auto name = QString::fromLatin1(field);
        if (question.contains(name)) identity.insert(name, question.value(name));
    }
    return identity;
}
}

// As the session view: a required question first, else the optional ones,
// newest first, in the order the owner steps through them.
void TeamConversation::renderQuestion(int navigation)
{
    if (!m_questionsShown || m_name.isEmpty()) { m_question->hide(); m_composerRow->show(); return; }
    const QString key = sessionKey();
    QJsonObject question;
    QList<QJsonObject> optional;
    int pending = 0; QSet<QString> seen;
    for (const auto &value : m_details.value("pending_questions").toArray()) {
        const auto candidate = value.toObject();
        const auto id = candidate.value("question_id").toString();
        if (id.isEmpty() || seen.contains(id) || (candidate.value("questions").toArray().isEmpty() && candidate.value("can_answer").toBool(true))) continue;
        seen.insert(id);
        if (m_question->hasSubmittedAnswer(key, candidate)) continue;
        pending += qMax<qsizetype>(1, candidate.value("questions").toArray().size());
        if (candidate.value("optional").toBool()) optional.append(candidate);
        else if (question.isEmpty() || (!question.value("can_answer").toBool() && candidate.value("can_answer").toBool())) question = candidate;
    }
    std::stable_sort(optional.begin(), optional.end(), [](const QJsonObject &a, const QJsonObject &b) {
        return a.value("created_at").toDouble() > b.value("created_at").toDouble();
    });
    int index = 0;
    if (!m_questionChoice.isEmpty()) {
        const auto found = std::find_if(optional.cbegin(), optional.cend(), [this](const QJsonObject &item) { return item.value("question_id").toString() == m_questionChoice; });
        if (found == optional.cend()) m_questionChoice.clear(); else index = int(found - optional.cbegin());
    }
    if (question.isEmpty() && !optional.isEmpty()) {
        if (navigation) {
            index = qBound(0, index + navigation, int(optional.size()) - 1);
            m_questionChoice = optional[index].value("question_id").toString();
        }
        question = optional[index];
    }
    const bool available = !question.isEmpty();
    m_question->setTerminalAvailable(false, tr("Open the session in Activity for its terminal."));
    m_question->setQuestion(key, question, pending);
    m_question->setQueueNavigation(index, int(optional.size()));
    m_question->setVisible(available);
    m_composerRow->setVisible(!available || question.value("optional").toBool());
    QString reason;
    if (question.value("run_id") != m_details.value("run_id") || question.value("conversation_id") != m_details.value("conversation_id"))
        reason = tr("Refresh the pending question before answering.");
    else if (m_details.value("runtime_state").toString() != "live" || m_details.value("process_state").toString() != "running")
        reason = tr("The agent is no longer running.");
    m_question->setAvailability(reason.isEmpty(), reason);
}

// The shown question, only while its session is read live and nothing else is
// being answered; the same checks as the session view.
QJsonObject TeamConversation::actionableQuestion(const QString &key, const QString &questionId, bool skipping) const
{
    if (key != sessionKey() || m_answerRequest || m_details.value("runtime_state").toString() != "live"
        || m_details.value("process_state").toString() != "running") return {};
    QJsonObject question;
    for (const auto &value : m_details.value("pending_questions").toArray())
        if (value.toObject().value("question_id").toString() == questionId) { question = value.toObject(); break; }
    if (question.isEmpty() || m_question->hasSubmittedAnswer(key, question)
        || !(skipping ? question.value("can_skip").toBool() : question.value("can_answer").toBool())
        || question.value("run_id") != m_details.value("run_id") || question.value("conversation_id") != m_details.value("conversation_id")) return {};
    return question;
}

void TeamConversation::answerQuestion(const QString &key, const QString &questionId, const QJsonArray &answers)
{
    // Copies: a nested event loop may replace what the signal refers to.
    const QString targetKey = key, targetId = questionId;
    const QJsonArray requested = answers;
    if (m_confirmingCache || m_answerRequest) return;
    const bool skipping = !requested.isEmpty() && requested.first().toObject().value("skip").toBool();
    const auto question = actionableQuestion(targetKey, targetId, skipping);
    if (question.isEmpty()) { m_question->setError(targetKey, targetId, tr("This question changed. Refresh before answering.")); return; }
    // A cold cache may resume the agent with its whole conversation: the owner agrees first.
    const bool nativeCacheChoice = question.value("source") == "kimi_cache_hint" && question.value("answer_transport") == "kimi_tui";
    if (!nativeCacheChoice && CacheStatus::expired(m_details)) {
        const auto draft = m_question->submissionSnapshot(targetKey, targetId);
        if (draft.isEmpty() || draft.value("sending").toBool() || draft.value("submitted").toBool() || draft.value("answered").toBool()
            || draft.value("uncertain").toBool() || !draft.value("storage_error").toString().isEmpty()) return;
        QScopedValueRollback<bool> confirming(m_confirmingCache, true);
        QMessageBox box(QMessageBox::Warning, tr("Submit with a cold cache?"),
            CacheStatus::warning(m_details) + "\n\n" + tr("Submitting this answer may resume the agent with the entire conversation, increasing token usage, cost or subscription usage. Compaction or clearing can invalidate the pending question. Cancel keeps your answers."),
            QMessageBox::NoButton, this);
        box.setObjectName("questionColdCacheConfirm");
        auto *proceed = box.addButton(tr("Submit with full context"), QMessageBox::AcceptRole);
        auto *cancel = box.addButton(QMessageBox::Cancel);
        box.setDefaultButton(cancel); box.setEscapeButton(cancel);
        box.exec();
        if (box.clickedButton() != proceed) return;
        if (questionIdentity(actionableQuestion(targetKey, targetId, skipping)) != questionIdentity(question)
            || m_question->submissionSnapshot(targetKey, targetId) != draft) {
            m_question->setError(targetKey, targetId, tr("The question or your answers changed. Your draft was kept; review it before submitting again."));
            return;
        }
    }
    m_question->prepareSendingAsync(targetKey, targetId, [this, targetKey, targetId, question, requested, skipping](bool okay) {
        if (!okay) return;
        if (questionIdentity(actionableQuestion(targetKey, targetId, skipping)) != questionIdentity(question)) {
            m_question->setError(targetKey, targetId, tr("The question changed. Your answers were kept without submitting."));
            return;
        }
        m_answerKey = targetKey; m_answerQuestion = targetId;
        m_answerRequest = m_client->requestAnswerQuestion(m_host, m_name, question, requested);
    });
}

void TeamConversation::finishAnswer(quint64 request, bool ok, const QString &error, bool uncertain, bool submitted)
{
    if (!request || request != m_answerRequest) return;
    m_answerRequest = 0;
    if (ok && submitted) m_question->setSubmitted(m_answerKey, m_answerQuestion);
    else if (ok) m_question->setAnswered(m_answerKey, m_answerQuestion);
    else m_question->setError(m_answerKey, m_answerQuestion, error, uncertain);
    renderQuestion();
    poll();
}

void TeamConversation::setAgentName(const QString &name)
{
    m_agentName = name;
    renderHeld();
}

void TeamConversation::renderHeld()
{
    QJsonArray held = m_details.value("held_inputs").toArray();
    QSet<QString> listed;
    for (const auto &value : held) listed.insert(value.toObject().value("request_id").toString());
    for (const auto &value : m_justHeld)
        if (!listed.contains(value.toObject().value("request_id").toString())) held.append(value);
    const QString agent = !m_agentName.isEmpty() ? m_agentName
        : m_details.value("agent").toString().isEmpty() ? tr("the agent") : m_details.value("agent").toString();
    m_held->setHeld(m_name.isEmpty() ? QJsonArray() : held, agent);
}

// A refusal names what holds the message. At a dialog, draft or unknown prompt
// the owner finishes it in the agent's terminal: Open terminal, not "Terminal".
void TeamConversation::showRefusal(const QString &error, bool uncertain)
{
    QString text = error;
    const bool terminal = !uncertain && error.contains(QStringLiteral("Terminal"));
    if (terminal) {
        if (text.startsWith(QStringLiteral("hgs: "))) text = text.mid(5);
        for (const auto *tail : {"; open Terminal", "; inspect Terminal", "; wait for Ready or use Terminal", ". Open Terminal"})
            if (const int at = text.indexOf(QString::fromLatin1(tail)); at >= 0) text = text.left(at);
        text.replace(QStringLiteral(" in Activity or Terminal"), QString()).replace(QStringLiteral(" in Terminal"), QString());
        text = text.trimmed();
        if (!text.isEmpty()) text[0] = text[0].toUpper();
        if (!text.endsWith('.')) text += '.';
    }
    // An uncertain delivery may have arrived: the text stays, nothing is resent.
    m_status->setText(uncertain ? tr("Delivery unconfirmed — check the conversation before sending again. %1").arg(error) : text);
    m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).danger.name()));
    m_status->show();
    m_openTerminal->setVisible(terminal && openTerminal);
}
