// WhiteWh fork, U59: free planning in the team workspace. While a plan waits
// for approval: the lead's proposed mode with its one-line reason and the
// owner's choice (no choice keeps standard). While the team runs: the switch
// to free as its own dialog bound to the request's id and content hash; the
// mode beside each phase; the open free phase, which reads idle (never done)
// when its queue is empty, folds its accepted tasks, adds a build task on
// request and closes only on the owner's confirmation.
#include "TeamWorkspace.h"
#include "TeamController.h"
#include "TeamPanel.h"
#include "TeamPhaseScroll.h"
#include "TeamWidgets.h"
#include "WorkspaceIcons.h"
#include <QCheckBox>
#include <QDateTime>
#include <QDialog>
#include <QGridLayout>
#include <QLineEdit>
#include <QPointer>
#include <QRadioButton>
#include <QTimer>
#include <QVBoxLayout>

using namespace Teams;
using namespace TeamUi;

namespace {
QString timeText(double at)
{
    const auto time = QDateTime::fromSecsSinceEpoch(qint64(at));
    return time.toString(time.date() == QDate::currentDate() ? QStringLiteral("HH:mm") : QStringLiteral("d MMM HH:mm"));
}

QString phaseName(const Team &team, int phase)
{
    const auto *record = team.phase(phase);
    return record && !record->title.isEmpty() ? QObject::tr("Phase %1: %2").arg(phase).arg(record->title) : QObject::tr("Phase %1").arg(phase);
}

// A line of the switch dialog's "what changes": its icon and text.
QWidget *changeLine(const Colors &c, const QString &kind, const QString &text, bool workspace = false)
{
    auto *line = new QWidget; line->setObjectName("teamSwitchModeChange");
    auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(10);
    auto *icon = new QLabel;
    icon->setPixmap(workspace ? workspaceIcon(kind, c.text2).pixmap(12, 12) : glyphPixmap(c.violet, kind, 12));
    icon->setFixedWidth(14);
    h->addWidget(icon, 0, Qt::AlignTop);
    auto *words = label(text, c.fg, 12); words->setWordWrap(true); words->setTextFormat(Qt::RichText);
    h->addWidget(words, 1);
    return line;
}
}

bool TeamWorkspace::freeOpenPhase(const Team &team, int phase)
{
    return team.freeMode() && team.freePhase.known && team.freePhase.phase == phase
        && (team.status == QLatin1String("running") || team.status == QLatin1String("paused"));
}

int TeamWorkspace::openBuildTask(const Team &team)
{
    for (const auto &task : team.tasks)
        if (task.phaseBuild && task.phase == team.freePhase.phase && !task.finished()) return task.id;
    return 0;
}

QString TeamWorkspace::closeBlocked(const Team &team)
{
    if (!team.freeMode() || !team.freePhase.known) return tr("The team is not planning in free mode");
    const int n = team.freePhase.phase;
    if (team.freePhase.closePending > 0) {
        const int build = openBuildTask(team);
        return build ? tr("Phase %1 closes when its build #%2 is accepted").arg(n).arg(build)
                     : tr("Phase %1 closes when its build is accepted").arg(n);
    }
    if (!team.freePhase.running.isEmpty()) {
        QStringList ids; for (int id : team.freePhase.running) ids << QStringLiteral("#%1").arg(id);
        return tr("Phase %1 still has %2 running; close it when they are decided").arg(n).arg(ids.join(QStringLiteral(", ")));
    }
    return {};
}

QString TeamWorkspace::freeBuildLine(const Team &team) const
{
    const auto &free = team.freePhase;
    if (free.lastBuildAt <= 0)
        return free.acceptedSince > 0 ? tr("No build yet, %n accepted", nullptr, free.acceptedSince) : tr("No build yet");
    const QString last = tr("Last build %1").arg(timeText(free.lastBuildAt));
    return free.acceptedSince > 0 ? tr("%1, %n accepted since", nullptr, free.acceptedSince).arg(last) : last;
}

QWidget *TeamWorkspace::phaseModeBadge(const Team &team, int phase)
{
    if (!team.planModeKnown) return nullptr;
    const Colors c = colorsFor(m_dark);
    QString mode = QStringLiteral("standard");
    if (team.status == "planning") {
        const QString chosen = m_panel->planModeChoice(team.id);
        if (!chosen.isEmpty()) mode = chosen;
    } else if (freeOpenPhase(team, phase)) {
        mode = QStringLiteral("free");
    }
    const bool free = mode == "free";
    auto *badge = new PhaseBadge(free ? tr("Free") : tr("Standard"), free ? c.violet : c.text3, free ? QStringLiteral("plan_free") : QStringLiteral("plan_standard"), false);
    badge->setObjectName("teamPhaseMode"); badge->setProperty("mode", mode);
    badge->setToolTip(free ? tr("Free planning: the phase stays open and takes tasks as they come until you close it")
                           : tr("Standard planning: the phase closes with its integration and build"));
    return badge;
}

QWidget *TeamWorkspace::planModeCard(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const QString offer = team.planModeOffer;
    const QString chosen = m_panel->planModeChoice(team.id);
    const QString selected = chosen.isEmpty() ? QStringLiteral("standard") : chosen;
    auto *box = new QFrame; box->setObjectName("teamPlanModeCard"); box->setProperty("selected", selected);
    box->setStyleSheet(QStringLiteral("QFrame#teamPlanModeCard{background:%1;border:1px solid %2;border-radius:10px;}")
                           .arg(c.cardFill.name(), c.cardBorder.name()));
    auto *v = new QVBoxLayout(box); v->setContentsMargins(12, 9, 12, 10); v->setSpacing(7);
    auto *head = row(7);
    auto *icon = new QLabel; icon->setPixmap(glyphPixmap(c.text3, "plan_standard", 11)); head->addWidget(icon);
    head->addWidget(label(tr("PLAN MODE"), c.text3, 10, true));
    head->addWidget(label(tr("How this goal's work is phased. Chosen once at the start; with no choice, Standard."), c.muted, 11), 1);
    v->addLayout(head);
    if (!offer.isEmpty()) {
        const QString name = offer == "free" ? tr("Free") : tr("Standard");
        auto *proposal = label(tr("<b>Lead proposes %1:</b> “%2”").arg(name, team.planModeReason.toHtmlEscaped()), c.fg, 12, false, "teamPlanModeOffer");
        proposal->setTextFormat(Qt::RichText); proposal->setWordWrap(true);
        proposal->setProperty("mode", offer);
        proposal->setStyleSheet(QStringLiteral("QLabel{color:%1;background:%2;border:1px solid %3;border-radius:8px;padding:6px 10px;}")
                                    .arg(c.fg.name(), alpha(c.violet, .10).name(QColor::HexArgb), alpha(c.violet, .40).name(QColor::HexArgb)));
        v->addWidget(proposal);
    }
    auto *options = row(10);
    struct Option { QString mode, name, text, fits; };
    const QList<Option> list{
        {QStringLiteral("standard"), tr("Standard"),
         tr("Phases are planned, approved and run. A phase closes when its tasks are accepted, with its integration and a build; "
            "the next phase is planned and approved again."),
         tr("Fits a known scope with a release.")},
        {QStringLiteral("free"), tr("Free"),
         tr("One open phase. Tasks are added at any time and run as they are added, with the same pipelines and acceptance. "
            "Integration and a build run only when you or the lead ask. It never closes by itself; you close it."),
         tr("Fits a stream of small requests, support or polishing.")}};
    for (const auto &option : list) {
        const bool on = option.mode == selected;
        const QColor tone = option.mode == "free" ? c.violet : c.fg;
        auto *frame = new ClickFrame; frame->setObjectName("teamPlanModeOption");
        frame->setProperty("mode", option.mode); frame->setProperty("selected", on);
        frame->setCursor(Qt::PointingHandCursor);
        frame->setStyleSheet(QStringLiteral("QFrame#teamPlanModeOption{background:%1;border:1px solid %2;border-radius:9px;}")
                                 .arg(on ? alpha(tone, .07).name(QColor::HexArgb) : QStringLiteral("transparent"),
                                      on ? alpha(tone, .55).name(QColor::HexArgb) : c.border.name()));
        auto *fh = new QHBoxLayout(frame); fh->setContentsMargins(10, 8, 12, 9); fh->setSpacing(8);
        auto *radio = new QRadioButton; radio->setObjectName("teamPlanModeRadio"); radio->setProperty("mode", option.mode);
        radio->setChecked(on); radio->setAutoExclusive(false); radio->setAccessibleName(option.name);
        radio->setStyleSheet(QStringLiteral("QRadioButton{background:transparent;} QRadioButton::indicator{width:12px;height:12px;border-radius:7px;"
                                            "border:1px solid %1;background:transparent;} QRadioButton::indicator:checked{width:6px;height:6px;border:4px solid %2;background:%3;}")
                                 .arg(c.muted.name(), tone.name(), c.bg.name()));
        fh->addWidget(radio, 0, Qt::AlignTop);
        auto *fv = new QVBoxLayout; fv->setSpacing(4);
        auto *title = row(6);
        auto *glyphLabel = new QLabel; glyphLabel->setPixmap(glyphPixmap(tone, option.mode == "free" ? "plan_free" : "plan_standard", 12));
        title->addWidget(glyphLabel);
        title->addWidget(label(option.name, tone, 13, true, "teamPlanModeName"));
        QStringList tags;
        if (option.mode == "standard") tags << tr("default");
        if (option.mode == offer) tags << tr("Lead proposes");
        for (const auto &tag : tags) {
            auto *pill = label(tag, tag == tr("default") ? c.muted : c.violet, 11, false, "teamPlanModeTag");
            pill->setStyleSheet(QStringLiteral("QLabel{color:%1;background:%2;border-radius:5px;padding:1px 6px;}")
                                    .arg((tag == tr("default") ? c.muted : c.violet).name(), alpha(tag == tr("default") ? c.muted : c.violet, .14).name(QColor::HexArgb)));
            title->addWidget(pill);
        }
        title->addStretch();
        fv->addLayout(title);
        auto *text = label(option.text, c.text2, 12); text->setWordWrap(true); fv->addWidget(text);
        auto *fits = label(option.fits, c.muted, 11); fits->setWordWrap(true); fv->addWidget(fits);
        fh->addLayout(fv, 1);
        const auto choose = [this, id = team.id, mode = option.mode] { m_panel->choosePlanMode(id, mode); scheduleRender(); };
        frame->clicked = choose;
        QObject::connect(radio, &QRadioButton::clicked, this, choose);
        options->addWidget(frame, 1);
    }
    v->addLayout(options);
    return box;
}

QWidget *TeamWorkspace::freeAcceptedFold(const Team &team, int phase, const QList<int> &accepted)
{
    const Colors c = colorsFor(m_dark);
    const QString key = QStringLiteral("%1/%2").arg(team.id).arg(phase);
    const bool unfolded = m_freeUnfolded.contains(key);
    auto *line = new QWidget; line->setObjectName("teamFreeAccepted"); line->setProperty("phase", phase);
    line->setProperty("tasks", accepted.size()); line->setProperty("folded", !unfolded);
    auto *h = new QHBoxLayout(line); h->setContentsMargins(42, 0, 12, 6); h->setSpacing(8);
    auto *mark = new QLabel; mark->setPixmap(glyphPixmap(c.accent, "accepted", 12)); h->addWidget(mark);
    h->addWidget(label(tr("%n accepted", nullptr, int(accepted.size())), c.fg, 12, false, "teamFreeAcceptedCount"));
    QStringList ids; for (int id : accepted) ids << QStringLiteral("#%1").arg(id);
    QString shown = ids.mid(0, 12).join(QLatin1Char(' '));
    if (ids.size() > 12) shown += tr(" and %1 more").arg(ids.size() - 12);
    auto *list = label(shown, c.dim, 11, false, "teamFreeAcceptedIds"); list->setToolTip(ids.join(QLatin1Char(' ')));
    h->addWidget(list);
    auto *toggle = button(unfolded ? tr("Hide") : tr("Show"), "teamFreeAcceptedShow"); toggle->setProperty("link", "accent");
    toggle->setToolTip(unfolded ? tr("Fold the accepted tasks of the open phase") : tr("Show the accepted tasks of the open phase"));
    QObject::connect(toggle, &QPushButton::clicked, this, [this, key] {
        if (!m_freeUnfolded.remove(key)) m_freeUnfolded.insert(key);
        scheduleRender();
    });
    h->addWidget(toggle);
    h->addStretch();
    return line;
}

QWidget *TeamWorkspace::freeIdleCard(const Team &team, int phase)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QFrame; box->setObjectName("teamFreeIdleCard"); box->setProperty("phase", phase);
    box->setStyleSheet(QStringLiteral("QFrame#teamFreeIdleCard{background:transparent;border:1px dashed %1;border-radius:10px;}").arg(c.cardBorder.name()));
    auto *h = new QHBoxLayout(box); h->setContentsMargins(14, 12, 14, 12); h->setSpacing(12);
    auto *tile = new QLabel; tile->setFixedSize(30, 30); tile->setAlignment(Qt::AlignCenter);
    tile->setStyleSheet(QStringLiteral("QLabel{background:%1;border-radius:8px;}").arg(c.tile.name()));
    tile->setPixmap(glyphPixmap(c.text2, "delivered", 14));
    h->addWidget(tile, 0, Qt::AlignTop);
    auto *v = new QVBoxLayout; v->setSpacing(4);
    v->addWidget(label(tr("Phase %1 is open and idle").arg(phase), c.fg, 13, true, "teamFreeIdleTitle"));
    auto *text = label(tr("A free phase does not close by itself. New work joins it as a task; the lead adds what comes in."), c.text2, 12);
    text->setWordWrap(true); v->addWidget(text);
    auto *actions = row(6);
    auto *add = button(tr("Add task"), "teamFreeIdleAdd", true); add->setIcon(workspaceIcon("add", c.accentText)); add->setIconSize(QSize(12, 12));
    QObject::connect(add, &QPushButton::clicked, this, [this, id = team.id, phase] { if (const auto *t = currentTeam(id)) addLateDialog(*t, phase); });
    actions->addWidget(add);
    auto *build = button(tr("Integrate && build"), "teamFreeIdleBuild"); build->setIcon(workspaceIcon("terminal", c.text2)); build->setIconSize(QSize(12, 12));
    const int open = openBuildTask(team);
    build->setEnabled(!open && !m_buildSent.contains(team.id));
    build->setToolTip(open ? tr("#%1 builds the phase").arg(open) : tr("Add a lead task that integrates the accepted work and runs the build"));
    QObject::connect(build, &QPushButton::clicked, this, [this, id = team.id] { if (const auto *t = currentTeam(id)) integrateBuild(*t); });
    actions->addWidget(build);
    auto *close = button(tr("Close phase…"), "teamFreeIdleClose"); close->setIcon(workspaceIcon("stop", c.text2)); close->setIconSize(QSize(12, 12));
    const QString blocked = closeBlocked(team);
    close->setEnabled(blocked.isEmpty());
    close->setToolTip(blocked.isEmpty() ? tr("Close the free phase; the team returns to standard planning") : blocked);
    QObject::connect(close, &QPushButton::clicked, this, [this, id = team.id] {
        QTimer::singleShot(0, this, [this, id] { if (const auto *t = currentTeam(id)) closePhaseDialog(*t); });
    });
    actions->addWidget(close);
    actions->addStretch();
    v->addLayout(actions);
    h->addLayout(v, 1);
    auto *outer = new QWidget; outer->setObjectName("teamFreeIdle");
    auto *ov = new QVBoxLayout(outer); ov->setContentsMargins(42, 2, 12, 8); ov->setSpacing(0);
    ov->addWidget(box);
    return outer;
}

void TeamWorkspace::integrateBuild(const Team &team)
{
    if (m_buildSent.contains(team.id) || openBuildTask(team)) return;
    m_buildSent.insert(team.id);
    m_panel->act("integrate-build", {{"team", team.id}}, [this, id = team.id](bool ok, const QString &message) {
        m_buildSent.remove(id);
        if (!ok) m_error = message;
        scheduleRender();
    });
    scheduleRender();
}

void TeamWorkspace::closePhaseDialog(const Team &team)
{
    if (auto *open = findChild<QDialog *>("teamClosePhaseDialog")) { open->raise(); open->activateWindow(); return; }
    if (!closeBlocked(team).isEmpty()) return;
    const Colors c = colorsFor(m_dark);
    const auto &free = team.freePhase;
    const int n = free.phase;
    auto *dialog = new QDialog(this);
    dialog->setObjectName("teamClosePhaseDialog"); dialog->setProperty("phase", n);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(tr("Close phase %1").arg(n));
    dialog->setModal(true);
    dialog->resize(540, 340);
    dialog->setStyleSheet(dialogSheet(c));
    auto *v = new QVBoxLayout(dialog); v->setContentsMargins(16, 16, 16, 14); v->setSpacing(10);
    auto *head = row(12);
    auto *tile = new QLabel; tile->setFixedSize(36, 36); tile->setAlignment(Qt::AlignCenter);
    tile->setStyleSheet(QStringLiteral("QLabel{background:%1;border-radius:9px;}").arg(c.tile.name()));
    tile->setPixmap(workspaceIcon("stop", c.text2).pixmap(14, 14));
    head->addWidget(tile, 0, Qt::AlignTop);
    auto *titles = new QVBoxLayout; titles->setSpacing(2);
    titles->addWidget(label(tr("Close %1?").arg(phaseName(team, n)), c.fg, 15, true, "teamClosePhaseTitle"));
    int tasks = 0; for (const auto &task : team.tasks) tasks += task.phase == n;
    titles->addWidget(label(tr("Free planning, %n tasks", nullptr, tasks), c.muted, 12));
    head->addLayout(titles, 1);
    v->addLayout(head);
    auto *facts = new QFrame; facts->setObjectName("teamClosePhaseFacts");
    facts->setStyleSheet(QStringLiteral("QFrame#teamClosePhaseFacts{background:%1;border:1px solid %2;border-radius:9px;}").arg(c.cardFill.name(), c.cardBorder.name()));
    auto *grid = new QGridLayout(facts); grid->setContentsMargins(14, 10, 14, 10); grid->setHorizontalSpacing(18); grid->setVerticalSpacing(6);
    const auto fact = [&](int at, const QString &name, const QString &value, const char *object, const QColor &tone) {
        grid->addWidget(label(name, c.muted, 12), at, 0, Qt::AlignTop);
        auto *text = label(value, tone, 12, false, object); text->setWordWrap(true);
        grid->addWidget(text, at, 1);
    };
    fact(0, tr("Tasks"), tr("%n accepted, none running or pending", nullptr, free.accepted), "teamClosePhaseTasks", c.fg);
    const int build = [&team, n] { int last = 0; for (const auto &task : team.tasks) if (task.phaseBuild && task.phase == n && task.status == "accepted") last = qMax(last, task.id); return last; }();
    QString built = free.lastBuildAt > 0 ? (build ? tr("%1, from #%2").arg(timeText(free.lastBuildAt)).arg(build) : timeText(free.lastBuildAt)) : tr("None yet");
    if (free.acceptedSince > 0) built += tr(", %n accepted since", nullptr, free.acceptedSince);
    fact(1, tr("Last build"), built, "teamClosePhaseBuild", free.acceptedSince > 0 ? c.warn : c.fg);
    fact(2, tr("Then"), tr("Standard planning. The lead plans the next phase for your approval; phase %1 keeps its history.").arg(n), "teamClosePhaseThen", c.fg);
    grid->setColumnStretch(1, 1);
    v->addWidget(facts);
    auto *integrate = new QCheckBox(tr("Integrate and build first")); integrate->setObjectName("teamClosePhaseIntegrate");
    {QFont f = integrate->font(); f.setWeight(QFont::DemiBold); integrate->setFont(f);}
    // On by default when work was accepted since the last build; with nothing
    // new the phase closes at once and no build task is added.
    integrate->setChecked(free.acceptedSince > 0);
    integrate->setEnabled(free.acceptedSince > 0);
    v->addWidget(integrate);
    auto *hint = label(free.acceptedSince > 0 ? tr("The phase closes when the build passes; a failed build keeps it open and reaches the lead.")
                                              : tr("Nothing was accepted since the last build: the phase closes at once."),
                       c.text2, 12, false, "teamClosePhaseHint");
    hint->setWordWrap(true); hint->setContentsMargins(26, 0, 0, 0);
    v->addWidget(hint);
    auto *error = label(QString(), c.danger, 12, false, "teamClosePhaseError"); error->setWordWrap(true); error->hide();
    v->addWidget(error);
    v->addStretch();
    auto *buttons = row(8);
    buttons->addStretch();
    auto *cancel = button(tr("Cancel"), "teamClosePhaseCancel");
    auto *confirm = button(QString(), "teamClosePhaseConfirm", true);
    const auto relabel = [integrate, confirm, c] {
        confirm->setText(integrate->isChecked() ? tr("Build and close") : tr("Close phase"));
        confirm->setIcon(workspaceIcon(integrate->isChecked() ? "terminal" : "stop", c.accentText)); confirm->setIconSize(QSize(12, 12));
    };
    relabel();
    QObject::connect(integrate, &QCheckBox::toggled, dialog, relabel);
    QObject::connect(cancel, &QPushButton::clicked, dialog, &QDialog::reject);
    QObject::connect(confirm, &QPushButton::clicked, dialog, [this, dialog, integrate, error, confirm, id = team.id] {
        const auto *current = currentTeam(id);
        const QString blocked = current ? closeBlocked(*current) : tr("The team is gone");
        if (!blocked.isEmpty()) { error->setText(blocked); error->show(); return; }
        confirm->setEnabled(false);
        m_panel->act("close-phase", {{"team", id}, {"integrate", integrate->isChecked()}},
                     [guard = QPointer<QDialog>(dialog), error, confirm](bool ok, const QString &message) {
                         if (!guard) return;
                         if (!ok) { error->setText(message); error->show(); confirm->setEnabled(true); return; }
                         guard->accept();
                     });
    });
    buttons->addWidget(cancel); buttons->addWidget(confirm);
    v->addLayout(buttons);
    dialog->show();
}

void TeamWorkspace::popSwitchRequest(const Team &team)
{
    // A request decided elsewhere (the lead withdrew it, another window
    // answered) closes its dialog.
    if (auto *open = findChild<QDialog *>("teamSwitchModeDialog")) {
        bool still = false;
        for (const auto &proposal : team.proposals)
            still = still || (proposal.id == open->property("proposal").toString() && proposal.open());
        if (open->property("team").toString() == team.id && !still && !open->property("sent").toBool()) open->close();
        return;
    }
    if (team.status != "running" && team.status != "paused") return;
    for (const auto &proposal : team.proposals) {
        if (!proposal.open() || proposal.kind != "switch-plan-mode") continue;
        const QString key = team.id + QLatin1Char('/') + proposal.id;
        if (m_switchShown.contains(key) || m_decidedProposals.contains(proposal.id)) continue;
        m_switchShown.insert(key);
        QTimer::singleShot(0, this, [this, id = team.id, proposal = proposal.id] {
            if (const auto *current = currentTeam(id)) switchModeDialog(*current, proposal);
        });
        return;   // one dialog at a time; the next pops when this one is answered
    }
}

void TeamWorkspace::switchModeDialog(const Team &team, const QString &proposalId)
{
    if (auto *open = findChild<QDialog *>("teamSwitchModeDialog")) { open->raise(); open->activateWindow(); return; }
    const Team::Proposal *proposal = nullptr;
    for (const auto &candidate : team.proposals) if (candidate.id == proposalId && candidate.open()) proposal = &candidate;
    if (!proposal) return;
    const Colors c = colorsFor(m_dark);
    const bool toFree = proposal->mode != "standard";
    // The phase that stays open: the running one, else the last accepted.
    int n = 0;
    for (const auto &group : phaseGroups(team)) {
        const QString state = ownerPhaseState(team, group);
        if (state == "running") { n = group.n; break; }
        if (state == "done" || state == "queued" || state == "awaiting_acceptance") n = n ? n : group.n;
    }
    QList<int> queued;
    for (const auto &group : phaseGroups(team)) if (group.n != n && ownerPhaseState(team, group) == "queued") queued << group.n;
    auto *dialog = new QDialog(this);
    dialog->setObjectName("teamSwitchModeDialog");
    dialog->setProperty("team", team.id); dialog->setProperty("proposal", proposal->id); dialog->setProperty("hash", proposal->hash);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(toFree ? tr("Switch to free planning") : tr("Switch to standard planning"));
    dialog->setModal(true);
    dialog->resize(600, 460);
    dialog->setStyleSheet(dialogSheet(c));
    auto *v = new QVBoxLayout(dialog); v->setContentsMargins(18, 18, 18, 14); v->setSpacing(10);
    auto *head = row(12);
    auto *tile = new QLabel; tile->setFixedSize(38, 38); tile->setAlignment(Qt::AlignCenter);
    tile->setStyleSheet(QStringLiteral("QLabel{background:%1;border-radius:9px;}").arg(alpha(c.violet, .16).name(QColor::HexArgb)));
    tile->setPixmap(glyphPixmap(c.violet, toFree ? "plan_free" : "plan_standard", 16));
    head->addWidget(tile, 0, Qt::AlignTop);
    auto *titles = new QVBoxLayout; titles->setSpacing(2);
    titles->addWidget(label(toFree ? tr("Switch to free planning?") : tr("Switch to standard planning?"), c.fg, 15, true, "teamSwitchModeTitle"));
    titles->addWidget(label(n ? tr("%1, %2").arg(team.name, phaseName(team, n)) : team.name, c.muted, 12));
    head->addLayout(titles, 1);
    v->addLayout(head);
    const QString who = proposal->by.isEmpty() ? tr("Lead") : proposal->by;
    const QString when = proposal->at > 0 ? tr("%1 asks, %2:").arg(who, timeText(proposal->at)) : tr("%1 asks:").arg(who);
    auto *request = label(QStringLiteral("<b>%1</b> “%2”").arg(when.toHtmlEscaped(), proposal->reason.toHtmlEscaped()), c.fg, 12, false, "teamSwitchModeRequest");
    request->setTextFormat(Qt::RichText); request->setWordWrap(true);
    request->setStyleSheet(QStringLiteral("QLabel{color:%1;background:%2;border:1px solid %3;border-radius:8px;padding:8px 12px;}")
                               .arg(c.fg.name(), alpha(c.violet, .10).name(QColor::HexArgb), alpha(c.violet, .40).name(QColor::HexArgb)));
    v->addWidget(request);
    v->addWidget(label(tr("WHAT CHANGES"), c.text3, 10, true));
    const QString open = n ? QStringLiteral("<b>%1</b>").arg(phaseName(team, n).toHtmlEscaped()) : tr("The current phase");
    if (toFree) {
        v->addWidget(changeLine(c, "plan_free", tr("%1 stays open when its tasks are accepted: idle, not done.").arg(open)));
        v->addWidget(changeLine(c, "add", tr("New tasks join it and run as they are added, with the same pipelines, reports and acceptance."), true));
        v->addWidget(changeLine(c, "terminal", tr("Integration and a build run only when you or the lead ask, never as a phase barrier."), true));
        v->addWidget(changeLine(c, "stop", tr("You close the phase to return to standard."), true));
    } else {
        v->addWidget(changeLine(c, "plan_standard", tr("New work is planned in phases again, each approved by you and closed with its integration and build.")));
    }
    QString unchanged = tr("Unchanged: running tasks and their attempts, members, pipelines");
    if (!queued.isEmpty()) {
        QStringList names; for (int q : queued) names << tr("phase %1").arg(q);
        unchanged += tr("; the queued %1 stays as it is").arg(names.join(QStringLiteral(", ")));
    }
    auto *same = label(unchanged + QLatin1Char('.'), c.muted, 11, false, "teamSwitchModeUnchanged"); same->setWordWrap(true);
    v->addWidget(same);
    auto *reason = new QLineEdit; reason->setObjectName("teamSwitchModeReason"); reason->setMaxLength(2000);
    reason->setPlaceholderText(toFree ? tr("Why keep standard? The lead receives this (optional)") : tr("Why not? The lead receives this (optional)"));
    v->addWidget(reason);
    auto *error = label(QString(), c.danger, 12, false, "teamSwitchModeError"); error->setWordWrap(true); error->hide();
    v->addWidget(error);
    v->addStretch();
    auto *foot = row(8);
    const QString hash = proposal->hash.left(8);
    auto *bound = label(hash.isEmpty() ? tr("request %1, this request only").arg(proposal->id)
                                       : tr("request %1 (%2), this request only").arg(proposal->id, hash),
                        c.muted, 11, false, "teamSwitchModeBound");
    bound->setFont(NewSessionDense::mono(11));
    bound->setToolTip(tr("Your answer applies to this request and its exact content"));
    foot->addWidget(bound);
    foot->addStretch();
    auto *keep = button(toFree ? tr("Keep standard") : tr("Keep free"), "teamSwitchModeKeep");
    auto *change = button(toFree ? tr("Switch to free") : tr("Switch to standard"), "teamSwitchModeSwitch", true);
    change->setIcon(glyphIcon(c.accentText, toFree ? "plan_free" : "plan_standard", 12));
    foot->addWidget(keep); foot->addWidget(change);
    v->addLayout(foot);
    const auto decide = [this, dialog, reason, error, keep, change, id = team.id, proposalId = proposal->id, full = proposal->hash](bool accept) {
        if (m_decidedProposals.contains(proposalId)) return;
        m_decidedProposals.insert(proposalId);
        keep->setEnabled(false); change->setEnabled(false);
        dialog->setProperty("sent", true);
        QJsonObject input{{"team", id}, {"proposal", proposalId}, {"accept", accept}, {"expected_hash", full}};
        if (const QString why = reason->text().trimmed(); !why.isEmpty()) input.insert("reason", why);
        m_panel->act("decide-proposal", input, [this, guard = QPointer<QDialog>(dialog), error, keep, change, proposalId](bool ok, const QString &message) {
            if (!ok) m_decidedProposals.remove(proposalId);
            scheduleRender();
            if (!guard) return;
            if (!ok) {
                guard->setProperty("sent", false);
                error->setText(message); error->show(); keep->setEnabled(true); change->setEnabled(true);
                return;
            }
            guard->accept();
        });
        scheduleRender();
    };
    QObject::connect(keep, &QPushButton::clicked, dialog, [decide] { decide(false); });
    QObject::connect(change, &QPushButton::clicked, dialog, [decide] { decide(true); });
    // Closing without an answer leaves the request open: the strip reopens it.
    dialog->show();
}
