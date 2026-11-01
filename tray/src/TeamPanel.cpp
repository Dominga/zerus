#include "TeamPanel.h"
#include <memory>
#include "TeamTreeIndent.h"
#include "TeamDesignerPanel.h"
#include "TeamTaskCard.h"
#include "TeamWidgets.h"
#include <QStackedWidget>
#include "NewSessionDense.h"
#include "SlowEventLog.h"
#include "TeamController.h"
#include "WorkspaceIcons.h"
#include "WorkspaceSkin.h"
#include <QClipboard>
#include <QComboBox>
#include <QCursor>
#include <QDialog>
#include <QDialogButtonBox>
#include <QGuiApplication>
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonDocument>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QScrollBar>
#include <QThread>
#include <QTimer>
#include <QToolButton>
#include <QUuid>
#include <QVBoxLayout>
#include <cmath>
#include <tuple>

using namespace Teams;

using namespace TeamUi;

TeamPanel::TeamPanel(TeamController *controller, QWidget *parent) : QWidget(parent), m_controller(controller)
{
    setObjectName("teamPanel");
    setStyleSheet(teamSheet(colorsFor(true)));
    auto *layout = new QVBoxLayout(this);
    m_outer = layout;
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    m_scroll = new QScrollArea;
    m_scroll->setObjectName("teamPanelScroll");
    m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_stack = new QStackedWidget;
    m_stack->addWidget(m_scroll);
    m_card = new TeamTaskCard;
    m_stack->addWidget(m_card);
    m_designer = new TeamDesignerPanel(m_controller);
    m_designer->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return confirm(name, title, text, action); };
    m_stack->addWidget(m_designer);
    layout->addWidget(m_stack);
    m_card->back = [this] { m_openTask = 0; render(); };
    m_card->act = [this](const QString &operation, const QJsonObject &input) { act(operation, input); };
    m_card->openTask = [this](int task) { openTask(task); };
    m_card->openTeam = [this](const QString &team) { m_controller->watch(team); };
    m_card->openSession = [this](const QString &host, const QString &session) { if (openSession) openSession(host, session); };
    m_card->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return confirm(name, title, text, action); };
    m_card->reassign = [this](int task, const QPoint &global) {
        if (const auto *team = m_controller->watched()) reassignMenu(*team, task, global);
    };
    m_card->problemActions = [this](const Member &member, const Status &status) -> QWidget * {
        const auto *team = m_controller->watched();
        return team ? problemActions(*team, member, status) : nullptr;
    };
    QObject::connect(m_controller, &TeamController::teamsChanged, this, [this] { pollRender(); });
    QObject::connect(m_controller, &TeamController::watchedChanged, this, [this] { pollRender(); });
    QObject::connect(m_controller, &TeamController::showCancelledChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_controller, &TeamController::feedChanged, this, [this] { pollRender(); });
    render();
}

void TeamPanel::setTheme(bool dark)
{
    m_dark = dark;
    setStyleSheet(teamSheet(colorsFor(dark)));   // accent primary buttons, segments, fields
    m_designer->setDark(dark);
    scheduleRender();
}

void TeamPanel::setEmbedded(bool on)
{
    m_embedded = on;
    m_scroll->setVerticalScrollBarPolicy(on ? Qt::ScrollBarAlwaysOff : Qt::ScrollBarAsNeeded);
    fitEmbedded();
}

void TeamPanel::fitEmbedded()
{
    if (!m_embedded || !m_content) return;
    const int width = m_scroll->viewport()->width();
    auto *layout = m_content->layout();
    const int height = layout && layout->hasHeightForWidth() ? layout->totalHeightForWidth(width) : m_content->sizeHint().height();
    if (m_scroll->height() != height) m_scroll->setFixedHeight(qMax(0, height));
}

void TeamPanel::showDesigner(bool on)
{
    if (m_designerShown == on) return;
    m_designerShown = on;
    m_controller->setWatchCatalog(on);
    if (on) { m_designer->render(); m_stack->setCurrentWidget(m_designer); if (answersChanged) answersChanged(0); }
    else render();
}

void TeamPanel::openTask(int task)
{
    m_openTask = task;
    render();
}

void TeamPanel::showTeam(const QString &team)
{
    if (m_team == team) return;
    m_team = team; m_showBlockers = false; m_feedMember.clear(); m_notice.clear(); m_openTask = 0;
    m_controller->watch(team);
    render();
}

void TeamPanel::scheduleRender()
{
    m_forced = true;
    queueRender();
}

void TeamPanel::pollRender()
{
    queueRender();
}

void TeamPanel::queueRender()
{
    if (m_pending) return;
    m_pending = true;
    QTimer::singleShot(0, this, [this] {
        m_pending = false;
        if (std::exchange(m_forced, false)) render(); else refresh();
    });
}

void TeamPanel::refresh()
{
    // An owner's change waiting in the queue renders anyway.
    if (m_forced) return;
    if (!m_renderedKey.isEmpty() && renderKey() == m_renderedKey) return;
    render();
}

QByteArray TeamPanel::renderKey() const
{
    const Team *team = m_controller->watched();
    if (!team && !m_team.isEmpty()) team = m_controller->team(m_team);
    // A plan is rebuilt on every refresh, as it always was: it shows the
    // window's free sessions and the catalog besides the team.
    if (!team || team->status == "planning") return {};
    QByteArray key = m_team.toUtf8() + '\n' + team->id.toUtf8() + ' ' + char('0' + team->full) + char('0' + (width() < 400)) + char('0' + m_dark)
                     + char('0' + m_controller->showCancelled()) + ' ' + QByteArray::number(m_openTask) + ' ' + m_notice.toUtf8() + '\n'
                     + m_controller->shapeOf(team->id) + '\n';
    // The summary's age, "2h 05m", changes once a minute.
    if (team->created > 0) key += duration(QDateTime::currentSecsSinceEpoch() - team->created).toUtf8() + '\n';
    const auto feed = m_controller->feed();
    key += QByteArray::number(feed.size()) + ' ' + QByteArray::number(feed.isEmpty() ? 0 : feed.first().value("seq").toInteger()) + '\n';
    // The summary reads only whether a member is offline or asks (and what);
    // working and idle change nothing in it.
    if (m_live)
        for (const auto &member : team->members) {
            const Live live = m_live(member);
            key += member.id.toUtf8() + ' ' + char('0' + live.offline) + char('0' + live.attention)
                   + ' ' + (live.attention ? live.attentionText.toUtf8() : QByteArray()) + '\n';
        }
    return key;
}

void TeamPanel::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    // The graph needs 400px; below that the list by layer replaces it.
    static const int threshold = 400;
    if (m_content && (m_content->property("narrow").toBool() != (width() < threshold))) scheduleRender();
    fitEmbedded();
}

bool TeamPanel::confirm(const QString &name, const QString &title, const QString &text, const QString &action)
{
    if (findChild<QMessageBox *>(name)) return false;
    QMessageBox dialog(QMessageBox::Warning, title, text, QMessageBox::NoButton, this);
    dialog.setObjectName(name); dialog.setTextFormat(Qt::PlainText);
    // The box takes the panel's sheet (light, transparent buttons in the dark
    // theme) but not its background, so it gets the panel's own colours.
    const auto c = colorsFor(m_dark);
    dialog.setStyleSheet(QStringLiteral("QMessageBox{background:%1;}QMessageBox QLabel{color:%2;}").arg(c.card.name(), c.fg.name()));
    auto *ok = dialog.addButton(action, QMessageBox::DestructiveRole);
    auto *cancel = dialog.addButton(QMessageBox::Cancel);
    dialog.setDefaultButton(cancel); dialog.setEscapeButton(cancel);
    RenderHold hold(this);
    dialog.exec();
    return dialog.clickedButton() == ok;
}

void TeamPanel::holdWhileOpen(QMenu *menu)
{
    auto held = std::make_shared<bool>(false);
    auto level = std::make_shared<int>(0);
    const auto release = [this, held, level] {
        if (!*held) return;
        *held = false;
        // After aboutToHide the triggered action still runs, and it may open
        // a dialog whose loop runs our timers: release once every loop the
        // menu and its action opened has returned.
        releaseUnwound(*level);
    };
    QObject::connect(menu, &QMenu::aboutToShow, this, [this, held, level] {
        if (*held) return;
        *held = true; ++m_confirming;
        *level = QThread::currentThread()->loopLevel();
    });
    QObject::connect(menu, &QMenu::aboutToHide, this, release);
    QObject::connect(menu, &QObject::destroyed, this, release);
}

void TeamPanel::releaseUnwound(int level)
{
    QTimer::singleShot(0, this, [this, level] {
        if (QThread::currentThread()->loopLevel() > level) {
            // Still inside the menu's or the action's own loop: look again.
            QTimer::singleShot(50, this, [this, level] { releaseUnwound(level); });
            return;
        }
        releaseHold();
    });
}

void TeamPanel::releaseHold()
{
    // The polls kept coming: the views catch up once the caller has returned.
    if (--m_confirming == 0)
        QTimer::singleShot(0, this, [this] { scheduleRender(); if (confirmClosed) confirmClosed(); });
}

void TeamPanel::approvePlan(const Team &team)
{
    // The question names the team; the answer goes to that team, whatever
    // shows by then. #724: the same question as Waiting for you's Approve.
    const auto approval = Teams::planApproval(team, planModeChoice(team.id));
    if (!confirm(approval.name, approval.title, approval.text, approval.action)) return;
    QJsonObject input = approval.input;
    input.insert("team", team.id);
    act("approve", input);
}

void TeamPanel::choosePlanMode(const QString &team, const QString &mode)
{
    if (team.isEmpty() || m_planModes.value(team) == mode) return;
    m_planModes.insert(team, mode);
    scheduleRender();
}

void TeamPanel::act(const QString &operation, const QJsonObject &input, const std::function<void(bool, const QString &)> &done)
{
    QJsonObject body = input;
    if (!body.contains("team")) body.insert("team", m_team);
    m_notice.clear();
    m_controller->act(operation, body, [this, guard = QPointer<TeamPanel>(this), done](bool ok, const QString &message) {
        if (!guard) return;
        m_notice = ok ? QString() : message;
        if (done) done(ok, message);
        scheduleRender();
    });
}

void TeamPanel::finishTeam(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    if (m_finishDialog) m_finishDialog->close();
    auto *dialog = new QDialog(this);
    dialog->setObjectName("teamFinishDialog");
    dialog->setWindowTitle(tr("Finish team"));
    dialog->setModal(true);
    dialog->resize(460, 300);
    auto *v = new QVBoxLayout(dialog);
    v->setContentsMargins(14, 14, 14, 12); v->setSpacing(8);
    v->addWidget(label(tr("Finish %1?").arg(team.name), c.fg, 14, true, "teamFinishTitle"));
    auto *hint = label(tr("Every task is decided. The summary goes to the lead and the team becomes Done."), c.text2, 12);
    hint->setWordWrap(true); v->addWidget(hint);
    auto *summary = new QPlainTextEdit; summary->setObjectName("teamFinishSummary"); summary->setFixedHeight(110);
    summary->setPlaceholderText(tr("What the team delivered, and anything left undone…"));
    v->addWidget(summary);
    auto *count = label(QStringLiteral("0 / 4000"), c.muted, 11, false, "teamFinishCount");
    count->setAlignment(Qt::AlignRight); v->addWidget(count);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Cancel);
    auto *finish = buttons->addButton(tr("Finish team"), QDialogButtonBox::AcceptRole);
    finish->setObjectName("teamFinishConfirm"); finish->setEnabled(false); finish->setDefault(true);
    v->addWidget(buttons);
    const auto update = [summary, finish, count] {
        const QString text = summary->toPlainText().trimmed();
        const int characters = int(text.toUcs4().size());
        count->setText(tr("%1 / 4000").arg(characters));
        finish->setEnabled(!text.isEmpty() && characters <= 4000);
    };
    QObject::connect(summary, &QPlainTextEdit::textChanged, dialog, update);
    QObject::connect(buttons, &QDialogButtonBox::accepted, dialog, &QDialog::accept);
    QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    QObject::connect(dialog, &QDialog::accepted, this, [this, summary] {
        const QString text = summary->toPlainText().trimmed();
        if (text.isEmpty() || int(text.toUcs4().size()) > 4000) return;
        m_controller->finishTeam(text, [this, guard = QPointer<TeamPanel>(this)](bool ok, const QString &message) {
            if (!guard) return;
            m_notice = ok ? QString() : message;
            scheduleRender();
        });
    });
    m_finishDialog = dialog;
    // A hidden dialog is not kept: each Finish click would parent another one.
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->open();
}

void TeamPanel::remind(const Team &team, const Member &member, int taskId)
{
    const auto *task = team.task(taskId);
    const auto *attempt = task ? task->last() : nullptr;
    if (!task || !attempt || !sendMessage) return;
    const QJsonObject report{{"request_id", QUuid::createUuid().toString(QUuid::WithoutBraces)}, {"team", team.id},
                             {"task", task->id}, {"attempt", attempt->id}, {"text", "..."}};
    const QString text = tr("[Zerus team · from: You · task: #%1 · attempt: %2]\nYour turn ended without a report for task #%1 %3. "
                            "Report once with JSON on stdin:\n  hgs %4team report --json\n  %5")
                             .arg(task->id).arg(attempt->n).arg(task->title, member.host.isEmpty() ? QString() : QStringLiteral("@HOST "),
                                  QString::fromUtf8(QJsonDocument(report).toJson(QJsonDocument::Compact)));
    sendMessage(member.host, member.session, text);
}

void TeamPanel::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    if (m_dirty) render();
}

void TeamPanel::render()
{
    const SlowEventLog::Scope scope(this, "render");
    m_renderedKey.clear();
    // A hidden tab renders once it is shown; polls keep coming meanwhile.
    if (!isVisible() && m_content) { m_dirty = true; return; }
    if (m_confirming) return;
    if (m_designerShown) { m_stack->setCurrentWidget(m_designer); return; }
    m_dirty = false;
    const Colors c = colorsFor(m_dark);
    const int scroll = m_scroll->verticalScrollBar()->value();
    const bool narrow = width() < 400;
    auto *content = new QWidget;
    content->setObjectName("teamPanelContent");
    content->setProperty("narrow", narrow);
    auto *layout = new QVBoxLayout(content);
    layout->setContentsMargins(narrow ? 8 : 12, narrow ? 8 : 12, narrow ? 8 : 12, 12);
    layout->setSpacing(12);
    const Team *team = m_controller->watched();
    if (!team && !m_team.isEmpty()) team = m_controller->team(m_team);
    if (!team && m_team.isEmpty()) {
        layout->addWidget(emptyState(narrow));
        if (answersChanged) answersChanged(0);
    } else if (!team) {
        layout->addWidget(label(tr("Loading team…"), c.text2, 13, true, "teamEmptyTitle"));
        if (answersChanged) answersChanged(0);
    } else if (team->status != "planning") {
        // Running and later: a short summary; the work is in the Team tab (D4).
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        const QString status = displayStatus(*team, live);
        layout->addWidget(summary(*team, status, narrow));
        if (!m_notice.isEmpty()) {
            auto *notice = label(m_notice, c.danger, 12, false, "teamNotice");
            notice->setWordWrap(true);
            layout->addWidget(notice);
        }
        const int waiting = Teams::answers(*team, live);
        if (waiting) layout->addWidget(answers(*team));
        layout->addWidget(summaryMembers(*team, narrow));
        auto *foot = label(tr("Plan, graph and member changes live in the Team tab."), c.dim, 11); foot->setWordWrap(true);
        layout->addWidget(foot);
        if (answersChanged) answersChanged(waiting);
    } else {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        const QString status = displayStatus(*team, live);
        layout->addWidget(header(*team, status, narrow));
        if (!m_notice.isEmpty()) {
            auto *notice = label(m_notice, c.danger, 12, false, "teamNotice");
            notice->setWordWrap(true);
            layout->addWidget(notice);
        }
        layout->addWidget(planning(*team));
        const int waiting = Teams::answers(*team, live);
        if (waiting) layout->addWidget(answers(*team));
        layout->addWidget(planMembers(*team, narrow));
        // U58: the plan leaves out cancelled tasks unless Show cancelled is on.
        layout->addWidget(tasks(m_controller->showCancelled() ? *team : withoutCancelled(*team), narrow));
        if (team->full) layout->addWidget(feed(*team));
        if (answersChanged) answersChanged(waiting);
    }
    layout->addStretch();
    retire(m_scroll->takeWidget());   // a click inside it may be running
    m_scroll->setWidget(content);
    m_content = content;
    m_scroll->verticalScrollBar()->setValue(scroll);
    if (m_embedded) { fitEmbedded(); QTimer::singleShot(0, this, [this] { fitEmbedded(); }); }
    // While planning, the plan's actions stay pinned below the content.
    if (m_footer) { retire(m_footer); m_footer = nullptr; }
    if (team && team->status == "planning" && !m_openTask) { m_footer = planFooter(*team); m_outer->addWidget(m_footer); }
    if (team && m_openTask) {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        m_card->setTask(*team, m_openTask, live, m_dark);
        m_stack->setCurrentWidget(m_card);
    } else m_stack->setCurrentWidget(m_scroll);
    m_renderedKey = renderKey();
}

QWidget *TeamPanel::emptyState(bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c); box->setObjectName("teamEmpty");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(narrow ? 12 : 20, 20, narrow ? 12 : 20, 20); v->setSpacing(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.text2).pixmap(36, 36)); icon->setAlignment(Qt::AlignCenter);
    v->addWidget(icon);
    auto *title = label(tr("No team in this session"), c.fg, 13, true, "teamEmptyTitle"); title->setAlignment(Qt::AlignCenter);
    v->addWidget(title);
    auto *hint = label(tr("Build a team around a goal with this session as the coordinator, or set up roles and templates first."), c.muted, 12);
    hint->setWordWrap(true); hint->setAlignment(Qt::AlignCenter); hint->setObjectName("teamEmptyHint");
    v->addWidget(hint);
    if (newTeam) {
        auto *create = button(tr("New team…"), "teamEmptyNew", true);
        QObject::connect(create, &QPushButton::clicked, this, [this] { newTeam(); });
        v->addWidget(create);
    }
    if (openDesigner) {
        auto *designer = button(tr("Team designer…"), "teamEmptyDesigner");
        QObject::connect(designer, &QPushButton::clicked, this, [this] { openDesigner(); });
        v->addWidget(designer);
    }
    return box;
}

QWidget *TeamPanel::summary(const Team &team, const QString &status, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *box = card(c); box->setObjectName("teamCard");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(7);
    auto *top = row(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.fg).pixmap(16, 16)); top->addWidget(icon);
    auto *name = elided(team.name, c.fg, 14); QFont f = name->font(); f.setWeight(QFont::DemiBold); name->setFont(f);
    name->setObjectName("teamName");
    top->addWidget(name, 1);
    auto *badge = new Pill(statusLabel(status), toneColor(c, statusTone(status)), status, false);
    badge->setObjectName("teamStatusBadge");
    top->addWidget(badge);
    v->addLayout(top);
    if (team.pendingPhase > 0 || team.waitingForLead) {
        auto *chips = row(6);
        if (team.pendingPhase > 0) {
            const auto drafts = team.draftPhases();
            QStringList ids; for (int n : drafts) ids << QString::number(n);
            auto *pill = new Pill(drafts.size() > 1 ? tr("phases %1 pending approval").arg(ids.join(QStringLiteral(", ")))
                                                    : tr("phase %1 pending approval").arg(team.pendingPhase), c.warn);
            pill->setObjectName("teamPendingPhase");
            chips->addWidget(pill);
        }
        if (team.waitingForLead) {
            auto *pill = new Pill(tr("waiting for the lead"), c.accent);
            pill->setObjectName("teamWaitingForLead");
            chips->addWidget(pill);
        }
        chips->addStretch();
        v->addLayout(chips);
    }
    // G6: two bars — the phase in work (or the next draft) and the phases —
    // instead of one over every task of every phase.
    v->addWidget(new PhaseProgressBars(c, phaseProgress(team)));
    const int problemCount = problems(team, live).size();
    QString meta = tr("%1/%2 accepted").arg(accepted(team)).arg(team.tasks.size()).toHtmlEscaped();
    if (problemCount) meta += QStringLiteral(" &nbsp;<span style=\"color:%1;\">%2</span>").arg(c.warn.name(),
        problemCount == 1 ? tr("1 problem") : tr("%1 problems").arg(problemCount));
    if (team.created > 0) meta += QStringLiteral(" &nbsp;· %1").arg(duration(QDateTime::currentSecsSinceEpoch() - team.created).toHtmlEscaped());
    auto *metaLabel = new QLabel(meta); metaLabel->setTextFormat(Qt::RichText); metaLabel->setObjectName("teamLegend");
    metaLabel->setStyleSheet(QStringLiteral("color:%1;font-size:11px;").arg(c.text3.name()));
    v->addWidget(metaLabel);
    if (team.hasFinish) {
        v->addWidget(label(tr("Finished by %1").arg(team.finish.by.isEmpty() ? tr("the owner") : team.finish.by), c.fg, 12, true, "teamFinishBy"));
        auto *summaryText = label(team.finish.summary, c.text2, 12); summaryText->setWordWrap(true);
        summaryText->setTextInteractionFlags(Qt::TextSelectableByMouse); summaryText->setObjectName("teamFinishSummaryText");
        v->addWidget(summaryText);
    }
    if (status == "stuck") {
        auto *stuckLine = label(tr("Stuck — nothing can start; see the Team tab."), c.warn, 12, false, "teamStuck"); stuckLine->setWordWrap(true);
        v->addWidget(stuckLine);
    }
    auto *actions = row(6);
    if (allDecided(team) && (team.status == "running" || team.status == "paused")) {
        auto *finish = button(tr("Finish…"), "teamFinish", true);
        QObject::connect(finish, &QPushButton::clicked, this, [this, id = team.id] {
            const auto *current = m_controller->watched();
            if (!current || current->id != id) current = m_controller->team(id);
            if (current) finishTeam(*current);
        });
        actions->addWidget(finish);
    }
    auto *open = button(tr("Open Team"), "teamOpenTeam", true);
    QObject::connect(open, &QPushButton::clicked, this, [this] { if (openWorkspace) openWorkspace(); });
    actions->addWidget(open, 1);
    v->addLayout(actions);
    Q_UNUSED(narrow);
    return box;
}

QWidget *TeamPanel::summaryMembers(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *box = new QWidget; box->setObjectName("teamSummaryMembers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(0);
    auto *title = row(6);
    title->addWidget(label(tr("Members"), c.fg, 12, true));
    // F7: the sessions a finished team left running are counted apart.
    int active = 0;
    for (const auto &m : team.members)
        if (!((team.status == "done" || team.status == "stopped") && leftMember(team, m.id))) ++active;
    title->addWidget(label(QString::number(active), c.muted, 12));
    title->addStretch();
    v->addLayout(title);
    // Spec 5.6 / 6: the members as a tree, a sub-lead's reports one level in
    // with their line; rows sit 2 px apart and the elbow meets the row's middle.
    v->addSpacing(2);
    TeamTreeIndent::Metrics metrics; metrics.elbowY = 14; metrics.gap = 2;
    bool firstRow = true;
    for (const auto &node : team.memberTree()) {
        const auto &m = *node.member;
        // F7: a session that still runs after a Done/Stopped team is shown
        // apart, never as an active member.
        if ((team.status == "done" || team.status == "stopped") && leftMember(team, m.id)) continue;
        const auto status = memberStatus(team, m, live(m));
        const Task *current = status.task ? team.task(status.task) : nullptr;
        const auto mark = current ? phaseMark(team, *current) : PhaseMark{};
        auto *block = new QWidget; block->setObjectName("teamSummaryMemberBlock");
        auto *bv = new QVBoxLayout(block); bv->setContentsMargins(0, 0, 0, 0); bv->setSpacing(1);
        auto *line = new QWidget; line->setObjectName("teamSummaryMember"); line->setFixedHeight(28);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(2, 0, 2, 0); h->setSpacing(6);
        auto *tile = new QLabel; tile->setPixmap(agentLogo(m.agent, c.fg).pixmap(16, 16)); h->addWidget(tile);
        const QString role = team.roleOf(m.id);
        h->addWidget(new Pill(role, roleColor(role, m_dark)));
        if (!narrow) h->addWidget(elided(m.model.isEmpty() ? m.agent : m.model, c.text3, 11), 1);
        else h->addStretch();
        // A1: the attempt's phase icon and its tooltip beside the member.
        auto *statusMark = new StatusMark(status.text, toneColor(c, mark.tone != Tone::Neutral ? mark.tone : status.tone),
                                          mark.kind.isEmpty() ? status.kind : mark.kind, 0, Qt::AlignLeft, nullptr, mark.tooltip);
        statusMark->setObjectName("teamSummaryMemberState");
        statusMark->setMaximumWidth(narrow ? 120 : 170);
        h->addWidget(statusMark);
        bv->addWidget(line);
        // A5: its queue, the current task first, indented under the member.
        if (auto *queue = memberQueueWidget(team, m, c, [this](int id) { openTask(id); })) {
            auto *indent = row(0); indent->addSpacing(34); indent->addWidget(queue, 1);
            bv->addLayout(indent);
        }
        auto *tree = new TeamTreeIndent(block, qMax(0, node.depth - 1), node.last, node.through.mid(1), c.border, firstRow, metrics);
        tree->setObjectName("teamSummaryMemberTree");
        v->addWidget(tree);
        firstRow = false;
    }
    if ((team.status == "done" || team.status == "stopped") && !team.leftRunning.isEmpty()) v->addWidget(leftoverMembers(team, narrow));
    return box;
}

QWidget *TeamPanel::leftoverMembers(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamLeftover");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 6, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Left from %1").arg(team.name), c.fg, 12, true, "teamLeftoverTitle"));
    title->addWidget(new Pill(QString::number(team.leftRunning.size()), c.muted));
    title->addStretch();
    v->addLayout(title);
    auto *hint = label(tr("These sessions still run after the team ended. Zerus never stops them by itself."), c.muted, 11);
    hint->setWordWrap(true);
    v->addWidget(hint);
    for (const auto &left : team.leftRunning) {
        auto *line = card(c);
        line->setObjectName("teamLeftoverMember");
        line->setProperty("member", left.member);
        line->setProperty("session", left.session);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(8, 6, 6, 6); h->setSpacing(8);
        auto *tile = new QLabel; tile->setPixmap(workspaceIcon("terminal", c.muted).pixmap(14, 14));
        h->addWidget(tile, 0, Qt::AlignTop);
        auto *body = new QVBoxLayout; body->setSpacing(2);
        auto *top = row(6);
        const QString role = left.role.isEmpty() ? left.member : left.role;
        top->addWidget(new Pill(role, roleColor(role, m_dark)));
        top->addWidget(elided(left.session, c.text2, 11), 1);
        body->addLayout(top);
        body->addWidget(label((left.host.isEmpty() ? tr("this computer") : left.host) + tr(" · still running"), c.muted, 11));
        h->addLayout(body, 1);
        auto *stop = button(tr("Stop…"), "teamLeftoverStop", false);
        stop->setProperty("danger", true);
        // N7: this row names a session, not the action. The session's own stop
        // dialog names what really happens (stop and archive, terminate without
        // an archive, or forget an OpenCode binding) once its details load, so
        // the Team tab promises nothing here.
        stop->setToolTip(tr("Asks the session's own stop confirmation"));
        QObject::connect(stop, &QPushButton::clicked, this, [this, host = left.host, name = left.session] {
            if (stopSession) stopSession(host, name);
        });
        h->addWidget(stop);
        auto *take = button(tr("Take into a new team"), "teamLeftoverTake");
        take->setToolTip(tr("Start a new team with this session as a member"));
        QObject::connect(take, &QPushButton::clicked, this, [this, host = left.host, name = left.session] {
            if (takeIntoTeam) takeIntoTeam(host, name);
        });
        h->addWidget(take);
        v->addWidget(line);
    }
    return box;
}

QWidget *TeamPanel::header(const Team &team, const QString &status, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c);
    box->setObjectName("teamCard");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(7);
    auto *top = row(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.fg).pixmap(16, 16)); top->addWidget(icon);
    auto *name = elided(team.name, c.fg, 14); QFont f = name->font(); f.setWeight(QFont::DemiBold); name->setFont(f);
    name->setObjectName("teamName");
    top->addWidget(name, 1);
    auto *badge = new Pill(statusLabel(status), toneColor(c, statusTone(status)), status, false);
    badge->setObjectName("teamStatusBadge");
    top->addWidget(badge);
    auto *expand = new QToolButton; expand->setObjectName("teamOpenWorkspace"); expand->setIcon(workspaceIcon("external", c.fg)); expand->setAutoRaise(true);
    expand->setToolTip(tr("Open the team workspace"));
    QObject::connect(expand, &QToolButton::clicked, this, [this] { if (openWorkspace) openWorkspace(); });
    top->addWidget(expand);
    auto *more = new QToolButton; more->setObjectName("teamMore"); more->setText(QStringLiteral("⋯")); more->setAutoRaise(true);
    more->setPopupMode(QToolButton::InstantPopup); more->setStyleSheet(QStringLiteral("QToolButton::menu-indicator{image:none;width:0;}"));
    auto *menu = new QMenu(more);
    menu->addAction(tr("Copy team ID"), this, [id = team.id] { QGuiApplication::clipboard()->setText(id); });
    if (QStringList{"running", "paused", "planning"}.contains(team.status))
        menu->addAction(tr("Stop…"), this, [this, team] {
            const auto impact = subtreeImpact(team, m_controller->teams());
            QString text = tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                              "Member sessions keep running; their current turns finish.").arg(team.name);
            if (impact.first > 0) text += tr(" It stops %1 sub-teams and affects %2 member sessions in them.").arg(impact.first).arg(impact.second);
            if (confirm("teamStopConfirm", tr("Stop team?"), text, tr("Stop team")))
                act("stop", {});
        });
    holdWhileOpen(menu);
    more->setMenu(menu);
    top->addWidget(more);
    v->addLayout(top);
    // G6: the header shows the same two bars, not one over every task.
    v->addWidget(new PhaseProgressBars(c, phaseProgress(team)));
    QMap<Tone, int> counts;
    for (const auto &task : team.tasks) ++counts[taskStatus(task).tone];
    QStringList legend;
    legend << tr("%1/%2 accepted").arg(accepted(team)).arg(team.tasks.size());
    if (counts.value(Tone::Active) + counts.value(Tone::Assigned)) legend << tr("Active %1").arg(counts.value(Tone::Active) + counts.value(Tone::Assigned));
    if (counts.value(Tone::Danger) + counts.value(Tone::Warn)) legend << tr("Problems %1").arg(counts.value(Tone::Danger) + counts.value(Tone::Warn));
    int pending = 0; for (const auto &task : team.tasks) pending += task.status == "pending";
    if (pending) legend << tr("Pending %1").arg(pending);
    auto *legendLabel = label(legend.join(QStringLiteral("  ·  ")), c.text3, 11, false, "teamLegend");
    legendLabel->setWordWrap(true);
    v->addWidget(legendLabel);
    QStringList meta;
    if (team.created > 0) meta << tr("%1 elapsed").arg(duration(QDateTime::currentSecsSinceEpoch() - team.created));
    if (!team.project.isEmpty()) meta << team.project;
    else if (!team.folder.isEmpty()) meta << team.folder;
    if (!meta.isEmpty()) v->addWidget(elided(meta.join(QStringLiteral(" · ")), c.muted, 11));
    if (const auto next = nextLine(team); !next.isEmpty()) {
        auto *nextLabel = elided(next, c.text2, 12); nextLabel->setObjectName("teamNext");
        v->addWidget(nextLabel);
    }
    auto *actions = row(6);
    if (team.status == "running") {
        auto *pause = button(tr("Pause"), "teamPause");
        pause->setToolTip(tr("Running turns finish; nothing new is handed out"));
        QObject::connect(pause, &QPushButton::clicked, this, [this, team] {
            const auto impact = subtreeImpact(team, m_controller->teams());
            if (impact.first > 0 && !confirm("teamPauseTreeConfirm", tr("Pause the team tree?"),
                    tr("Pause %1? It also pauses %2 sub-teams and %3 member sessions in them.").arg(team.name).arg(impact.first).arg(impact.second),
                    tr("Pause")))
                return;
            act("pause", {});
        });
        actions->addWidget(pause);
    } else if (team.status == "paused") {
        auto *resume = button(tr("Resume"), "teamResume", true);
        QObject::connect(resume, &QPushButton::clicked, this, [this] { act("resume", {}); });
        actions->addWidget(resume);
    }
    if (team.status == "running" || team.status == "paused") {
        auto *stop = button(tr("Stop…"), "teamStop");
        QObject::connect(stop, &QPushButton::clicked, this, [this, team] {
            const auto impact = subtreeImpact(team, m_controller->teams());
            QString text = tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                              "Member sessions keep running; their current turns finish.").arg(team.name);
            if (impact.first > 0) text += tr(" It stops %1 sub-teams and affects %2 member sessions in them.").arg(impact.first).arg(impact.second);
            if (confirm("teamStopConfirm", tr("Stop team?"), text, tr("Stop team")))
                act("stop", {});
        });
        actions->addWidget(stop);
    }
    if (team.status == "done" || team.status == "stopped") {
        auto *archive = button(tr("Archive"), "teamArchive");
        QObject::connect(archive, &QPushButton::clicked, this, [this, name = team.name] {
            if (confirm("teamArchiveConfirm", tr("Archive team?"), tr("Archive %1? Its history and feed are kept; it leaves the session list.").arg(name), tr("Archive")))
                act("archive", {});
        });
        actions->addWidget(archive);
    }
    actions->addStretch();
    if (actions->count() > 1) v->addLayout(actions); else delete actions;
    if (team.status == "paused") {
        auto *paused = label(tr("Paused — running turns finish, nothing new is handed out"), c.muted, 11);
        paused->setWordWrap(true);
        v->addWidget(paused);
    }
    Q_UNUSED(narrow);
    return box;
}

QWidget *TeamPanel::planning(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget; w->setObjectName("teamPlanning");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    auto *banner = card(c, alpha(c.accent, .6));
    banner->setStyleSheet(banner->styleSheet() + QStringLiteral("QFrame#teamCard{background:%1;}").arg(alpha(c.accent, .08).name(QColor::HexArgb)));
    auto *b = new QVBoxLayout(banner); b->setContentsMargins(10, 8, 10, 8); b->setSpacing(3);
    b->addWidget(label(tr("Plan from Lead — review before running"), c.fg, 12, true));
    auto *hint = label(tr("Edit members and tasks here; drag a node's handle onto another task to add a dependency, click a dependency "
                          "to remove it. Members are created only after Approve & Run."), c.text2, 12);
    hint->setWordWrap(true); b->addWidget(hint);
    v->addWidget(banner);
    if (!team.errors.isEmpty()) {
        auto *errors = card(c, c.danger);
        errors->setObjectName("teamPlanErrorsBox");
        auto *e = new QVBoxLayout(errors); e->setContentsMargins(10, 8, 10, 8); e->setSpacing(3);
        e->addWidget(new StatusMark(tr("%n plan error(s)", nullptr, int(team.errors.size())), c.fg, "stuck"));
        auto *count = label(tr("%n plan error(s)", nullptr, int(team.errors.size())), c.danger, 12, true, "teamPlanErrors");
        count->hide(); e->addWidget(count);
        for (const auto &error : team.errors) { auto *l = label(QStringLiteral("• ") + error, c.danger, 12); l->setWordWrap(true); e->addWidget(l); }
        v->addWidget(errors);
    }
    for (const auto &warning : team.warnings) { auto *l = label(QStringLiteral("⚠ ") + warning, c.warn, 12); l->setWordWrap(true); v->addWidget(l); }
    // C7: the plan-health numbers the lead reads before approval and the owner
    // sees here, with the members starving on the plan now.
    if (team.health.known) {
        auto *health = card(c, c.warn);
        health->setObjectName("teamPlanHealth");
        auto *hv = new QVBoxLayout(health); hv->setContentsMargins(10, 8, 10, 8); hv->setSpacing(3);
        auto *numbers = label(tr("%1 tasks · critical path %2 · parallelism %3 · widest ready %4")
                                  .arg(team.health.tasks).arg(team.health.criticalPath).arg(team.health.parallelism, 0, 'f', 1).arg(team.health.widestReady),
                              c.fg, 12, true, "teamPlanHealthNumbers");
        numbers->setWordWrap(true);
        hv->addWidget(numbers);
        for (const auto &member : team.health.perMember)
            if (member.second.second > 0)
                hv->addWidget(label(tr("%1: %2 tasks, %3 on the critical path").arg(member.first).arg(member.second.first).arg(member.second.second), c.text2, 11));
        for (const auto &fanIn : team.health.fanIn) {
            QStringList waits; for (int id : fanIn.waitsFor) waits << QStringLiteral("#%1").arg(id);
            hv->addWidget(label(tr("#%1 waits for %2 · %3").arg(fanIn.task).arg(waits.join(QStringLiteral(", ")), fanIn.kind), c.text2, 11));
        }
        // L6: a starved member is named once, whatever the length of the list,
        // and its warning is not repeated as a separate health warning.
        QStringList starvedShown;
        for (const auto &starved : team.health.starved) {
            const QString who = starved.member.isEmpty() ? starved.role : starved.member;
            if (starvedShown.contains(who)) continue;
            starvedShown << who;
            QStringList blocking; for (int id : starved.blocking) blocking << QStringLiteral("#%1").arg(id);
            auto *line = label(tr("%1 has no ready task for %2 min · blocked by %3").arg(starved.role).arg(starved.minutes).arg(blocking.join(QStringLiteral(", "))),
                               c.warn, 11, true, "teamPlanStarved");
            line->setWordWrap(true);
            hv->addWidget(line);
        }
        for (const auto &warning : team.health.warnings) {
            bool repeatsStarved = false;
            for (const auto &who : starvedShown)
                if (!who.isEmpty() && warning.contains(who, Qt::CaseInsensitive)) { repeatsStarved = true; break; }
            if (repeatsStarved) continue;
            auto *line = label(QStringLiteral("⚠ ") + warning, c.warn, 11, false, "teamPlanHealthWarning");
            line->setWordWrap(true);
            hv->addWidget(line);
        }
        v->addWidget(health);
    }
    // F13: the lead approved on the owner's word; the quote stays visible.
    if (!team.ownerWord.isEmpty()) {
        auto *word = label(tr("Approved by Lead on your word: “%1”").arg(team.ownerWord), c.accent, 11, false, "teamOwnerWord");
        word->setWordWrap(true);
        v->addWidget(word);
    }
    return w;
}

QWidget *TeamPanel::stuck(const Team &team, const QList<Problem> &problems)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c, c.warn);
    box->setObjectName("teamStuck");
    box->setStyleSheet(box->styleSheet() + QStringLiteral("QFrame#teamStuck{background:%1;}").arg(alpha(c.warn, .1).name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(5);
    auto *title = row(6);
    auto *mark = new StatusMark(tr("Team stuck — nothing can start"), c.fg, "stuck");
    title->addWidget(mark, 1); v->addLayout(title);
    int open = 0; for (const auto &task : team.tasks) open += !task.finished() && task.status != "draft";
    v->addWidget(label(problems.isEmpty() ? tr("%n open task(s) wait; none can start.", nullptr, open)
                                          : tr("%1 open tasks wait on %2 problems:").arg(open).arg(problems.size()), c.text2, 12));
    for (const auto &problem : problems) {
        auto *link = button(QStringLiteral("• ") + problem.text, "teamStuckProblem");
        link->setFlat(true); link->setStyleSheet(QStringLiteral("text-align:left;padding:0;color:%1;").arg(c.accent.name()));
        QObject::connect(link, &QPushButton::clicked, this, [this, task = problem.task, member = problem.member, t = team] {
            if (task) openTask(task);
            else if (const auto *m = t.member(member); m && openSession) openSession(m->host, m->session);
        });
        v->addWidget(link);
    }
    auto *blockers = button(m_showBlockers ? tr("Hide blockers") : tr("Show what blocks"), "teamShowBlockers");
    QObject::connect(blockers, &QPushButton::clicked, this, [this] { m_showBlockers = !m_showBlockers; scheduleRender(); });
    auto *r = row(); r->addWidget(blockers); r->addStretch(); v->addLayout(r);
    return box;
}

QWidget *TeamPanel::answers(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamAnswers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    int n = 0;
    auto *items = new QVBoxLayout; items->setSpacing(6);
    for (const auto &member : team.members) {
        const auto state = m_live ? m_live(member) : Live{};
        if (member.session.isEmpty() || !state.attention) continue;
        ++n;
        auto *line = card(c);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(8, 5, 6, 5); h->setSpacing(6);
        h->addWidget(new StatusMark(QString(), c.warn, "answer", 16));
        if (width() >= 400) h->addWidget(new Pill(team.roleOf(member.id), roleColor(team.roleOf(member.id), m_dark)));
        h->addWidget(elided(state.attentionText.isEmpty() ? tr("Question in %1").arg(member.session) : state.attentionText, c.fg, 12), 1);
        auto *go = button(tr("Go to question"), "teamGoToQuestion");
        QObject::connect(go, &QPushButton::clicked, this, [this, host = member.host, session = member.session] { if (openSession) openSession(host, session); });
        h->addWidget(go);
        items->addWidget(line);
    }
    // A3: a question asked with `hgs team ask` waits here too; the answer field
    // is in its task card.
    for (const auto &task : team.tasks) {
        for (const auto &asked : openQuestions(task)) {
            ++n;
            auto *line = card(c);
            auto *h = new QHBoxLayout(line); h->setContentsMargins(8, 5, 6, 5); h->setSpacing(6);
            h->addWidget(new StatusMark(QString(), c.warn, "asking", 16));
            if (width() >= 400) h->addWidget(new Pill(team.roleOf(task.assignee), roleColor(team.roleOf(task.assignee), m_dark)));
            h->addWidget(elided(QStringLiteral("#%1 %2 — %3").arg(task.id).arg(task.title, asked.text), c.fg, 12), 1);
            auto *answer = button(tr("Answer"), "teamGoToTaskAnswer");
            QObject::connect(answer, &QPushButton::clicked, this, [this, id = task.id] { openTask(id); });
            h->addWidget(answer);
            items->addWidget(line);
        }
    }
    auto *title = row(6);
    title->addWidget(label(tr("Needs your answer"), c.fg, 12, true));
    title->addWidget(new Pill(QString::number(n), c.warn));
    title->addStretch();
    v->addLayout(title); v->addLayout(items);
    return box;
}

QWidget *TeamPanel::members(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamMembers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Members"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.members.size()), c.muted, 12));
    title->addStretch();
    v->addLayout(title);
    for (const auto &member : team.members) v->addWidget(memberRow(team, member, narrow));
    return box;
}

QWidget *TeamPanel::memberRow(const Team &team, const Member &member, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    const auto state = m_live ? m_live(member) : Live{};
    const auto status = memberStatus(team, member, state);
    const Task *openTask_ = status.task ? team.task(status.task) : nullptr;
    // A1: the attempt's phase decides the icon and its tooltip.
    const auto mark = openTask_ ? phaseMark(team, *openTask_) : PhaseMark{};
    const QColor tone = mark.tone != Tone::Neutral ? toneColor(c, mark.tone) : toneColor(c, status.tone);
    const QString markKind = mark.kind.isEmpty() ? status.kind : mark.kind;
    auto *box = card(c, status.problem ? alpha(tone, .8) : QColor());
    box->setObjectName("teamCard");
    box->setProperty("teamMember", member.id);
    box->setAccessibleName(tr("%1, %2").arg(team.roleOf(member.id), status.text));
    if (member.session.isEmpty() && team.status == "planning")
        box->setStyleSheet(QStringLiteral("QFrame#teamCard{background:transparent;border:1px dashed %1;border-radius:9px;}").arg(c.border.name()));
    auto *outer = new QVBoxLayout(box); outer->setContentsMargins(8, 7, 6, 7); outer->setSpacing(4);
    auto *h = row(8);
    auto *tile = new QLabel; tile->setFixedSize(28, 28); tile->setAlignment(Qt::AlignCenter);
    tile->setStyleSheet(QStringLiteral("background:%1;border-radius:7px;").arg(c.tile.name()));
    tile->setPixmap(agentLogo(member.agent, c.fg).pixmap(16, 16));
    tile->setToolTip(member.session.isEmpty() ? member.agent : member.agent + QStringLiteral(" · ") + member.session);
    h->addWidget(tile, 0, Qt::AlignTop);
    auto *body = new QVBoxLayout; body->setSpacing(3);
    auto *line1 = row(6);
    const QString role = team.roleOf(member.id);
    line1->addWidget(new Pill(member.lead && member.role != role ? role + QStringLiteral(" · ") + member.role : role, roleColor(role, m_dark), member.lead ? "lead" : QString()));
    if (state.attention) line1->addWidget(new Pill(QStringLiteral("?1"), c.warn));
    line1->addWidget(elided(member.model.isEmpty() ? member.agent : member.model, c.text3, 12), 1);
    if (!narrow) line1->addWidget(new StatusMark(status.text, tone, markKind, 112, Qt::AlignRight, nullptr, mark.tooltip));
    body->addLayout(line1);
    if (narrow) body->addWidget(new StatusMark(status.text, tone, markKind, 0, Qt::AlignLeft, nullptr, mark.tooltip));
    auto *line2 = row(8);
    const Task *current = team.task(status.task);
    QString doing = current ? QStringLiteral("#%1 %2").arg(current->id).arg(current->title) : tr("No task");
    if (current && current->last() && current->last()->n > 1) doing += tr(" · att %1").arg(current->last()->n);
    auto *doingLabel = elided(doing, current ? c.text2 : c.muted, 11);
    line2->addWidget(doingLabel, 1);
    line2->addWidget(label(member.computer.isEmpty() ? tr("this computer") : member.computer, c.muted, 11));
    if (!narrow) {
        // Task chips: this member's tasks, colored by status.
        int shown = 0;
        for (const auto &task : team.tasks) {
            if (task.assignee != member.id || shown >= 4) continue;
            ++shown;
            auto *chip = new QPushButton(QStringLiteral("#%1").arg(task.id));
            chip->setObjectName("teamTaskChip"); chip->setFlat(true); chip->setCursor(Qt::PointingHandCursor);
            const QColor chipColor = toneColor(c, taskStatus(task).tone);
            chip->setStyleSheet(QStringLiteral("QPushButton{font:10px;color:%1;border:1px solid %2;border-radius:4px;padding:0 4px;min-height:16px;}")
                                    .arg(chipColor.name(), alpha(chipColor, task.id == status.task ? 1 : .4).name(QColor::HexArgb)));
            chip->setToolTip(QStringLiteral("#%1 %2 · %3").arg(task.id).arg(task.title, taskStatus(task).text));
            QObject::connect(chip, &QPushButton::clicked, this, [this, id = task.id] { openTask(id); });
            line2->addWidget(chip);
        }
    }
    body->addLayout(line2);
    // #756: its build through the build broker, short.
    if (const auto build = team.buildStates.value(member.id); !build.state.isEmpty()) {
        auto *buildLabel = label(build.text(double(QDateTime::currentSecsSinceEpoch())), build.building() ? c.accent : c.text3, 11, false, "teamMemberBuild");
        buildLabel->setProperty("buildState", build.state);
        buildLabel->setAccessibleName(tr("Build: %1").arg(buildLabel->text()));
        body->addWidget(buildLabel);
    }
    // A5: the member's queue, the current task first.
    if (auto *queue = memberQueueWidget(team, member, c, [this](int id) { openTask(id); })) body->addWidget(queue);
    h->addLayout(body, 1);
    auto *more = new QToolButton; more->setObjectName("teamMemberMore"); more->setText(QStringLiteral("⋯")); more->setAutoRaise(true);
    more->setPopupMode(QToolButton::InstantPopup); more->setAccessibleName(tr("Member actions"));
    more->setStyleSheet(QStringLiteral("QToolButton::menu-indicator{image:none;width:0;}"));
    auto *menu = new QMenu(more);
    auto *open = menu->addAction(tr("Open session"), this, [this, host = member.host, session = member.session] { if (openSession) openSession(host, session); });
    open->setEnabled(!member.session.isEmpty());
    if (!member.lead) {
        if (member.state == "paused")
            menu->addAction(tr("Resume member"), this, [this, id = member.id] { act("resume-member", {{"member", id}}); });
        else if (team.status != "planning")
            menu->addAction(tr("Pause member"), this, [this, id = member.id] { act("pause-member", {{"member", id}}); });
        auto *replace = menu->addAction(tr("Replace agent / model…"));
        replace->setEnabled(false); replace->setToolTip(tr("Comes with the plan editor"));
        menu->addAction(tr("Remove from team…"), this, [this, id = member.id, role] {
            if (confirm("teamRemoveMemberConfirm", tr("Remove member?"), tr("Remove %1 from the team? Its session keeps running. "
                                                                            "Reassign or cancel its open tasks first.").arg(role), tr("Remove")))
                act("remove-member", {{"member", id}});
        });
    }
    holdWhileOpen(menu);
    more->setMenu(menu);
    h->addWidget(more, 0, Qt::AlignTop);
    outer->addLayout(h);
    if (status.problem) outer->addWidget(problemActions(team, member, status));
    return box;
}

QWidget *TeamPanel::problemActions(const Team &team, const Member &member, const Status &status)
{
    auto *box = new QWidget; box->setObjectName("teamProblemActions");
    auto *h = new QHBoxLayout(box); h->setContentsMargins(36, 0, 0, 0); h->setSpacing(6);
    const Task *task = team.task(status.task);
    const auto *attempt = task ? task->last() : nullptr;
    const auto add = [&](const QString &text, const char *name, bool primary, const std::function<void()> &run) {
        auto *b = button(text, name, primary); QObject::connect(b, &QPushButton::clicked, this, run); h->addWidget(b); return b;
    };
    const QString host = member.host, session = member.session;
    if (status.kind == "unconfirmed" && task && attempt) {
        add(tr("Check session"), "teamCheckSession", true, [this, host, session] { if (openSession) openSession(host, session); });
        auto *arrived = add(tr("It arrived"), "teamCheckDelivered", false, [this, id = task->id] { act("check", {{"task", id}, {"outcome", "delivered"}}); });
        arrived->setToolTip(tr("The session shows the task: mark it delivered"));
        add(tr("Send again"), "teamSendAgain", false, [this, id = task->id, n = attempt->n, role = team.roleOf(member.id)] {
            if (confirm("teamSendAgainConfirm", tr("Send the task again?"),
                        tr("Attempt %1 may have reached %2. Send the task again as a new attempt? The old attempt is revoked; "
                           "a late report for it is refused.").arg(n).arg(role), tr("Send again")))
                act("retry", {{"task", id}});
        });
        auto *cancel = add(tr("Cancel attempt"), "teamCancelAttempt", false, [this, id = task->id] { act("check", {{"task", id}, {"outcome", "not_delivered"}}); });
        cancel->setToolTip(tr("Nothing arrived: the task returns to Pending"));
    } else if (status.kind == "no_report" && task && attempt) {
        add(tr("Remind"), "teamRemind", true, [this, t = team, member, id = task->id] { remind(t, member, id); });
        add(tr("Use last reply as report"), "teamUseLastReply", false, [this, id = task->id, attemptId = attempt->id] {
            act("use-last-reply", {{"task", id}, {"attempt", attemptId}});
        });
        add(tr("Reassign…"), "teamReassign", false, [this, t = team, id = task->id] { reassignMenu(t, id, QCursor::pos()); });
    } else if (status.kind == "offline") {
        if (task) add(tr("Reassign…"), "teamReassign", true, [this, t = team, id = task->id] { reassignMenu(t, id, QCursor::pos()); });
        auto *wait = add(tr("Wait"), "teamWait", false, [] {});
        wait->setToolTip(tr("Open attempts are not revoked; the member continues when its computer answers"));
    } else if (status.kind == "asking") {
        // A3: the answer field is in the task card.
        add(tr("Answer…"), "teamAnswerQuestion", true, [this, id = task ? task->id : 0] { if (id) openTask(id); });
    } else if (status.kind == "answer") {
        add(tr("Go to question"), "teamGoToQuestion", true, [this, host, session] { if (openSession) openSession(host, session); });
    } else if (status.kind == "failed") {
        auto *again = add(tr("Start again"), "teamRelaunch", true, [this, id = member.id] { act("relaunch", {{"member", id}}); });
        again->setToolTip(member.error);
        for (const auto &t : team.tasks)
            if (t.assignee == member.id && !t.finished()) {
                add(tr("Reassign…"), "teamReassign", false, [this, tm = team, id = t.id] { reassignMenu(tm, id, QCursor::pos()); });
                break;
            }
    }
    h->addStretch();
    return box;
}

void TeamPanel::reassignMenu(const Team &team, int taskId, const QPoint &global)
{
    const auto *task = team.task(taskId);
    if (!task) return;
    QMenu menu(this);
    menu.setObjectName("teamReassignMenu");
    for (const auto &member : team.members) {
        if (member.id == task->assignee) continue;
        const auto status = memberStatus(team, member, m_live ? m_live(member) : Live{});
        auto *action = menu.addAction(QStringLiteral("%1 · %2 — %3").arg(team.roleOf(member.id), member.model.isEmpty() ? member.agent : member.model, status.text));
        QObject::connect(action, &QAction::triggered, this, [this, team, taskId, id = member.id] {
            const auto choice = subteamChoice(this, team, taskId, tr("Reassign"));
            if (choice.isEmpty()) return;
            QJsonObject input{{"task", taskId}, {"member", id}};
            const auto *delegated = team.task(taskId);
            if (delegated && delegated->subteam.known) input["subteam"] = choice;
            act("reassign", input);
        });
    }
    if (menu.isEmpty()) menu.addAction(tr("No other member"))->setEnabled(false);
    RenderHold hold(this);
    menu.exec(global);
}

bool TeamPanel::phaseOpen(const Team &team, const PhaseGroup &group) const
{
    const auto teamIt = m_phaseOpen.constFind(team.id);
    if (teamIt != m_phaseOpen.constEnd()) {
        const auto it = teamIt->constFind(group.n);
        if (it != teamIt->constEnd()) return it.value();
    }
    // A finished phase starts collapsed, the rest open; the last phase stays
    // open, so a finished one-phase plan still shows its tasks.
    if (!group.finished()) return true;
    for (const auto &task : team.tasks) if (task.phase > group.n) return false;
    return true;
}

void TeamPanel::togglePhase(const Team &team, int phase)
{
    PhaseGroup found;
    bool known = false;
    for (const auto &group : phaseGroups(team)) if (group.n == phase) { found = group; known = true; }
    if (!known) return;
    const bool open = phaseOpen(team, found);
    // Bounded: only the teams met in this session, the least recently met
    // dropped, in the order they were first opened here.
    if (!m_phaseOpen.contains(team.id)) {
        m_phaseOrder.removeAll(team.id);
        m_phaseOrder.append(team.id);
        while (m_phaseOrder.size() > 16) m_phaseOpen.remove(m_phaseOrder.takeFirst());
    }
    m_phaseOpen[team.id][phase] = !open;
}

QWidget *TeamPanel::tasks(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamTasks");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Tasks"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.tasks.size()), c.muted, 12));
    title->addStretch();
    // U58: `team` may be a copy without its cancelled tasks; every edit reads
    // the whole team again by id.
    const auto whole = [this, id = team.id]() -> const Team * {
        const auto *current = m_controller->watched();
        return current && current->id == id ? current : m_controller->team(id);
    };
    const int cancelled = [&] { const auto *all = whole(); return all ? cancelledTasks(*all) : 0; }();
    if (cancelled) {
        auto *show = button(m_controller->showCancelled() ? tr("Hide cancelled (%1)").arg(cancelled) : tr("Show cancelled (%1)").arg(cancelled), "teamShowCancelled");
        show->setCheckable(true); show->setChecked(m_controller->showCancelled());
        QObject::connect(show, &QPushButton::clicked, this, [this](bool on) { m_controller->setShowCancelled(on); });
        title->addWidget(show);
    }
    const bool planningNow = team.status == "planning";
    if (planningNow) {
        auto *add = button(tr("+ Task"), "teamAddTask");
        QObject::connect(add, &QPushButton::clicked, this, [this, whole] { if (const auto *t = whole()) editTask(*t, 0); });
        title->addWidget(add);
    }
    for (const auto &[key, text] : QList<std::pair<QString, QString>>{{"all", tr("All")}, {"active", tr("Active")}, {"problems", tr("Problems")}}) {
        if (planningNow) break;
        auto *b = button(text, "teamTaskFilter");
        b->setCheckable(true); b->setChecked(m_filter == key); b->setProperty("filter", key);
        QObject::connect(b, &QPushButton::clicked, this, [this, key = key] { m_filter = key; scheduleRender(); });
        title->addWidget(b);
    }
    v->addLayout(title);
    // Problem tasks: their own state, or a problem of the member working on them.
    QSet<int> problemTasks;
    QMap<int, Tone> problemTone;
    for (const auto &p : problems(team, m_live ? m_live : LiveLookup([](const Member &) { return Live{}; })))
        if (p.task) { problemTasks.insert(p.task); if (problemTone.value(p.task, Tone::Warn) != Tone::Danger) problemTone[p.task] = p.tone; }
    for (const auto &task : team.tasks) if (const auto status = taskStatus(task); status.problem) { problemTasks.insert(task.id); problemTone[task.id] = status.tone; }
    auto *view = new TaskView(c, !narrow);
    view->setObjectName(narrow ? "teamTaskList" : "teamTaskGraph");
    view->zoom = m_zoom;
    view->chain = [team](int id) { return Teams::chain(team, id); };
    view->cycle = team.cycle;
    view->bands = phaseBands(team, [this, &team](const PhaseGroup &group) { return phaseOpen(team, group); });
    const bool grouped = !view->bands.isEmpty();
    view->phaseToggled = [this, id = team.id](int phase) {
        // The team is re-read by id: a poll may have replaced the one rendered.
        const auto *current = m_controller->watched();
        if (!current || current->id != id) current = m_controller->team(id);
        if (current) togglePhase(*current, phase);
        scheduleRender();
    };
    // Edges exist for every task: a collapsed phase's edges attach to its
    // summary node.
    for (const auto &task : team.tasks) {
        for (const auto &dep : task.deps) {
            view->edges[task.id].append(dep.task);
            if (dep.soft && !team.allHard) view->soft.insert({dep.task, task.id});
        }
    }
    QMap<int, int> localLayer;
    if (grouped) for (const auto &band : view->bands) {
        if (band.collapsed) continue;
        const auto layers = phaseLayers(team, band.tasks);
        for (auto it = layers.cbegin(); it != layers.cend(); ++it) localLayer.insert(it.key(), it.value());
    }
    QList<int> order;
    if (grouped) order = phaseOrder(team, view->bands);
    else {
        const auto byLayer = layered(team);
        for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) order += it.value();
    }
    for (int id : order) {
        const auto *task = team.task(id);
        const auto status = taskStatus(*task);
        const QString role = team.assigneeRole(*task, planningNow ? tr("No assignee") : tr("You"));
        TaskView::Node node;
        node.id = id; node.phase = task->phase; node.layer = grouped ? localLayer.value(id) : team.layers.value(id);
        node.title = task->title; node.role = role;
        node.problem = problemTasks.contains(id);
        node.tone = toneColor(c, node.problem ? problemTone.value(id, Tone::Danger) : status.tone);
        node.kind = kindOfTask(*task, status);
        node.statusText = status.text; node.roleTone = roleColor(role, m_dark);
        node.draft = task->status == "draft";
        node.error = team.cycle.contains(id) || (task->assignee.isEmpty() ? planningNow : !team.member(task->assignee));
        node.attempt = task->last() ? task->last()->n : 0;
        QStringList deps;
        for (const auto &dep : task->deps) deps << QStringLiteral("#%1").arg(dep.task);
        node.deps = deps.isEmpty() ? QString() : QStringLiteral("← ") + deps.join(' ');
        const bool active = QStringList{"assigned", "in_progress", "submitted", "in_review", "in_test"}.contains(task->status);
        node.match = m_filter == "all" || (m_filter == "active" && active) || (m_filter == "problems" && node.problem);
        view->nodes.append(node);
    }
    if (m_showBlockers) for (int id : problemTasks) view->emphasis += Teams::chain(team, id);
    if (planningNow) {
        view->editable = !narrow;
        view->clicked = [this, whole](int id, const QPoint &) { if (const auto *t = whole()) editTask(*t, id); };
        view->linked = [this, whole](int dependency, int task) { if (const auto *t = whole()) link(*t, dependency, task, true); };
        view->unlinked = [this, whole](int dependency, int task) { if (const auto *t = whole()) link(*t, dependency, task, false); };
    } else view->clicked = [this](int id, const QPoint &) { openTask(id); };
    view->layout();
    if (narrow) v->addWidget(view);
    else {
        auto *tools = row(4);
        const auto zoomTo = [this](double zoom) { m_zoom = qBound(0.5, std::round(zoom * 10) / 10, 1.5); scheduleRender(); };
        auto *out = button(QStringLiteral("−"), "teamZoomOut"); QObject::connect(out, &QPushButton::clicked, this, [=] { zoomTo(m_zoom - .1); });
        auto *level = label(QStringLiteral("%1%").arg(int(std::round(m_zoom * 100))), c.text3, 11, false, "teamZoomLevel");
        auto *in = button(QStringLiteral("+"), "teamZoomIn"); QObject::connect(in, &QPushButton::clicked, this, [=] { zoomTo(m_zoom + .1); });
        auto *fit = button(tr("Fit"), "teamZoomFit");
        out->setFixedWidth(28); in->setFixedWidth(28); fit->setFixedWidth(44);
        tools->addWidget(out); tools->addWidget(level); tools->addWidget(in); tools->addWidget(fit);
        // The hint gives way first: nothing may be wider than the panel.
        auto *hint = elided(tr("Hover = chain · click = actions"), c.muted, 11);
        hint->setAlignment(Qt::AlignRight | Qt::AlignVCenter); hint->setMinimumWidth(0); hint->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        tools->addWidget(hint, 1);
        v->addLayout(tools);
        auto *area = new QScrollArea; area->setObjectName("teamGraphArea");
        area->setWidget(view); area->setWidgetResizable(false); area->setFrameShape(QFrame::NoFrame);
        area->setMaximumHeight(430);
        area->setMinimumHeight(qMin(430, view->height() + 4));
        area->setStyleSheet(QStringLiteral("QScrollArea#teamGraphArea{background:%1;border:1px solid %2;border-radius:9px;}")
                                .arg(c.surface.name(), c.border.name()));
        area->viewport()->setAutoFillBackground(true);
        QPalette surface = area->viewport()->palette(); surface.setColor(QPalette::Window, c.surface); area->viewport()->setPalette(surface);
        QObject::connect(fit, &QPushButton::clicked, this, [=] {
            const double natural = view->width() / m_zoom;
            zoomTo(natural > 0 ? qMin(1.5, (area->viewport()->width() - 4) / natural) : 1.0);
        });
        v->addWidget(area);
        auto *legend = label(tr("L = layer: tasks that can run at the same time"), c.muted, 10);
        v->addWidget(legend);
    }
    return box;
}

QWidget *TeamPanel::feed(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamFeed");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(2);
    auto *title = row(6);
    title->addWidget(label(tr("Feed"), c.fg, 12, true));
    title->addStretch();
    auto *filter = new QComboBox; filter->setObjectName("teamFeedFilter");
    filter->addItem(tr("All members"), QString());
    for (const auto &member : team.members) filter->addItem(team.roleOf(member.id), team.roleOf(member.id));
    filter->setCurrentIndex(qMax(0, filter->findData(m_feedMember)));
    QObject::connect(filter, &QComboBox::currentIndexChanged, this, [this, filter] { m_feedMember = filter->currentData().toString(); scheduleRender(); });
    title->addWidget(filter);
    v->addLayout(title);
    int shown = 0;
    for (const auto &event : m_controller->feed()) {
        const auto line = feedLine(team, event);
        if (!m_feedMember.isEmpty() && line.by != m_feedMember) continue;
        ++shown;
        auto *r = new QWidget; r->setObjectName("teamFeedRow");
        auto *h = new QHBoxLayout(r); h->setContentsMargins(0, 3, 0, 3); h->setSpacing(6);
        auto *time = label(clock(line.at), c.dim, 11); time->setFont(NewSessionDense::mono(11)); time->setFixedWidth(38);
        time->setToolTip(QDateTime::fromSecsSinceEpoch(qint64(line.at)).toString(Qt::ISODate));
        h->addWidget(time, 0, Qt::AlignTop);
        if (!line.by.isEmpty()) h->addWidget(new Pill(line.by, line.by == tr("You") ? c.text3 : roleColor(line.by, m_dark)), 0, Qt::AlignTop);
        auto *text = label(line.text, line.tone == Tone::Neutral ? c.text2 : toneColor(c, line.tone), 12);
        text->setWordWrap(true);
        h->addWidget(text, 1);
        v->addWidget(r);
    }
    if (!shown) v->addWidget(label(tr("Nothing yet"), c.muted, 12));
    if (m_controller->feedHasOlder()) {
        auto *older = button(tr("Load older"), "teamFeedOlder");
        QObject::connect(older, &QPushButton::clicked, this, [this] { m_controller->loadOlder(); });
        auto *r = row(); r->addWidget(older); r->addStretch(); v->addLayout(r);
    } else if (shown) v->addWidget(label(tr("Beginning of team history"), c.muted, 11));
    return box;
}
