#include "TeamConversation.h"
#include "AccountSwitch.h"
#include "ActivityView.h"
#include "CacheStatus.h"
#include "HeldInputBar.h"
#include "HgsClient.h"
#include "TerminalDraftBar.h"
#include "QuestionCard.h"
#include "TeamWidgets.h"
#include "CacheStatus.h"
#include "ComposerToolbar.h"
#include "SessionUsage.h"
#include <QAbstractTextDocumentLayout>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QKeyEvent>
#include <QMessageBox>
#include <QScopedValueRollback>
#include <algorithm>
#include <QBuffer>
#include <QImage>
#include <QMimeData>
#include <QMimeDatabase>
#include <QPixmap>
#include <QScrollBar>
#include <QStyle>
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

// Calls back when the watched widget shows or hides.
class VisibilityFilter : public QObject {
public:
    VisibilityFilter(std::function<void()> changed, QObject *parent) : QObject(parent), m_changed(std::move(changed)) {}
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        if (event->type() == QEvent::Show || event->type() == QEvent::Hide) m_changed();
        return QObject::eventFilter(watched, event);
    }
private:
    std::function<void()> m_changed;
};

// Owner, 2026-10-28: a pasted image or file became "[Image #1]" text and the
// file was lost. As in the session's composer, a pasted or dropped image or
// local file becomes an attachment; other text pastes as text.
class ConversationInput : public QPlainTextEdit {
public:
    std::function<void(const QString &)> attachFile;
    std::function<void(const QImage &)> attachImage;
protected:
    bool canInsertFromMimeData(const QMimeData *source) const override {
        return !isReadOnly() && (source->hasImage() || source->hasUrls() || QPlainTextEdit::canInsertFromMimeData(source));
    }
    void insertFromMimeData(const QMimeData *source) override {
        if (isReadOnly()) return;
        if (source->hasUrls() && attachFile) {
            const auto urls = source->urls();
            bool local = !urls.isEmpty();
            for (const auto &url : urls) local = local && url.isLocalFile();
            if (local) { for (const auto &url : urls) attachFile(url.toLocalFile()); return; }
        }
        if (source->hasImage() && attachImage) {
            const auto data = source->imageData();
            QImage image = qvariant_cast<QImage>(data);
            if (image.isNull() && data.canConvert<QPixmap>()) image = qvariant_cast<QPixmap>(data).toImage();
            if (!image.isNull()) { attachImage(image); return; }
        }
        if (source->hasText()) insertPlainText(source->text());
    }
    void dropEvent(QDropEvent *event) override {
        const auto *mime = event->mimeData();
        if (!isReadOnly() && (mime->hasImage() || mime->hasUrls())) {
            QDragLeaveEvent leave; QPlainTextEdit::dragLeaveEvent(&leave);
            setTextCursor(cursorForPosition(event->position().toPoint()));
            insertFromMimeData(mime); event->acceptProposedAction(); return;
        }
        QPlainTextEdit::dropEvent(event);
    }
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
    // Text left in the agent's Terminal input holds every send: show it here.
    m_terminalDraft = new TerminalDraftBar(m_client); m_terminalDraft->setObjectName("teamConversationTerminalDraft");
    v->addWidget(m_terminalDraft);
    m_terminalDraft->openTerminal = [this] { if (openTerminal) openTerminal(m_host, m_name); };
    m_terminalDraft->submitted = [this] { m_status->hide(); m_openTerminal->hide(); poll(); };
    auto *bottom = new QWidget; bottom->setObjectName("teamConversationBottom");
    auto *bv = new QVBoxLayout(bottom); bv->setContentsMargins(10, 6, 10, 10); bv->setSpacing(6);
    m_asks = new QWidget; m_asksLayout = new QHBoxLayout(m_asks); m_asksLayout->setContentsMargins(0, 0, 0, 0); m_asksLayout->setSpacing(6);
    m_asks->hide();
    bv->addWidget(m_asks);
    m_status = new QLabel; m_status->setObjectName("teamConversationStatus"); m_status->setWordWrap(true); m_status->hide();
    // A dialog or draft in the agent's terminal: the owner finishes it there.
    m_openTerminal = button(tr("Open terminal"), "teamConversationOpenTerminal"); m_openTerminal->hide();
    QObject::connect(m_openTerminal, &QPushButton::clicked, this, [this] { if (openTerminal) openTerminal(m_host, m_name); });
    m_statusRow = new QHBoxLayout; m_statusRow->setSpacing(8);
    m_statusRow->addWidget(m_status, 1); m_statusRow->addWidget(m_openTerminal, 0, Qt::AlignTop);
    bv->addLayout(m_statusRow);
    // The files to send, and Attach beside Send: every conversation takes
    // files, not only the designer's (owner, 2026-10-28).
    m_filesRow = new QWidget; m_filesRow->setObjectName("teamConversationFiles"); m_filesRow->setLayout(new FlowLayout(6)); m_filesRow->hide();
    bv->addWidget(m_filesRow);
    m_composerRow = new QWidget;
    auto *row = new QHBoxLayout(m_composerRow); row->setContentsMargins(0, 0, 0, 0); row->setSpacing(6);
    auto *input = new ConversationInput; m_input = input; m_input->setObjectName("teamConversationInput"); m_input->setFixedHeight(56);
    input->attachFile = [this](const QString &path) { addFile(path, true); };
    input->attachImage = [this](const QImage &image) { addImage(image); };
    m_send = button(tr("Send"), "teamConversationSend", true);
    m_attach = new QPushButton; m_attach->setObjectName("teamConversationAttach"); m_attach->setFixedSize(30, 28);
    m_attach->setToolTip(tr("Attach files")); m_attach->setAccessibleName(tr("Attach files")); m_attach->setCursor(Qt::PointingHandCursor);
    QObject::connect(m_attach, &QPushButton::clicked, this, [this] {
        for (const auto &path : QFileDialog::getOpenFileNames(this, tr("Attach files"))) addFile(path, false);
    });
    m_accountChip = new AccountSwitchChip;
    row->addWidget(m_input, 1); row->addWidget(m_accountChip, 0, Qt::AlignBottom); row->addWidget(m_attach, 0, Qt::AlignBottom); row->addWidget(m_send, 0, Qt::AlignBottom);
    bv->addWidget(m_composerRow);
    v->addWidget(bottom);
    m_input->installEventFilter(new ComposerFilter([this] { send(m_input->toPlainText()); }, this));
    QObject::connect(m_send, &QPushButton::clicked, this, [this] { send(m_input->toPlainText()); });
    QObject::connect(m_client, &HgsClient::inspectionReady, this, [this](const QString &host, const QString &name, const QJsonObject &data, const QString &archive) {
        received(host, name, data, archive);
    });
    // U53: the session computer's accounts, read when the session changes.
    QObject::connect(m_client, &HgsClient::accountsReady, this, [this](quint64 request, const QString &host, const QJsonObject &result) {
        if (request != m_accountsRequest || host != m_host) return;
        m_accountsRequest = 0; m_accountProfiles = {};
        for (const auto &value : result.value("profiles").toArray()) { auto p = value.toObject(); p["host"] = host; m_accountProfiles.append(p); }
        updateAccountChip();
    });
    m_accountChip->switchRequested = [this](const QString &key, const QString &account) {
        if (m_switchRequest || key != m_host + '\n' + m_name) { m_accountChip->finish(key, {}, tr("the conversation changed before the switch began; choose again")); return; }
        m_switchRequest = m_client->requestSwitchAccount(m_host, m_name, account);
    };
    QObject::connect(m_client, &HgsClient::switchAccountFinished, this, [this](quint64 request, const QString &host, const QString &name, const QJsonObject &receipt, const QString &error) {
        if (request != m_switchRequest) return;
        m_switchRequest = 0;
        const auto outcome = m_accountChip->finish(host + '\n' + name, receipt, error);
        if (host == m_host && name == m_name && !outcome.text.isEmpty()) {
            m_status->setText(outcome.text); m_status->setProperty("tone", outcome.kind == AccountSwitch::Outcome::Switched ? "info" : "warn"); m_status->show();
        }
        // The account changed (or stayed): the next read names it.
        m_accountHost = QStringLiteral("\x1f"); poll();
    });
    QObject::connect(m_client, &HgsClient::messageSent, this, [this](quint64 request, const QString &, const QString &, const QJsonObject &receipt) {
        if (request != m_sendRequest) return;
        m_sendRequest = 0; m_send->setEnabled(true); m_status->hide(); m_openTerminal->hide();
        if (m_input->toPlainText() == m_pendingText) m_input->clear();
        // The files went with it; any added since stay.
        for (const auto &file : std::as_const(m_sentFiles))
            m_files.erase(std::remove_if(m_files.begin(), m_files.end(), [&](const MessageAttachment &f) { return f.reference == file.reference && f.name == file.name; }), m_files.end());
        m_sentFiles.clear(); showFiles(); updateSend();
        // Held by hgs: its line shows at once, then as the next reads list it.
        if (receipt.value("status") == "queued") {
            m_justHeld.append(QJsonObject{{"request_id", receipt.value("request_id")}, {"text", receipt.value("text")}});
            m_sentHeld.add(receipt.value("request_id").toString(), m_pendingText.isEmpty() ? receipt.value("text").toString() : m_pendingText);
            renderHeld();
        }
        if (sent) sent({});
        poll();
    });
    QObject::connect(m_client, &HgsClient::messageFailed, this, [this](quint64 request, const QString &, const QString &, const QString &error, bool uncertain) {
        if (request != m_sendRequest) return;
        m_sendRequest = 0; m_send->setEnabled(true); m_sentFiles.clear(); updateSend();
        showRefusal(error, uncertain);
        if (!uncertain && error.contains(QStringLiteral("has a draft"))) m_terminalDraft->check();
        if (sent) sent(error.isEmpty() ? tr("Not sent") : error);
    });
    QObject::connect(m_client, &HgsClient::heldCancelFinished, this, [this](quint64 request, const QString &, const QString &, bool ok, const QJsonObject &receipt, const QString &error) {
        if (!m_cancels.contains(request)) return;
        const QString text = m_cancels.take(request);
        if (!ok) {
            m_held->cancelFinished(receipt.value("request_id").toString());
            m_status->setText(error); m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).danger.name())); m_status->show();
            fitDesigner(); poll(); return;
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
    QObject::connect(m_question, &QuestionCard::openTerminalRequested, this, [this] { if (openTerminal) openTerminal(m_host, m_name); });
    QObject::connect(m_client, &HgsClient::questionAnswered, this, [this](quint64 request) { finishAnswer(request, true); });
    QObject::connect(m_client, &HgsClient::questionAnswerSubmitted, this, [this](quint64 request) { finishAnswer(request, true, {}, false, true); });
    QObject::connect(m_client, &HgsClient::questionAnswerFailed, this, [this](quint64 request, const QString &, const QString &, const QString &error, bool uncertain) {
        finishAnswer(request, false, error, uncertain);
    });
    m_timer.setObjectName(QStringLiteral("teamConversationPoll"));
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
    // The designer's composer asks for its session; any other needs one.
    m_input->setEnabled(m_designer || !name.isEmpty()); m_send->setEnabled((m_designer || !name.isEmpty()) && !m_sendRequest && !m_starting);
    if (!name.isEmpty() && m_starting) { m_starting = false; m_input->clear(); }
    m_status->hide(); m_openTerminal->hide();
    m_terminalDraft->setSession(host, name, {}, {});
    m_questionChoice.clear(); renderQuestion();
    m_attach->setEnabled(!name.isEmpty());
    if (m_designer) {
        static_cast<CacheStatus::Chip *>(m_cache)->setData({});
        static_cast<SessionUsage::ContextButton *>(m_context)->setData({});
        m_reference->setVisible(!name.isEmpty());
        m_attach->setEnabled(!name.isEmpty()); m_voice->setEnabled(!name.isEmpty()); m_video->setEnabled(!name.isEmpty());
        if (m_intro) m_intro->setVisible(name.isEmpty());
        updateSend(); fitDesigner();
    }
    m_justHeld = {}; m_sentHeld.clear(); renderHeld();
    updateAccountChip();
    if (isVisible()) poll();
}

void TeamConversation::setAccountLocked(const QString &reason)
{
    m_accountLock = reason; updateAccountChip();
}

// U53: the session's account by its label, the same agent's signed-in
// accounts on its computer, and why it cannot change now.
void TeamConversation::updateAccountChip()
{
    // The accounts are read once per computer, again after a switch.
    if (!m_name.isEmpty() && m_accountHost != m_host && !m_accountsRequest) {
        m_accountHost = m_host; m_accountProfiles = {};
        m_accountsRequest = m_client->requestAccounts(m_host);
    }
    const auto agent = m_details.value("agent").toString();
    const auto account = m_details.value("account_id").toString();
    m_accountChip->setSession(m_name.isEmpty() ? QString() : m_host + '\n' + m_name, agent, account,
                              m_name.isEmpty() ? QString() : m_details.value("account_label").toString());
    m_accountChip->setOptions(AccountSwitch::options(m_accountProfiles, m_host, agent, AccountSwitch::accountOf(agent, account), m_dark));
    m_accountChip->setLocked(m_accountLock);
    const auto phase = m_details.value("phase").toString();
    m_accountChip->setBlocked(m_details.value("runtime_state").toString() != QLatin1String("live") || m_details.value("process_state").toString() != QLatin1String("running")
                                  ? tr("The session is not running; resume it to change its account.")
                              : QStringList{"working", "tool", "compacting"}.contains(phase) ? tr("The agent is working; change its account once it is idle.")
                              : m_sendRequest ? tr("Wait for the message to be delivered.")
                              : m_details.value("build").isObject() ? tr("A build Zerus runs for the session is still going; wait for it.") : QString());
}

void TeamConversation::setTheme(bool dark)
{
    m_dark = dark;
    m_accountChip->setTheme(dark);
    const Colors c = colorsFor(dark);
    setStyleSheet(QStringLiteral("QWidget#teamConversationBottom{border-top:1px solid %1;} QPlainTextEdit#teamConversationInput{background:%2;color:%3;border:1px solid %1;border-radius:8px;padding:4px 6px;}")
                      .arg(c.border.name(), c.surface.name(), c.fg.name()));
    // After the sheet: a repolish would reset the Activity canvas palette.
    m_view->setTheme(dark);
    m_question->setTheme(dark);
    m_held->setColors(c.fg, c.muted, c.border, c.surface);
    m_terminalDraft->setColors(c.fg, c.muted, c.border, c.surface);
    m_attach->setIcon(workspaceIcon("attachment", colorsFor(dark).text2));
    m_view->setBubbles(WorkspaceSkin::current().view(QStringLiteral("activity")) == QLatin1String("bubbles"));
    if (m_designer) {
        const Colors c = colorsFor(dark);
        m_bottom->setStyleSheet(QStringLiteral(
            "QFrame#teamDesignerComposerCard{background:%1;border:1px solid %2;border-radius:12px;}"
            "QFrame#teamDesignerComposerCard QPlainTextEdit{background:transparent;border:none;color:%3;padding:0;}"
            "QPushButton#teamConversationAttach,QPushButton#teamConversationVoice,QPushButton#teamConversationVideo,QPushButton#teamConversationReference{border:1px solid transparent;border-radius:7px;padding:0;background:transparent;}"
            "QPushButton#teamConversationReference{border-color:%2;background:%4;}"
            "QPushButton#teamConversationAttach:hover,QPushButton#teamConversationVoice:hover,QPushButton#teamConversationVideo:hover,QPushButton#teamConversationReference:hover{background:%5;}"
            "QPushButton#teamDesignerModel{border:none;background:transparent;color:%6;padding:2px 6px;}"
            "QPushButton#teamDesignerModel:hover{color:%3;}"
            "QPushButton[attachmentChip=\"true\"]{border:1px solid %2;border-radius:10px;padding:1px 8px;color:%6;background:transparent;}")
            .arg(c.card.name(), c.border.name(), c.fg.name(), c.tile.name(), alpha(c.fg, .06).name(QColor::HexArgb), c.text2.name()));
        m_row->setTheme(dark);
        static_cast<CacheStatus::Chip *>(m_cache)->setTheme(dark);
        static_cast<SessionUsage::ContextButton *>(m_context)->setTheme(dark);
        m_attach->setIcon(workspaceIcon("attachment", c.text2)); m_voice->setIcon(workspaceIcon("microphone", c.text2));
        m_video->setIcon(workspaceIcon("video", c.text2)); m_reference->setIcon(workspaceIcon("link", c.text2));
        updateSend();
    }
}

void TeamConversation::useDesignerLayout(QWidget *intro, QWidget *above)
{
    if (m_designer) return;
    m_designer = true; m_intro = intro; m_above = above;
    auto *v = static_cast<QVBoxLayout *>(layout());
    auto *oldBottom = findChild<QWidget *>("teamConversationBottom");
    // The field and Send in one card; the row of the session's composer under it.
    m_bottom = new QWidget; m_bottom->setObjectName("teamDesignerComposer");
    auto *bv = new QVBoxLayout(m_bottom); bv->setContentsMargins(16, 6, 16, 4); bv->setSpacing(6);
    bv->addWidget(m_filesRow);
    // #521: the held messages, a Terminal draft, the status and Open terminal
    // sit above the card; the column refits as they show or hide.
    for (QWidget *bar : {static_cast<QWidget *>(m_held), static_cast<QWidget *>(m_terminalDraft)}) {
        v->removeWidget(bar); bv->addWidget(bar);
        bar->installEventFilter(new VisibilityFilter([this] { QTimer::singleShot(0, this, [this] { fitDesigner(); }); }, bar));
    }
    if (oldBottom) oldBottom->layout()->removeItem(m_statusRow);
    m_statusRow->setParent(nullptr); bv->addLayout(m_statusRow);
    m_card = new QFrame; m_card->setObjectName("teamDesignerComposerCard");
    auto *ch = new QHBoxLayout(m_card); ch->setContentsMargins(14, 8, 8, 8); ch->setSpacing(8);
    m_input->setParent(m_card); m_send->setParent(m_card);
    m_input->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_input->setMinimumHeight(0); m_input->setMaximumHeight(QWIDGETSIZE_MAX);
    ch->addWidget(m_input, 1); ch->addWidget(m_send, 0, Qt::AlignVCenter);
    m_send->setMinimumHeight(34);
    bv->addWidget(m_card);
    m_row = new ComposerToolbar; m_row->setObjectName("teamDesignerComposerRow"); m_row->setThreeZones(true);
    // Voice and video record where the session's composer has them.
    m_voice = new QPushButton; m_voice->setObjectName("teamConversationVoice"); m_voice->setFixedSize(30, 28);
    m_voice->setToolTip(tr("Record a voice message in the designer's session")); m_voice->setAccessibleName(tr("Voice message"));
    m_video = new QPushButton; m_video->setObjectName("teamConversationVideo"); m_video->setFixedSize(30, 28);
    m_video->setToolTip(tr("Record a video message in the designer's session")); m_video->setAccessibleName(tr("Video message"));
    for (auto *b : {m_voice, m_video}) {
        b->setCursor(Qt::PointingHandCursor); b->setEnabled(false);
        QObject::connect(b, &QPushButton::clicked, this, [this] { if (openSession && !m_name.isEmpty()) openSession(); });
    }
    auto *cache = new CacheStatus::Chip; m_cache = cache;
    auto *context = new SessionUsage::ContextButton; context->setObjectName("teamConversationContext"); m_context = context;
    m_reference = new QPushButton; m_reference->setObjectName("teamConversationReference"); m_reference->setFixedSize(30, 28);
    m_reference->setToolTip(tr("Copy session reference")); m_reference->setAccessibleName(tr("Copy session reference")); m_reference->setCursor(Qt::PointingHandCursor);
    m_reference->hide();
    m_model = new QPushButton; m_model->setObjectName("teamDesignerModel"); m_model->setCursor(Qt::PointingHandCursor);
    m_row->add(ComposerToolbar::Slot::Attach, m_attach);
    m_row->add(ComposerToolbar::Slot::Voice, m_voice);
    m_row->add(ComposerToolbar::Slot::VoiceMode, m_video);
    m_row->add(ComposerToolbar::Slot::Cache, cache);
    m_row->add(ComposerToolbar::Slot::Context, context);
    m_row->add(ComposerToolbar::Slot::SessionReference, m_reference);
    m_row->add(ComposerToolbar::Slot::Account, m_accountChip);
    m_row->add(ComposerToolbar::Slot::Model, m_model);
    bv->addWidget(m_row);
    QObject::connect(m_reference, &QPushButton::clicked, this, [this] {
        if (m_name.isEmpty()) return;
        m_client->requestSessionReference(m_host, m_name, m_details.value("run_id").toString(), m_details.value("conversation_id").toString());
    });
    QObject::connect(m_input, &QPlainTextEdit::textChanged, this, [this] { updateSend(); });
    QObject::connect(m_input->document()->documentLayout(), &QAbstractTextDocumentLayout::documentSizeChanged, this, [this] { fitInput(); });
    // Rebuild the column: [space] intro, the messages, above, the composer [space].
    v->removeWidget(m_view); v->removeWidget(m_question); if (oldBottom) { v->removeWidget(oldBottom); oldBottom->hide(); }
    v->addStretch(1);
    if (m_intro) v->addWidget(m_intro);
    v->addWidget(m_view);
    if (m_above) v->addWidget(m_above);
    // U16: a pending question takes the composer's place here too.
    v->addWidget(m_question);
    v->addWidget(m_bottom);
    v->addStretch(1);
    m_view->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);
    if (auto *text = m_view->findChild<QTextBrowser *>("teamConversationText"))
        QObject::connect(text->verticalScrollBar(), &QScrollBar::rangeChanged, this, [this] { QTimer::singleShot(0, this, [this] { fitDesigner(); }); });
    m_input->setEnabled(true);
    setPlaceholder(tr("Ask for changes…"));
    setTheme(m_dark);
    fitInput(); fitDesigner();
}

void TeamConversation::startFinished(const QString &error)
{
    if (error.isEmpty()) return;   // the session's arrival clears the field
    m_starting = false; updateSend();
    m_status->setText(error);
    m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).danger.name()));
    m_status->show();
    fitDesigner();
}

void TeamConversation::updateSend()
{
    if (!m_designer) return;
    // Send is primary while there is something to send.
    const bool ready = !m_input->toPlainText().trimmed().isEmpty() && !m_sendRequest && !m_starting;
    if (m_send->property("primary").toBool() != ready) {
        m_send->setProperty("primary", ready);
        m_send->style()->unpolish(m_send); m_send->style()->polish(m_send);
    }
    m_send->setEnabled(!m_sendRequest && !m_starting);
    m_send->setText(m_starting ? tr("Starting…") : tr("Send"));
    const Colors c = colorsFor(m_dark);
    m_send->setIcon(workspaceIcon("send", ready ? c.accentText : c.text2));
}

// 2 to 8 lines, without a scroll bar until the text needs more.
void TeamConversation::fitInput()
{
    if (!m_designer) return;
    const int line = m_input->fontMetrics().lineSpacing();
    const int lines = int(m_input->document()->size().height());
    const int shown = qBound(2, lines, 8);
    m_input->setVerticalScrollBarPolicy(lines > 8 ? Qt::ScrollBarAsNeeded : Qt::ScrollBarAlwaysOff);
    m_input->setFixedHeight(shown * line + 10);
    fitDesigner();
}

bool TeamConversation::addFile(const QString &path, bool reference)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly) || file.size() > HgsClient::MaximumAttachmentBytes || m_files.size() >= HgsClient::MaximumAttachments) return false;
    const QByteArray bytes = file.readAll();
    const QString mime = QMimeDatabase().mimeTypeForData(bytes).name();
    const QString token = QStringLiteral("[%1 #%2]").arg(mime.startsWith("image/") ? QStringLiteral("Image") : QStringLiteral("File")).arg(m_files.size() + 1);
    m_files.append(MessageAttachment{QFileInfo(path).fileName(), mime, bytes, token});
    if (reference) m_input->insertPlainText(token + QLatin1Char(' '));
    showFiles(); updateSend();
    return true;
}

void TeamConversation::addImage(const QImage &image)
{
    QByteArray bytes; QBuffer buffer(&bytes); buffer.open(QIODevice::WriteOnly);
    if (!image.save(&buffer, "PNG") || bytes.size() > HgsClient::MaximumAttachmentBytes || m_files.size() >= HgsClient::MaximumAttachments) return;
    const QString token = QStringLiteral("[Image #%1]").arg(m_files.size() + 1);
    m_files.append(MessageAttachment{QStringLiteral("pasted-image-%1.png").arg(m_files.size() + 1), QStringLiteral("image/png"), bytes, token});
    m_input->insertPlainText(token + QLatin1Char(' '));
    showFiles(); updateSend();
}

void TeamConversation::showFiles()
{
    if (!m_filesRow) return;
    auto *flow = m_filesRow->layout();
    while (auto *item = flow->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    for (int i = 0; i < m_files.size(); ++i) {
        auto *chip = new QPushButton(m_files[i].name + QStringLiteral("  ×")); chip->setObjectName("teamConversationFile"); chip->setProperty("attachmentChip", true);
        chip->setToolTip(tr("Remove %1").arg(m_files[i].name));
        QObject::connect(chip, &QPushButton::clicked, this, [this, i] { if (i < m_files.size()) m_files.removeAt(i); showFiles(); });
        flow->addWidget(chip);
    }
    m_filesRow->setVisible(!m_files.isEmpty());
    fitDesigner();
}

// The messages take their own height up to what the column leaves them; the
// space above and below shares the rest, so the block stays centred.
void TeamConversation::fitDesigner()
{
    if (!m_designer) return;
    auto *text = m_view->findChild<QTextBrowser *>("teamConversationText");
    if (text && text->document() != m_watched) {
        m_watched = text->document();
        QObject::connect(m_watched->documentLayout(), &QAbstractTextDocumentLayout::documentSizeChanged, this, [this] { QTimer::singleShot(0, this, [this] { fitDesigner(); }); });
    }
    if (m_name.isEmpty()) { m_view->hide(); return; }
    auto *v = static_cast<QVBoxLayout *>(layout());
    int others = v->contentsMargins().top() + v->contentsMargins().bottom();
    for (auto *w : {m_intro, m_above, static_cast<QWidget *>(m_question), m_bottom}) if (w && !w->isHidden()) others += w->sizeHint().height() + v->spacing();
    const int available = qMax(80, height() - others);
    const int content = text ? int(text->document()->size().height()) + 24 : available;
    m_view->setFixedHeight(qBound(80, content, available));
    m_view->show();
}

void TeamConversation::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    fitDesigner();
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
    updateAccountChip();
    if (m_designer) {
        static_cast<CacheStatus::Chip *>(m_cache)->setData(data);
        static_cast<SessionUsage::ContextButton *>(m_context)->setData(data.value("session_usage").toObject());
        QTimer::singleShot(0, this, [this] { fitDesigner(); });
    }
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
    // #511 N5: a held message hgs could not deliver comes back to the composer
    // with why; an uncertain one says Delivery unconfirmed.
    for (const auto &lost : m_sentHeld.take(m_details.value("held_settled").toArray())) {
        m_input->setPlainText(HeldSent::restored(m_input->toPlainText(), lost.text));
        m_status->setText(HeldSent::notice(lost));
        m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).danger.name()));
        m_status->show();
    }
}

void TeamConversation::send(const QString &raw)
{
    const QString text = raw.trimmed();
    if (text.isEmpty() || m_sendRequest || m_starting) return;
    // The designer starts with the first message as its goal.
    if (m_name.isEmpty()) {
        if (!m_designer || !startRequested) return;
        m_starting = true; m_status->hide(); updateSend();
        startRequested(text);
        return;
    }
    if (m_details.value("run_id").toString().isEmpty()) {
        m_status->setText(tr("The session is not read yet; try again in a moment."));
        m_status->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).warn.name()));
        m_status->show();
        return;
    }
    m_sentFiles = m_files;
    // U35: an hgs that holds messages keeps this one until the agent takes the previous.
    m_sendRequest = m_client->requestSendMessage(m_host, m_name, text, m_sentFiles, m_details.value("run_id").toString(), m_details.value("conversation_id").toString(),
                                                 {}, {}, m_details.value("input_hold_supported").toBool());
    if (!m_sendRequest) { m_sentFiles.clear(); return; }
    m_send->setEnabled(false);
    m_pendingText = raw;
    updateSend();
}

void TeamConversation::setCards(QWidget *cards)
{
    auto *v = static_cast<QVBoxLayout *>(layout());
    v->insertWidget(v->indexOf(m_question), cards);
}

void TeamConversation::setBanner(QWidget *banner)
{
    // The banner hides itself, its margins included, while nothing waits.
    static_cast<QVBoxLayout *>(layout())->insertWidget(0, banner);
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
    // The designer's composer is its card and row (#488), the others' the row.
    QWidget *composer = m_designer ? m_bottom : m_composerRow;
    if (!m_questionsShown || m_name.isEmpty()) { m_question->hide(); composer->show(); fitDesigner(); return; }
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
    m_question->setTerminalAvailable(static_cast<bool>(openTerminal), tr("Open the session in Activity for its terminal."));
    m_question->setQuestion(key, question, pending);
    m_question->setQueueNavigation(index, int(optional.size()));
    m_question->setVisible(available);
    composer->setVisible(!available || question.value("optional").toBool());
    fitDesigner();
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
    // Messages waiting for an agent that is not working: a Terminal draft may hold them.
    m_terminalDraft->setSession(m_host, m_name, m_details.value("run_id").toString(), m_details.value("conversation_id").toString());
    if (!held.isEmpty() && m_details.value("activity").toString() != QLatin1String("busy")) m_terminalDraft->check(true);
    fitDesigner();
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
    fitDesigner();
}
