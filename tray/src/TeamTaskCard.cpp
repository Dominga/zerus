#include "TeamTaskCard.h"
#include "TeamWidgets.h"
#include <QCheckBox>
#include <QCursor>
#include <QGraphicsOpacityEffect>
#include <QLineEdit>
#include <QTimer>
#include <QMenu>
#include <QPlainTextEdit>
#include <QScrollArea>
#include <QScrollBar>
#include <QToolButton>
#include <QVBoxLayout>

using namespace TeamUi;

TeamTaskCard::TeamTaskCard(QWidget *parent) : QWidget(parent)
{
    setObjectName("teamTaskCard");
    m_layout = new QVBoxLayout(this);
    m_layout->setContentsMargins(0, 0, 0, 0);
    m_layout->setSpacing(0);
    m_scroll = new QScrollArea;
    m_scroll->setObjectName("teamTaskCardScroll");
    m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_layout->addWidget(m_scroll, 1);
    m_footer = new QWidget;
    m_footer->setObjectName("teamTaskCardFooter");
    m_layout->addWidget(m_footer);
}

void TeamTaskCard::setTask(const Team &team, int task, const LiveLookup &live, bool dark)
{
    if (task != m_task) m_form.clear();
    m_team = team; m_task = task; m_live = live; m_dark = dark;
    rebuild();
}

void TeamTaskCard::rebuild()
{
    const Colors c = colorsFor(m_dark);
    const int scroll = m_scroll->verticalScrollBar()->value();
    const Task *task = m_team.task(m_task);
    auto *content = new QWidget;
    content->setObjectName("teamTaskCardContent");
    auto *v = new QVBoxLayout(content);
    const bool narrow = width() < 400;
    v->setContentsMargins(narrow ? 8 : 12, 10, narrow ? 8 : 12, 12);
    v->setSpacing(10);
    delete m_footer->layout();
    // A footer button may be the one being clicked: it goes once its signal returns.
    for (auto *child : m_footer->findChildren<QWidget *>(Qt::FindDirectChildrenOnly)) retire(child);
    if (!task) {
        v->addWidget(label(tr("This task is no longer in the plan."), c.muted, 12));
        v->addStretch();
        retire(m_scroll->takeWidget());
    m_scroll->setWidget(content);
        return;
    }
    const auto lookup = [&](const Member &m) { return m_live ? m_live(m) : Live{}; };
    const Member *assignee = m_team.member(task->assignee);
    const auto status = taskStatus(*task);
    const auto memberState = assignee ? memberStatus(m_team, *assignee, lookup(*assignee)) : Status{};
    const bool problem = status.problem || (memberState.problem && memberState.task == task->id);
    const QColor tone = toneColor(c, problem && !status.problem ? memberState.tone : status.tone);
    // Header: back · #id · title; status · attempt · role.
    auto *head = row(6);
    auto *backButton = new QToolButton; backButton->setObjectName("teamTaskBack"); backButton->setAutoRaise(true);
    backButton->setIcon(workspaceIcon("chevron-left", c.fg)); backButton->setToolTip(tr("Back to the team"));
    QObject::connect(backButton, &QToolButton::clicked, this, [this] { if (back) back(); });
    head->addWidget(backButton);
    auto *idLabel = label(QStringLiteral("#%1").arg(task->id), c.dim, 13); idLabel->setFont(NewSessionDense::mono(13));
    head->addWidget(idLabel);
    auto *title = elided(task->title, c.fg, 14); QFont tf = title->font(); tf.setWeight(QFont::DemiBold); title->setFont(tf);
    title->setObjectName("teamTaskTitle");
    head->addWidget(title, 1);
    v->addLayout(head);
    auto *meta = row(8);
    const QString statusText = problem && !status.problem ? memberState.text : status.shortText;
    // A1: the attempt's phase icon, with the tooltip that names the state, the
    // time and the next step (spec 11).
    const auto phase = phaseMark(m_team, *task);
    auto *state = new StatusMark(statusText, tone, problem && !status.problem ? memberState.kind : kindOfTask(*task, status),
                                 0, Qt::AlignLeft, nullptr, phase.tooltip);
    state->setObjectName("teamTaskState");
    meta->addWidget(state);
    if (task->last()) meta->addWidget(label(tr("Attempt %1").arg(task->last()->n), c.muted, 11));
    const QString role = task->assignee.isEmpty() ? tr("You") : m_team.roleOf(task->assignee);
    meta->addWidget(new Pill(role, roleColor(role, m_dark)));
    meta->addStretch();
    v->addLayout(meta);
    // The problem and what can be done.
    if (problem && assignee) {
        auto *banner = card(c, problem && tone == c.warn ? c.warn : c.danger);
        banner->setObjectName("teamTaskProblem");
        banner->setStyleSheet(banner->styleSheet() + QStringLiteral("QFrame#teamTaskProblem{background:%1;}").arg(alpha(tone, .08).name(QColor::HexArgb)));
        auto *b = new QVBoxLayout(banner); b->setContentsMargins(10, 8, 10, 8); b->setSpacing(4);
        const QString kind = status.problem ? (status.tone == Tone::Danger ? QStringLiteral("unconfirmed") : QStringLiteral("no_report")) : memberState.kind;
        b->addWidget(new StatusMark(status.problem ? status.text : memberState.text, c.fg, kind));
        QString explain;
        const auto *attempt = task->last();
        const QString where = assignee->computer.isEmpty() ? tr("this computer") : assignee->computer;
        if (kind == "unconfirmed" && attempt)
            explain = tr("Zerus sent attempt %1 to %2 on %3 at %4 and got no confirmation. It is never re-sent automatically.")
                          .arg(attempt->n).arg(assignee->agent, where, clock(attempt->sentAt));
        else if (kind == "no_report") explain = tr("The turn ended without hgs team report.");
        else if (kind == "asking") explain = tr("The attempt stays open until you answer; nothing is captured as its report meanwhile.");
        else if (kind == "offline") explain = tr("%1 does not answer. Open attempts are not revoked automatically.").arg(where);
        else if (kind == "answer") explain = tr("A question or approval waits in the member's session.");
        else if (kind == "failed") explain = assignee->error;
        if (attempt && !attempt->error.isEmpty() && kind == "unconfirmed") explain += QStringLiteral(" (") + attempt->error + QStringLiteral(")");
        auto *text = label(explain, c.text2, 12); text->setWordWrap(true); b->addWidget(text);
        if (problemActions) {
            Status forTask = problem && !status.problem ? memberState : memberStatus(m_team, *assignee, lookup(*assignee));
            if (status.problem) { forTask.kind = kind; forTask.task = task->id; forTask.problem = true; }
            if (auto *actions = problemActions(*assignee, forTask)) { actions->layout()->setContentsMargins(0, 2, 0, 0); b->addWidget(actions); }
        }
        v->addWidget(banner);
    }
    const auto section = [&](const QString &name) { v->addWidget(label(name, c.text3, 12, true)); };
    // A3: open questions to the lead. The attempt stays open until one is
    // answered, and the answer goes out once as `reply`.
    const auto questions = openQuestions(*task);
    if (!questions.isEmpty()) {
        section(tr("Questions"));
        for (const auto &asked : questions) {
            auto *box = card(c, c.warn);
            box->setObjectName("teamTaskQuestion");
            box->setProperty("question", asked.id);
            auto *b = new QVBoxLayout(box); b->setContentsMargins(10, 7, 10, 7); b->setSpacing(5);
            auto *text = label(asked.text, c.fg, 12); text->setWordWrap(true); b->addWidget(text);
            auto *answerRow = row(6);
            auto *edit = new QLineEdit; edit->setObjectName("teamAnswerText"); edit->setProperty("question", asked.id);
            edit->setPlaceholderText(tr("Answer %1…").arg(m_team.roleOf(task->assignee)));
            answerRow->addWidget(edit, 1);
            auto *send = button(tr("Send"), "teamAnswerSend", true);
            send->setProperty("question", asked.id);
            send->setToolTip(tr("Send this answer to %1; it reaches the member as a message").arg(m_team.roleOf(task->assignee)));
            QObject::connect(send, &QPushButton::clicked, this, [this, edit, id = asked.id] {
                const auto answer = edit->text().trimmed();
                if (answer.isEmpty()) { edit->setFocus(); return; }
                if (act) act("reply", {{"question", id}, {"text", answer}});
            });
            QObject::connect(edit, &QLineEdit::returnPressed, send, &QPushButton::click);
            answerRow->addWidget(send);
            b->addLayout(answerRow);
            v->addWidget(box);
        }
    }
    if (!task->description.isEmpty()) {
        section(tr("Description"));
        auto *d = label(task->description, c.fg, 12); d->setWordWrap(true); d->setTextInteractionFlags(Qt::TextSelectableByMouse);
        d->setObjectName("teamTaskDescription");
        v->addWidget(d);
    }
    if (!task->criteria.isEmpty()) {
        section(tr("Acceptance criteria"));
        for (const auto &criterion : task->criteria) {
            auto *box = new QCheckBox(criterion);
            box->setChecked(task->status == "accepted"); box->setEnabled(false);
            box->setObjectName("teamTaskCriterion");
            v->addWidget(box);
        }
    }
    // Depends on / Blocks / Assignee as a two-column form.
    const auto linkRow = [&](const QString &name, const QList<int> &ids, const QSet<int> &softIds) {
        if (ids.isEmpty()) return;
        auto *r = row(8);
        auto *caption = label(name, c.muted, 11); caption->setFixedWidth(84); r->addWidget(caption, 0, Qt::AlignTop);
        auto *links = new QVBoxLayout; links->setSpacing(3);
        for (int other : ids) {
            const auto *t = m_team.task(other);
            if (!t) continue;
            const auto s = taskStatus(*t);
            auto *link = new QPushButton(QStringLiteral("#%1 %2%3").arg(t->id).arg(t->title, softIds.contains(other) ? tr(" (soft)") : QString()));
            link->setObjectName("teamTaskLink"); link->setFlat(true); link->setCursor(Qt::PointingHandCursor);
            link->setToolTip(s.text);
            link->setStyleSheet(QStringLiteral("QPushButton{text-align:left;padding:2px 6px;border:1px solid %1;border-radius:6px;color:%2;}")
                                    .arg(c.border.name(), toneColor(c, s.tone).name()));
            QObject::connect(link, &QPushButton::clicked, this, [this, other] { if (openTask) openTask(other); });
            links->addWidget(link);
        }
        r->addLayout(links, 1);
        v->addLayout(r);
    };
    QList<int> deps, blocks; QSet<int> soft;
    for (const auto &d : task->deps) { deps << d.task; if (d.soft && !m_team.allHard) soft << d.task; }
    for (const auto &t : m_team.tasks) for (const auto &d : t.deps) if (d.task == task->id) blocks << t.id;
    linkRow(tr("Depends on"), deps, soft);
    linkRow(tr("Blocks"), blocks, {});
    if (assignee) {
        auto *r = row(6);
        auto *caption = label(tr("Assignee"), c.muted, 11); caption->setFixedWidth(84); r->addWidget(caption);
        r->addWidget(new Pill(role, roleColor(role, m_dark)));
        if (task->pinned) {
            auto *pin = new QLabel; pin->setObjectName("teamTaskPinned");
            pin->setPixmap(workspaceIcon("pinned", c.text2).pixmap(15, 15));
            pin->setToolTip(tr("Pinned: stays with %1").arg(role));
            pin->setAccessibleName(pin->toolTip());
            r->addWidget(pin);
        }
        auto *icon = new QLabel; icon->setPixmap(agentLogo(assignee->agent, c.fg).pixmap(14, 14)); r->addWidget(icon);
        r->addWidget(elided((assignee->model.isEmpty() ? assignee->agent : assignee->model) + QStringLiteral(" · ")
                                + (assignee->computer.isEmpty() ? tr("this computer") : assignee->computer), c.text2, 12), 1);
        v->addLayout(r);
    }
    // Attempts, newest first; revoked and rejected ones fade.
    if (!task->attempts.isEmpty()) {
        section(tr("Attempts"));
        for (int i = task->attempts.size() - 1; i >= 0; --i) {
            const auto &a = task->attempts[i];
            auto *box = card(c);
            box->setObjectName("teamTaskAttempt");
            auto *b = new QVBoxLayout(box); b->setContentsMargins(10, 7, 10, 7); b->setSpacing(3);
            auto *top = row(6);
            top->addWidget(label(tr("Attempt %1").arg(a.n), c.fg, 12, true));
            const QString who = m_team.roleOf(a.member);
            top->addWidget(new Pill(who, roleColor(who, m_dark)));
            if (a.sentAt > 0) top->addWidget(label(clock(a.sentAt), c.muted, 11));
            static const QMap<QString, std::pair<const char *, Tone>> states{
                {"delivering", {"Sending", Tone::Assigned}}, {"delivered", {"In progress", Tone::Active}},
                {"unconfirmed", {"Delivery unconfirmed", Tone::Danger}}, {"refused", {"Not delivered", Tone::Warn}},
                {"revoked", {"Revoked", Tone::Dim}}, {"reported", {"Reported", Tone::Done}}, {"rejected", {"Rejected", Tone::Danger}}};
            const auto st = states.value(a.state, {"", Tone::Neutral});
            top->addWidget(label(a.noReport ? tr("No report") : tr(st.first), toneColor(c, a.noReport ? Tone::Warn : st.second), 11));
            top->addStretch();
            const Member *m = m_team.member(a.member);
            if (m && !m->session.isEmpty()) {
                auto *open = new QPushButton(tr("Open in Activity"));
                open->setObjectName("teamTaskOpenActivity"); open->setFlat(true); open->setCursor(Qt::PointingHandCursor);
                open->setStyleSheet(QStringLiteral("QPushButton{color:%1;border:none;padding:0;}").arg(c.accent.name()));
                QObject::connect(open, &QPushButton::clicked, this, [this, host = m->host, session = m->session] { if (openSession) openSession(host, session); });
                top->addWidget(open);
            }
            b->addLayout(top);
            QString detail = a.error;
            if (a.state == "rejected" && !task->reason.isEmpty() && i == task->attempts.size() - 1) detail = task->reason;
            if (a.state == "rejected" && detail.isEmpty()) detail = tr("Rejected by the reviewer");
            if (!detail.isEmpty()) { auto *d = label(detail, c.text2, 12); d->setWordWrap(true); b->addWidget(d); }
            if (a.state == "revoked" || a.state == "rejected") {
                auto *effect = new QGraphicsOpacityEffect(box); effect->setOpacity(.7); box->setGraphicsEffect(effect);
            }
            v->addWidget(box);
        }
    }
    // The newest report.
    for (int i = task->attempts.size() - 1; i >= 0; --i) {
        const auto &a = task->attempts[i];
        if (a.report.isEmpty()) continue;
        section(tr("Report"));
        auto *box = new QFrame; box->setObjectName("teamTaskReport");
        box->setStyleSheet(QStringLiteral("QFrame#teamTaskReport{background:%1;border-left:3px solid %2;border-radius:6px;}")
                               .arg(c.card.name(), c.accentMuted.name()));
        auto *b = new QVBoxLayout(box); b->setContentsMargins(10, 8, 10, 8);
        auto *text = label(a.report, c.fg, 12); text->setWordWrap(true); text->setTextInteractionFlags(Qt::TextSelectableByMouse);
        text->setObjectName("teamReportText");
        b->addWidget(text);
        // A2: the reply Zerus captured while the member may still be working.
        if (a.captured) {
            auto *captured = label(tr("Captured from the member's last reply; it can still send the real report."), c.warn, 11, false, "teamReportCaptured");
            captured->setWordWrap(true);
            b->addWidget(captured);
        }
        // A6: the real report replaced the captured one; the old text stays,
        // folded.
        if (!a.replaced.isEmpty()) {
            auto *replaced = new QWidget; replaced->setObjectName("teamReportReplaced");
            auto *rv = new QVBoxLayout(replaced); rv->setContentsMargins(0, 6, 0, 0); rv->setSpacing(3);
            auto *head = row(6);
            head->addWidget(label(tr("Replaced the captured reply"), c.text3, 11, true));
            auto *toggle = new QToolButton; toggle->setObjectName("teamReportReplacedToggle");
            toggle->setText(tr("Show")); toggle->setCheckable(true); toggle->setAutoRaise(true);
            toggle->setToolTip(tr("Show the reply Zerus captured before the report"));
            head->addWidget(toggle); head->addStretch();
            rv->addLayout(head);
            auto *old = label(a.replaced.last().text, c.muted, 11, false, "teamReportReplacedText");
            old->setWordWrap(true); old->setTextInteractionFlags(Qt::TextSelectableByMouse);
            old->setVisible(false);
            QObject::connect(toggle, &QToolButton::toggled, old, [old, toggle](bool on) {
                old->setVisible(on);
                toggle->setText(on ? TeamTaskCard::tr("Hide") : TeamTaskCard::tr("Show"));
            });
            rv->addWidget(old);
            b->addWidget(replaced);
        }
        v->addWidget(box);
        break;
    }
    if (!task->reason.isEmpty() && task->status == "accepted") {
        auto *reason = label(tr("Decision: %1").arg(task->reason), c.muted, 11); reason->setWordWrap(true); v->addWidget(reason);
    }
    // Inline forms: reject, hold or a manual done.
    if (!m_form.isEmpty()) {
        const bool reject = m_form == "reject", hold = m_form == "hold";
        auto *box = card(c, reject ? c.danger : hold ? c.muted : c.accent);
        box->setObjectName(reject ? "teamRejectForm" : hold ? "teamHoldForm" : "teamDoneForm");
        auto *b = new QVBoxLayout(box); b->setContentsMargins(10, 8, 10, 8); b->setSpacing(6);
        b->addWidget(label(reject ? tr("Reject with comment") : hold ? tr("Hold task") : tr("Mark done manually"), c.fg, 12, true));
        b->addWidget(label(reject ? tr("The comment goes to the member with the next attempt.")
                                 : hold ? tr("The task stays with its member and is not handed out until you release it.")
                                        : tr("The task counts as accepted without a report; the reason stays in the feed."), c.muted, 11));
        auto *edit = new QPlainTextEdit; edit->setObjectName("teamTaskFormText"); edit->setFixedHeight(66);
        edit->setPlaceholderText(reject ? tr("What must change?") : hold ? tr("Why does it wait?") : tr("Why is it done?"));
        b->addWidget(edit);
        // A5: a rejection may return the task with a hold instead of handing it
        // straight back.
        QCheckBox *returnLater = nullptr;
        if (reject) {
            returnLater = new QCheckBox(tr("Return later (hold): keep it with the member until released"));
            returnLater->setObjectName("teamRejectHold");
            b->addWidget(returnLater);
        }
        auto *buttons = row(6);
        auto *ok = button(reject ? tr("Reject && send back") : hold ? tr("Hold") : tr("Mark done"), "teamTaskFormSubmit", true);
        auto *cancel = button(tr("Cancel"), "teamTaskFormCancel");
        QObject::connect(cancel, &QPushButton::clicked, this, [this] { m_form.clear(); rebuild(); });
        QObject::connect(ok, &QPushButton::clicked, this, [this, edit, returnLater, id = task->id] {
            const auto reason = edit->toPlainText().trimmed();
            if (reason.isEmpty()) { edit->setFocus(); return; }
            if (act) {
                if (m_form == "reject") {
                    QJsonObject input{{"task", id}, {"reason", reason}};
                    if (returnLater && returnLater->isChecked()) input.insert("hold", true);
                    act("reject", input);
                } else if (m_form == "hold") act("hold", {{"task", id}, {"reason", reason}});
                else act("accept", {{"task", id}, {"reason", reason}, {"manual", true}});
            }
            m_form.clear(); rebuild();
        });
        buttons->addWidget(ok); buttons->addWidget(cancel); buttons->addStretch();
        b->addLayout(buttons);
        v->addWidget(box);
        QTimer::singleShot(0, edit, [edit] { edit->setFocus(); });
    }
    v->addStretch();
    retire(m_scroll->takeWidget());
    m_scroll->setWidget(content);
    m_scroll->verticalScrollBar()->setValue(scroll);
    // Footer: actions by status; the rest under More, disabled with a reason.
    auto *footer = new QHBoxLayout(m_footer);
    footer->setContentsMargins(narrow ? 8 : 12, 8, narrow ? 8 : 12, 10);
    footer->setSpacing(6);
    m_footer->setStyleSheet(QStringLiteral("QWidget#teamTaskCardFooter{border-top:1px solid %1;}").arg(c.border.name()));
    const bool decided = task->finished();
    const bool open = !decided && task->status != "draft";
    const bool submitted = task->status == "submitted";
    const auto add = [&](const QString &text, const char *name, bool primary, const std::function<void()> &run) {
        auto *b = button(text, name, primary); QObject::connect(b, &QPushButton::clicked, this, run); footer->addWidget(b); return b;
    };
    const int id = task->id;
    const auto accept = [this, id] { if (act) act("accept", {{"task", id}}); };
    const auto rejectForm = [this] { m_form = "reject"; rebuild(); };
    const auto reassignNow = [this, id] { if (reassign) reassign(id, QCursor::pos()); };
    const auto takeOver = [this, id] {
        if (!confirm || confirm("teamTakeOverConfirm", tr("Take over task?"), tr("You do this task yourself; its current attempt is revoked and no member gets it."), tr("Take over")))
            if (act) act("take-over", {{"task", id}});
    };
    const auto cancelTask = [this, id] {
        if (!confirm || confirm("teamCancelTaskConfirm", tr("Cancel task?"), tr("Cancel this task? Tasks that depend on it hard never become ready."), tr("Cancel task")))
            if (act) act("cancel", {{"task", id}});
    };
    const auto retry = [this, id, n = task->last() ? task->last()->n : 0] {
        if (n == 0 || !confirm || confirm("teamRetryConfirm", tr("Retry task?"), tr("Revoke attempt %1 and hand the task out again?").arg(n), tr("Retry")))
            if (act) act("retry", {{"task", id}});
    };
    const auto holdForm = [this] { m_form = "hold"; rebuild(); };
    const auto releaseNow = [this, id] { if (act) act("release", {{"task", id}}); };
    if (submitted) {
        add(tr("Accept"), "teamTaskAccept", true, accept);
        add(tr("Reject with comment…"), "teamTaskReject", false, rejectForm);
        add(tr("Reassign…"), "teamTaskReassign", false, reassignNow);
    } else if (open) {
        // A5: a held task waits for this button; a pending one can be parked.
        if (task->held) add(tr("Release"), "teamTaskRelease", true, releaseNow);
        else if (task->status == "pending") add(tr("Hold…"), "teamTaskHold", false, holdForm);
        add(tr("Reassign…"), "teamTaskReassign", false, reassignNow);
        add(tr("Take over"), "teamTaskTakeOver", false, takeOver);
        auto *cancel = add(tr("Cancel"), "teamTaskCancel", false, cancelTask);
        cancel->setStyleSheet(QStringLiteral("color:%1;").arg(c.danger.name()));
    }
    footer->addStretch();
    auto *more = new QToolButton; more->setObjectName("teamTaskMore"); more->setText(tr("More"));
    more->setPopupMode(QToolButton::InstantPopup);
    auto *menu = new QMenu(more); menu->setToolTipsVisible(true); menu->setObjectName("teamTaskMoreMenu");
    const auto item = [&](const QString &text, bool enabled, const QString &why, const std::function<void()> &run) {
        auto *a = menu->addAction(text, this, run);
        a->setEnabled(enabled);
        if (!enabled) a->setToolTip(why);
    };
    const QString decidedWhy = tr("Task #%1 is already decided").arg(id);
    item(tr("Retry"), open, decided ? decidedWhy : tr("Nothing to retry yet"), retry);
    // A5: park and release are here too, with the reason they do not apply.
    item(tr("Hold…"), !decided && task->status == "pending" && !task->held,
         task->held ? tr("Already held") : decided ? decidedWhy : tr("Only a pending task can be held"), holdForm);
    item(tr("Release"), task->held, task->held ? QString() : tr("Not held"), releaseNow);
    if (!submitted) {
        item(tr("Accept"), false, decided ? decidedWhy : tr("Nothing to accept yet: no report"), accept);
        item(tr("Reject with comment…"), false, decided ? decidedWhy : tr("Nothing to reject yet: no report"), rejectForm);
    }
    if (submitted || decided) {
        item(tr("Reassign…"), open, decidedWhy, reassignNow);
        item(tr("Take over"), open, decidedWhy, takeOver);
        item(tr("Cancel task…"), open, decidedWhy, cancelTask);
    }
    item(tr("Mark done manually…"), !decided && task->status != "draft", decided ? decidedWhy : tr("Approve the plan first"),
         [this] { m_form = "done"; rebuild(); });
    more->setMenu(menu);
    footer->addWidget(more);
}
