#include "TaskDialogs.h"
#include "MonospaceFont.h"
#include "ScopeFilterWidgets.h"
#include "SessionStatusBadge.h"
#include "TasksView.h"
#include "WorkspaceIcons.h"
#include "AttachmentViewer.h"
#include "LocalFile.h"
#include "TaskContent.h"
#include <QFileDialog>
#include <QMimeData>
#include <QMimeDatabase>
#include <QBuffer>
#include <QButtonGroup>
#include <QCheckBox>
#include <QDateTime>
#include <QHash>
#include <QIcon>
#include <QPainter>
#include <QPixmap>
#include <algorithm>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QSpinBox>
#include <QTextBrowser>
#include <QVBoxLayout>

namespace {
class TaskPrompt : public QPlainTextEdit {
public:
    using QPlainTextEdit::QPlainTextEdit;
    std::function<bool(const QMimeData *)> media;
protected:
    bool canInsertFromMimeData(const QMimeData *source) const override {
        return source->hasImage() || source->hasUrls() || QPlainTextEdit::canInsertFromMimeData(source);
    }
    void insertFromMimeData(const QMimeData *source) override {
        if (!isReadOnly() && media && media(source)) return;
        QPlainTextEdit::insertFromMimeData(source);
    }
};
// A field's name, small and muted; a heading groups fields.
QLabel *section(const QString &text) {
    auto *label = new QLabel(text); label->setProperty("taskSection", true);
    QFont font = label->font(); font.setPixelSize(11); label->setFont(font);
    return label;
}
QLabel *heading(const QString &text, const QString &name) {
    auto *label = new QLabel(text); label->setObjectName(name);
    QFont font = label->font(); font.setPixelSize(13); font.setWeight(QFont::DemiBold); label->setFont(font);
    return label;
}
// A session's status as a dot: the session list's colour for it, muted otherwise.
QIcon statusDot(int kind, bool dark, const QColor &muted) {
    QColor color = SessionStatusBadge::edge(SessionStatusBadge::Kind(kind), dark);
    if (!color.isValid()) color = muted;
    QPixmap dot(16, 16); dot.fill(Qt::transparent);
    QPainter p(&dot); p.setRenderHint(QPainter::Antialiasing); p.setPen(Qt::NoPen); p.setBrush(color); p.drawEllipse(QRectF(4, 4, 8, 8));
    return QIcon(dot);
}
QLabel *errorLabel(const QString &name) {
    auto *label = new QLabel; label->setObjectName(name); label->setWordWrap(true); label->setTextFormat(Qt::PlainText);
    label->setStyleSheet(QStringLiteral("color:#e06c75;")); label->hide(); return label;
}
QString time(qint64 ms) { return QDateTime::fromMSecsSinceEpoch(ms).toString(QStringLiteral("d MMM HH:mm")); }
QString actor(const QJsonObject &actor) {
    return actor.value("kind").toString() == QLatin1String("agent") ? TaskBoard::sessionLabel(actor.value("host").toString(), actor.value("session").toString())
                                                                    : QObject::tr("You");
}
}

// One history line: who, when and what happened.
QString taskHistoryLine(const QJsonObject &entry) {
    const QString kind = entry.value("kind").toString(), text = entry.value("text").toString();
    QString what;
    if (kind == QLatin1String("created")) what = QObject::tr("created the task");
    else if (kind == QLatin1String("edited")) what = QObject::tr("edited the prompt");
    else if (kind == QLatin1String("stage")) what = QObject::tr("moved it to %1").arg(TaskBoard::stageLabel(text));
    else if (kind == QLatin1String("target")) what = QObject::tr("changed the session: %1").arg(text);
    else if (kind == QLatin1String("sent")) {
        const auto session = entry.value("session").toObject();
        what = QObject::tr("sent it to %1").arg(TaskBoard::sessionLabel(session.value("host").toString(), session.value("session").toString()));
    } else if (kind == QLatin1String("report")) what = QObject::tr("reported: %1").arg(text);
    else if (kind == QLatin1String("cancelled")) what = QObject::tr("cancelled it");
    else if (kind == QLatin1String("comment")) what = QObject::tr("commented: %1").arg(text);
    else if (kind == QLatin1String("started")) what = QObject::tr("started the task");
    else what = text.isEmpty() ? kind : text;
    return QStringLiteral("<p style='margin:0 0 6px 0'><b>%1</b> <span style='opacity:.7'>%2</span> · %3</p>")
        .arg(actor(entry.value("actor").toObject()).toHtmlEscaped(), time(qint64(entry.value("at").toDouble())), what.toHtmlEscaped());
}

TaskDialog::TaskDialog(const QJsonObject &task, const QList<TaskSession> &sessions, std::function<QString(const TaskTarget &)> sendBlocker,
                       bool dark, QWidget *parent)
    : QDialog(parent), m_task(TaskBoard::item(task)), m_sendBlocker(std::move(sendBlocker)) {
    setObjectName("taskDialog"); setWindowTitle(tr("Task %1").arg(m_task.id)); resize(900, 740);
    setMinimumSize(640, 520); setSizeGripEnabled(true);
    m_editable = m_task.writable && (m_task.stage == QLatin1String("todo") || m_task.stage == QLatin1String("backlog"));
    m_target = {m_task.targetSession.isEmpty(), m_task.targetHost, m_task.targetSession, m_task.targetId};
    const auto colors = TaskColors::of(dark);
    auto *layout = new QVBoxLayout(this); layout->setSpacing(8);
    auto *header = new QHBoxLayout; header->setSpacing(10);
    auto *stage = new QLabel(TaskBoard::stageLabel(m_task.stage)); stage->setObjectName("taskDialogStage");
    stage->setStyleSheet(QStringLiteral("color:%1;border:1px solid %1;border-radius:9px;padding:1px 8px;font-size:11px;font-weight:600;")
        .arg(colors.stage(m_task.stage).name()));
    auto *title = new QLabel(QStringLiteral("<b>%1</b>").arg(m_task.id.toHtmlEscaped()));
    auto *meta = new QLabel((m_task.author.isEmpty() ? tr("You") : TaskBoard::sessionLabel(m_task.authorHost, m_task.author)) + QStringLiteral(" · ") + time(m_task.createdAt));
    meta->setObjectName("taskDialogMeta"); meta->setTextFormat(Qt::PlainText);
    meta->setStyleSheet(QStringLiteral("color:%1;font-size:11px;").arg(colors.muted.name()));
    header->addWidget(stage); header->addWidget(title); header->addWidget(meta); header->addStretch(); layout->addLayout(header);
    layout->addWidget(section(tr("Prompt")));
    auto *prompt = new TaskPrompt(m_task.text); m_prompt = prompt;
    prompt->media = [this](const QMimeData *source) { return insertMedia(source); };
    m_prompt->setObjectName("taskPrompt"); m_prompt->setReadOnly(!m_editable);
    m_prompt->setAccessibleName(tr("Task prompt")); layout->addWidget(m_prompt, 2);
    auto *media = new QWidget; m_attachmentRows = new QVBoxLayout(media); m_attachmentRows->setContentsMargins(0, 0, 0, 0);
    auto *mediaArea = new QScrollArea; mediaArea->setObjectName("taskAttachments"); mediaArea->setWidgetResizable(true);
    mediaArea->setFrameShape(QFrame::NoFrame); mediaArea->setMaximumHeight(140); mediaArea->setWidget(media); layout->addWidget(mediaArea);
    if (m_editable) {
        auto *attach = new QPushButton(tr("Attach files…")); attach->setObjectName("attachTaskFiles"); attach->setAutoDefault(false);
        connect(attach, &QPushButton::clicked, this, [this] { const auto paths = QFileDialog::getOpenFileNames(this, tr("Attach files to task")); if (!paths.isEmpty()) addFiles(paths); });
        layout->addWidget(attach, 0, Qt::AlignLeft);
    }
    m_originalAttachments = task.value("attachments").toArray();
    if (!m_originalAttachments.isEmpty()) {
        m_mediaBusy = true;
        AttachmentFiles::runAsync<QList<MessageAttachment>>(this, [files = m_originalAttachments] { return TaskContent::decode(files); },
            [this](const QList<MessageAttachment> &files) {
                m_mediaBusy = false;
                if (files.size() != m_originalAttachments.size()) { setError(tr("The task attachments could not be read.")); m_mediaBusy = true; }
                else { m_attachments = files; refreshAttachments(); }
                refreshButtons();
            });
    }
    refreshAttachments();
    if (m_task.stage == QLatin1String("review") && !m_task.summary.isEmpty()) {
        auto *summary = new QLabel(QStringLiteral("<b>%1 reported:</b> %2").arg(TaskBoard::sessionLabel(m_task.sessionHost, m_task.sessionName).toHtmlEscaped(),
            m_task.summary.toHtmlEscaped()));
        summary->setObjectName("taskDialogSummary"); summary->setWordWrap(true);
        summary->setStyleSheet(QStringLiteral("border-left:2px solid %1;padding:1px 0 1px 10px;").arg(colors.warn.name()));
        layout->addWidget(summary);
    }
    layout->addWidget(section(tr("Session")));
    if (m_editable) {
        // The sessions wrap onto rows and scroll past three, however many the project has.
        auto *area = m_choicesArea = new QScrollArea; area->setObjectName("taskSessionChoices"); area->setWidgetResizable(true);
        area->setFrameShape(QFrame::NoFrame); area->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff); area->setMaximumHeight(104);
        auto *choices = new QWidget; auto *chips = new ScopeFilterWidgets::ChipFlow(choices); m_sessions = new QButtonGroup(this);
        m_choices << TaskTarget{true, {}, {}};
        for (const auto &session : sessions) m_choices << TaskTarget{false, session.host, session.name, session.id};
        // A pinned session that no longer exists stays a choice until it is changed.
        if (!m_target.any && !m_choices.contains(m_target)) m_choices << m_target;
        // Sessions the session list names alike keep their full names here.
        QHash<QString, int> named;
        for (const auto &choice : std::as_const(m_choices)) if (!choice.any) ++named[TaskBoard::sessionLabel(choice.host, choice.session)];
        for (int i = 0; i < m_choices.size(); ++i) {
            const auto &choice = m_choices.at(i);
            const QString label = TaskBoard::sessionLabel(choice.host, choice.session);
            const QString full = choice.session + (choice.host.isEmpty() ? QString() : QStringLiteral(" @") + choice.host);
            auto *chip = new QPushButton(choice.any ? tr("Any session") : named.value(label) > 1 ? full : label);
            chip->setObjectName(QStringLiteral("taskSession:") + (choice.any ? QStringLiteral("any") : choice.host + QLatin1Char('/') + choice.session));
            chip->setProperty("taskSessionChoice", true); chip->setCursor(Qt::PointingHandCursor);
            chip->setCheckable(true); chip->setChecked(choice == m_target); m_sessions->addButton(chip, i); chips->addWidget(chip);
            if (choice.any) continue;
            const auto found = std::find_if(sessions.begin(), sessions.end(), [&](const TaskSession &s) { return s.host == choice.host && s.name == choice.session; });
            chip->setIcon(statusDot(found == sessions.end() ? 0 : found->kind, dark, colors.muted));
            chip->setToolTip(full + (found == sessions.end() ? QString() : QStringLiteral(" · ") + found->status));
        }
        area->setWidget(choices); layout->addWidget(area);
        auto *drag = new QLabel(tr("Or drag the task onto a session in the list; onto the project header for Any session."));
        drag->setObjectName("taskSessionHint"); drag->setProperty("taskSection", true); drag->setWordWrap(true); layout->addWidget(drag);
        connect(m_sessions, &QButtonGroup::idClicked, this, &TaskDialog::refreshButtons);
    } else {
        auto *fixed = new QLabel(m_task.sessionName.isEmpty() ? (m_task.targetSession.isEmpty() ? tr("Any session")
            : TaskBoard::sessionLabel(m_task.targetHost, m_task.targetSession)) : TaskBoard::sessionLabel(m_task.sessionHost, m_task.sessionName));
        fixed->setObjectName("taskDialogSession"); layout->addWidget(fixed);
    }
    layout->addWidget(section(tr("History")));
    m_history = new QTextBrowser; m_history->setObjectName("taskHistory"); m_history->setOpenLinks(false); layout->addWidget(m_history, 1);
    setHistory(task.value("log").toArray());
    auto *comment = new QHBoxLayout;
    m_comment = new QLineEdit; m_comment->setObjectName("taskComment");
    m_comment->setPlaceholderText(tr("Comment — people and agents see it; it is not sent to the session"));
    auto *add = new QPushButton(tr("Add")); add->setObjectName("addTaskComment"); add->setAutoDefault(false);
    const auto commentNow = [this] { if (!m_comment->text().trimmed().isEmpty()) emit commentRequested(m_comment->text().trimmed()); };
    connect(add, &QPushButton::clicked, this, commentNow); connect(m_comment, &QLineEdit::returnPressed, this, commentNow);
    comment->addWidget(m_comment, 1); comment->addWidget(add); layout->addLayout(comment);
    m_error = errorLabel("taskDialogError"); layout->addWidget(m_error);
    auto *actions = new QHBoxLayout; actions->setSpacing(8);
    const auto stageActions = TaskBoard::actions(m_task.stage);
    for (const auto action : stageActions) {
        if (action == TaskAction::Edit) continue;
        auto *button = new QPushButton(workspaceIcon(TaskBoard::actionIcon(action), colors.muted), TaskBoard::actionLabel(action).remove(QChar(0x2026)));
        button->setObjectName(QStringLiteral("taskDialogAction:") + TaskRow::actionKey(action)); button->setAutoDefault(false);
        button->setProperty("taskAction", int(action));
        if (action == stageActions.last()) {
            QColor edge = colors.accent; edge.setAlphaF(.5);
            button->setStyleSheet(QStringLiteral("QPushButton:enabled{color:%1;border-color:%2;}").arg(colors.accent.name(), edge.name(QColor::HexArgb)));
        }
        connect(button, &QPushButton::clicked, this, [this, action] { setBusy(true); emit actionRequested(edits(), action); });
        actions->addWidget(button);
    }
    actions->addStretch();
    if (m_editable) {
        auto *cancel = new QPushButton(tr("Cancel")); cancel->setObjectName("discardTaskEdits"); cancel->setAutoDefault(false);
        auto *save = new QPushButton(tr("Save")); save->setObjectName("saveTask"); save->setDefault(true); save->setProperty("primaryAction", true);
        connect(cancel, &QPushButton::clicked, this, &QDialog::reject);
        connect(save, &QPushButton::clicked, this, [this] { setBusy(true); emit saveRequested(edits()); });
        connect(m_prompt, &QPlainTextEdit::textChanged, this, &TaskDialog::refreshButtons);
        actions->addWidget(cancel); actions->addWidget(save);
    } else {
        auto *close = new QPushButton(tr("Close")); close->setObjectName("closeTask"); close->setDefault(true);
        connect(close, &QPushButton::clicked, this, &QDialog::reject); actions->addWidget(close);
    }
    layout->addLayout(actions);
    refreshButtons();
}

// The sessions take the rows they need, up to three, then scroll.
void TaskDialog::resizeEvent(QResizeEvent *event) {
    QDialog::resizeEvent(event);
    if (m_choicesArea) m_choicesArea->setFixedHeight(qMin(104, m_choicesArea->widget()->layout()->heightForWidth(m_choicesArea->width())));
}

TaskTarget TaskDialog::chosenTarget() const {
    return m_sessions && m_sessions->checkedId() >= 0 ? m_choices.value(m_sessions->checkedId()) : m_target;
}

void TaskDialog::refreshButtons() {
    const bool empty = m_editable && m_prompt->toPlainText().trimmed().isEmpty();
    for (auto *button : findChildren<QPushButton *>()) {
        QString why, label;
        if (button->objectName() == QLatin1String("saveTask")) {
            why = empty ? tr("The prompt is empty.") : QString(); label = tr("Save the prompt and the session");
        } else if (button->property("taskAction").isValid()) {
            const auto action = TaskAction(button->property("taskAction").toInt());
            label = TaskBoard::actionLabel(action);
            why = !m_task.writable ? tr("Update Zerus to edit this task") : empty ? tr("The prompt is empty.")
                : action == TaskAction::SendNow && m_sendBlocker ? m_sendBlocker(chosenTarget()) : QString();
        } else continue;
        button->setEnabled(!m_busy && !m_mediaBusy && why.isEmpty()); button->setToolTip(why.isEmpty() ? label : why);
    }
    if (auto *attach = findChild<QPushButton *>("attachTaskFiles")) attach->setEnabled(!m_busy && !m_mediaBusy);
    for (auto *remove : findChildren<QPushButton *>("removeTaskAttachment")) remove->setEnabled(!m_busy && !m_mediaBusy);
    m_prompt->setReadOnly(!m_editable || m_busy || m_mediaBusy);
}

TaskEdits TaskDialog::edits() const {
    TaskEdits edits;
    if (!m_editable) return edits;
    if (const QString text = m_prompt->toPlainText(); text != m_task.text && !text.trimmed().isEmpty()) edits.text = text;
    if (const auto chosen = chosenTarget(); !(chosen == m_target)) edits.target = chosen;
    const auto files = TaskContent::encode(m_attachments);
    if (files != m_originalAttachments) edits.attachments = files;
    return edits;
}

bool TaskDialog::insertMedia(const QMimeData *source) {
    if (!source->hasImage() && !source->hasUrls()) return false;
    if (!m_editable || m_busy || m_mediaBusy) return true;
    if (source->hasImage()) {
        const auto image = qvariant_cast<QImage>(source->imageData());
        m_mediaBusy = true; refreshButtons();
        AttachmentFiles::runAsync<QByteArray>(this, [image] {
            QByteArray bytes; QBuffer buffer(&bytes); buffer.open(QIODevice::WriteOnly);
            image.save(&buffer, "PNG"); return bytes;
        }, [this](const QByteArray &bytes) {
            m_mediaBusy = false; addAttachments({MessageAttachment{"image.png", "image/png", bytes, {}}}); refreshButtons();
        });
    } else {
        QStringList paths;
        for (const auto &url : source->urls()) if (url.isLocalFile()) paths << url.toLocalFile();
        if (paths.isEmpty()) return false;
        addFiles(paths);
    }
    return true;
}

void TaskDialog::addFiles(const QStringList &paths) {
    if (!m_editable || m_busy || m_mediaBusy) return;
    if (paths.size() + m_attachments.size() > HgsClient::MaximumAttachments) { setError(tr("Attach up to 8 files to a task.")); return; }
    struct Loaded { QList<MessageAttachment> files; QString error; };
    m_mediaBusy = true; refreshButtons();
    AttachmentFiles::runAsync<Loaded>(this, [paths] {
        Loaded out; qsizetype total = 0;
        for (const auto &path : paths) {
            const QFileInfo info(path); QFile file(path);
            if (!LocalFile::readable(info) || LocalFile::size(info) > HgsClient::MaximumAttachmentBytes || !file.open(QIODevice::ReadOnly)) {
                out.error = QObject::tr("Choose readable files of at most 10 MiB each."); break;
            }
            const auto bytes = file.read(HgsClient::MaximumAttachmentBytes + 1); total += bytes.size();
            if (bytes.isEmpty() || bytes.size() > HgsClient::MaximumAttachmentBytes || total > HgsClient::MaximumAttachmentsBytes) {
                out.error = QObject::tr("Attachments: 10 MiB per file, 20 MiB per task."); break;
            }
            out.files << MessageAttachment{info.fileName(), QMimeDatabase().mimeTypeForData(bytes).name(), bytes, {}};
        }
        return out;
    }, [this](const Loaded &loaded) {
        m_mediaBusy = false;
        if (!loaded.error.isEmpty()) setError(loaded.error); else addAttachments(loaded.files);
        refreshButtons();
    });
}

void TaskDialog::addAttachments(const QList<MessageAttachment> &files) {
    qsizetype total = 0; for (const auto &file : m_attachments) total += file.data.size();
    for (const auto &file : files) total += file.data.size();
    if (files.size() + m_attachments.size() > HgsClient::MaximumAttachments || total > HgsClient::MaximumAttachmentsBytes
        || std::any_of(files.begin(), files.end(), [](const auto &file) { return file.data.isEmpty() || file.data.size() > HgsClient::MaximumAttachmentBytes; })) {
        setError(tr("Attach up to 8 nonempty files: 10 MiB each, 20 MiB per task.")); return;
    }
    for (auto file : files) {
        int number = 1;
        do { file.reference = QString("[%1 #%2]").arg(file.mime.startsWith("image/") ? "Image" : "File").arg(number++); }
        while (m_prompt->toPlainText().contains(file.reference) || std::any_of(m_attachments.begin(), m_attachments.end(), [&](const auto &other) { return other.reference == file.reference; }));
        m_attachments << file;
        auto cursor = m_prompt->textCursor(); cursor.insertText(QLatin1Char(' ') + file.reference + QLatin1Char(' ')); m_prompt->setTextCursor(cursor);
    }
    refreshAttachments();
}

void TaskDialog::refreshAttachments() {
    while (auto *item = m_attachmentRows->takeAt(0)) { if (auto *widget = item->widget()) { widget->hide(); widget->deleteLater(); } delete item; }
    for (int i = 0; i < m_attachments.size(); ++i) {
        const auto file = m_attachments.at(i);
        auto *row = new QWidget; auto *layout = new QHBoxLayout(row); layout->setContentsMargins(0, 0, 0, 0);
        auto *open = new QPushButton(file.reference + QLatin1Char(' ') + file.name); open->setObjectName("openTaskAttachment");
        open->setAutoDefault(false); open->setToolTip(tr("Open attached file"));
        if (file.mime.startsWith("image/")) {
            QPointer<QPushButton> guard(open);
            AttachmentFiles::runAsync<QImage>(this, [bytes = file.data] {
                QBuffer buffer; buffer.setData(bytes); buffer.open(QIODevice::ReadOnly); QImageReader reader(&buffer);
                const auto size = reader.size();
                if (!size.isValid() || qint64(size.width()) * size.height() > 100000000) return QImage();
                reader.setScaledSize(size.scaled(96, 72, Qt::KeepAspectRatio)); return reader.read();
            }, [guard](const QImage &image) { if (guard && !image.isNull()) { guard->setIcon(QPixmap::fromImage(image)); guard->setIconSize(image.size()); } });
        }
        connect(open, &QPushButton::clicked, this, [this, file] {
            AttachmentFiles::prepareAsync({file}, this, [this](const QJsonArray &files, const QString &error) {
                if (!error.isEmpty() || files.isEmpty()) { setError(error); return; }
                const auto path = files.first().toObject().value("local_path").toString();
                if (ExternalFile::open(path, ExternalFile::Origin::OtherMachine) == ExternalFile::Outcome::OpenFailed)
                    setError(tr("Could not open the attachment."));
            });
        });
        layout->addWidget(open, 1);
        if (m_editable) {
            auto *remove = new QPushButton(tr("Remove")); remove->setObjectName("removeTaskAttachment"); remove->setAutoDefault(false);
            connect(remove, &QPushButton::clicked, this, [this, i] {
                if (m_busy || m_mediaBusy) return;
                const auto reference = m_attachments.takeAt(i).reference;
                if (!reference.isEmpty()) { auto cursor = m_prompt->document()->find(reference); if (!cursor.isNull()) cursor.removeSelectedText(); }
                refreshAttachments();
            });
            layout->addWidget(remove);
        }
        m_attachmentRows->addWidget(row);
    }
    if (auto *area = findChild<QScrollArea *>("taskAttachments")) area->setVisible(!m_attachments.isEmpty());
}

void TaskDialog::setHistory(const QJsonArray &log) {
    m_log = log;
    QString html;
    for (const auto &entry : log) html += taskHistoryLine(entry.toObject());
    m_history->setHtml(html);
}

void TaskDialog::updateSessionLabels(const QList<TaskSession> &sessions) {
    bool changed = false;
    const auto name = [&](const QString &host, const QString &id, QString &label) {
        if (id.isEmpty()) return;
        for (const auto &session : sessions) if (session.host == host && session.id == id && session.name != label) {
            label = session.name; changed = true; break;
        }
    };
    name(m_task.targetHost, m_task.targetId, m_task.targetSession);
    name(m_task.sessionHost, m_task.sessionId, m_task.sessionName);
    name(m_task.authorHost, m_task.authorId, m_task.author);
    name(m_target.host, m_target.id, m_target.session);
    for (int i = 0; i < m_choices.size(); ++i) {
        auto &choice = m_choices[i]; name(choice.host, choice.id, choice.session);
    }
    auto log = m_log;
    for (auto it = log.begin(); it != log.end(); ++it) {
        auto entry = it->toObject();
        for (const auto &key : {"session", "actor"}) if (entry.value(key).isObject()) {
            auto reference = entry.value(key).toObject(); QString label = reference.value("session").toString();
            name(reference.value("host").toString(), reference.value("session_id").toString(), label);
            reference["session"] = label; entry[key] = reference;
        }
        *it = entry;
    }
    if (!changed) return;
    QHash<QString, int> named;
    for (const auto &choice : std::as_const(m_choices)) if (!choice.any) ++named[TaskBoard::sessionLabel(choice.host, choice.session)];
    for (int i = 0; i < m_choices.size(); ++i) {
        const auto &choice = m_choices[i];
        if (!choice.any && m_sessions) if (auto *button = m_sessions->button(i)) {
            const auto label = TaskBoard::sessionLabel(choice.host, choice.session);
            const auto full = choice.session + (choice.host.isEmpty() ? QString() : QStringLiteral(" @") + choice.host);
            button->setText(named.value(label) > 1 ? full : label); button->setToolTip(full);
        }
    }
    if (auto *meta = findChild<QLabel *>("taskDialogMeta")) meta->setText((m_task.author.isEmpty() ? tr("You") : TaskBoard::sessionLabel(m_task.authorHost, m_task.author)) + QStringLiteral(" · ") + time(m_task.createdAt));
    if (auto *fixed = findChild<QLabel *>("taskDialogSession")) fixed->setText(m_task.sessionName.isEmpty()
        ? (m_task.targetSession.isEmpty() ? tr("Any session") : TaskBoard::sessionLabel(m_task.targetHost, m_task.targetSession)) : TaskBoard::sessionLabel(m_task.sessionHost, m_task.sessionName));
    if (auto *summary = findChild<QLabel *>("taskDialogSummary")) summary->setText(QStringLiteral("<b>%1 reported:</b> %2")
        .arg(TaskBoard::sessionLabel(m_task.sessionHost, m_task.sessionName).toHtmlEscaped(), m_task.summary.toHtmlEscaped()));
    setHistory(log);
}

void TaskDialog::clearComment() { m_comment->clear(); }

// While hgs works the buttons wait; a refusal shows here and gives them back.
void TaskDialog::setBusy(bool busy) {
    m_busy = busy; refreshButtons();
    if (busy) m_error->hide();
}

void TaskDialog::setError(const QString &error) { m_error->setText(error); m_error->show(); setBusy(false); }

TaskCancelDialog::TaskCancelDialog(const TaskItem &task, bool running, bool working, QWidget *parent) : QDialog(parent) {
    setObjectName("taskCancelDialog"); setWindowTitle(tr("Cancel %1?").arg(task.id));
    auto *layout = new QVBoxLayout(this); layout->setSpacing(10);
    auto *title = new QLabel(QStringLiteral("<b>%1</b>").arg(tr("Cancel %1?").arg(task.id).toHtmlEscaped())); layout->addWidget(title);
    auto *text = new QLabel(task.text); text->setObjectName("taskCancelText"); text->setWordWrap(true); text->setTextFormat(Qt::PlainText);
    text->setMaximumHeight(text->fontMetrics().lineSpacing() * 4 + 4); layout->addWidget(text);
    const QString session = TaskBoard::sessionLabel(task.sessionHost, task.sessionName);
    QString what = tr("%1 moves to In review, and %2 gets the cancel message.").toHtmlEscaped()
        .arg(QStringLiteral("<b>%1</b>").arg(task.id.toHtmlEscaped()), session.toHtmlEscaped());
    if (!running) what += QLatin1Char(' ') + tr("The agent is not running, so hgs will refuse the message; the task is cancelled all the same.").toHtmlEscaped();
    else if (working) what += QLatin1Char(' ') + tr("%1 is working; it reads the message when its turn ends.").toHtmlEscaped().arg(session.toHtmlEscaped());
    auto *warning = new QLabel(what); warning->setObjectName("taskCancelWarning"); warning->setTextFormat(Qt::RichText); warning->setWordWrap(true);
    layout->addWidget(warning);
    layout->addWidget(section(tr("Reason (optional) — saved as a comment on the task, not sent to the agent")));
    m_reason = new QLineEdit; m_reason->setObjectName("taskCancelReason"); m_reason->setPlaceholderText(tr("Why it is cancelled"));
    layout->addWidget(m_reason);
    auto *buttons = new QHBoxLayout;
    auto *keep = new QPushButton(tr("Keep running")); keep->setObjectName("keepTaskRunning"); keep->setDefault(true);
    auto *cancel = new QPushButton(tr("Cancel task")); cancel->setObjectName("confirmTaskCancel"); cancel->setAutoDefault(false);
    connect(keep, &QPushButton::clicked, this, &QDialog::reject); connect(cancel, &QPushButton::clicked, this, &QDialog::accept);
    buttons->addWidget(keep); buttons->addStretch(); buttons->addWidget(cancel); layout->addLayout(buttons);
}

QString TaskCancelDialog::reason() const { return m_reason->text().trimmed(); }

TaskSettingsDialog::TaskSettingsDialog(const QString &projectName, const QJsonObject &settings, QWidget *parent)
    : QDialog(parent), m_settings(settings) {
    setObjectName("taskSettingsDialog"); setWindowTitle(tr("Project tasks · %1").arg(projectName)); resize(620, 560);
    auto *layout = new QVBoxLayout(this); layout->setSpacing(8);
    layout->addWidget(heading(tr("Service prompts"), "taskServicePrompts"));
    // The templates are code: a fixed-pitch font, as the session list and Activity name it.
    QFont code(monospaceFamily()); code.setStyleHint(QFont::TypeWriter); code.setPixelSize(12);
    layout->addWidget(section(tr("Task (Send now)")));
    m_task = new QPlainTextEdit(settings.value("task_template").toString()); m_task->setObjectName("taskTemplate"); m_task->setFont(code);
    layout->addWidget(m_task, 1);
    layout->addWidget(section(tr("Cancel message")));
    m_cancel = new QPlainTextEdit(settings.value("cancel_template").toString()); m_cancel->setObjectName("cancelTemplate"); m_cancel->setFont(code);
    layout->addWidget(m_cancel, 1);
    auto *hint = section(tr("Placeholders: {ref}, {author}, {prompt}, {tools}, {history}")); hint->setObjectName("taskTemplateHint");
    layout->addWidget(hint);
    layout->addSpacing(6); layout->addWidget(heading(tr("Agents"), "taskAgentsHeading"));
    m_agentsOther = new QCheckBox(tr("Agents may start and cancel tasks in other sessions")); m_agentsOther->setObjectName("agentsOther");
    m_agentsOther->setChecked(settings.value("agents_other").toBool(true)); layout->addWidget(m_agentsOther);
    auto *limit = new QHBoxLayout;
    m_limit = new QSpinBox; m_limit->setObjectName("agentLimit"); m_limit->setRange(0, 1000); m_limit->setSpecialValueText(tr("no limit"));
    m_limit->setValue(settings.value("agent_limit").toInt());
    limit->addWidget(new QLabel(tr("Limit tasks started by agents"))); limit->addWidget(m_limit); limit->addWidget(new QLabel(tr("per hour")));
    limit->addStretch(); layout->addLayout(limit);
    m_error = errorLabel("taskSettingsError"); layout->addWidget(m_error);
    auto *buttons = new QHBoxLayout; buttons->addStretch();
    auto *cancel = new QPushButton(tr("Cancel")); cancel->setObjectName("cancelTaskSettings"); cancel->setAutoDefault(false);
    m_save = new QPushButton(tr("Save")); m_save->setObjectName("saveTaskSettings"); m_save->setDefault(true); m_save->setProperty("primaryAction", true);
    connect(cancel, &QPushButton::clicked, this, &QDialog::reject);
    connect(m_save, &QPushButton::clicked, this, [this] { m_save->setEnabled(false); m_error->hide(); emit saveRequested(changes()); });
    buttons->addWidget(cancel); buttons->addWidget(m_save); layout->addLayout(buttons);
}

void TaskSettingsDialog::setError(const QString &error) { m_error->setText(error); m_error->show(); m_save->setEnabled(true); }

QJsonObject TaskSettingsDialog::changes() const {
    QJsonObject changes;
    const auto changed = [&](const char *key, const QJsonValue &value) { if (m_settings.value(key) != value) changes.insert(key, value); };
    changed("task_template", m_task->toPlainText()); changed("cancel_template", m_cancel->toPlainText());
    changed("agents_other", m_agentsOther->isChecked()); changed("agent_limit", m_limit->value());
    return changes;
}
