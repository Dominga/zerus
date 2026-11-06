#include "TeamHeat.h"
#include "Settings.h"
#include "WorkspaceIcons.h"
#include <QHBoxLayout>
#include <QJsonArray>
#include <QLinearGradient>
#include <QPainter>
#include <QDateTime>
#include <algorithm>

namespace TeamUi {

namespace {

// The legend's scale: the surface on the left, the warmest tint on the right.
class HeatScale : public QWidget {
public:
    explicit HeatScale(const Colors &c) : m_c(c) { setObjectName("teamHeatScale"); setFixedSize(180, 10); }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        QLinearGradient g(0, 0, width(), 0);
        g.setColorAt(0, heatFill(m_c, 0)); g.setColorAt(1, heatFill(m_c, 1));
        p.setPen(QPen(m_c.border, 1)); p.setBrush(g);
        p.drawRoundedRect(QRectF(rect()).adjusted(.5, .5, -.5, -.5), 5, 5);
    }
private:
    Colors m_c;
};

QColor mixed(const QColor &a, const QColor &b, double t)
{
    return QColor::fromRgbF(float(a.redF() + (b.redF() - a.redF()) * t), float(a.greenF() + (b.greenF() - a.greenF()) * t),
                            float(a.blueF() + (b.blueF() - a.blueF()) * t));
}

QString list(const QJsonValue &value)
{
    QStringList ids;
    for (const auto &id : value.toArray()) ids << QStringLiteral("#%1").arg(id.toInt());
    return ids.join(QStringLiteral(", "));
}

}

qint64 Heat::lost(int task) const
{
    // Task 82: Time is the task's own lateness against the plan, not every
    // wait since the phase began; inherited lateness does not tint.
    if (metric != QLatin1String("tokens")) {
        const auto late = lateness.value(task);
        return late.known ? qMax<qint64>(0, late.own) : 0;
    }
    qint64 sum = 0;
    for (const auto &loss : losses.value(task)) sum += metric == QLatin1String("tokens") ? qMax<qint64>(0, loss.tokens) : loss.seconds;
    return sum;
}

qint64 Heat::most() const
{
    qint64 top = 0;
    for (auto it = losses.cbegin(); it != losses.cend(); ++it) top = qMax(top, lost(it.key()));
    for (auto it = lateness.cbegin(); it != lateness.cend(); ++it) top = qMax(top, lost(it.key()));
    return top;
}

double Heat::share(int task) const
{
    const qint64 scale = metric == QLatin1String("tokens") ? most() : HeatTimeScale;
    return scale > 0 ? qBound(0.0, double(lost(task)) / double(scale), 1.0) : 0.0;
}

QString heatMode(const QString &team)
{
    const QString mode = Settings::value(QStringLiteral("teams/heat/") + team).toString();
    return mode == QLatin1String("time") || mode == QLatin1String("tokens") ? mode : QStringLiteral("off");
}

// #729 (#709 N6): through Settings, so a click never waits on the INI's lock.
void setHeatMode(const QString &team, const QString &mode)
{
    if (mode == QLatin1String("time") || mode == QLatin1String("tokens")) Settings::set(QStringLiteral("teams/heat/") + team, mode);
    else Settings::remove(QStringLiteral("teams/heat/") + team);
}

HeatCursors addHeatPage(Heat &heat, const QJsonObject &profile)
{
    const bool tokens = heat.metric == QLatin1String("tokens");
    const bool lossesWanted = heat.pages == 0 || !heat.complete, tasksWanted = heat.pages == 0 || !heat.tasksComplete;
    // Task 82: each task's lateness and overrun, from the task rows.
    const auto rows = profile.value("tasks").toObject();
    if (tasksWanted) {
        for (const auto &item : rows.value("items").toArray()) {
            const auto o = item.toObject();
            const int task = o.value("task").toInt();
            if (!task) continue;
            const auto at = [](const QJsonValue &v) { return v.isDouble() ? v.toDouble() : -1.0; };
            if (const auto l = o.value("lateness").toObject(); !l.isEmpty()) {
                HeatLateness late;
                late.plannedStart = at(l.value("planned_start")); late.plannedEnd = at(l.value("planned_end"));
                late.actualStart = at(l.value("actual_start")); late.actualEnd = at(l.value("actual_end"));
                late.unknown = l.value("unknown").toString();
                late.known = l.value("start_late_s").isDouble() && late.unknown.isEmpty();
                late.startLate = l.value("start_late_s").toInteger(); late.inherited = l.value("inherited_s").toInteger();
                late.own = l.value("own_s").toInteger();
                late.endLate = l.value("end_late_s").isDouble() ? l.value("end_late_s").toInteger() : -1;
                late.from = l.value("from").toInt();
                heat.lateness.insert(task, late);
            }
            if (const auto v = o.value("overrun").toObject(); !v.isEmpty()) {
                HeatOverrun over;
                over.started = !v.value("not_started").toBool();
                over.known = v.value("overrun_s").isDouble() && v.value("estimate_s").isDouble();
                over.estimate = v.value("estimate_s").toInteger(); over.work = v.value("work_s").toInteger();
                over.overrun = v.value("overrun_s").toInteger(); over.ratio = v.value("ratio").toDouble();
                over.running = v.value("running").toBool();
                heat.overrun.insert(task, over);
            }
        }
    }
    const auto list = profile.value("losses").toObject().value(tokens ? "by_tokens" : "by_time").toObject();
    for (const auto &item : lossesWanted ? list.value("items").toArray() : QJsonArray()) {
        const auto o = item.toObject();
        // An idle member has no task: Graph and Board have nothing to tint.
        const int task = o.value("task").toInt();
        if (!task) continue;
        HeatLoss loss;
        loss.kind = o.value("kind").toString();
        loss.seconds = o.value("s").toInteger();
        loss.tokens = o.contains("tokens") ? o.value("tokens").toInteger() : -1;
        loss.detail = o;
        if (o.value("tokens_unknown").toBool()) heat.tokensPartial = true;
        heat.losses[task].append(loss);
    }
    const auto spent = profile.value("summary").toObject().value("tokens").toObject();
    if (spent.value("partial").toBool() || spent.value("unknown").toBool()) heat.tokensPartial = true;
    if (profile.value("token_reads").toObject().value("truncated").toBool()) heat.tokensPartial = true;
    ++heat.pages;
    // The pages come in the profile's own order; keep each task's largest first.
    for (auto &losses : heat.losses)
        std::stable_sort(losses.begin(), losses.end(), [tokens](const HeatLoss &a, const HeatLoss &b) {
            return tokens ? a.tokens > b.tokens : a.seconds > b.seconds;
        });
    HeatCursors cursors;
    if (lossesWanted) {
        const QJsonValue next = list.value("next_cursor");
        heat.complete = next.isNull() || next.isUndefined();
        if (!heat.complete) cursors.losses = next;
    }
    if (tasksWanted) {
        const QJsonValue next = rows.value("next_cursor");
        heat.tasksComplete = next.isNull() || next.isUndefined();
        if (!heat.tasksComplete) cursors.tasks = next;
    }
    return cursors;
}

QString heatMoment(double at)
{
    if (at <= 0) return {};
    const QDateTime when = QDateTime::fromSecsSinceEpoch(qint64(at)).toLocalTime();
    return when.date() == QDate::currentDate() ? when.toString(QStringLiteral("HH:mm")) : when.toString(QStringLiteral("d MMM HH:mm"));
}

QString overrunText(const HeatOverrun &overrun, bool compact)
{
    if (!overrun.started) return {};
    if (!overrun.known) return compact ? QStringLiteral("x?") : QObject::tr("no estimate");
    if (compact) return QStringLiteral("x%1").arg(QString::number(overrun.ratio, 'f', 1));
    return overrun.overrun > 0 ? QObject::tr("+%1 over %2").arg(workedText(overrun.overrun), workedText(overrun.estimate))
                               : QObject::tr("within %1").arg(workedText(overrun.estimate));
}

QColor overrunColor(const Colors &c, const HeatOverrun &overrun)
{
    if (!overrun.known || overrun.ratio <= 1.0) return c.muted;
    return mixed(c.text3, c.violet, qBound(0.0, overrun.ratio - 1.0, 1.0));
}

QLabel *heatOverrunBadge(const Heat &heat, const Teams::Task &task, const Colors &c)
{
    if (!heatCovers(task) || !heat.overrun.contains(task.id)) return nullptr;
    const auto over = heat.overrun.value(task.id);
    const QString text = overrunText(over, false);
    if (text.isEmpty()) return nullptr;
    const QColor tone = overrunColor(c, over);
    auto *badge = new QLabel(text); badge->setObjectName("teamHeatOverrun");
    badge->setProperty("ratio", over.known ? over.ratio : -1.0);
    badge->setStyleSheet(QStringLiteral("QLabel#teamHeatOverrun{color:%1;background:%2;border:1px solid %3;border-radius:4px;padding:0 4px;font-size:10px;font-weight:600;}")
                             .arg(tone.name(), QColor(tone.red(), tone.green(), tone.blue(), 30).name(QColor::HexArgb), tone.name()));
    badge->setToolTip(over.known ? QObject::tr("Own work %1 against a %2 estimate (x%3)").arg(workedText(over.work), workedText(over.estimate)).arg(QString::number(over.ratio, 'f', 1))
                                 : QObject::tr("Own work %1; the task has no estimate").arg(workedText(over.work)));
    return badge;
}

QString tokensText(qint64 tokens)
{
    if (tokens < 1000) return QString::number(tokens);
    if (tokens < 10000) return QStringLiteral("%1k").arg(QString::number(tokens / 1000.0, 'f', 1));
    if (tokens < 1000000) return QStringLiteral("%1k").arg(qRound(tokens / 1000.0));
    return QStringLiteral("%1M").arg(QString::number(tokens / 1000000.0, 'f', 1));
}

QString heatLossName(const HeatLoss &loss)
{
    const auto &d = loss.detail;
    if (loss.kind == "rework")
        return QObject::tr("Rework: attempt %1 %2").arg(d.value("attempt").toInt())
            .arg(d.value("cause").toString() == "returned" ? QObject::tr("returned") : QObject::tr("rejected"));
    if (loss.kind == "lead_wait") {
        const qint64 questions = d.value("question_s").toInteger();
        if (questions > 0 && d.value("lead_s").toInteger() > 0)
            return QObject::tr("Waited for the lead (decision %1, questions %2)").arg(workedText(d.value("lead_s").toInteger()), workedText(questions));
        return questions > 0 ? QObject::tr("Waited for the lead's answers") : QObject::tr("Waited for the lead's decision");
    }
    if (loss.kind == "permission_wait") {
        QStringList by; for (const auto &name : d.value("answered_by").toArray()) by << name.toString();
        return by.isEmpty() ? QObject::tr("Waited for a permission") : QObject::tr("Waited for a permission (answered by %1)").arg(by.join(QStringLiteral(", ")));
    }
    if (loss.kind == "blocked_chain") {
        const bool deps = d.value("deps_s").toInteger() > 0, held = d.value("folder_held_s").toInteger() > 0;
        const QString on = list(d.value("on")), by = list(d.value("held_by"));
        if (deps && held) return QObject::tr("Blocked: dependencies %1, folder held by %2").arg(on, by);
        if (held) return by.isEmpty() ? QObject::tr("Folder held") : QObject::tr("Folder held by %1").arg(by);
        return on.isEmpty() ? QObject::tr("Waited for dependencies") : QObject::tr("Waited for dependencies %1").arg(on);
    }
    if (loss.kind == "member_wait") return QObject::tr("Waited for a free member");
    if (loss.kind == "long_test") return QObject::tr("Long test run");
    if (loss.kind == "merge") return QObject::tr("Merge");
    if (loss.kind == "conflict") return QObject::tr("Merge conflict");
    return loss.kind;
}

bool heatCovers(const Teams::Task &task)
{
    return task.status != QLatin1String("draft") && task.status != QLatin1String("cancelled");
}

namespace {
// Task 82: Time's label is the lateness against the plan.
QString lateLabel(const Heat &heat, int task)
{
    if (!heat.lateness.contains(task)) return {};
    const auto late = heat.lateness.value(task);
    if (!late.known) return QObject::tr("lateness unknown");
    if (late.own > 0 && late.inherited > 0)
        return QObject::tr("late %1 (%2 from #%3)").arg(workedText(late.own), workedText(late.inherited)).arg(late.from);
    if (late.own > 0) return QObject::tr("late %1").arg(workedText(late.own));
    if (late.inherited > 0) return late.from ? QObject::tr("late %1 from #%2").arg(workedText(late.inherited)).arg(late.from)
                                             : QObject::tr("late %1, inherited").arg(workedText(late.inherited));
    return QObject::tr("on time");
}

QString lateTip(const Heat &heat, int task)
{
    QStringList lines;
    if (heat.lateness.contains(task)) {
        const auto late = heat.lateness.value(task);
        const auto moment = [](double at, const QString &none) { const QString text = heatMoment(at); return text.isEmpty() ? none : text; };
        lines << QObject::tr("Planned: start %1, end %2").arg(moment(late.plannedStart, QObject::tr("unknown")), moment(late.plannedEnd, QObject::tr("unknown")));
        lines << QObject::tr("Actual: start %1, end %2").arg(moment(late.actualStart, QObject::tr("not yet")), moment(late.actualEnd, QObject::tr("not yet")));
        if (!late.known) lines << QObject::tr("Lateness unknown: %1").arg(late.unknown.isEmpty() ? QObject::tr("an estimate is missing") : late.unknown);
        else if (late.startLate <= 0) lines << QObject::tr("On schedule");
        else {
            QString line = QObject::tr("Late %1: own %2").arg(workedText(late.startLate), workedText(late.own));
            if (late.inherited > 0) line += late.from ? QObject::tr(", %1 from #%2 (it ran late)").arg(workedText(late.inherited)).arg(late.from)
                                                      : QObject::tr(", %1 inherited").arg(workedText(late.inherited));
            lines << line;
        }
    } else {
        lines << QObject::tr("No plan baseline: this task was planned before lateness was measured");
    }
    if (heat.overrun.contains(task)) {
        const auto over = heat.overrun.value(task);
        if (!over.started) lines << QObject::tr("Overrun: not started");
        else if (!over.known) lines << QObject::tr("Overrun: own work %1, no estimate").arg(workedText(over.work));
        else lines << QObject::tr("Overrun: own work %1 against a %2 estimate (x%3)").arg(workedText(over.work), workedText(over.estimate)).arg(QString::number(over.ratio, 'f', 1));
    }
    QStringList causes;
    const auto losses = heat.losses.value(task);
    for (int i = 0; i < losses.size() && i < 3; ++i) causes << QStringLiteral("%1 %2").arg(heatLossName(losses.at(i)), workedText(losses.at(i).seconds));
    if (!causes.isEmpty()) lines << QObject::tr("Causes: %1").arg(causes.join(QStringLiteral("; ")));
    return lines.join(QLatin1Char('\n'));
}

QString lossLabel(const Heat &heat, int task)
{
    if (heat.metric != QLatin1String("tokens")) return lateLabel(heat, task);
    const qint64 lost = heat.lost(task);
    if (lost <= 0) return heat.complete ? QObject::tr("no loss") : QString();
    return heat.metric == QLatin1String("tokens") ? QObject::tr("lost %1 tokens").arg(tokensText(lost)) : QObject::tr("lost %1").arg(workedText(lost));
}

QString lossTip(const Heat &heat, int task)
{
    const bool tokens = heat.metric == QLatin1String("tokens");
    if (!tokens) return lateTip(heat, task);
    const qint64 lost = heat.lost(task);
    if (lost <= 0)
        return !heat.complete ? QString() : tokens ? QObject::tr("No tokens lost to rework or long tests") : QObject::tr("No time lost to waits or rework");
    const auto amount = [tokens](const HeatLoss &loss) {
        return tokens ? QObject::tr("%1 tokens").arg(tokensText(qMax<qint64>(0, loss.tokens))) : workedText(loss.seconds);
    };
    QStringList lines{tokens ? QObject::tr("Lost %1 tokens to rework and long tests").arg(tokensText(lost))
                             : QObject::tr("Lost %1 to waits and rework").arg(workedText(lost))};
    const auto losses = heat.losses.value(task);
    QString top = QObject::tr("Top loss: %1, %2").arg(heatLossName(losses.first()), amount(losses.first()));
    if (tokens && losses.first().detail.contains("share"))
        top += QObject::tr(", %1% of the task's tokens").arg(qRound(losses.first().detail.value("share").toDouble() * 100));
    lines << top;
    QStringList next;
    for (int i = 1; i < losses.size() && i < 3; ++i) next << QStringLiteral("%1 %2").arg(heatLossName(losses.at(i)), amount(losses.at(i)));
    if (!next.isEmpty()) lines << QObject::tr("Then: %1").arg(next.join(QStringLiteral("; ")));
    return lines.join(QLatin1Char('\n'));
}
}

QString heatLabel(const Heat &heat, const Teams::Task &task)
{
    return heatCovers(task) ? lossLabel(heat, task.id) : QString();
}

QString heatTip(const Heat &heat, const Teams::Task &task)
{
    return heatCovers(task) ? lossTip(heat, task.id) : QString();
}

QByteArray heatDigest(const Heat &heat, const Colors &c)
{
    // What Graph, Board and the legend draw from it: each task's loss text,
    // tint, text color and tooltip, and the legend's end and notes. A wait
    // that grew by seconds the loss text does not show draws the same.
    const bool tokens = heat.metric == QLatin1String("tokens");
    QByteArray digest = heat.metric.toUtf8() + ' ' + char('0' + heat.complete) + char('0' + heat.tokensPartial) + ' ' + QByteArray::number(heat.pages)
                        + ' ' + (tokens ? tokensText(heat.most()).toUtf8() : QByteArray()) + '\n';
    QList<int> tasks = heat.losses.keys() + heat.lateness.keys() + heat.overrun.keys();
    std::sort(tasks.begin(), tasks.end());
    tasks.erase(std::unique(tasks.begin(), tasks.end()), tasks.end());
    for (int task : tasks) {
        const double share = heat.share(task);
        digest += QByteArray::number(task) + ' ' + lossLabel(heat, task).toUtf8() + ' ' + heatFill(c, share).name().toUtf8() + ' '
                  + heatTextColor(c, share).name().toUtf8() + ' ' + overrunText(heat.overrun.value(task), false).toUtf8() + ' '
                  + overrunColor(c, heat.overrun.value(task)).name().toUtf8() + ' ' + lossTip(heat, task).toUtf8() + '\n';
    }
    return digest;
}

QColor heatFill(const Colors &c, double share)
{
    // The mock-up's tint: up to 42% of the heat color over the surface.
    return mixed(c.surface, c.danger, qBound(0.0, share, 1.0) * .42);
}

QColor heatTextColor(const Colors &c, double share)
{
    return share > .5 ? c.danger : c.muted;
}

void applyHeat(TaskView::Node &node, const Heat &heat, const Teams::Task &task, const Colors &c)
{
    const QString text = heatLabel(heat, task);
    // Task 82: the overrun badge beside the lateness, in its own colour.
    const QString badge = heatCovers(task) ? overrunText(heat.overrun.value(task.id), true) : QString();
    if (text.isEmpty() && badge.isEmpty()) return;
    node.worked.clear(); node.runningSince = 0;
    node.heat = text;
    node.heatBadge = badge;
    node.heatBadgeColor = overrunColor(c, heat.overrun.value(task.id));
    const bool lost = heat.lost(task.id) > 0;
    node.heatFill = lost ? heatFill(c, heat.share(task.id)) : QColor();
    node.heatColor = lost ? heatTextColor(c, heat.share(task.id)) : c.dim;
    if (const QString tip = heatTip(heat, task); !tip.isEmpty()) node.tooltip += QLatin1Char('\n') + tip;
}

QLabel *heatBoardLabel(const Heat &heat, const Teams::Task &task, const Colors &c)
{
    const QString text = heatLabel(heat, task);
    if (text.isEmpty()) return nullptr;
    const bool lost = heat.lost(task.id) > 0;
    auto *l = label(text, lost ? heatTextColor(c, heat.share(task.id)) : c.dim, 11, lost, "teamBoardHeat");
    l->setProperty("share", heat.share(task.id));
    l->setToolTip(heatTip(heat, task));
    return l;
}

QColor heatCardFill(const Heat *heat, const Teams::Task &task, const Colors &c)
{
    if (!heat || !heatCovers(task) || heat->lost(task.id) <= 0) return c.surface;
    return heatFill(c, heat->share(task.id));
}

QWidget *heatLegend(const Colors &c, const QString &metric, const Heat *heat, bool reading, const QString &error)
{
    auto *box = new QWidget; box->setObjectName("teamHeatLegend");
    auto *h = new QHBoxLayout(box); h->setContentsMargins(14, 0, 12, 6); h->setSpacing(8);
    const bool tokens = metric == QLatin1String("tokens");
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon(QStringLiteral("flame"), c.text2).pixmap(12, 12)); h->addWidget(icon);
    // Task 82: Time is the lateness against the plan; the overrun is its own badge.
    h->addWidget(label(tokens ? QObject::tr("Tokens lost: rework and long tests") : QObject::tr("Late against the plan: own lateness tints, inherited is muted (from #N)"),
                       c.text2, 11, false, "teamHeatMetric"));
    if (!error.isEmpty()) {
        auto *why = label(QObject::tr("The profile could not be read: %1").arg(error.section(QLatin1Char('\n'), 0, 0).left(160)), c.warn, 11, false, "teamHeatError");
        why->setToolTip(error); h->addWidget(why);
    } else if (!heat) {
        h->addWidget(label(reading ? QObject::tr("Reading the profile…") : QString(), c.dim, 11, false, "teamHeatReading"));
    } else {
        h->addWidget(label(QStringLiteral("0"), c.dim, 11));
        h->addWidget(new HeatScale(c));
        const qint64 most = heat->most();
        h->addWidget(label(tokens ? (most > 0 ? QObject::tr("%1, the most").arg(tokensText(most)) : QObject::tr("no tokens lost")) : QObject::tr("2h or more late"),
                           c.dim, 11, false, "teamHeatEnd"));
        if (!tokens) {
            h->addSpacing(10);
            h->addWidget(label(QObject::tr("Overrun, own work against the estimate:"), c.text2, 11, false, "teamHeatOverrunLegend"));
            for (const double ratio : {1.0, 1.5, 2.0}) {
                HeatOverrun sample; sample.started = true; sample.known = true; sample.ratio = ratio;
                const QColor tone = overrunColor(c, sample);
                auto *chip = label(ratio >= 2.0 ? QStringLiteral("x2+") : QStringLiteral("x%1").arg(QString::number(ratio, 'f', 1)), tone, 10, true, "teamHeatOverrunSample");
                chip->setStyleSheet(QStringLiteral("color:%1;border:1px solid %1;border-radius:4px;padding:0 4px;").arg(tone.name()));
                h->addWidget(chip);
            }
        }
        QStringList notes{QObject::tr("Tint only; the state icon and border stay.")};
        if (!heat->complete) notes << QObject::tr("The %1 largest losses.").arg(heat->pages * 100);
        if (tokens && heat->tokensPartial) notes << QObject::tr("Some tokens are unknown.");
        auto *note = label(notes.join(QLatin1Char(' ')), c.dim, 11, false, "teamHeatNote"); h->addSpacing(10); h->addWidget(note);
    }
    h->addStretch(1);
    return box;
}

}
