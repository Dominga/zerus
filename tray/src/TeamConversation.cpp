#include "TeamConversation.h"
#include "ActivityView.h"
#include "HgsClient.h"
#include "TeamWidgets.h"
#include "CacheStatus.h"
#include "ComposerToolbar.h"
#include "SessionUsage.h"
#include <QAbstractTextDocumentLayout>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QKeyEvent>
#include <QMimeDatabase>
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
        // The files went with it; any added since stay.
        for (const auto &file : std::as_const(m_sentFiles))
            m_files.erase(std::remove_if(m_files.begin(), m_files.end(), [&](const MessageAttachment &f) { return f.reference == file.reference && f.name == file.name; }), m_files.end());
        m_sentFiles.clear(); showFiles(); updateSend();
        if (sent) sent({});
        poll();
    });
    QObject::connect(m_client, &HgsClient::messageFailed, this, [this](quint64 request, const QString &, const QString &, const QString &error, bool uncertain) {
        if (request != m_sendRequest) return;
        m_sendRequest = 0; m_send->setEnabled(true); m_sentFiles.clear(); updateSend();
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
    // The designer's composer asks for its session; any other needs one.
    m_input->setEnabled(m_designer || !name.isEmpty()); m_send->setEnabled((m_designer || !name.isEmpty()) && !m_sendRequest && !m_starting);
    if (!name.isEmpty() && m_starting) { m_starting = false; m_input->clear(); }
    m_status->hide();
    if (m_designer) {
        static_cast<CacheStatus::Chip *>(m_cache)->setData({});
        static_cast<SessionUsage::ContextButton *>(m_context)->setData({});
        m_reference->setVisible(!name.isEmpty());
        m_attach->setEnabled(!name.isEmpty()); m_voice->setEnabled(!name.isEmpty()); m_video->setEnabled(!name.isEmpty());
        if (m_intro) m_intro->setVisible(name.isEmpty());
        updateSend(); fitDesigner();
    }
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
    m_filesRow = new QWidget; m_filesRow->setObjectName("teamConversationFiles"); m_filesRow->setLayout(new FlowLayout(6)); m_filesRow->hide();
    bv->addWidget(m_filesRow);
    bv->addWidget(m_status);
    m_card = new QFrame; m_card->setObjectName("teamDesignerComposerCard");
    auto *ch = new QHBoxLayout(m_card); ch->setContentsMargins(14, 8, 8, 8); ch->setSpacing(8);
    m_input->setParent(m_card); m_send->setParent(m_card);
    m_input->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_input->setMinimumHeight(0); m_input->setMaximumHeight(QWIDGETSIZE_MAX);
    ch->addWidget(m_input, 1); ch->addWidget(m_send, 0, Qt::AlignVCenter);
    m_send->setMinimumHeight(34);
    bv->addWidget(m_card);
    m_row = new ComposerToolbar; m_row->setObjectName("teamDesignerComposerRow"); m_row->setThreeZones(true);
    m_attach = new QPushButton; m_attach->setObjectName("teamConversationAttach"); m_attach->setFixedSize(30, 28);
    m_attach->setToolTip(tr("Attach files")); m_attach->setAccessibleName(tr("Attach files")); m_attach->setCursor(Qt::PointingHandCursor);
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
    m_row->add(ComposerToolbar::Slot::Model, m_model);
    bv->addWidget(m_row);
    QObject::connect(m_attach, &QPushButton::clicked, this, [this] {
        const auto paths = QFileDialog::getOpenFileNames(this, tr("Attach files"));
        for (const auto &path : paths) {
            QFile file(path);
            if (!file.open(QIODevice::ReadOnly) || file.size() > HgsClient::MaximumAttachmentBytes || m_files.size() >= HgsClient::MaximumAttachments) continue;
            const QByteArray bytes = file.readAll();
            const QString mime = QMimeDatabase().mimeTypeForData(bytes).name();
            m_files.append(MessageAttachment{QFileInfo(path).fileName(), mime, bytes,
                                             QStringLiteral("[%1 #%2]").arg(mime.startsWith("image/") ? QStringLiteral("Image") : QStringLiteral("File")).arg(m_files.size() + 1)});
        }
        showFiles();
    });
    QObject::connect(m_reference, &QPushButton::clicked, this, [this] {
        if (m_name.isEmpty()) return;
        m_client->requestSessionReference(m_host, m_name, m_details.value("run_id").toString(), m_details.value("conversation_id").toString());
    });
    QObject::connect(m_input, &QPlainTextEdit::textChanged, this, [this] { updateSend(); });
    QObject::connect(m_input->document()->documentLayout(), &QAbstractTextDocumentLayout::documentSizeChanged, this, [this] { fitInput(); });
    // Rebuild the column: [space] intro, the messages, above, the composer [space].
    v->removeWidget(m_view); if (oldBottom) { v->removeWidget(oldBottom); oldBottom->hide(); }
    v->addStretch(1);
    if (m_intro) v->addWidget(m_intro);
    v->addWidget(m_view);
    if (m_above) v->addWidget(m_above);
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
    for (auto *w : {m_intro, m_above, m_bottom}) if (w && !w->isHidden()) others += w->sizeHint().height() + v->spacing();
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
    if (m_designer) {
        static_cast<CacheStatus::Chip *>(m_cache)->setData(data);
        static_cast<SessionUsage::ContextButton *>(m_context)->setData(data.value("session_usage").toObject());
        QTimer::singleShot(0, this, [this] { fitDesigner(); });
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
    m_sendRequest = m_client->requestSendMessage(m_host, m_name, text, m_sentFiles, m_details.value("run_id").toString(), m_details.value("conversation_id").toString());
    if (!m_sendRequest) { m_sentFiles.clear(); return; }
    m_send->setEnabled(false);
    m_pendingText = raw;
    updateSend();
}
