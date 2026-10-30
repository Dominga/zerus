#include "TeamStages.h"
#include "NewSessionDense.h"
#include <QGridLayout>
#include <QRegularExpression>
#include <QResizeEvent>

namespace TeamUi {
namespace {
QString tr(const char *text) { return QCoreApplication::translate("TeamStages", text); }

// A chip: a rounded outline with its glyph and text, struck through when the
// stage was skipped.
class StageChipWidget : public QWidget {
public:
    StageChipWidget(const StageChip &chip, QWidget *parent = nullptr) : QWidget(parent), m_chip(chip)
    {
        QFont f = font(); f.setPixelSize(11); f.setWeight(QFont::Medium); f.setStrikeOut(chip.strike); setFont(f);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
        setToolTip(chip.tooltip.isEmpty() ? chip.text : chip.tooltip);
        setAccessibleName(chip.text);
        setObjectName("teamStageChip");
        setProperty("text", chip.text);
        setProperty("state", chip.state);
        setProperty("kind", chip.kind);
    }
    QSize sizeHint() const override { return {fontMetrics().horizontalAdvance(m_chip.text) + 26, 19}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const QRectF r = QRectF(rect()).adjusted(.5, .5, -.5, -.5);
        const bool outlined = m_chip.state == "returned" || m_chip.state == "limit" || m_chip.state == "held" || m_chip.state == "waiting"
                              || m_chip.state == "skipped" || m_chip.state == "stuck";
        p.setPen(QPen(alpha(m_chip.color, outlined ? .85 : .35), 1)); p.setBrush(alpha(m_chip.color, outlined ? .06 : .14));
        p.drawRoundedRect(r, 6, 6);
        glyph(p, QRectF(6, 4.5, 10, 10), m_chip.color, m_chip.kind);
        p.setPen(m_chip.color);
        p.drawText(QRect(20, 0, width() - 25, height()), Qt::AlignVCenter | Qt::AlignLeft,
                   fontMetrics().elidedText(m_chip.text, Qt::ElideRight, width() - 25));
    }
private:
    StageChip m_chip;
};

// The line on a card: its chips first; the track gives way when a narrow
// column has no room for both.
class StageLineBox : public QWidget {
public:
    QWidget *track = nullptr;
protected:
    void resizeEvent(QResizeEvent *event) override
    {
        QWidget::resizeEvent(event);
        if (!track) return;
        int need = 0, items = 0;
        for (auto *child : findChildren<QWidget *>(QString(), Qt::FindDirectChildrenOnly)) if (child != track) { need += child->sizeHint().width(); ++items; }
        need += track->width() + items * layout()->spacing();
        track->setVisible(need <= width());
    }
};

// The track: one short bar per segment of the task's way.
class StageTrack : public QWidget {
public:
    explicit StageTrack(const QList<QColor> &segments, QWidget *parent = nullptr) : QWidget(parent), m_segments(segments)
    {
        setObjectName("teamStageTrack"); setFixedSize(int(segments.size()) * 11, 12);
        setProperty("segments", int(segments.size()));
    }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing); p.setPen(Qt::NoPen);
        for (int i = 0; i < m_segments.size(); ++i) { p.setBrush(m_segments[i]); p.drawRoundedRect(QRectF(i * 11, 4.5, 8, 3), 1.5, 1.5); }
    }
private:
    QList<QColor> m_segments;
};

QString acceptKind(const PipelineRun &run) { return run.accept == "auto" ? QStringLiteral("auto") : run.accept == "sub_lead" ? QStringLiteral("sub_lead") : QStringLiteral("lead"); }
// U47/U48: Zerus runs a command or land stage itself, with no member.
bool runsCommand(const StageRun &stage) { return StageSpec{stage.kind}.runsCommand(); }
const StageSpec *specOf(const PipelineRun &run, const StageRun &stage)
{
    return stage.stage >= 0 && stage.stage < run.stages.size() ? &run.stages[stage.stage] : nullptr;
}
QString glyphOf(const QString &kind)
{
    if (kind == "review") return QStringLiteral("stage_review");
    if (kind == "test") return QStringLiteral("stage_test");
    if (kind == "auto") return QStringLiteral("stage_auto");
    return QStringLiteral("stage_lead");
}

// The newest run of each stage in the task's current round.
QMap<int, const StageRun *> currentRuns(const PipelineRun &run)
{
    QMap<int, const StageRun *> runs;
    for (const auto &stage : run.runs) if (stage.round == run.round) runs[stage.stage] = &stage;
    return runs;
}

int findingsCount(const PipelineRun &run)
{
    int n = 0;
    // A command's return has no finding lines: its output tail is the one finding.
    for (const auto &stage : run.runs) if (stage.state == "returned") n += runsCommand(stage) ? qMax(1, int(stage.findings.size())) : int(stage.findings.size());
    return n;
}

// QA #539: the build and test templates a stage member ran, as hgs recorded
// them when each ended: "qt-test test_teamsui ✓ in 29 s · cargo-test exit 1 in 2 min".
QString ranText(const StageRun &stage)
{
    QStringList parts;
    for (const auto &ran : stage.ran) {
        QString part = ran.suite.isEmpty() ? ran.templateId : tr("%1 %2").arg(ran.templateId, ran.suite);
        part += QLatin1Char(' ') + (ran.timedOut ? tr("timed out") : !ran.exited ? tr("did not start") : ran.exit == 0 ? QStringLiteral("✓") : tr("exit %1").arg(ran.exit));
        if (ran.ms > 0) part += QLatin1Char(' ') + (ran.ms < 60000 ? tr("in %1 s").arg(qMax<qint64>(1, ran.ms / 1000)) : tr("in %1 min").arg(ran.ms / 60000));
        parts << part;
    }
    return parts.join(QStringLiteral(" · "));
}

// The files a stage's change lists: a commit's count, or each file.
int changedFiles(const StageRun &run, bool *commits = nullptr)
{
    int files = 0; bool sawCommit = false;
    for (const auto &value : run.changed) {
        const auto o = value.toObject();
        if (o.contains("commit")) { sawCommit = true; files += o.value("files").toInt(); }
        else if (o.contains("path")) ++files;
    }
    if (commits) *commits = sawCommit;
    return files;
}

QString memberAgent(const Team &team, const QString &member)
{
    const auto *m = team.member(member);
    return m ? m->agent : QString();
}

// A command stage's verdict in a few words: "exit 1", "timed out".
QString exitText(const StageRun &stage)
{
    if (!stage.check.known) return {};
    return stage.check.exit < 0 ? tr("did not finish") : tr("exit %1").arg(stage.check.exit);
}

// The last lines of a command's output, for a tooltip or the inspector.
QString lastLines(const QString &tail, int lines)
{
    const auto all = tail.trimmed().split(QLatin1Char('\n'));
    return all.mid(qMax(0, int(all.size()) - lines)).join(QLatin1Char('\n'));
}

// A land stage's exit map as the preset has it: "0 accepted · 1 held".
QString exitsText(const StageSpec &spec)
{
    if (spec.exits.isEmpty()) return tr("0 accepted, any other code held");
    QStringList parts;
    for (auto it = spec.exits.cbegin(); it != spec.exits.cend(); ++it) parts << tr("%1 %2").arg(it.key(), it.value());
    return parts.join(QStringLiteral(" · "));
}
}

QColor stageColor(const Colors &c, const QString &kind)
{
    if (kind == "review") return c.violet;
    if (kind == "test") return c.folder;
    return c.accent;
}

QString ordinal(int n)
{
    if (n % 100 >= 11 && n % 100 <= 13) return tr("%1th").arg(n);
    switch (n % 10) {
    case 1: return tr("%1st").arg(n);
    case 2: return tr("%1nd").arg(n);
    case 3: return tr("%1rd").arg(n);
    default: return tr("%1th").arg(n);
    }
}

StageLine stageLine(const Team &team, const Task &task, const Colors &c)
{
    StageLine line;
    const auto &run = task.pipelineRun;
    // A pending task whose folder a test of another task holds (#532: the
    // wait says so; an older hgs: derived the same way).
    if (task.status == "pending" && !task.held) {
        int holder = folderHeldBy(team, task.assignee);
        if (holder == task.id) holder = 0;
        static const QRegularExpression held(QStringLiteral("folder held by the test of #(\\d+)"));
        if (const auto match = held.match(task.wait.unblock); match.hasMatch()) holder = match.captured(1).toInt();
        if (holder) {
            StageChip chip{QStringLiteral("folder_held"), tr("Folder held by the test of #%1").arg(holder),
                           tr("Ready, but its folder is under the test of #%1; it starts when that test ends.").arg(holder), QStringLiteral("held"), c.warn};
            line.chips << chip; line.warn = true;
            // A returned task keeps its findings' note under the hold.
            if (const auto *last = run.runs.isEmpty() ? nullptr : &run.runs.last(); last && last->state == "returned") {
                const int attempt = task.last() ? task.last()->n + 1 : 1;
                line.note = last->findings.size() == 1 ? tr("Attempt %1 · 1 finding").arg(attempt) : tr("Attempt %1 · %2 findings").arg(attempt).arg(last->findings.size());
            }
            return line;
        }
    }
    if (!run.known) return line;
    const int rounds = run.rounds();
    const auto runs = currentRuns(run);
    const StageRun *open = task.inStage() ? run.openRun() : nullptr;
    const StageRun *last = run.runs.isEmpty() ? nullptr : &run.runs.last();
    // The track: implement, each stage, the acceptance.
    const bool reported = task.inStage() || task.status == "submitted" || task.status == "accepted";
    line.track << (reported ? c.accent : task.status == "in_progress" || task.status == "assigned" ? c.accentMuted : c.border);
    for (int i = 0; i < run.stages.size(); ++i) {
        const auto *stage = runs.value(i);
        QColor color = c.border;
        if (stage && stage->state == "passed") color = c.accent;
        else if (stage && stage->state == "returned") color = c.warn;
        else if (stage && stage->state == "skipped") color = alpha(c.text3, .45);
        else if (stage && stage->open()) color = stageColor(c, stage->kind);
        line.track << color;
    }
    const bool limit = task.status == "submitted" && last && last->state == "returned" && run.returned() >= rounds;
    line.track << (task.status == "accepted" ? c.accent : limit ? c.warn : task.status == "submitted" ? stageColor(c, "lead") : c.border);
    // The passed and skipped stages so far, as a summary.
    const auto summary = [&] {
        for (int i = 0; i < run.stages.size(); ++i) {
            const auto *stage = runs.value(i);
            if (!stage) continue;
            const QString kind = run.stages[i].kind;
            if (stage->state == "passed")
                line.chips << StageChip{glyphOf(kind), stageName(kind) + QStringLiteral(" ✓"), tr("%1 passed in round %2").arg(stageName(kind)).arg(stage->round),
                                        QStringLiteral("passed"), stageColor(c, kind)};
            else if (stage->state == "skipped")
                line.chips << StageChip{glyphOf(kind), tr("%1 skipped").arg(stageName(kind)),
                                        stage->text.isEmpty() ? tr("%1 skipped").arg(stageName(kind)) : tr("%1 skipped by %2: %3").arg(stageName(kind), stage->by, stage->text),
                                        QStringLiteral("skipped"), stageColor(c, kind), true};
        }
    };
    if (open) {
        const QString kind = open->kind;
        const Member *member = team.member(open->member);
        // #538 N8: Zerus runs a command or land stage itself; no member, no queue.
        if (runsCommand(*open)) {
            const bool running = open->state == "delivered";
            QStringList tip;
            tip << (running ? tr("Zerus runs the %1 stage's command").arg(stageName(kind).toLower())
                            : tr("Reported; Zerus starts the %1 stage's command once the folder is free").arg(stageName(kind).toLower()));
            if (const auto *spec = specOf(run, *open); spec && !spec->command.isEmpty()) tip << spec->command;
            if (!open->commit.isEmpty()) tip << tr("On commit %1").arg(open->commit.left(7));
            const int next = open->stage + 1;
            if (kind != "land")
                tip << (next < run.stages.size() ? tr("Next: %1").arg(stageName(run.stages[next].kind))
                                                 : run.accept == "auto" ? tr("Next: accepted by the pipeline") : tr("Next: %1 accepts").arg(stageName(acceptKind(run))));
            line.chips << StageChip{glyphOf(kind), running ? tr("%1 running").arg(stageName(kind)) : tr("%1 queued").arg(stageName(kind)), tip.join(QLatin1Char('\n')),
                                    running ? QStringLiteral("running") : QStringLiteral("waiting"), stageColor(c, kind)};
            return line;
        }
        // #538 L6: nobody in the team may take the stage; it waits for the lead.
        if (stageWithoutCandidate(task)) {
            line.chips << StageChip{glyphOf(kind), tr("%1 waits · no member").arg(stageName(kind)),
                                    tr("No member of the team may take the %1: its role has nobody free who did not implement the task. The lead adds a member, skips the stage or sends the task back.")
                                        .arg(stageName(kind).toLower()),
                                    QStringLiteral("stuck"), c.warn};
            line.warn = true;
            return line;
        }
        if (open->state == "waiting") {
            const int place = stageQueuePlace(team, task.id);
            StageChip chip{glyphOf(kind), place > 0 ? tr("%1 waits · %2 in queue").arg(stageName(kind), ordinal(place)) : tr("%1 waits").arg(stageName(kind)),
                           tr("Reported; the %1 waits for a free member (%2 in its queue).").arg(stageName(kind).toLower(), ordinal(qMax(1, place))),
                           QStringLiteral("waiting"), stageColor(c, kind)};
            line.chips << chip;
        } else {
            QString text = tr("%1 r%2/%3").arg(stageName(kind)).arg(open->round).arg(rounds);
            bool commits = false;
            const int files = changedFiles(*open, &commits);
            if (kind == "test" && (!open->checkout.isEmpty() || (run.folder == "checkout" && !open->holds && !open->folder.isEmpty()))) text += tr(" · detached");
            else if (kind == "test" && !commits && files > 0) text += files == 1 ? tr(" · 1 file") : tr(" · %1 files").arg(files);
            QStringList tip;
            tip << tr("%1 round %2 of %3").arg(stageName(kind)).arg(open->round).arg(rounds);
            if (member) tip << QStringLiteral("%1 · %2").arg(team.roleOf(member->id), member->model.isEmpty() ? member->agent : member->model);
            if (!open->folder.isEmpty()) tip << tr("Works in %1").arg(open->checkout.isEmpty() ? open->folder : open->checkout);
            if (kind == "test" && folderHeldBy(team, implementerOf(task, *open)) == task.id) tip << tr("Holds %1's folder").arg(team.roleOf(implementerOf(task, *open)));
            const int next = open->stage + 1;
            tip << (next < run.stages.size() ? tr("Next: %1").arg(stageName(run.stages[next].kind))
                                             : run.accept == "auto" ? tr("Next: accepted by the pipeline") : tr("Next: %1 accepts").arg(stageName(acceptKind(run))));
            line.chips << StageChip{glyphOf(kind), text, tip.join(QLatin1Char('\n')), QStringLiteral("running"), stageColor(c, kind)};
            line.agent = memberAgent(team, open->member);
        }
        return line;
    }
    if (limit) {
        const int found = findingsCount(run);
        line.chips << StageChip{QStringLiteral("stage_lead"), found == 1 ? tr("Lead decides · 1 finding") : tr("Lead decides · %1 findings").arg(found),
                                tr("Out of rounds: returned %1 times; the lead decides with every round's findings.").arg(run.returned()),
                                QStringLiteral("limit"), c.warn};
        line.note = tr("%1 %2 rounds used").arg(run.returned()).arg(stageName(last->kind).toLower());
        line.warn = true;
        return line;
    }
    if (task.status == "submitted" || task.status == "accepted") {
        summary();
        if (task.status == "submitted")
            line.chips << StageChip{QStringLiteral("stage_lead"), stageName(acceptKind(run)),
                                    tr("Every stage is through; %1 decides.").arg(stageName(acceptKind(run))), QStringLiteral("accept"), stageColor(c, "lead")};
        else if (run.accept == "auto")
            line.chips << StageChip{QStringLiteral("stage_auto"), stageName(QStringLiteral("auto")), tr("Accepted by the pipeline once its stages passed"),
                                    QStringLiteral("accept"), stageColor(c, "auto")};
        return line;
    }
    // Returned to its implementer, without the lead: the round's findings go
    // with the next attempt. #538 L5: once the lead or the owner decided after
    // that return (a send-back at the round limit), the chip would say the
    // opposite, so the task shows no stage line.
    const bool decided = task.decidedBy.compare(QLatin1String("lead"), Qt::CaseInsensitive) == 0
                         || task.decidedBy.compare(QLatin1String("human"), Qt::CaseInsensitive) == 0;
    if (!task.finished() && last && last->state == "returned" && !decided) {
        const int attempt = task.status == "pending" ? (task.last() ? task.last()->n + 1 : 1) : (task.last() ? task.last()->n : 1);
        // #538 N8: a command or land stage returns with its exit and output tail.
        if (runsCommand(*last)) {
            const QString exit = exitText(*last);
            QString tip = tr("The %1 stage's command returned round %2 to %3 without the lead.").arg(stageName(last->kind).toLower()).arg(last->round).arg(team.roleOf(task.assignee));
            if (!last->check.tail.trimmed().isEmpty()) tip += QLatin1Char('\n') + lastLines(last->check.tail, 6);
            line.chips << StageChip{QStringLiteral("rejected"), exit.isEmpty() ? tr("%1 returned r%2").arg(stageName(last->kind)).arg(last->round)
                                                                               : tr("%1 returned r%2 · %3").arg(stageName(last->kind)).arg(last->round).arg(exit),
                                    tip, QStringLiteral("returned"), c.warn};
            line.note = exit.isEmpty() ? tr("Attempt %1").arg(attempt) : tr("Attempt %1 · %2").arg(attempt).arg(exit);
            return line;
        }
        line.chips << StageChip{QStringLiteral("rejected"), tr("%1 returned r%2").arg(stageName(last->kind)).arg(last->round),
                                tr("%1 round %2 returned %3 to %4 without the lead.").arg(stageName(last->kind)).arg(last->round)
                                    .arg(last->findings.size() == 1 ? tr("1 finding") : tr("%1 findings").arg(last->findings.size()), team.roleOf(task.assignee)),
                                QStringLiteral("returned"), c.warn};
        line.note = last->findings.size() == 1 ? tr("Attempt %1 · 1 finding").arg(attempt) : tr("Attempt %1 · %2 findings").arg(attempt).arg(last->findings.size());
        return line;
    }
    line.track.clear();
    return line;
}

QWidget *stageLineWidget(const Team &team, const Task &task, const Colors &c)
{
    const auto line = stageLine(team, task, c);
    if (!line.known()) return nullptr;
    auto *box = new StageLineBox; box->setObjectName("teamStageLine"); box->setProperty("task", task.id);
    auto *h = new QHBoxLayout(box); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(5);
    for (const auto &chip : line.chips) h->addWidget(new StageChipWidget(chip));
    if (!line.agent.isEmpty()) {
        auto *logo = new QLabel; logo->setObjectName("teamStageAgent"); logo->setPixmap(agentLogo(line.agent, c.text3).pixmap(14, 14));
        h->addWidget(logo);
    }
    h->addStretch(1);
    if (!line.track.isEmpty()) { box->track = new StageTrack(line.track); h->addWidget(box->track); }
    box->setProperty("note", line.note);
    box->setProperty("warn", line.warn);
    return box;
}

bool usesPipelines(const Team &team)
{
    return std::any_of(team.tasks.cbegin(), team.tasks.cend(), [](const Task &task) {
        return task.pipelineRun.known || task.inStage() || (!task.pipeline.isEmpty() && task.pipeline != QLatin1String("none"));
    });
}

QString heldFoldersNote(const Team &team, const QList<int> &tasks, QString *tip)
{
    QStringList lines;
    for (const auto &member : team.members)
        if (const int holder = folderHeldBy(team, member.id); holder && tasks.contains(holder))
            lines << tr("%1's folder: held by the test of #%2").arg(team.roleOf(member.id)).arg(holder);
    if (tip) *tip = lines.join(QLatin1Char('\n'));
    if (lines.isEmpty()) return {};
    return lines.size() == 1 ? tr("1 folder held") : tr("%1 folders held").arg(lines.size());
}

QString waitingNote(const Team &, const QList<const Task *> &tasks)
{
    int waiting = 0;
    // A queued command or land stage waits for its folder, not for a member.
    for (const auto *task : tasks) if (const auto *run = task->pipelineRun.openRun(); run && task->inStage() && run->state == "waiting" && !runsCommand(*run)) ++waiting;
    return waiting ? tr("%1 waiting").arg(waiting) : QString();
}

QWidget *pipelineSection(const Team &team, const Task &task, const Colors &c, bool dark)
{
    const auto &run = task.pipelineRun;
    if (!run.known) return nullptr;
    const int rounds = run.rounds();
    auto *box = new QWidget; box->setObjectName("teamTaskPipeline");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    auto *head = row(6);
    head->addWidget(label(tr("Pipeline"), c.text3, 12, true));
    head->addStretch();
    head->addWidget(label(run.name.isEmpty() ? run.id : run.name, c.text2, 11, false, "teamTaskPipelineName"));
    v->addLayout(head);
    // The flow: the implementer, each stage, the acceptance.
    const auto runs = currentRuns(run);
    const StageRun *open = task.inStage() ? run.openRun() : nullptr;
    auto *flow = new FlowLayout(4);
    auto *flowBox = new QWidget; flowBox->setObjectName("teamTaskPipelineFlow"); flowBox->setLayout(flow);
    const auto arrow = [&] { flow->addWidget(label(QStringLiteral("›"), c.dim, 12)); };
    {
        int attempts = int(task.attempts.size());
        const bool done = task.inStage() || task.status == "submitted" || task.status == "accepted";
        flow->addWidget(new StageChipWidget(StageChip{QStringLiteral("reported"),
                                                      done ? (attempts == 1 ? tr("Implement ✓") : tr("Implement ✓ %1 attempts").arg(attempts)) : tr("Implement"),
                                                      {}, done ? QStringLiteral("passed") : QStringLiteral("running"), done ? c.text2 : c.accent}));
    }
    for (int i = 0; i < run.stages.size(); ++i) {
        arrow();
        const QString kind = run.stages[i].kind;
        const auto *stage = runs.value(i);
        StageChip chip{glyphOf(kind), stageName(kind), {}, QStringLiteral("next"), stageColor(c, kind)};
        if (stage && stage->state == "passed") { chip.text = tr("%1 ✓ r%2").arg(stageName(kind)).arg(stage->round); chip.state = QStringLiteral("passed"); }
        else if (stage && stage->state == "skipped") { chip.text = tr("%1 skipped").arg(stageName(kind)); chip.state = QStringLiteral("skipped"); chip.strike = true; }
        else if (stage && stage->state == "returned") { chip.text = tr("%1 returned r%2").arg(stageName(kind)).arg(stage->round); chip.state = QStringLiteral("returned"); chip.color = c.warn; chip.kind = QStringLiteral("rejected"); }
        else if (stage && stage->open()) { chip.text = tr("%1 r%2/%3").arg(stageName(kind)).arg(stage->round).arg(rounds); chip.state = stage->state == "waiting" ? QStringLiteral("waiting") : QStringLiteral("running"); }
        flow->addWidget(new StageChipWidget(chip));
    }
    arrow();
    flow->addWidget(new StageChipWidget(StageChip{glyphOf(acceptKind(run)), stageName(acceptKind(run)), {},
                                                  task.status == "accepted" ? QStringLiteral("passed") : QStringLiteral("accept"), stageColor(c, "lead")}));
    v->addWidget(flowBox);
    // Where the open stage works, what changed, what it may do and what it holds.
    if (open) {
        auto *facts = new QFrame; facts->setObjectName("teamTaskPipelineFacts");
        facts->setStyleSheet(QStringLiteral("QFrame#teamTaskPipelineFacts{background:%1;border:1px solid %2;border-radius:8px;}").arg(c.card.name(), c.border.name()));
        auto *g = new QGridLayout(facts); g->setContentsMargins(10, 8, 10, 8); g->setHorizontalSpacing(12); g->setVerticalSpacing(4);
        g->setColumnStretch(1, 1);
        int r = 0;
        const auto fact = [&](const QString &name, const QString &text, const char *object) {
            g->addWidget(label(name, c.muted, 11), r, 0, Qt::AlignTop);
            auto *value = label(text, c.fg, 11, false, object); value->setWordWrap(true); value->setTextInteractionFlags(Qt::TextSelectableByMouse);
            g->addWidget(value, r, 1); ++r;
            return value;
        };
        const QString implementer = implementerOf(task, *open);
        const Member *owner = team.member(implementer);
        const bool command = runsCommand(*open);
        QString where;
        if (!open->checkout.isEmpty()) where = tr("%1 · a detached checkout of the attempt's commits").arg(open->checkout);
        else if (!open->folder.isEmpty()) where = tr("%1 · %2's folder").arg(open->folder, team.roleOf(implementer));
        else if (open->state == "waiting" && command) where = tr("%1's folder, once it is free").arg(team.roleOf(implementer));
        else if (open->state == "waiting") where = run.folder == "checkout" && open->kind == "test" ? tr("a detached checkout, once a member takes it")
                                                                                                   : tr("%1's folder, once a member takes it").arg(team.roleOf(implementer));
        else where = owner && !owner->directory.isEmpty() ? tr("%1 · %2's folder").arg(owner->directory, team.roleOf(implementer)) : tr("%1's folder").arg(team.roleOf(implementer));
        fact(tr("Works in"), where, "teamTaskPipelineWhere");
        bool commits = false;
        const int files = changedFiles(*open, &commits);
        if (!open->changed.isEmpty()) {
            QStringList shas;
            for (const auto &value : open->changed) if (const auto sha = value.toObject().value("commit").toString(); !sha.isEmpty()) shas << sha.left(7);
            const QString count = files == 1 ? tr("1 file") : tr("%1 files").arg(files);
            fact(tr("Changes"), commits ? (shas.size() == 1 ? tr("%1 · %2").arg(shas.first(), count) : tr("%1..%2 · %3 commits · %4").arg(shas.first(), shas.last()).arg(shas.size()).arg(count))
                                        : tr("%1 the report listed").arg(count), "teamTaskPipelineChanges");
        } else fact(tr("Changes"), tr("the attempt's commits, or the report's files"), "teamTaskPipelineChanges");
        if (command) {
            // #538 N8: what Zerus runs, what it expects, and on which commit.
            const auto *spec = specOf(run, *open);
            if (spec && !spec->command.isEmpty()) fact(tr("Command"), spec->command, "teamTaskPipelineCommand")->setFont(NewSessionDense::mono(11));
            else fact(tr("Command"), tr("the preset's command"), "teamTaskPipelineCommand");
            if (spec)
                fact(open->kind == "land" ? tr("Exits") : tr("Expects"),
                     open->kind == "land" ? tr("%1 · %2 min limit").arg(exitsText(*spec)).arg(spec->timeoutMinutes)
                                          : tr("exit %1 · %2 min limit").arg(spec->expectExit).arg(spec->timeoutMinutes), "teamTaskPipelineExpect");
            fact(tr("Runs"), open->state == "delivered" ? (open->commit.isEmpty() ? tr("Zerus itself, outside any member's sandbox")
                                                                                  : tr("Zerus itself on commit %1, outside any member's sandbox").arg(open->commit.left(7)))
                                                        : tr("Zerus itself, once the folder is free; no member takes it"), "teamTaskPipelineRuns");
        } else {
            fact(open->kind == "test" ? tr("Test may") : tr("Review may"),
                 open->kind == "test" ? tr("read; build; write build output and its own temporary folder") : tr("read the change and the task's files"), "teamTaskPipelineMay");
            // QA #539: the suites the stage ran so far, as hgs records each
            // when it ends; nothing recorded, no line.
            if (const QString ran = ranText(*open); !ran.isEmpty()) fact(tr("Ran"), ran, "teamTaskPipelineRan");
        }
        if (open->kind == "test") {
            QString holds = tr("nothing: it works on a detached checkout");
            if (folderHeldBy(team, implementer) == task.id) {
                holds = tr("%1's folder").arg(team.roleOf(implementer));
                for (const auto &next : team.tasks)
                    if (next.id != task.id && next.assignee == implementer && next.status == "pending" && !next.held) { holds = tr("the folder: #%1 %2 waits").arg(next.id).arg(next.title); break; }
            }
            fact(tr("Holds"), holds, "teamTaskPipelineHolds");
        }
        v->addWidget(facts);
    }
    // Rounds: each attempt and the stages that checked it, oldest first, then
    // what comes next.
    v->addWidget(label(tr("Rounds"), c.text3, 11, true));
    auto *timeline = new QWidget; timeline->setObjectName("teamTaskRounds");
    auto *tl = new QVBoxLayout(timeline); tl->setContentsMargins(0, 0, 0, 0); tl->setSpacing(6);
    const auto entry = [&](const QString &kind, const QColor &color, const QString &title, const QString &verdict, const QColor &verdictColor,
                           const QString &when, const char *object) {
        auto *item = new QWidget; item->setObjectName(object);
        auto *iv = new QVBoxLayout(item); iv->setContentsMargins(0, 0, 0, 0); iv->setSpacing(3);
        auto *top = row(6);
        auto *icon = new QLabel; icon->setPixmap(kind == QLatin1String("pencil") ? workspaceIcon(kind, color).pixmap(14, 14) : glyphPixmap(color, kind, 14)); top->addWidget(icon);
        top->addWidget(label(title, c.fg, 12, true));
        if (!verdict.isEmpty()) {
            auto *badge = label(verdict, verdictColor, 10, true, "teamRoundVerdict");
            badge->setStyleSheet(QStringLiteral("color:%1;background:%2;border-radius:5px;padding:1px 6px;").arg(verdictColor.name(), alpha(verdictColor, .14).name(QColor::HexArgb)));
            top->addWidget(badge);
        }
        top->addStretch();
        if (!when.isEmpty()) top->addWidget(label(when, c.muted, 11));
        iv->addLayout(top);
        tl->addWidget(item);
        return iv;
    };
    const auto indent = [](QWidget *w) { w->setContentsMargins(20, 0, 0, 0); return w; };
    // A stage's later pass fixes the findings of its earlier returns.
    QMap<int, double> passedAt;
    for (const auto &stage : run.runs) if (stage.state == "passed") passedAt[stage.stage] = qMax(passedAt.value(stage.stage), stage.endedAt);
    static const QRegularExpression severity(QStringLiteral("^\\s*\\[?(blocker|major|minor|nit)\\]?\\s*[:\\-—]?\\s*"), QRegularExpression::CaseInsensitiveOption);
    for (const auto &attempt : task.attempts) {
        const QString who = team.roleOf(attempt.member);
        const double end = attempt.report.isEmpty() && !attempt.structured.present ? 0 : attempt.reportAt;
        const QString when = attempt.sentAt > 0 ? (end > 0 ? QStringLiteral("%1–%2").arg(clock(attempt.sentAt), clock(end)) : clock(attempt.sentAt)) : QString();
        auto *iv = entry(QStringLiteral("pencil"), c.text2, tr("Attempt %1").arg(attempt.n), QString(), {}, when, "teamRoundAttempt");
        iv->parentWidget()->setProperty("attempt", attempt.n);
        auto *by = row(6); by->addWidget(new Pill(who, roleColor(who, dark))); by->addStretch();
        auto *byBox = new QWidget; byBox->setLayout(by); iv->addWidget(indent(byBox));
        const QString text = attempt.structured.present && !attempt.structured.summary.isEmpty() ? attempt.structured.summary : attempt.report;
        if (!text.isEmpty()) { auto *t = label(text.section(QLatin1Char('\n'), 0, 0).left(300), c.text2, 11); t->setWordWrap(true); iv->addWidget(indent(t)); }
        for (const auto &stage : run.runs) {
            if (stage.attempt != attempt.n) continue;
            static const QMap<QString, std::pair<const char *, int>> verdicts{
                {"passed", {"PASS", 0}}, {"returned", {"RETURN", 1}}, {"delivered", {"RUNNING", 2}}, {"waiting", {"WAITING", 3}},
                {"skipped", {"SKIPPED", 3}}, {"revoked", {"REVOKED", 3}}};
            const auto verdict = verdicts.value(stage.state, {"", 3});
            const QColor tone = verdict.second == 0 ? c.accent : verdict.second == 1 ? c.warn : verdict.second == 2 ? stageColor(c, stage.kind) : c.muted;
            QString badge = tr(verdict.first);
            if (stage.state == "delivered" && stage.deliveredAt > 0)
                badge += QStringLiteral(" ") + agoText(stage.deliveredAt, workedClock()()).remove(QStringLiteral(" ago"));
            const double at = stage.endedAt > 0 ? stage.endedAt : stage.deliveredAt > 0 ? stage.deliveredAt : stage.at;
            auto *sv = entry(glyphOf(stage.kind), stageColor(c, stage.kind), tr("%1 · round %2").arg(stageName(stage.kind)).arg(stage.round), badge, tone,
                             at > 0 ? clock(at) : QString(), "teamRoundStage");
            sv->parentWidget()->setProperty("state", stage.state);
            sv->parentWidget()->setProperty("stage", stage.kind);
            sv->parentWidget()->setProperty("round", stage.round);
            auto *by = row(6);
            const bool command = runsCommand(stage);
            if (command) {
                // #538 N8: Zerus ran it: the exit, the commit and what it meant.
                QStringList facts{tr("Zerus")};
                if (const QString exit = exitText(stage); !exit.isEmpty()) facts << exit;
                else if (stage.state == "waiting") facts << tr("starts once the folder is free");
                else if (stage.state == "delivered") facts << tr("running");
                if (!stage.commit.isEmpty()) facts << stage.commit.left(7);
                if (stage.flaky) facts << tr("flaky: the same output on the same commit; the lead decides");
                else if (stage.kind == "land" && !stage.outcome.isEmpty()) facts << tr("landing %1").arg(stage.outcome);
                by->addWidget(label(facts.join(QStringLiteral(" · ")), stage.flaky ? c.warn : c.muted, 11, false, "teamRoundCommand"));
            } else if (!stage.member.isEmpty()) {
                const QString role = team.roleOf(stage.member);
                by->addWidget(new Pill(role, roleColor(role, dark)));
                if (const auto *m = team.member(stage.member)) {
                    auto *logo = new QLabel; logo->setPixmap(agentLogo(m->agent, c.fg).pixmap(14, 14)); by->addWidget(logo);
                    by->addWidget(label(m->model.isEmpty() ? m->agent : m->model, c.text2, 11));
                }
            } else if (stage.state == "waiting") by->addWidget(label(tr("waits for a free member"), c.muted, 11));
            else if (!stage.by.isEmpty()) by->addWidget(label(tr("by %1").arg(stage.by), c.muted, 11));
            by->addStretch();
            auto *byBox = new QWidget; byBox->setLayout(by); sv->addWidget(indent(byBox));
            if (command && stage.check.known) {
                // The command's own words: hgs's summary, then the output tail.
                auto *t = label(stage.text.section(QStringLiteral(" Output tail:"), 0, 0).trimmed(), c.text2, 11, false, "teamRoundText");
                t->setWordWrap(true); sv->addWidget(indent(t));
                if (!stage.check.tail.trimmed().isEmpty()) {
                    auto *tail = label(lastLines(stage.check.tail, 12), c.text2, 10, false, "teamRoundTail");
                    tail->setFont(NewSessionDense::mono(10)); tail->setWordWrap(true); tail->setTextInteractionFlags(Qt::TextSelectableByMouse);
                    tail->setToolTip(stage.check.tail);
                    sv->addWidget(indent(tail));
                }
            } else if (!stage.text.isEmpty()) {
                auto *t = label(stage.state == "skipped" || stage.state == "revoked" ? tr("%1: %2").arg(stage.by, stage.text) : stage.text, c.text2, 11, false, "teamRoundText");
                t->setWordWrap(true); sv->addWidget(indent(t));
            }
            if (const QString ran = ranText(stage); !ran.isEmpty()) {
                auto *t = label(tr("Ran %1").arg(ran), c.muted, 11, false, "teamRoundRan"); t->setWordWrap(true); sv->addWidget(indent(t));
            }
            const bool fixed = stage.state == "returned" && passedAt.value(stage.stage) > stage.endedAt;
            for (const auto &finding : stage.findings) {
                auto *f = new QFrame; f->setObjectName("teamRoundFinding"); f->setProperty("fixed", fixed);
                f->setStyleSheet(QStringLiteral("QFrame#teamRoundFinding{border:1px solid %1;border-radius:6px;}").arg(c.border.name()));
                auto *fh = new QHBoxLayout(f); fh->setContentsMargins(8, 4, 8, 4); fh->setSpacing(8);
                QString text = finding;
                if (const auto match = severity.match(finding); match.hasMatch()) {
                    const QString level = match.captured(1).toUpper();
                    const QColor tint = level == "BLOCKER" || level == "MAJOR" ? c.danger : level == "MINOR" ? c.warn : c.muted;
                    auto *sev = label(level, tint, 10, true, "teamRoundSeverity"); fh->addWidget(sev, 0, Qt::AlignTop);
                    text = finding.mid(match.capturedLength());
                }
                auto *body = label(text, fixed ? c.muted : c.fg, 11); body->setWordWrap(true); body->setTextInteractionFlags(Qt::TextSelectableByMouse);
                fh->addWidget(body, 1);
                sv->addWidget(indent(f));
            }
        }
    }
    // What comes next in this round, then the acceptance.
    if (!task.finished()) {
        const int from = open ? open->stage + 1 : task.status == "submitted" ? int(run.stages.size()) : 0;
        if (task.inStage() || task.status == "submitted")
            for (int i = from; i < run.stages.size(); ++i)
                entry(glyphOf(run.stages[i].kind), c.dim, tr("%1 · round %2").arg(stageName(run.stages[i].kind)).arg(qMax(1, run.round)), tr("NEXT"), c.muted, {}, "teamRoundNext");
        const bool limit = task.status == "submitted" && !run.runs.isEmpty() && run.runs.last().state == "returned" && run.returned() >= rounds;
        auto *av = entry(glyphOf(acceptKind(run)), limit ? c.warn : c.dim,
                         run.accept == "auto" ? tr("Accepted by the pipeline") : tr("%1 accepts").arg(stageName(acceptKind(run))),
                         task.status == "submitted" ? tr("NOW") : tr("NEXT"), task.status == "submitted" ? stageColor(c, "lead") : c.muted, {}, "teamRoundAccept");
        auto *note = label(limit ? tr("Out of rounds: %1 returns; decide with every round's findings.").arg(run.returned())
                                 : run.accept == "auto" ? tr("After the last stage passes, without the lead.")
                                                        : tr("After the last stage passes. Out of rounds goes here too, with every round's findings."),
                           limit ? c.warn : c.muted, 11);
        note->setWordWrap(true); av->addWidget(indent(note));
    }
    v->addWidget(timeline);
    return box;
}

QWidget *memberStageQueue(const Team &team, const Member &member, const Colors &c, double now, const std::function<void(int)> &open)
{
    const auto queue = stageQueue(team, member.id);
    // Only a member that takes stages: it has one, may take one, or is a
    // candidate's role (a reviewer or tester with nothing yet).
    bool stageRole = !queue.isEmpty();
    for (const auto &task : team.tasks) {
        if (!task.pipelineRun.known) continue;
        for (const auto &spec : task.pipelineRun.stages)
            if (spec.member == member.id || (!spec.role.isEmpty() && (spec.role.compare(member.role, Qt::CaseInsensitive) == 0
                                                                      || spec.role.compare(member.id, Qt::CaseInsensitive) == 0)))
                stageRole = true;
    }
    if (!stageRole) return nullptr;
    auto *box = new QFrame; box->setObjectName("teamMemberStageQueue"); box->setProperty("member", member.id);
    box->setStyleSheet(QStringLiteral("QFrame#teamMemberStageQueue{background:%1;border-radius:7px;}").arg(alpha(c.text3, .08).name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(box); v->setContentsMargins(8, 5, 8, 5); v->setSpacing(3);
    QList<Member::StageEntry> waiting;
    const Member::StageEntry *current = nullptr;
    for (const auto &entry : queue) { if (entry.state == "delivered" && !current) current = &entry; else waiting << entry; }
    auto *head = row(6);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("inbox", c.text3).pixmap(12, 12)); head->addWidget(icon);
    QString title = waiting.isEmpty() ? tr("Stage queue empty") : tr("Stage queue %1").arg(waiting.size());
    if (waiting.isEmpty() && current && current->since > 0) title += QStringLiteral(" · ") + tr("%1 in this stage").arg(agoText(current->since, now).remove(QStringLiteral(" ago")));
    head->addWidget(label(title, c.text2, 11, false, "teamMemberStageQueueTitle"));
    head->addStretch();
    v->addLayout(head);
    for (const auto &entry : waiting) {
        const auto *task = team.task(entry.task);
        if (!task) continue;
        auto *line = new QPushButton; line->setObjectName("teamMemberStageQueueRow"); line->setProperty("task", entry.task);
        line->setFlat(true); line->setCursor(Qt::PointingHandCursor);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
        h->addWidget(new StageChipWidget(StageChip{glyphOf(entry.stage), tr("%1 r%2").arg(stageName(entry.stage)).arg(qMax(1, entry.round)), {},
                                                   QStringLiteral("waiting"), stageColor(c, entry.stage)}));
        h->addWidget(elided(QStringLiteral("#%1 %2").arg(task->id).arg(task->title), c.fg, 11), 1);
        if (entry.since > 0) h->addWidget(label(agoText(entry.since, now).remove(QStringLiteral(" ago")), c.muted, 10));
        line->setMinimumHeight(21);
        line->setStyleSheet(QStringLiteral("QPushButton#teamMemberStageQueueRow{border:none;background:transparent;text-align:left;}"));
        QObject::connect(line, &QPushButton::clicked, line, [open, id = entry.task] { if (open) open(id); });
        v->addWidget(line);
    }
    return box;
}

}
