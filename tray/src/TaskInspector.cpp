#include "Settings.h"
#include "SettingsJson.h"
#include "TaskInspector.h"
#include <QScopeGuard>
#include "ActivityWidth.h"
#include "ActivityView.h"
#include "TaskDialogs.h"
#include "TaskContent.h"
#include "MessageComposer.h"
#include "ComposerToolbar.h"
#include <QUuid>
#include <QJsonDocument>
#include <QKeyEvent>
#include <QMouseEvent>
#include "AttachmentViewer.h"
#include <QMenu>
#include <QWidgetAction>
#include "TaskRow.h"
#include "TaskStageFilter.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"
#include <QDateTime>
#include <QDropEvent>
#include <QFrame>
#include <QScrollBar>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QToolButton>
#include <QComboBox>
#include <QPushButton>
#include <QResizeEvent>
#include <QScrollArea>
#include <QSet>
#include <QTextBrowser>
#include <QPlainTextEdit>
#include <QStyle>
#include <QTabBar>
#include <QSettings>
#include <QSignalBlocker>
#include <QSplitter>
#include <QStackedWidget>
#include <QVBoxLayout>
#include <algorithm>

namespace {
constexpr int VisibleTasks = 100;
constexpr int VisibleHistory = 80;
// Native Cocoa tab sizing can elide these labels even in a wide pane. Use the
// actual label font to reserve a complete caption on every platform.
class DetailTabs : public QTabBar {
protected:
    QSize tabSizeHint(int index) const override {
        return {fontMetrics().horizontalAdvance(tabText(index)) + 24, 34};
    }
};
void stableIcon(QAbstractButton *button, const QString &name, const QColor &color) {
    const auto key = name + color.name(QColor::HexArgb);
    if (button->property("taskIconKey").toString() == key) return;
    button->setProperty("taskIconKey", key); button->setIcon(workspaceIcon(name, color));
}
QString authorLabel(const QJsonObject &actor) {
    if (actor.value("kind") != "agent") return QObject::tr("You");
    const auto name = TaskBoard::sessionLabel(actor.value("host").toString(), actor.value("session").toString());
    return name.isEmpty() ? QObject::tr("Agent") : name;
}
QJsonObject journalEntry(const QString &id, const QJsonObject &actor, const QString &label, const QString &text, double at, bool event = false) {
    return {{"id", id}, {"author", authorLabel(actor)}, {"label", label}, {"text", text}, {"at", at},
        {"role", event ? QStringLiteral("event") : actor.value("kind") == "agent" ? QStringLiteral("agent") : QStringLiteral("person")}};
}
QString sessionId(const QString &host, const QString &name, const QList<TaskSession> &sessions) {
    for (const auto &session : sessions) if (session.host == host && session.name == name) return session.id;
    return {};
}
}

TaskInspector::TaskInspector(HgsClient *client, TasksView *board, QWidget *parent)
    : QWidget(parent), m_client(client), m_board(board) {
    setObjectName("taskInspector");
    auto *layout = new QVBoxLayout(this); layout->setContentsMargins(2, 10, 2, 0); layout->setSpacing(8);
    auto *header = new QHBoxLayout;
    m_scope = new QLabel; m_scope->setObjectName("taskInspectorScope"); m_scope->setTextFormat(Qt::PlainText); m_scope->setWordWrap(true);
    m_creationHeader = new QStackedWidget; m_creationHeader->setObjectName("taskCreationHeader"); m_creationHeader->setFixedHeight(32);
    m_creationHeader->addWidget(m_scope); header->addWidget(m_creationHeader, 1);
    m_add = new QToolButton; m_add->setObjectName("taskNewCard"); m_add->setFixedSize(32, 32); m_add->setIconSize({20, 20});
    m_add->setEnabled(false); m_add->setToolTip(tr("New task")); m_add->setAccessibleName(m_add->toolTip()); header->addWidget(m_add);
    m_board->moveControlsTo(header); layout->addLayout(header);
    m_newTitle = new QLineEdit; m_newTitle->setObjectName("taskNewTitle"); m_newTitle->setMaxLength(512);
    m_newTitle->installEventFilter(this); m_newTitle->setPlaceholderText(tr("Task title · Enter to create")); m_newTitle->setAccessibleName(tr("New task title")); m_creationHeader->addWidget(m_newTitle);
    m_createError = new QLabel; m_createError->setObjectName("taskCreateError"); m_createError->setWordWrap(true); m_createError->setTextFormat(Qt::PlainText); m_createError->hide();
    connect(m_add, &QToolButton::clicked, this, [this] { m_creationHeader->setCurrentWidget(m_newTitle); m_newTitle->setFocus(); });
    connect(m_newTitle, &QLineEdit::textEdited, this, [this](const QString &text) {
        if (m_creation.isEmpty()) Settings::set("tasks/newCard/" + m_project + "/title", text);
    });
    connect(m_newTitle, &QLineEdit::returnPressed, this, &TaskInspector::createCard);
    m_filters = new QWidget; auto *filters = new QHBoxLayout(m_filters);
    filters->setContentsMargins(0, 0, 0, 0); filters->setSpacing(4);
    for (const auto &stage : TaskBoard::filters()) {
        auto *tile = new TaskTile(stage, m_filters); m_tiles.insert(stage, tile); filters->addWidget(tile, 1);
        tile->setAcceptDrops(stage != QLatin1String("current")); tile->installEventFilter(this);
        connect(tile, &QAbstractButton::clicked, this, [this, stage] {
            if (m_stage != stage && !canLeaveDetails()) return;
            if (m_stage != stage) { m_stage = stage; m_showOlder = false; if (!m_detached) { closeInlineDetails(); m_selected.clear(); m_detail = {}; ++m_generation; } }
            refresh();
        });
    }
    layout->addWidget(m_filters);
    m_scroll = new QScrollArea; m_scroll->setObjectName("taskInspectorList"); m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame); m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_listPage = new QWidget; m_listPage->setObjectName("taskInspectorCards");
    m_listPage->setAcceptDrops(true); m_listPage->installEventFilter(this);
    m_dropLine = new QFrame(m_listPage); m_dropLine->setObjectName("taskInspectorDropLine"); m_dropLine->hide();
    m_autoScroll.setInterval(30);
    connect(&m_autoScroll, &QTimer::timeout, this, [this] {
        m_scroll->verticalScrollBar()->setValue(m_scroll->verticalScrollBar()->value() + m_scrollStep);
        followDrag(m_drag, m_listPage->mapFrom(m_scroll->viewport(), m_dragAt));
    });
    connect(m_board, &TasksView::taskDragFinished, this, [this] {
        endListDrag(); for (auto *tile : std::as_const(m_tiles)) tile->setDropTarget(false);
    });
    m_cards = new QVBoxLayout(m_listPage); m_cards->setContentsMargins(0, 0, 0, 0); m_cards->setSpacing(4); m_cards->addStretch();
    m_scroll->setWidget(m_listPage); m_scroll->setMinimumHeight(150);
    auto *boardPane = new QWidget; auto *boardLayout = new QVBoxLayout(boardPane);
    boardLayout->setContentsMargins(0, 0, 0, 0); boardLayout->setSpacing(0); boardLayout->addWidget(m_scroll, 1);
    m_older = new QPushButton; m_older->setObjectName("taskInspectorOlder"); m_older->hide(); boardLayout->addWidget(m_older);
    boardLayout->addWidget(m_createError);
    connect(m_older, &QPushButton::clicked, this, [this] { m_showOlder = !m_showOlder; refresh(); });
    m_splitter = new QSplitter(Qt::Vertical); m_splitter->setObjectName("taskInspectorSplit");
    m_splitter->setHandleWidth(7); m_splitter->setChildrenCollapsible(false); m_splitter->addWidget(boardPane);
    layout->addWidget(m_splitter, 1);
    m_details = new QWidget; m_details->setObjectName("taskDetailsPane"); m_details->setMinimumHeight(220); m_details->installEventFilter(this);
    auto *details = new QVBoxLayout(m_details); details->setContentsMargins(0, 4, 0, 0); details->setSpacing(4);
    m_headerColumn = new ActivityWidth::ColumnLayout; details->addLayout(m_headerColumn);
    auto *heading = new QHBoxLayout; heading->setContentsMargins(14, 10, 14, 0); heading->setSpacing(12);
    m_title = new QLabel; m_title->setObjectName("taskViewTitle"); m_title->setTextFormat(Qt::PlainText); m_title->setWordWrap(true);
    m_title->setCursor(Qt::IBeamCursor); m_title->installEventFilter(this);
    auto *titleColumn = new QVBoxLayout; titleColumn->setContentsMargins(0, 0, 0, 0); titleColumn->setSpacing(4); titleColumn->addWidget(m_title);
    m_titleEditor = new QPlainTextEdit; m_titleEditor->setObjectName("taskTitleEditor"); m_titleEditor->setAccessibleName(tr("Task title · Enter to save, Escape to cancel"));
    m_titleEditor->setTabChangesFocus(true); m_titleEditor->installEventFilter(this); m_titleEditor->hide(); titleColumn->addWidget(m_titleEditor);
    connect(m_titleEditor, &QPlainTextEdit::textChanged, this, [this] {
        if (m_titleEditor->isVisible() && !m_titleSaving) Settings::set("tasks/titleDraft/" + m_project + '/' + m_selected, m_titleEditor->toPlainText());
    });
    m_titleError = new QLabel; m_titleError->setObjectName("taskTitleError"); m_titleError->setTextFormat(Qt::PlainText); m_titleError->setWordWrap(true); m_titleError->hide(); titleColumn->addWidget(m_titleError);
    heading->addLayout(titleColumn, 1);
    auto *rename = new QPushButton; rename->setObjectName("taskViewRename"); rename->setProperty("taskHeaderButton", true);
    rename->setFixedSize(32, 32); rename->setIconSize({20, 20}); rename->setToolTip(tr("Task actions")); rename->setAccessibleName(rename->toolTip());
    heading->addWidget(rename, 0, Qt::AlignTop); m_headerColumn->addLayout(heading);
    m_detailTabs = new DetailTabs; m_detailTabs->setObjectName("taskDetailTabs");
    m_detailTabs->addTab(tr("Task")); m_detailTabs->addTab(tr("Result")); m_detailTabs->addTab(tr("History"));
    // These are internal destinations, reached through the task menu. The normal
    // page contains both the document and its continuing discussion.
    m_detailTabs->hide(); details->addWidget(m_detailTabs);
    connect(rename, &QPushButton::clicked, this, [this, rename] {
        if (selectionPending()) return;
        auto *menu = new QMenu(rename); menu->setAttribute(Qt::WA_DeleteOnClose);
        const auto task = TaskBoard::item(labels(m_detail));
        menu->addAction(tr("Rename task"), this, &TaskInspector::beginRename)->setEnabled(task.writable);
        menu->addAction(tr("Task and discussion"), this, [this] { m_detailTabs->setCurrentIndex(0); });
        menu->addAction(tr("Result"), this, [this] { m_detailTabs->setCurrentIndex(1); });
        menu->addAction(tr("History"), this, [this] { m_detailTabs->setCurrentIndex(2); });
        menu->popup(rename->mapToGlobal(QPoint(0, rename->height())));
    });
    auto *context = new QHBoxLayout; context->setContentsMargins(14, 4, 14, 8); context->setSpacing(8);
    m_identity = new QLabel; m_identity->setObjectName("taskViewIdentity"); m_identity->setTextFormat(Qt::PlainText); context->addWidget(m_identity);
    m_assignmentRow = new QWidget; auto *assignment = new QHBoxLayout(m_assignmentRow); assignment->setContentsMargins(0, 0, 0, 0); assignment->setSpacing(4);
    m_assignment = new ToolbarChip; m_assignment->setObjectName("taskInspectorAssignment"); static_cast<ToolbarChip *>(m_assignment)->setTone(ChipTone::Quiet); static_cast<ToolbarChip *>(m_assignment)->setActive(true);
    m_assignment->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed); m_assignment->setMinimumWidth(90); m_assignment->setMaximumWidth(320); m_assignment->setAutoDefault(false);
    m_assignment->setAccessibleName(tr("Task executor")); assignment->addWidget(m_assignment, 1); context->addWidget(m_assignmentRow, 1);
    m_open = new QPushButton; m_open->setObjectName("taskOpenActivity"); m_open->setProperty("taskHeaderButton", true);
    m_edit = new QPushButton; m_edit->setObjectName("taskInspectorEdit"); m_edit->setProperty("taskHeaderButton", true);
    m_open->setFixedSize(30, 28); m_edit->setFixedSize(30, 28);
    for (auto *button : {m_open, m_edit}) { button->setIconSize({20, 20}); context->addWidget(button); }
    m_open->setToolTip(tr("Open executor Activity")); m_open->setAccessibleName(m_open->toolTip());
    m_edit->setToolTip(tr("Edit description")); m_edit->setAccessibleName(m_edit->toolTip());
    connect(m_open, &QPushButton::clicked, this, [this] {
        if (selectionPending()) return;
        if (const auto *found = executor()) {
            const auto worker = m_detail.value("worker").toObject();
            if (!worker.isEmpty()) emit subagentRequested(found->host, found->name, found->id, worker.value("conversation_id").toString(), worker.value("id").toString());
            else emit executorRequested(found->host, found->name, found->id);
        }
    });
    connect(m_edit, &QPushButton::clicked, this, &TaskInspector::openInlineDetails);
    connect(m_assignment, &QPushButton::clicked, this, [this] {
        const auto stage = m_detail.value("stage").toString();
        if (stage == "todo" || stage == "backlog") showAssignment(); else m_open->click();
    });
    m_headerColumn->addLayout(context);
    m_state = new QLabel; m_state->setObjectName("taskInspectorState"); m_state->setTextFormat(Qt::PlainText); m_state->setWordWrap(true);
    m_headerColumn->addWidget(m_state);
    m_moreDetail = new QPushButton; m_moreDetail->setObjectName("taskDetailMore"); m_moreDetail->setFlat(true); m_moreDetail->hide();
    auto *historyNavigation = new QHBoxLayout; historyNavigation->addWidget(m_moreDetail, 0, Qt::AlignLeft); historyNavigation->addStretch();
    m_latestHistory = new QPushButton(tr("Latest")); m_latestHistory->setObjectName("taskHistoryLatest"); m_latestHistory->setFlat(true); m_latestHistory->hide();
    historyNavigation->addWidget(m_latestHistory); details->addLayout(historyNavigation);
    connect(m_latestHistory, &QPushButton::clicked, this, [this] { m_historyBefore = {}; m_historyVisible = 8; ++m_generation; fetch(); });
    connect(m_moreDetail, &QPushButton::clicked, this, [this] {
        if (m_detailTabs->currentIndex() == 2 && m_historyVisible < m_detail.value("log").toArray().size()) {
            m_historyVisible += 8; renderDetail();
        } else if (m_detailTabs->currentIndex() == 2 && m_detail.value("history_end").toInt() > m_detail.value("log").toArray().size()) {
            m_historyBefore = m_detail.value("log").toArray().first().toObject(); m_historyVisible = 8; ++m_generation; fetch();
        } else openInlineDetails();
    });
    m_content = new QStackedWidget; m_content->setObjectName("taskDetailContent");
    m_emptyDetail = new QLabel; m_emptyDetail->setObjectName("taskDetailEmpty"); m_emptyDetail->setAlignment(Qt::AlignCenter);
    m_emptyDetail->setWordWrap(true); m_content->addWidget(m_emptyDetail);
    m_activity = new ActivityView; m_activity->setObjectName("taskDetailActivity");
    m_activity->browser()->setObjectName("taskInspectorDetail"); m_activity->browser()->setAccessibleName(tr("Selected task details"));
    // Task files are resolved only from their content-addressed private cache.
    connect(m_activity, &ActivityView::fileReferenceActivated, this, [this] {
        openInlineDetails();
    });
    connect(m_activity, &ActivityView::attachmentActivated, this, [this](const QJsonObject &file) {
        if (selectionPending()) return;
        const auto path = AttachmentFiles::cached(file);
        if (path.isEmpty()) return;
        const auto outcome = ExternalFile::open(path, ExternalFile::Origin::OtherMachine);
        if (outcome == ExternalFile::Outcome::OpenFailed || outcome == ExternalFile::Outcome::RevealFailed) {
            m_noteError->setText(tr("Could not open this attachment.")); m_noteError->show();
        }
    });
    connect(m_activity, &ActivityView::attachmentPreviewRequested, this, [this](const QString &key, const QJsonObject &file) {
        const auto path = AttachmentFiles::cached(file); if (path.isEmpty()) return;
        const auto generation = m_journalGeneration;
        AttachmentFiles::runAsync<QImage>(this, [path] { return AttachmentLoader::thumbnail(path); }, [this, key, generation](const QImage &image) {
            if (generation == m_journalGeneration) m_activity->setAttachmentPreview(key, image);
        });
    });
    m_content->addWidget(m_activity); details->addWidget(m_content, 1);
    connect(m_activity, &ActivityView::taskDocumentEditRequested, this, &TaskInspector::openInlineDetails);
    m_commentComposer = new MessageComposer; m_commentComposer->setObjectName("taskCommentComposer");
    m_commentComposer->setTaskMode(true); m_commentComposer->setDraftCacheLimit(2);
    m_commentComposer->setSendLabel(tr("Add comment")); m_commentComposer->setSendIconOnly(true); m_commentComposer->setPlaceholder(tr("Comment on this task…"));
    m_commentComposer->editor()->setObjectName("taskCommentInput"); m_commentComposer->editor()->installEventFilter(this);
    m_inputColumn = new ActivityWidth::ColumnLayout; details->addLayout(m_inputColumn);
    m_messageMode = new ToolbarChip; m_messageMode->setObjectName("taskMessageMode"); m_messageMode->setTone(ChipTone::Quiet); m_messageMode->setActive(true);
    m_commentComposer->toolbar()->add(ComposerToolbar::Slot::Recovery, m_messageMode);
    m_checkMessage = new QPushButton(tr("Recover saved comment")); m_checkMessage->setObjectName("taskMessageCheck"); m_checkMessage->setProperty("taskHeaderButton", true);
    m_checkMessage->hide(); m_checkMessage->setFixedHeight(24); m_commentComposer->toolbar()->add(ComposerToolbar::Slot::Recovery, m_checkMessage);
    m_checkMessage->setToolTip(tr("Keep the previous comment in the task and check its delivery. It will not be sent again."));
    auto *recoveryMenu = new QMenu(m_checkMessage); m_checkMessage->setContextMenuPolicy(Qt::CustomContextMenu);
    connect(m_checkMessage, &QWidget::customContextMenuRequested, this, [this, recoveryMenu](const QPoint &point) { recoveryMenu->popup(m_checkMessage->mapToGlobal(point)); });
    auto *checkReceipt = recoveryMenu->addAction(tr("Check delivery receipt"));
    connect(m_checkMessage, &QPushButton::clicked, checkReceipt, &QAction::trigger);
    auto *releaseRequest = recoveryMenu->addAction(tr("Release local recovery record…"));
    connect(releaseRequest, &QAction::triggered, this, [this] {
        if (!canLeaveDetails() || selectionPending()) return;
        auto *confirm = new QMessageBox(QMessageBox::Warning, tr("Release message recovery"),
            tr("The previous message may already have reached the executor. Releasing this local record neither cancels nor resends it. Check the task and executor Activity before sending another message."),
            QMessageBox::Cancel | QMessageBox::Yes, m_details);
        confirm->setAttribute(Qt::WA_DeleteOnClose); confirm->setDefaultButton(QMessageBox::Cancel);
        const auto uuid = m_selected;
        connect(confirm, &QDialog::finished, this, [this, uuid](int result) {
            if (result == QMessageBox::Yes) m_board->releaseTaskMessage(uuid, [this] { renderDetail(); }, [this](const QString &error) { m_noteError->setText(error); m_noteError->show(); });
        }); confirm->open();
    });
    connect(checkReceipt, &QAction::triggered, this, [this] {
        if (!canLeaveDetails() || selectionPending()) return;
        m_noteBusy = true; const auto key = inputKey(m_detail);
        m_board->recoverTaskMessage(m_selected, [this, key](const QJsonObject &content) {
            m_noteBusy = false;
            if (key == m_commentComposer->sessionKey() && m_commentComposer->editor()->toPlainText() == content.value("text").toString()
                && TaskContent::encode(m_commentComposer->draftSnapshot().attachments) == content.value("attachments").toArray()) m_commentComposer->deliveryFinished(key, true);
            fetch();
        }, [this](const QString &error) { m_noteBusy = false; m_noteError->setText(error); m_noteError->show(); });
    });
    m_cancelDescription = new QPushButton(tr("Cancel")); m_cancelDescription->setObjectName("taskDescriptionCancel"); m_cancelDescription->setProperty("taskHeaderButton", true);
    m_cancelDescription->setFixedHeight(24); m_commentComposer->toolbar()->add(ComposerToolbar::Slot::Recovery, m_cancelDescription);
    connect(m_cancelDescription, &QPushButton::clicked, this, [this] { if (canLeaveDetails()) { closeInlineDetails(); renderDetail(); } });
    connect(m_messageMode, &QPushButton::clicked, this, [this] {
        if (selectionPending() || descriptionMode(m_detail)) return;
        auto *menu = new QMenu(m_messageMode); menu->setAttribute(Qt::WA_DeleteOnClose);
        const auto stage = m_detail.value("stage").toString();
        const auto primary = stage == "active" ? tr("Send to current executor") : stage == "review" ? tr("Request changes and send") : stage == "done" ? tr("Reopen and send") : tr("Add to task · send on Start");
        for (bool note : {false, true}) {
            auto *action = menu->addAction(note ? tr("Save note only") : primary); action->setCheckable(true); action->setChecked(note == m_noteOnly);
            connect(action, &QAction::triggered, this, [this, note] { m_noteOnly = note; renderDetail(); });
        }
        menu->popup(m_messageMode->mapToGlobal(QPoint(0, m_messageMode->height())));
    });
    m_inputColumn->addWidget(m_commentComposer);
    m_activity->useCompactJumpButton();
    m_commentComposer->toolbar()->add(ComposerToolbar::Slot::Latest, m_activity->jumpButton());
    m_noteError = new QLabel; m_noteError->setObjectName("taskInspectorNoteError"); m_noteError->setTextFormat(Qt::PlainText); m_noteError->setWordWrap(true); m_noteError->hide(); m_inputColumn->addWidget(m_noteError);
    connect(m_commentComposer, &MessageComposer::controlsChanged, this, [this] {
        if (selectionPending()) return;
        const auto key = m_commentComposer->sessionKey();
        m_board->setDescriptionDraft(m_detail.value("uuid").toString(), descriptionMode(m_detail) && key == inputKey(m_detail) && m_commentComposer->hasDraft(key));
    });
    connect(m_commentComposer, &MessageComposer::draftLoaded, this, [this](const QString &key) {
        if (m_descriptionSeed && key == inputKey(m_detail)) seedDescriptionDraft();
        if (m_pendingDetail.isEmpty()) return;
        if (key == inputKey(m_pendingDetail)) publishDetail(m_pendingDetail);
    });
    connect(m_commentComposer, &MessageComposer::sendRequested, this, [this](const QString &key, const QString &text, const QList<MessageAttachment> &files) {
        if (selectionPending() || m_noteBusy || key != m_commentComposer->sessionKey()) return;
        if (m_board->board().projectId != m_project) { m_noteError->setText(tr("Return to this task’s project to save the draft.")); m_noteError->show(); return; }
        const auto uuid = m_detail.value("uuid").toString();
        const auto project = m_project;
        const bool description = descriptionMode(m_detail); const auto task = TaskBoard::item(labels(m_detail));
        m_commentComposer->prepareSendingAsync(key, false, [this, key, uuid, text, files, description, task, project](bool ready) {
            if (!ready) return;
            m_noteBusy = true; m_noteError->hide(); QPointer<TaskInspector> guard(this);
            const auto saved = [guard, key, description] {
                if (!guard) return;
                guard->m_noteBusy = false; guard->m_commentComposer->deliveryFinished(key, true);
                guard->m_descriptionEditing = false;
                if (!description) guard->m_detailTabs->setCurrentIndex(0);
                guard->fetch();
            };
            const auto refused = [guard, key](const QString &error) {
                if (!guard) return;
                guard->m_noteBusy = false; guard->m_commentComposer->deliveryFinished(key, false, error);
                guard->m_noteError->setText(error); guard->m_noteError->show(); guard->renderDetail();
            };
            if (!description) {
                const auto stage = m_detail.value("stage").toString();
                const auto mode = m_noteOnly ? QStringLiteral("note") : stage == "active" ? QStringLiteral("send") : stage == "review" || stage == "done" ? QStringLiteral("reopen") : QStringLiteral("queue");
                m_board->saveTaskMessage(uuid, mode, m_detail.value("dispatch_id").toString(), text, files, saved, refused); return;
            }
            AttachmentFiles::runAsync<QJsonArray>(this, [files] { return TaskContent::encode(files); }, [guard, task, text, saved, refused, project](const QJsonArray &encoded) {
                if (!guard) return;
                if (guard->m_board->board().projectId != project) { refused(tr("The project changed. The description draft was kept.")); return; }
                TaskEdits edits; edits.text = text; edits.attachments = encoded;
                guard->m_board->saveTaskEdits(task, edits, saved, refused);
            });
        });
    });
    m_blocker = new QLabel; m_blocker->setObjectName("taskStartReason"); m_blocker->setWordWrap(true); m_blocker->setTextFormat(Qt::PlainText); m_blocker->hide(); m_headerColumn->addWidget(m_blocker);
    m_reviewLaunch = new QPushButton(tr("Review executor launch")); m_reviewLaunch->setObjectName("taskExecutorLaunchReview"); m_reviewLaunch->hide(); m_headerColumn->addWidget(m_reviewLaunch, 0, Qt::AlignRight);
    connect(m_reviewLaunch, &QPushButton::clicked, this, [this] { if (!selectionPending()) m_board->reviewExecutorLaunch(m_selected); });
    m_commands = new QWidget; m_commands->setObjectName("taskInspectorActions");
    m_commandLayout = new QHBoxLayout(m_commands); m_commandLayout->setContentsMargins(0, 2, 0, 2); m_commandLayout->setSpacing(4);
    m_commands->setFixedHeight(34); m_commandLayout->addStretch();
    for (const auto action : {TaskAction::ToBacklog, TaskAction::Top, TaskAction::ToTodo, TaskAction::Cancel,
                             TaskAction::BackToTodo, TaskAction::Accept, TaskAction::SendNow, TaskAction::Pause}) {
        auto *button = new QPushButton; button->setAutoDefault(false); button->setObjectName(QStringLiteral("taskInspectorAction:") + TaskRow::actionKey(action));
        button->setIconSize(QSize(20, 20)); button->setFixedHeight(28); button->setText(action == TaskAction::Accept ? tr("Done") : action == TaskAction::BackToTodo || action == TaskAction::ToTodo ? tr("To do") : action == TaskAction::ToBacklog ? tr("Backlog") : TaskBoard::actionLabel(action));
        button->setAccessibleName(TaskBoard::actionLabel(action)); button->hide();
        connect(button, &QPushButton::clicked, this, [this, action] {
            if (!m_detail.isEmpty() && m_detail.value("uuid").toString() == m_selected) m_board->actOnTask(TaskBoard::item(labels(m_detail)), action);
        });
        m_commandLayout->addWidget(button); m_commandButtons.insert(int(action), button);
    }
    m_headerColumn->addWidget(m_commands); m_splitter->addWidget(m_details);
    auto saved = Settings::value("tasks/inspectorSplit").toList();
    m_splitter->setSizes(saved.size() == 2 && saved[0].toInt() > 0 && saved[1].toInt() > 0
        ? QList<int>{saved[0].toInt(), saved[1].toInt()} : QList<int>{400, 320});
    connect(m_splitter, &QSplitter::splitterMoved, this, [this] {
        const auto sizes = m_splitter->sizes(); Settings::set("tasks/inspectorSplit", QVariantList{sizes.value(0), sizes.value(1)});
    });
    connect(m_detailTabs, &QTabBar::currentChanged, this, [this](int index) { Q_UNUSED(index); renderDetail(); });
    connect(m_board, &TasksView::taskEditRequested, this, [this](const QString &uuid) {
        selectTask(uuid); if (m_selected != uuid) return;
        m_inlineRequested = uuid; openInlineDetails();
    });
    connect(m_board, &TasksView::taskRenameRequested, this, [this](const QString &uuid) {
        openTask(uuid); if (m_selected != uuid) return;
        m_renameRequested = uuid; if (!selectionPending()) beginRename();
    });
    connect(m_board, &TasksView::boardChanged, this, &TaskInspector::refresh);
    connect(m_client, &HgsClient::taskReady, this, [this](quint64 id, const QJsonObject &reply) {
        if (id != m_request) return;
        const auto generation = property("requestGeneration").toULongLong(); m_request = 0;
        if (generation != m_generation) { fetch(); return; }
        const bool refetch = m_refetchPending; m_refetchPending = false;
        // Keep making progress when list polling is faster than detail reads.
        // Publish this same-selection snapshot, then coalesce the next read.
        const auto followUp = qScopeGuard([this, refetch] { if (refetch) fetch(); });
        auto task = reply.value("task").toObject();
        if (task.value("uuid").toString() != m_selected) {
            m_detail = {}; renderDetail(); m_state->setText(tr("The task reply did not match the selected task.")); return;
        }
        // Legacy CLI replies include the full journal. Retain only this bounded
        // window; the execution model will introduce indexed history reads.
        const auto log = task.value("log").toArray(); QJsonArray recent; int end = log.size();
        bool started = false;
        for (const auto &value : log) {
            const auto kind = value.toObject().value("kind").toString();
            if (kind == QLatin1String("sent")) started = false;
            else if (kind == QLatin1String("started")) started = true;
        }
        if (!m_historyBefore.isEmpty()) {
            end = -1;
            for (int i = 0; i < log.size(); ++i) if (log.at(i).toObject() == m_historyBefore) { end = i; break; }
            if (end < 0) { m_state->setText(tr("History changed; return to Latest to refresh.")); return; }
        }
        for (int i = qMax(0, end - VisibleHistory); i < end; ++i) recent.append(log.at(i));
        task["log_total"] = log.size(); task["history_end"] = end; task["start_confirmed"] = started; task["log"] = recent;
        m_pendingDetail = task;
        const auto key = inputKey(task);
        m_commentComposer->ensureDraftLoaded(key);
        if (!m_commentComposer->isDraftLoading(key)) publishDetail(task);
    });
    connect(m_client, &HgsClient::taskFailed, this, [this](quint64 id, const QString &error) {
        if (id != m_request) return;
        const auto generation = property("requestGeneration").toULongLong(); m_request = 0;
        if (generation != m_generation || m_refetchPending) { fetch(); return; }
        m_noteError->setText(tr("Task could not be read: %1").arg(error)); m_noteError->show();
    });
    for (auto *child : m_details->findChildren<QWidget *>()) child->installEventFilter(this);
    setTheme(true);
}

void TaskInspector::setContext(const QString &project, const QString &host, const QString &name, const QList<TaskSession> &sessions) {
    if (project == m_project && host == m_host && name == m_session && sessions == m_sessions) return;
    const QString id = sessionId(host, name, sessions);
    const bool changed = project != m_project || host != m_host
        || (id.isEmpty() || m_sessionId.isEmpty() ? name != m_session : id != m_sessionId);
    if (changed && !canLeaveDetails()) return;
    if (changed) { closeInlineDetails(); m_titleEditor->hide(); m_title->show(); m_titleError->hide(); m_renameRequested.clear(); }
    m_project = project; m_host = host; m_session = name; m_sessionId = id; m_sessions = sessions;
    if (changed) { restoreNewCard(); m_descriptionEditing = false; ++m_generation; m_selected.clear(); m_detail = {}; m_stage.clear(); m_showOlder = false; }
    refresh();
}

void TaskInspector::showEvent(QShowEvent *event) {
    QWidget::showEvent(event); m_board->setInspectorVisible(true); refresh();
}

void TaskInspector::hideEvent(QHideEvent *event) {
    endListDrag(); QWidget::hideEvent(event); m_board->setInspectorVisible(m_details->isVisible());
}

void TaskInspector::selectTask(const QString &uuid) {
    if (m_selected != uuid && !canLeaveDetails()) return;
    const auto board = m_board->displayBoard();
    for (const auto &task : board.tasks) if (task.uuid == uuid) {
        if (m_stage != QLatin1String("current") || !TaskBoard::matchesFilter(task.stage, m_stage)) m_stage = task.stage;
        if (!TaskBoard::keptFor(task, m_host, m_session, m_sessionId)) m_board->setOnlyThisSession(false);
        if (task.stage == QLatin1String("done") && QDateTime::currentMSecsSinceEpoch() - task.stageAt > TaskBoard::RecentDoneMs) m_showOlder = true;
        break;
    }
    if (m_selected != uuid) {
        m_titleEditor->hide(); m_title->show(); m_titleError->hide(); m_renameRequested.clear();
        m_noteError->hide(); m_descriptionEditing = false; m_descriptionSeed = false; m_pendingDetail = {}; m_selected = uuid; m_inlineRequested.clear(); ++m_generation; m_historyVisible = 8; m_historyBefore = {};
        const QSignalBlocker blocker(m_detailTabs); m_detailTabs->setCurrentIndex(0);
    }
    refresh();
}

void TaskInspector::refresh() {
    if (!m_cards) return;
    const auto board = m_board->displayBoard(); const auto colors = TaskColors::of(m_dark);
    const TaskBoard::View view{m_host, m_session, m_board->onlyThisSession(), QDateTime::currentMSecsSinceEpoch(), m_sessionId};
    if (m_stage.isEmpty() && !board.projectId.isEmpty()) {
        m_stage = board.count("current", view) ? QStringLiteral("current") : m_board->selectedStage();
        if (!board.count(m_stage, view)) for (const auto &stage : TaskBoard::stages())
            if (board.count(stage, view)) { m_stage = stage; break; }
    }
    // Follow a selected task's recorded stage change while keeping its details open.
    for (const auto &task : board.tasks) if (task.uuid == m_selected && !m_detached && !TaskBoard::matchesFilter(task.stage, m_stage)) m_stage = task.stage;
    for (const auto &stage : TaskBoard::filters()) {
        auto *tile = m_tiles.value(stage); tile->setColors(colors); tile->setCount(board.count(stage, view)); tile->setChecked(stage == m_stage);
    }
    auto rows = board.rows(m_stage, view);
    const auto older = m_stage == QLatin1String("done") ? board.rows(m_stage, view, true) : QList<TaskItem>();
    if (m_showOlder) rows += older;
    const int total = rows.size();
    QList<TaskItem> shown = rows.mid(0, VisibleTasks);
    // An explicit selection beyond the window replaces its last card, never
    // growing the retained widget set past the bound.
    if (!m_selected.isEmpty() && std::none_of(shown.begin(), shown.end(), [this](const TaskItem &t) { return t.uuid == m_selected; }))
        for (const auto &task : rows) if (task.uuid == m_selected) { if (shown.size() == VisibleTasks) shown.removeLast(); shown.append(task); break; }
    QSet<QString> keep; int index = 0; bool selectedShown = false;
    for (const auto &task : shown) {
        auto *row = m_rows.value(task.uuid);
        if (!row) {
            row = new TaskRow(m_listPage); row->setCompact(true); m_rows.insert(task.uuid, row);
            row->setObjectName(QStringLiteral("taskInspectorRow:") + task.id);
            connect(row, &TaskRow::opened, this, [this, row] {
                openTask(row->task().uuid);
            });
            connect(row, &TaskRow::actionRequested, this, [this, row](TaskAction action) { m_board->actOnTask(row->task(), action); });
            connect(row, &TaskRow::dragStarted, this, [this, row](const QPoint &hotSpot) { m_board->dragTask(row, hotSpot); });
        }
        auto state = m_board->rowState(task); state.showStage = m_stage == QLatin1String("current");
        row->setTask(task, state, colors, view.now);
        if (row->property("expanded").toBool() != (task.uuid == m_selected)) { row->setProperty("expanded", task.uuid == m_selected); row->update(); }
        if (m_cards->indexOf(row) != index) { m_cards->removeWidget(row); m_cards->insertWidget(index, row); }
        ++index; row->show(); keep.insert(task.uuid);
        if (task.uuid == m_selected) selectedShown = true;
    }
    for (auto it = m_rows.begin(); it != m_rows.end();) {
        if (keep.contains(it.key())) { ++it; continue; }
        auto *row = it.value(); m_cards->removeWidget(row); m_board->retireTaskRow(row); it = m_rows.erase(it);
    }
    if (!selectedShown && !m_selected.isEmpty() && !m_detached) { m_selected.clear(); m_detail = {}; ++m_generation; }
    m_scope->setText(m_project.isEmpty() ? tr("Open a project session to see tasks")
        : total > VisibleTasks ? tr("%1 of %2 tasks").arg(VisibleTasks).arg(total)
        : total == 1 ? tr("1 task") : tr("%1 tasks").arg(total));
    m_older->setVisible(!older.isEmpty()); m_older->setText(m_showOlder ? tr("Hide older") : tr("Show older (%1)").arg(older.size()));
    updateFilters();
    renderDetail(); if (isVisible() || m_details->isVisible()) fetch();
    if (m_overList) QMetaObject::invokeMethod(this, [this] {
        if (m_overList) followDrag(m_drag, m_listPage->mapFrom(m_scroll->viewport(), m_dragAt));
    }, Qt::QueuedConnection);
}

void TaskInspector::resizeEvent(QResizeEvent *event) {
    QWidget::resizeEvent(event); updateFilters();
}

void TaskInspector::updateFilters() {
    if (!m_filters) return;
    int required = (m_tiles.size() - 1) * 4;
    for (auto *tile : std::as_const(m_tiles)) required += tile->expandedWidth();
    for (auto *tile : std::as_const(m_tiles)) tile->setCompact(m_filters->width() < required);
}

QList<TaskRow *> TaskInspector::shownRows(const QString &stage) const {
    QList<TaskRow *> rows;
    for (int i = 0; i < m_cards->count(); ++i)
        if (auto *row = qobject_cast<TaskRow *>(m_cards->itemAt(i)->widget()); row && !row->isHidden() && (stage.isEmpty() || row->task().stage == stage)) rows << row;
    return rows;
}

int TaskInspector::dropGap(const QString &uuid, const QPoint &point, const QString &stage) const {
    const auto rows = shownRows(stage); int gap = rows.size(), from = -1;
    for (int i = 0; i < rows.size(); ++i) {
        if (rows[i]->task().uuid == uuid) from = i;
        if (gap == rows.size() && point.y() < rows[i]->y() + rows[i]->height() / 2) gap = i;
    }
    return from >= 0 && (gap == from || gap == from + 1) ? -1 : gap;
}

void TaskInspector::followDrag(const TaskDrag &drag, const QPoint &point) {
    const auto stage = m_stage == QLatin1String("current") ? drag.stage : QString();
    const auto rows = shownRows(stage);
    if (!TaskBoard::matchesFilter(drag.stage, m_stage) || !m_board->canReorderTask(drag, drag.stage) || rows.isEmpty()) { endListDrag(); return; }
    m_drag = drag; m_dragAt = m_listPage->mapTo(m_scroll->viewport(), point); m_overList = true;
    const int gap = dropGap(drag.uuid, point, stage);
    if (gap < 0) m_dropLine->hide();
    else {
        const auto *edge = gap < rows.size() ? rows[gap] : rows.last();
        const int y = edge->y() + (gap < rows.size() ? 0 : edge->height());
        m_dropLine->setGeometry(8, y - 1, m_listPage->width() - 16, 2); m_dropLine->show(); m_dropLine->raise();
    }
    const int height = m_scroll->viewport()->height();
    m_scrollStep = m_dragAt.y() < 28 ? -8 : m_dragAt.y() > height - 28 ? 8 : 0;
    if (!m_scrollStep) m_autoScroll.stop(); else if (!m_autoScroll.isActive()) m_autoScroll.start();
}

void TaskInspector::endListDrag() { m_autoScroll.stop(); m_overList = false; m_scrollStep = 0; m_dropLine->hide(); }

bool TaskInspector::eventFilter(QObject *watched, QEvent *event) {
    const auto type = event->type();
    if (watched == m_title && type == QEvent::MouseButtonRelease && static_cast<QMouseEvent *>(event)->button() == Qt::LeftButton && !selectionPending()) { beginRename(); return true; }
    if (watched == m_titleEditor) {
        if (type == QEvent::KeyPress || type == QEvent::ShortcutOverride) {
            const int key = static_cast<QKeyEvent *>(event)->key();
            if (key == Qt::Key_Escape || key == Qt::Key_Return || key == Qt::Key_Enter) {
                event->accept(); if (type == QEvent::KeyPress) { if (key == Qt::Key_Escape) cancelRename(); else finishRename(); } return true;
            }
        }
        if (type == QEvent::FocusOut && m_titleEditor->isVisible()) finishRename();
    }
    if (watched == m_newTitle && (type == QEvent::ShortcutOverride || type == QEvent::KeyPress)
        && static_cast<QKeyEvent *>(event)->key() == Qt::Key_Escape) {
        event->accept(); if (type == QEvent::KeyPress && !m_creating) m_creationHeader->setCurrentWidget(m_scope); return true;
    }
    if (watched == m_details && (type == QEvent::Show || type == QEvent::Hide)) {
        QTimer::singleShot(0, this, [this] { m_board->setInspectorVisible(isVisible() || m_details->isVisible()); if (m_details->isVisible()) fetch(); });
    }
    const auto *widget = qobject_cast<QWidget *>(watched);
    if (selectionPending() && widget && m_details->isAncestorOf(widget)
        && (type == QEvent::MouseButtonPress || type == QEvent::MouseButtonRelease || type == QEvent::KeyPress || type == QEvent::InputMethod || type == QEvent::Drop)) return true;
    if (type != QEvent::DragEnter && type != QEvent::DragMove && type != QEvent::DragLeave && type != QEvent::Drop)
        return QWidget::eventFilter(watched, event);
    if (watched == m_listPage) {
        if (type == QEvent::DragLeave) { endListDrag(); return true; }
        auto *drop = static_cast<QDropEvent *>(event); const auto drag = TaskDrag::from(drop->mimeData());
        const auto stage = m_stage == QLatin1String("current") && drag ? drag->stage : QString(); const auto rows = shownRows(stage);
        if (!drag || rows.isEmpty() || !TaskBoard::matchesFilter(drag->stage, m_stage) || !m_board->canReorderTask(*drag, drag->stage)) { endListDrag(); drop->ignore(); return true; }
        drop->setDropAction(Qt::MoveAction); drop->accept();
        if (type == QEvent::Drop) {
            const int gap = dropGap(drag->uuid, drop->position().toPoint(), stage); endListDrag();
            if (gap >= 0) m_board->reorderTask(*drag, gap < rows.size() ? rows[gap]->task().uuid : rows.last()->task().uuid, gap < rows.size());
        } else followDrag(*drag, drop->position().toPoint());
        return true;
    }
    for (auto it = m_tiles.cbegin(); it != m_tiles.cend(); ++it) {
        if (it.value() != watched) continue;
        auto *tile = it.value();
        if (type == QEvent::DragLeave) { tile->setDropTarget(false); return true; }
        auto *drop = static_cast<QDropEvent *>(event); const auto drag = TaskDrag::from(drop->mimeData());
        const bool takes = drag && m_board->canStageTask(*drag, it.key());
        tile->setDropTarget(takes && type != QEvent::Drop);
        if (!takes) { drop->ignore(); return true; }
        drop->setDropAction(Qt::MoveAction); drop->accept();
        if (type == QEvent::Drop) m_board->stageTask(*drag, it.key());
        return true;
    }
    return QWidget::eventFilter(watched, event);
}

void TaskInspector::fetch() {
    if (m_selected.isEmpty() || m_project.isEmpty() || (!isVisible() && !m_details->isVisible())) return;
    // A saved comment or board update must supersede an older detail snapshot,
    // even with polling disabled. Keep only one read and one pending refresh.
    if (m_request) { m_refetchPending = true; return; }
    m_refetchPending = false;
    setProperty("requestGeneration", QVariant::fromValue(m_generation));
    m_request = m_client->requestTask({"get", m_selected, "--project", m_project});
}

QJsonObject TaskInspector::labels(QJsonObject task) const {
    const auto resolve = [this](QJsonObject key) {
        const QString id = key.value("session_id").toString();
        if (id.isEmpty() || key.value("host").isNull()) return key;
        for (const auto &candidate : m_sessions) if (candidate.host == key.value("host").toString() && candidate.id == id) {
            key["session"] = candidate.name; break;
        }
        return key;
    };
    for (const auto *key : {"target", "session", "author"}) if (task.value(key).isObject()) task[key] = resolve(task.value(key).toObject());
    if (task.value("target").toObject().contains("parent")) { auto target = task.value("target").toObject(); target["parent"] = resolve(target.value("parent").toObject()); task["target"] = target; }
    auto log = task.value("log").toArray();
    for (auto it = log.begin(); it != log.end(); ++it) {
        auto entry = it->toObject();
        for (const auto *key : {"actor", "session"}) if (entry.value(key).isObject()) entry[key] = resolve(entry.value(key).toObject());
        *it = entry;
    }
    task["log"] = log; return task;
}

const TaskSession *TaskInspector::executor() const {
    const auto task = TaskBoard::item(labels(m_detail));
    const bool sent = task.stage != "todo" && task.stage != "backlog" && !task.sessionName.isEmpty();
    const QString host = sent ? task.sessionHost : task.targetHost;
    const QString id = sent ? task.sessionId : task.targetId;
    const QString name = sent ? task.sessionName : task.targetSession;
    for (const auto &candidate : m_sessions) if (candidate.host == host
        && (id.isEmpty() ? candidate.name == name : candidate.id == id)) return &candidate;
    return nullptr;
}

void TaskInspector::renderDetail() {
    if (!m_activity) return;
    // Retain the entire presented task while a new selection is loading. Input
    // handlers guard the binding; visual enabled states never cycle through false.
    if (selectionPending()) return;
    const auto data = labels(m_detail); auto task = TaskBoard::item(data);
    for (const auto &listed : m_board->board().tasks) if (listed.uuid == task.uuid) { task.queue = listed.queue; break; }
    m_identity->setText(task.id.isEmpty() ? QString() : task.id + " · " + TaskBoard::stageLabel(task.stage));
    m_title->setText(task.displayTitle()); m_title->setToolTip(task.ref + " · " + TaskBoard::stageLabel(task.stage) + tr("\nClick to rename"));
    m_details->findChild<QPushButton *>("taskViewRename")->setEnabled(!m_detail.isEmpty() && task.writable);
    m_open->setEnabled(!m_detail.isEmpty() && executor()); m_edit->setEnabled(!m_detail.isEmpty() && task.writable && (task.stage == QLatin1String("todo") || task.stage == QLatin1String("backlog")));
    m_detailTabs->setEnabled(!m_detail.isEmpty()); m_moreDetail->hide(); m_moreDetail->setEnabled(true);
    m_latestHistory->setVisible(!m_detail.isEmpty() && m_detailTabs->currentIndex() == 2 && !m_historyBefore.isEmpty());
    renderCommands(task); m_reviewLaunch->setVisible(m_board->hasExecutorLaunch(task.uuid));
    m_assignmentRow->setVisible(!m_detail.isEmpty());
    const auto assigned = task.targetSession.isEmpty() ? tr("Any available session") : TaskBoard::sessionLabel(task.targetHost, task.targetSession);
    const bool main = task.targetHost == m_host && (!task.targetId.isEmpty() ? task.targetId == m_sessionId : task.targetSession == m_session);
    const auto targetLabel = task.targetKind == QLatin1String("new_session") ? tr("New session · create when started")
        : task.targetKind == QLatin1String("subagent") ? tr("Subagent · coordinate through %1").arg(assigned)
        : main ? tr("Main session · %1").arg(assigned) : assigned;
    const bool queued = task.stage == "todo" || task.stage == "backlog";
    const auto workerLabel = TaskBoard::sessionLabel(task.sessionHost, task.sessionName);
    const auto assignmentText = queued ? tr("Planned: %1 ▾").arg(targetLabel)
        : (task.stage == "active" ? tr("Executor: %1") : tr("Last executor: %1")).arg(workerLabel.isEmpty() ? tr("Unconfirmed") : workerLabel);
    static_cast<ToolbarChip *>(m_assignment)->setLabels(assignmentText);
    m_assignment->setToolTip(queued ? tr("%1\nChange executor assignment").arg(assignmentText) : tr("%1\nOpen executor Activity").arg(assignmentText));
    m_assignment->setEnabled(queued ? task.writable : executor() != nullptr);
    const auto noteKey = m_selected.isEmpty() ? QString() : inputKey(m_detail);
    if (m_noteKey != noteKey) {
        m_commentComposer->setSessionKey(noteKey); m_noteKey = noteKey; m_noteError->hide();
        m_commentComposer->setDraftLabel((descriptionMode(m_detail) ? tr("Description · %1") : tr("Comment · %1")).arg(task.id));
        const auto legacyKey = QStringLiteral("tasks/noteDraft/%1/%2").arg(m_project, m_selected);
        const auto legacy = Settings::value(legacyKey).toString();
        if (!legacy.isEmpty() && m_commentComposer->offerDraft(noteKey, legacy)) Settings::remove(legacyKey);
    }
    m_commentComposer->setAvailability(!m_detail.isEmpty() && task.writable);
    m_commentComposer->setSessionStatus(task.writable ? QString() : tr("Update Zerus to comment on this task"), false, !task.writable);
    const bool description = descriptionMode(m_detail);
    m_checkMessage->setVisible(!m_selected.isEmpty() && Settings::contains(QStringLiteral("tasks/messageRequest/%1/%2").arg(m_project, m_selected)));
    m_cancelDescription->setVisible(description); m_messageMode->setEnabled(!description && canLeaveDetails());
    const auto messageLabel = m_noteOnly ? tr("Save note only · no delivery")
        : queued ? tr("Add to task · sent on Start") : task.stage == "active" ? tr("Send to %1").arg(workerLabel)
        : task.stage == "review" ? tr("Request changes and send to %1").arg(workerLabel) : tr("Reopen and send to %1").arg(workerLabel);
    const auto shortMode = m_noteOnly ? tr("Note only") : queued ? tr("On Start") : task.stage == "active" ? tr("Send feedback") : task.stage == "review" ? tr("Request changes") : tr("Reopen and send");
    m_messageMode->setLabels(description ? tr("Editing description") : messageLabel + QStringLiteral(" ▾"), description ? tr("Description") : shortMode + QStringLiteral(" ▾"));
    m_messageMode->setDetail(description ? tr("Save updates the canonical task document. Comments remain separate.") : messageLabel);
    m_commentComposer->setSendIconOnly(!description);
    m_commentComposer->setSendLabel(description ? tr("Save description") : m_noteOnly || queued ? tr("Add to task") : tr("Send feedback"));
    m_commentComposer->setPlaceholder(description ? tr("Describe the task…") : m_noteOnly ? tr("Write a note for this task…") : queued ? tr("Add context for the executor…") : tr("Message this task’s executor…"));
    m_edit->setToolTip(description ? tr("Task description is being edited below") : tr("Edit description"));
    const auto empty = [this](const QString &text) {
        ++m_journalGeneration; m_journalKey.clear(); m_journalSource = {};
        m_activity->setTaskJournal({}, {}); m_emptyDetail->setText(text); m_content->setCurrentWidget(m_emptyDetail);
    };
    if (m_detail.isEmpty()) {
        m_state->setText(tr("Task details")); m_state->setToolTip({});
        empty(m_selected.isEmpty() ? tr("Select a task above to see its details.") : tr("Loading task…")); return;
    }
    const auto log = data.value("log").toArray();
    const bool started = data.value("start_confirmed").toBool();
    QString context;
    if (!task.sessionName.isEmpty()) {
        const QString worker = TaskBoard::sessionLabel(task.sessionHost, task.sessionName);
        context = task.stage == QLatin1String("active") && !started ? tr("Dispatched to %1 · start unconfirmed").arg(worker)
            : task.stage == QLatin1String("active") ? tr("Executor: %1").arg(worker) : tr("Last executor: %1").arg(worker);
    } else context = tr("Assigned: %1").arg(task.targetSession.isEmpty() ? tr("Any session") : TaskBoard::sessionLabel(task.targetHost, task.targetSession));
    if (task.stage == QLatin1String("todo")) context += tr(" · #%1 in queue").arg(task.queue);
    if (!task.worker.isEmpty()) context = tr("Subagent: %1 · coordinated by %2").arg(task.worker.value("name").toString(), TaskBoard::sessionLabel(task.sessionHost, task.sessionName));
    else if (task.targetKind == QLatin1String("subagent") && task.stage == QLatin1String("active")) context = tr("Subagent requested · coordinator %1 · child unconfirmed").arg(TaskBoard::sessionLabel(task.sessionHost, task.sessionName));
    m_state->setVisible(task.stage == "active" && (!started || task.targetKind == "subagent"));
    m_state->setText(context); m_state->setToolTip(QStringLiteral("%1 · %2\n%3").arg(task.id, TaskBoard::stageLabel(task.stage), context));
    QJsonArray entries;
    switch (m_detailTabs->currentIndex()) {
    case 0: {
        const auto result = [&] {
            if (task.summary.isEmpty() && !task.cancelledAt) return;
            QJsonObject record;
            for (const auto &value : log) if (value.toObject().value("kind") == "report") record = value.toObject();
            entries.append(journalEntry("result", record.contains("actor") ? record.value("actor").toObject() : QJsonObject{{"kind", "agent"}, {"host", task.sessionHost}, {"session", task.sessionName}}, tr("Result"), task.cancelledAt ? task.cancelReason : task.summary, record.value("at").toDouble()));
        };
        if (task.stage == "review" || task.stage == "done") result();
        QJsonObject document{{"id", "description"}, {"role", "document"}, {"label", tr("Description")},
            {"text", task.text.isEmpty() ? tr("No description yet.") : task.text}, {"attachments", data.value("attachments")},
            {"editable", task.writable && queued}};
        entries.append(document);
        entries.append(QJsonObject{{"id", "discussion-heading"}, {"role", "document"}, {"label", tr("Discussion")}, {"text", QString()}});
        int count = 0;
        for (const auto &value : log) {
            const auto record = value.toObject(); const auto kind = record.value("kind").toString();
            if (kind != "comment" && kind != "message") continue;
            const auto delivery = record.value("delivery").toObject(); const auto state = delivery.value("state").toString();
            const auto status = kind == "comment" || state == "note" ? tr("Saved note · not sent")
                : state == "queued" ? tr("Waiting for Start") : state == "submitted" ? (delivery.value("mode") == "queue" ? tr("Included in Start request") : task.worker.isEmpty() ? tr("Submitted to executor") : tr("Submitted to coordinator · child receipt unavailable"))
                : state == "failed" ? tr("Saved in task · not sent") : tr("Saved in task · delivery unconfirmed");
            auto entry = journalEntry(record.value("request_id").toString(QString("comment-%1").arg(count)), record.value("actor").toObject(),
                kind == "comment" || state == "note" ? tr("Note") : tr("Message"), record.value("text").toString(), record.value("at").toDouble());
            entry["attachments"] = record.value("attachments"); entry["delivery"] = status;
            const auto error = delivery.value("error").toString();
            if (!error.isEmpty()) {
                const auto reason = error.contains("execution changed") || error.contains("identity changed") || error.contains("conversation changed")
                    || error.contains("dispatch receipt") || error.contains("dispatched run")
                    ? tr("Your comment is saved here. The task’s previous executor could not be verified, so it was not sent. Open executor Activity to continue the discussion, or return the task to To do and start it with the current executor.")
                    : error;
                entry["text"] = entry.value("text").toString() + "\n\n" + reason;
            }
            entries.append(entry); ++count;
        }
        if (!count) entries.append(QJsonObject{{"id", "discussion-empty"}, {"role", "event"}, {"text", tr("Messages and notes appear here.")}});
        if (data.value("log_total").toInt() > log.size()) {
            entries.append(QJsonObject{{"id", "discussion-earlier"}, {"role", "event"}, {"text", tr("Showing recent discussion. Earlier records are available in History.")}});
        }
        break;
    }
    case 1: {
        if (task.summary.isEmpty() && !task.cancelledAt) { empty(tr("No result reported yet.")); return; }
        QJsonObject record;
        const QString kind = task.cancelledAt ? QStringLiteral("cancelled") : QStringLiteral("report");
        for (const auto &value : log) {
            const auto entry = value.toObject();
            if (entry.value("kind") == kind) record = entry;
        }
        const auto actor = record.contains("actor") ? record.value("actor").toObject()
            : QJsonObject{{"kind", "agent"}, {"session", task.sessionName}, {"host", task.sessionHost}};
        entries.append(journalEntry("result", actor, task.cancelledAt ? tr("Cancelled") : tr("Report"),
            task.cancelledAt ? task.cancelReason : task.summary, record.value("at").toDouble()));
        break;
    }
    case 2: {
        const int first = qMax(0, int(log.size()) - m_historyVisible);
        if (first > 0) { m_moreDetail->show(); m_moreDetail->setText(tr("Earlier events (%1)").arg(first)); }
        else if (data.value("history_end").toInt() > log.size()) { m_moreDetail->show(); m_moreDetail->setText(tr("Earlier events…")); }
        for (int i = first; i < log.size(); ++i) {
            const auto entry = log.at(i).toObject(); const auto kind = entry.value("kind").toString();
            const bool message = kind == QLatin1String("comment") || kind == QLatin1String("report") || kind == QLatin1String("message");
            auto messageEntry = journalEntry(QString::number(data.value("history_end").toInt() - log.size() + i), entry.value("actor").toObject(),
                kind == QLatin1String("report") ? tr("Report") : tr("Comment"),
                message ? entry.value("text").toString() : taskHistoryAction(entry), entry.value("at").toDouble(), !message);
            messageEntry["attachments"] = entry.value("attachments"); entries.append(messageEntry);
        }
        if (log.isEmpty()) { empty(tr("No history yet.")); return; }
        break;
    }
    }
    m_content->setCurrentWidget(m_activity);
    publishJournal(QStringLiteral("task\n%1\n%2\n%3\n%4").arg(m_project, task.uuid).arg(m_detailTabs->currentIndex()).arg(m_historyVisible), entries,
        m_detailTabs->currentIndex() == 2 && m_historyVisible == 8);
}

void TaskInspector::publishJournal(const QString &key, const QJsonArray &entries, bool followLatest) {
    if (m_journalKey == key && m_journalSource == entries) return;
    m_journalKey = key; m_journalSource = entries;
    const auto generation = ++m_journalGeneration;
    const auto cacheRoot = AttachmentFiles::root();
    AttachmentFiles::runAsync<AttachmentFiles::Prepared>(this, [entries, cacheRoot] {
        AttachmentFiles::Prepared result;
        qsizetype total = 0;
        for (const auto &value : entries) {
            auto entry = value.toObject(); const auto encoded = entry.value("attachments").toArray();
            const auto files = TaskContent::decode(encoded); QJsonArray metadata;
            if (files.size() != encoded.size()) { result.error = QObject::tr("Task attachments could not be read."); return result; }
            for (const auto &file : files) {
                total += file.data.size();
                if (total > HgsClient::MaximumAttachmentsBytes) { result.error = QObject::tr("Task attachments exceed the size limit."); return result; }
                const auto path = AttachmentFiles::storeAt(cacheRoot, file.name, file.data);
                if (path.isEmpty()) { result.error = QObject::tr("Could not prepare task attachments."); return result; }
                metadata.append(QJsonObject{{"name", file.name}, {"mime", file.mime}, {"reference", file.reference}, {"bytes", qint64(file.data.size())}, {"local_path", path}});
            }
            entry["attachments"] = metadata; result.files.append(entry);
        }
        return result;
    }, [this, key, generation, followLatest](const AttachmentFiles::Prepared &result) {
        if (generation != m_journalGeneration || selectionPending()) return;
        if (!result.error.isEmpty()) { m_noteError->setText(result.error); m_noteError->show(); return; }
        m_activity->setTaskJournal(key, result.files, followLatest);
    });
}

bool TaskInspector::selectionPending() const {
    return !m_selected.isEmpty() && m_detail.value("uuid").toString() != m_selected;
}

void TaskInspector::publishDetail(QJsonObject task) {
    if (task.value("uuid").toString() != m_selected) return;
    if (m_detail.value("uuid") != task.value("uuid")) { closeInlineDetails(); m_noteOnly = task.value("stage") == "done"; }
    // Take the snapshot by value: draftLoaded may pass m_pendingDetail itself.
    m_pendingDetail = {};
    if (task != m_detail) { m_detail = task; renderDetail(); }
    if (m_inlineRequested == m_selected) openInlineDetails();
    if (m_renameRequested == m_selected) beginRename();
}

void TaskInspector::showAssignment() {
    if (m_detail.isEmpty() || selectionPending()) return;
    auto *menu = new QMenu(m_assignment); menu->setObjectName("taskAssignmentMenu"); menu->setAttribute(Qt::WA_DeleteOnClose);
    const auto task = TaskBoard::item(labels(m_detail)); const auto project = m_project;
    const auto assign = [this, task, project](const TaskTarget &target) {
        if (selectionPending() || task.uuid != m_selected || project != m_project) return;
        m_board->saveTaskAssignment(task, target, [this] { fetch(); }, [this](const QString &error) { m_noteError->setText(error); m_noteError->show(); });
    };
    auto *main = menu->addAction(tr("Main session · %1").arg(TaskBoard::sessionLabel(m_host, m_session)));
    connect(main, &QAction::triggered, this, [this, assign] { assign({false, m_host, m_session, m_sessionId}); });
    menu->addSection(tr("New executor · created when the task starts"));
    auto *child = menu->addAction(tr("Subagent"));
    connect(child, &QAction::triggered, this, [this, assign] { assign({false, m_host, m_session, m_sessionId, "subagent"}); });
    auto *fresh = menu->addAction(tr("New session"));
    connect(fresh, &QAction::triggered, this, [this, assign] { assign({false, m_host, m_session, m_sessionId, "new_session"}); });
    auto *any = menu->addAction(tr("Any session · use the selected session at start"));
    connect(any, &QAction::triggered, this, [assign] { assign({}); });
    auto *existing = menu->addMenu(tr("Specific session"));
    auto *searchAction = new QWidgetAction(existing); auto *search = new QLineEdit;
    search->setPlaceholderText(tr("Find a session…")); search->setObjectName("taskAssignmentSearch");
    searchAction->setDefaultWidget(search); existing->addAction(searchAction);
    auto *list = new QWidget; auto *listLayout = new QVBoxLayout(list); listLayout->setContentsMargins(4, 2, 4, 2);
    auto *scroller = new QScrollArea; scroller->setWidgetResizable(true); scroller->setMaximumHeight(320); scroller->setMinimumWidth(280);
    scroller->setWidget(list); auto *listAction = new QWidgetAction(existing); listAction->setDefaultWidget(scroller); existing->addAction(listAction);
    const auto sessions = m_sessions;
    auto *navigation = new QWidget; auto *navigationLayout = new QHBoxLayout(navigation);
    navigationLayout->setContentsMargins(4, 0, 4, 2);
    auto *previous = new QPushButton(tr("Previous")); auto *next = new QPushButton(tr("Next"));
    navigationLayout->addWidget(previous); navigationLayout->addWidget(next);
    auto *navigationAction = new QWidgetAction(existing); navigationAction->setDefaultWidget(navigation); existing->addAction(navigationAction);
    auto page = std::make_shared<int>(0);
    const auto populate = [sessions, search, list, listLayout, next, previous, page, assign, menu] {
        while (auto *item = listLayout->takeAt(0)) { delete item->widget(); delete item; }
        QList<TaskSession> matches;
        for (const auto &session : sessions) if (session.name.contains(search->text(), Qt::CaseInsensitive) || session.host.contains(search->text(), Qt::CaseInsensitive)) matches.append(session);
        *page = qBound(0, *page, qMax(0, (int(matches.size()) - 1) / 50));
        const int first = *page * 50;
        for (int i = first; i < qMin(first + 50, int(matches.size())); ++i) {
            const auto session = matches.at(i);
            auto *button = new QPushButton(session.name + (session.host.isEmpty() ? QString() : QStringLiteral(" @") + session.host), list);
            button->setFlat(true); button->setEnabled(!session.id.isEmpty()); listLayout->addWidget(button);
            QObject::connect(button, &QPushButton::clicked, menu, [assign, session, menu] { assign({false, session.host, session.name, session.id}); menu->close(); });
        }
        previous->setEnabled(first > 0); next->setEnabled(first + 50 < matches.size());
    };
    connect(search, &QLineEdit::textChanged, menu, [page, populate] { *page = 0; populate(); });
    connect(next, &QPushButton::clicked, menu, [page, populate] { ++*page; populate(); });
    connect(previous, &QPushButton::clicked, menu, [page, populate] { --*page; populate(); }); populate();
    menu->popup(m_assignment->mapToGlobal(QPoint(0, m_assignment->height())));
}

bool TaskInspector::canLeaveDetails() {
    return !m_titleSaving && !m_noteBusy && !m_commentComposer->isRecording() && !m_commentComposer->isSending(m_commentComposer->sessionKey()) && !m_commentComposer->isPreparingSending(m_commentComposer->sessionKey());
}

void TaskInspector::beginRename() {
    if (selectionPending() || m_detail.isEmpty() || !m_detail.value("writable").toBool(true) || !canLeaveDetails()) return;
    m_renameRequested.clear();
    const auto key = "tasks/titleDraft/" + m_project + '/' + m_selected;
    m_titleEditor->setPlainText(Settings::value(key, TaskBoard::item(m_detail).displayTitle()).toString());
    m_titleEditor->setFixedHeight(qMax(m_title->height() + 8, m_titleEditor->fontMetrics().lineSpacing() * 2 + 16));
    m_title->hide(); m_titleEditor->setReadOnly(false); m_titleEditor->show(); m_titleEditor->setFocus(); m_titleEditor->selectAll(); m_titleError->hide();
}

void TaskInspector::cancelRename() {
    if (m_titleSaving) return;
    Settings::remove("tasks/titleDraft/" + m_project + '/' + m_selected);
    m_titleEditor->hide(); m_title->show(); m_titleError->hide(); m_renameRequested.clear();
}

void TaskInspector::finishRename() {
    if (m_titleSaving || !m_titleEditor->isVisible() || selectionPending()) return;
    const auto title = m_titleEditor->toPlainText().trimmed();
    if (title.isEmpty() || title.toUtf8().size() > 512 || std::any_of(title.begin(), title.end(), [](QChar c) { return c.category() == QChar::Other_Control; })) {
        m_titleError->setText(tr("Enter a single-line title, up to 512 bytes. Enter saves; Escape cancels.")); m_titleError->show(); return;
    }
    if (title == TaskBoard::item(m_detail).displayTitle()) { cancelRename(); return; }
    m_titleSaving = true; m_titleEditor->setReadOnly(true); m_titleError->hide();
    const auto uuid = m_selected; const auto project = m_project; QPointer<TaskInspector> guard(this);
    m_board->renameTask(uuid, title, [guard, uuid, project, title] {
        Settings::remove("tasks/titleDraft/" + project + '/' + uuid);
        if (!guard) return; guard->m_titleSaving = false;
        if (guard->m_selected == uuid && guard->m_project == project) {
            ++guard->m_generation; guard->m_detail["title"] = title; guard->cancelRename(); guard->renderDetail(); guard->fetch();
        }
    }, [guard, uuid, project](const QString &error) {
        if (!guard) return; guard->m_titleSaving = false;
        if (guard->m_selected == uuid && guard->m_project == project) {
            guard->m_titleEditor->setReadOnly(false); guard->m_titleError->setText(tr("Title was not saved. Your edit is kept. Try Enter again.\n%1").arg(error)); guard->m_titleError->show();
        }
    });
}

void TaskInspector::closeInlineDetails() {
    m_descriptionEditing = false; m_descriptionSeed = false;
}

void TaskInspector::openInlineDetails() {
    if (m_detail.isEmpty() || selectionPending() || !canLeaveDetails()) return;
    const auto task = TaskBoard::item(m_detail);
    if (task.stage != QLatin1String("todo") && task.stage != QLatin1String("backlog")) return;
    m_inlineRequested.clear(); m_descriptionEditing = true; m_detailTabs->setCurrentIndex(0);
    const auto key = inputKey(m_detail);
    m_commentComposer->setSessionKey(key);
    m_commentComposer->ensureDraftLoaded(key);
    m_descriptionSeed = true; seedDescriptionDraft();
    renderDetail(); m_commentComposer->editor()->setFocus();
}

void TaskInspector::seedDescriptionDraft() {
    const auto key = inputKey(m_detail);
    if (!m_descriptionSeed || m_commentComposer->isDraftLoading(key)) return;
    const auto text = m_detail.value("text").toString(); const auto encoded = m_detail.value("attachments").toArray();
    m_descriptionSeed = false; QPointer<TaskInspector> guard(this);
    AttachmentFiles::runAsync<QList<MessageAttachment>>(this, [encoded] { return TaskContent::decode(encoded); }, [guard, key, text](const QList<MessageAttachment> &files) {
        if (!guard || guard->m_commentComposer->sessionKey() != key) return;
        guard->m_commentComposer->offerDraft(key, text, files);
    });
}

void TaskInspector::renderCommands(const TaskItem &task) {
    const auto state = m_board->rowState(task);
    QString key = task.uuid + QLatin1Char('/') + task.stage;
    for (auto i = state.blocked.cbegin(); i != state.blocked.cend(); ++i) key += QString::number(i.key()) + i.value();
    if (m_commandsKey == key) return;
    m_commandsKey = key;
    auto actions = m_detail.isEmpty() ? QList<TaskAction>() : TaskBoard::actions(task.stage); actions.removeAll(TaskAction::Edit); actions.removeAll(TaskAction::Rename); actions.removeAll(TaskAction::Top);
    const auto primary = TaskBoard::primaryAction(task.stage);
    if (!m_detail.isEmpty() && !actions.contains(primary)) actions << primary;
    const auto primaryReason = state.blocked.value(int(primary));
    m_blocker->setText(primary == TaskAction::SendNow && !primaryReason.isEmpty() ? tr("Cannot start: %1").arg(primaryReason) : primaryReason);
    m_blocker->setVisible(!m_detail.isEmpty() && !primaryReason.isEmpty());
    for (auto it = m_commandButtons.cbegin(); it != m_commandButtons.cend(); ++it) {
        const auto action = TaskAction(it.key()); auto *button = it.value();
        button->setVisible(actions.contains(action));
        stableIcon(button, TaskBoard::actionIcon(action), action == primary ? TaskColors::of(m_dark).accent : TaskColors::of(m_dark).muted);
        const auto why = state.blocked.value(int(action));
        button->setToolTip(why.isEmpty() ? TaskBoard::actionLabel(action) : why); button->setEnabled(why.isEmpty());
        if (button->property("primaryAction").toBool() != (action == primary)) {
            button->setProperty("primaryAction", action == primary); button->style()->unpolish(button); button->style()->polish(button);
        }
    }
}

void TaskInspector::setContentScale(double scale) { m_activity->setContentScale(scale); m_commentComposer->setContentScale(scale); }
void TaskInspector::setColumnWidth(int width) {
    m_headerColumn->setColumnWidth(width); m_inputColumn->setColumnWidth(width); m_activity->setColumnWidth(width);
}

void TaskInspector::setTheme(bool dark) {
    if (property("taskThemeApplied").toBool() && m_dark == dark) {
        // The detached pane's new parent and workspace repolishing can replace
        // its Window palette even when the selected theme has not changed.
        m_activity->setTheme(dark);
        return;
    }
    setProperty("taskThemeApplied", true); m_dark = dark; if (!m_cards) return;
    const auto colors = TaskColors::of(dark);
    const auto sheet = QStringLiteral("#taskInspector{background:%2;color:%1;}"
        "#taskInspector QLabel{color:%1;}QScrollArea,#taskInspectorCards{background:%2;border:0;}"
        "#taskDetailsPane{background:%6;color:%1;border:0;}#taskDetailsPane QLabel{color:%1;}"
        "#taskInspectorState,#taskDetailEmpty,#taskViewIdentity, #taskMessageMode{color:%4;font-size:11px;}"
        "#taskViewTitle,#taskTitleEditor{color:%1;font-size:16px;font-weight:600;}"
        "#taskDetailsPane QPushButton[taskHeaderButton=\"true\"],#taskInspectorActions QPushButton{color:%1;background:%2;border:1px solid %3;border-radius:7px;padding:5px 9px;}"
        "#taskDetailsPane QPushButton[taskHeaderButton=\"true\"]:disabled,#taskInspectorActions QPushButton:disabled{color:%4;}"
        "#taskDetailsPane #taskOpenActivity,#taskDetailsPane #taskInspectorEdit{padding:0;border:0;background:transparent;}"
        "#taskInspectorActions QPushButton[primaryAction=\"true\"]{color:%5;border-color:%5;}"
        "#taskDetailTabs::tab{font-size:12px;color:%4;padding:6px 10px;border:0;border-bottom:2px solid transparent;}"
        "#taskDetailTabs::tab:selected{color:%1;border-bottom-color:%5;}"
        "#taskInspectorSplit::handle{background:%3;}"
        "#taskDetailMore,#taskHistoryLatest{color:%5;border:0;padding:4px;}"
        "#taskDescriptionCancel,#taskMessageCheck{font-size:11px;min-height:0;padding:0 8px;}" )
        .arg(colors.fg.name(), colors.surface.name(), colors.border.name(), colors.muted.name(), colors.accent.name(), dark ? "#1c2229" : "#ffffff") + workspaceScrollbars(dark);
    setWorkspaceStyle(this, sheet); setWorkspaceStyle(m_details, sheet);
    stableIcon(m_add, "add", colors.muted);
    stableIcon(m_details->findChild<QPushButton *>("taskViewRename"), "more", colors.muted);
    m_activity->setTheme(dark); static_cast<ToolbarChip *>(m_assignment)->setTheme(dark);
    m_commandsKey.clear();
    setWorkspaceStyle(m_dropLine, QStringLiteral("background:%1;border-radius:1px;").arg(colors.accent.name()));
    m_commentComposer->setTheme(dark);
    m_blocker->setStyleSheet(QStringLiteral("color:%1;font-size:12px;").arg(colors.warn.name()));
    m_noteError->setStyleSheet(QStringLiteral("color:%1;").arg(QStringLiteral("#e06c75")));
    stableIcon(m_open, "external", colors.muted); stableIcon(m_edit, "task-list", colors.muted);
    for (auto *button : {m_open, m_edit}) setWorkspaceStyle(button,
        QStringLiteral("QPushButton{border:0;border-radius:5px;background:transparent;}QPushButton:hover{background:%1;}").arg(colors.hover.name()));
    refresh();
}

QWidget *TaskInspector::takeDetailsPane() {
    m_detached = true; m_details->setParent(nullptr); return m_details;
}

bool TaskInspector::descriptionMode(const QJsonObject &task) const {
    const auto stage = task.value("stage").toString();
    const bool editable = stage == QLatin1String("todo") || stage == QLatin1String("backlog");
    return editable && m_descriptionEditing;
}
QString TaskInspector::inputKey(const QJsonObject &task) const {
    return QStringLiteral("task-%1\n%2\n%3").arg(descriptionMode(task) ? QStringLiteral("description") : QStringLiteral("comment"), m_project, task.value("uuid").toString());
}
void TaskInspector::restoreNewCard() {
    QSettings settings; const auto key = "tasks/newCard/" + m_project;
    m_creation = SettingsJson::object(key + "/request");
    m_newTitle->setText(m_creation.isEmpty() ? settings.value(key + "/title").toString() : m_creation.value("title").toString());
    m_newTitle->setReadOnly(!m_creation.isEmpty()); m_creationHeader->setCurrentWidget(m_newTitle->text().isEmpty() ? static_cast<QWidget *>(m_scope) : m_newTitle);
    m_add->setEnabled(!m_project.isEmpty()); m_createError->hide();
    if (!m_creation.isEmpty()) { m_createError->setText(tr("Creation was interrupted. Press Enter to recover this same card.")); m_createError->show(); }
}
void TaskInspector::createCard() {
    if (m_creating || m_project.isEmpty() || !canLeaveDetails()) return;
    const auto title = m_newTitle->text().trimmed();
    if (title.isEmpty()) return;
    if (std::any_of(title.begin(), title.end(), [](QChar c) { return c.category() == QChar::Other_Control; })) { m_createError->setText(tr("Use a single-line task title without control characters.")); m_createError->show(); return; }
    if (title.toUtf8().size() > 512) { m_createError->setText(tr("Use a shorter task title (up to 512 bytes).")); m_createError->show(); return; }
    const auto project = m_project; const auto key = "tasks/newCard/" + project;
    if (m_creation.isEmpty()) {
        m_creation = {{"project", project}, {"title", title}, {"request_id", QUuid::createUuid().toString(QUuid::WithoutBraces)},
                      {"stage", m_stage == QLatin1String("backlog") ? "backlog" : "todo"}, {"session", m_sessionId.isEmpty() ? "any" : m_session}, {"host", m_sessionId.isEmpty() ? QString() : m_host}, {"session_id", m_sessionId}};
        // A reservation: stored now, or nothing is sent (Settings.h).
        if (!SettingsJson::storeNow(key + "/request", QJsonDocument(m_creation))) { m_creation = {}; m_createError->setText(tr("Could not save the new card identity. Nothing was sent.")); m_createError->show(); return; }
    }
    m_creating = true; m_newTitle->setReadOnly(true); m_createError->hide();
    QPointer<TaskInspector> guard(this);
    m_board->createNamedTask(m_creation, [guard, project, key](const QJsonObject &task) {
        if (!guard) return;
        guard->m_creating = false; Settings::remove(key);
        if (project != guard->m_project) return;
        guard->m_creation = {}; guard->m_newTitle->clear(); guard->m_newTitle->setReadOnly(false); guard->m_creationHeader->setCurrentWidget(guard->m_scope);
        guard->openTask(task.value("uuid").toString()); guard->m_inlineRequested = task.value("uuid").toString(); guard->publishDetail(task); guard->m_commentComposer->editor()->setFocus();
    }, [guard, project](const QString &error) {
        if (!guard) return; guard->m_creating = false;
        if (project != guard->m_project) return;
        guard->m_createError->setText(tr("%1\nPress Enter to retry with the same card identity.").arg(error)); guard->m_createError->show();
    });
}
