#pragma once
// WhiteWh fork: agent teams. Painted pieces shared by the Team tab, the task
// card and the team workspace: theme colors, role and status badges, the
// segmented progress and the task graph or layered list.
#include "AgentWorking.h"
#include "NewSessionDense.h"
#include "TeamModel.h"
#include "WorkspaceIcons.h"
#include "WorkspaceSkin.h"
#include "WorkspaceStyle.h"
#include <QCoreApplication>
#include <QCursor>
#include <QDateTime>
#include <QFrame>
#include <QHBoxLayout>
#include <QLabel>
#include <QMouseEvent>
#include <QMessageBox>
#include <QPainter>
#include <QPainterPath>
#include <QPointer>
#include <QPolygonF>
#include <QPushButton>
#include <QRegularExpression>
#include <QSet>
#include <QToolButton>
#include <QVBoxLayout>
#include <QKeyEvent>
#include <QTimer>
#include <algorithm>
#include <cmath>
#include <functional>
#include <tuple>

namespace TeamUi {

QIcon agentLogo(const QString &agent, const QColor &color);   // below, with agentIcon
using namespace Teams;

// U25: the prototype's design tokens of the selected skin (WorkspaceSkin::color).
// `card` is the field color team surfaces have always filled boxes with; the
// prototype's card color is `cardFill`.
struct Colors {
    QColor fg, text2, text3, muted, dim, accent, accentMuted, warn, danger, folder, surface, border, card, tile;
    QColor bg, label, hover, segOn, selAgent, cardFill, cardBorder, violet, accentText, accentHover;
    bool dark = true;
};

inline Colors colorsFor(bool dark)
{
    const auto &skin = WorkspaceSkin::active();
    const auto t = [&skin, dark](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)); };
    Colors c;
    c.dark = dark; c.fg = t("fg"); c.text2 = t("text2"); c.text3 = t("text3"); c.muted = t("muted"); c.dim = t("dim");
    c.accent = t("accent"); c.accentMuted = t("accentMuted"); c.warn = t("warn"); c.danger = t("danger"); c.folder = t("folder");
    c.surface = t("surface"); c.border = t("border"); c.card = t("field"); c.tile = t("tile");
    c.bg = t("bg"); c.label = t("label"); c.hover = t("hover"); c.segOn = t("segOn"); c.selAgent = t("selAgent");
    c.cardFill = t("card"); c.cardBorder = t("cardBorder"); c.violet = t("violet"); c.accentText = t("accentText"); c.accentHover = t("accentHover");
    return c;
}

inline QColor toneColor(const Colors &c, Tone tone)
{
    switch (tone) {
    case Tone::Active: return c.accent;
    case Tone::Assigned: return c.text3;
    case Tone::Done: return c.accentMuted;
    case Tone::Muted: return c.muted;
    case Tone::Dim: return c.dim;
    case Tone::Warn: return c.warn;
    case Tone::Danger: return c.danger;
    case Tone::Planning: return c.folder;
    case Tone::Neutral: break;
    }
    return c.text2;
}

// U25: a role's color. The color the catalog gives the role (Team designer)
// wins; then the skin's palette token role<Name> for the role or the palette
// role its name contains ("Lead (GPT)" is leadgpt, "backend-2" backend); any
// other role gets a stable palette color.
inline QColor roleColor(const QString &role, bool dark)
{
    const QString key = Teams::roleColorKey(role);
    if (const QColor own(Teams::catalogRoleColors().value(key)); own.isValid()) return own;
    const auto token = [dark](const QString &name) {
        return WorkspaceSkin::color(WorkspaceSkin::active(), dark, QStringLiteral("role") + name.left(1).toUpper() + name.mid(1));
    };
    const QStringList roles = WorkspaceSkin::paletteRoles();
    if (roles.contains(key)) return token(key);
    for (const auto &name : roles) if (key.contains(name)) return token(name);
    // Never a lead's color for a role that is not a lead.
    QStringList others = roles;
    others.removeAll(QStringLiteral("lead")); others.removeAll(QStringLiteral("leadgpt"));
    return token(others.at(int(qHash(key) % uint(others.size()))));
}

inline QColor alpha(QColor color, double a) { color.setAlphaF(a); return color; }

// A 12px status mark: icon and text always come together.
inline void glyph(QPainter &p, const QRectF &r, const QColor &color, const QString &kind)
{
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    const QRectF c = r.adjusted(1, 1, -1, -1);
    QPen pen(color, 1.3); pen.setCapStyle(Qt::RoundCap); p.setPen(pen); p.setBrush(Qt::NoBrush);
    const QPointF m = c.center(); const double s = c.width() / 2;
    if (kind == "accepted" || kind == "done") {
        p.setBrush(color); p.drawEllipse(c);
        p.setPen(QPen(QColor(Qt::white), 1.4, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin)); p.setBrush(Qt::NoBrush);
        QPainterPath tick; tick.moveTo(m + QPointF(-s * .45, 0)); tick.lineTo(m + QPointF(-s * .1, s * .35)); tick.lineTo(m + QPointF(s * .45, -s * .35)); p.drawPath(tick);
    } else if (kind == "draft" || kind == "not_created" || kind == "not_started") {
        pen.setStyle(Qt::DotLine); p.setPen(pen); p.drawEllipse(c);
    } else if (kind == "cancelled") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .6, s * .6), m + QPointF(s * .6, -s * .6));
    } else if (kind == "working" || kind == "in_progress" || kind == "running" || kind == "reviewing") {
        p.drawEllipse(c); p.setBrush(color); p.setPen(Qt::NoPen);
        QPolygonF play{m + QPointF(-s * .3, -s * .45), m + QPointF(s * .5, 0), m + QPointF(-s * .3, s * .45)}; p.drawPolygon(play);
    } else if (kind == "assigned") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .45, 0), m + QPointF(s * .45, 0));
        p.drawLine(m + QPointF(s * .1, -s * .35), m + QPointF(s * .45, 0)); p.drawLine(m + QPointF(s * .1, s * .35), m + QPointF(s * .45, 0));
    } else if (kind == "submitted") {
        p.drawEllipse(c); p.drawLine(m + QPointF(0, s * .45), m + QPointF(0, -s * .45));
        p.drawLine(m + QPointF(-s * .35, -s * .1), m + QPointF(0, -s * .45)); p.drawLine(m + QPointF(s * .35, -s * .1), m + QPointF(0, -s * .45));
    } else if (kind == "paused") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .25, -s * .35), m + QPointF(-s * .25, s * .35)); p.drawLine(m + QPointF(s * .25, -s * .35), m + QPointF(s * .25, s * .35));
    } else if (kind == "coordinating") {
        // Spec 5.6: a sub-lead between reviews - a hub with its reports.
        p.drawEllipse(QRectF(m.x() - s * .22, c.top(), s * .44, s * .44));
        p.drawLine(m + QPointF(0, -s * .05), m + QPointF(-s * .4, s * .45));
        p.drawLine(m + QPointF(0, -s * .05), m + QPointF(s * .4, s * .45));
    } else if (kind == "idle" || kind == "pending" || kind == "planning") {
        p.drawEllipse(c);
        if (kind == "idle") p.drawLine(m + QPointF(-s * .45, 0), m + QPointF(s * .45, 0));
    } else if (kind == "answer" || kind == "stuck") {
        p.drawEllipse(c);
        QFont f; f.setPixelSize(int(c.height() * .8)); f.setBold(true); p.setFont(f);
        p.drawText(c, Qt::AlignCenter, kind == "answer" ? QStringLiteral("?") : QStringLiteral("!"));
    } else if (kind == "sent") {
        // An outgoing arrow: the hand-off left for the member.
        p.drawLine(m + QPointF(-s * .45, s * .55), m + QPointF(s * .45, s * .55));
        p.drawLine(m + QPointF(-s * .45, 0), m + QPointF(s * .3, 0));
        p.drawLine(m + QPointF(s * .05, -s * .35), m + QPointF(s * .4, 0));
        p.drawLine(m + QPointF(s * .05, s * .35), m + QPointF(s * .4, 0));
    } else if (kind == "delivered" || kind == "late") {
        // An inbox: the task sits in the agent's terminal. "late" adds the dot
        // of a start that never came.
        QPainterPath tray;
        tray.moveTo(c.left() + s * .05, c.top() + s * .45);
        tray.lineTo(c.left() + s * .05, c.bottom() - s * .15);
        tray.lineTo(c.right() - s * .05, c.bottom() - s * .15);
        tray.lineTo(c.right() - s * .05, c.top() + s * .45);
        p.drawPath(tray);
        p.drawLine(QPointF(c.left() + s * .05, c.top() + s * .35), QPointF(c.right() - s * .05, c.top() + s * .35));
        if (kind == "late") {
            p.setPen(Qt::NoPen); p.setBrush(color);
            p.drawEllipse(QPointF(c.right() - s * .1, c.top() + s * .1), s * .3, s * .3);
        }
    } else if (kind == "asking") {
        // A speech bubble with the question it carries (A3).
        const QRectF bubble(c.left(), c.top() + s * .1, c.width(), c.height() - s * .4);
        p.drawRoundedRect(bubble, 2.5, 2.5);
        p.drawLine(QPointF(c.left() + s * .55, bubble.bottom()), QPointF(c.left() + s * .3, c.bottom() - s * .05));
        QFont f; f.setPixelSize(qMax(6, int(c.height() * .7))); f.setBold(true); p.setFont(f);
        p.drawText(bubble, Qt::AlignCenter, QStringLiteral("?"));
    } else if (kind == "held") {
        p.drawEllipse(c);
        p.drawLine(m + QPointF(-s * .22, -s * .35), m + QPointF(-s * .22, s * .35));
        p.drawLine(m + QPointF(s * .22, -s * .35), m + QPointF(s * .22, s * .35));
    } else if (kind == "reported" || kind == "no_report") {
        // A document: a check for a report, a cross for a turn that ended
        // without one.
        QPainterPath doc;
        doc.moveTo(c.left() + s * .25, c.top());
        doc.lineTo(c.right() - s * .55, c.top());
        doc.lineTo(c.right() - s * .15, c.top() + s * .4);
        doc.lineTo(c.right() - s * .15, c.bottom());
        doc.lineTo(c.left() + s * .25, c.bottom());
        doc.closeSubpath();
        p.drawPath(doc);
        p.drawLine(QPointF(c.right() - s * .55, c.top()), QPointF(c.right() - s * .55, c.top() + s * .4));
        p.drawLine(QPointF(c.right() - s * .55, c.top() + s * .4), QPointF(c.right() - s * .15, c.top() + s * .4));
        if (kind == "reported") {
            p.drawLine(m + QPointF(-s * .35, s * .1), m + QPointF(-s * .1, s * .35));
            p.drawLine(m + QPointF(-s * .1, s * .35), m + QPointF(s * .3, -s * .25));
        } else {
            p.drawLine(m + QPointF(-s * .3, -s * .2), m + QPointF(s * .2, s * .3));
            p.drawLine(m + QPointF(-s * .3, s * .3), m + QPointF(s * .2, -s * .2));
        }
    } else if (kind == "plan_free") {
        // U59: free planning, an infinity sign.
        QPainterPath loop;
        loop.moveTo(m);
        loop.cubicTo(m + QPointF(s * .45, -s * .75), m + QPointF(s * 1.05, -s * .45), m + QPointF(s, 0));
        loop.cubicTo(m + QPointF(s * 1.05, s * .45), m + QPointF(s * .45, s * .75), m);
        loop.cubicTo(m + QPointF(-s * .45, -s * .75), m + QPointF(-s * 1.05, -s * .45), m + QPointF(-s, 0));
        loop.cubicTo(m + QPointF(-s * 1.05, s * .45), m + QPointF(-s * .45, s * .75), m);
        p.drawPath(loop);
    } else if (kind == "plan_standard") {
        // U59: standard planning, phases as steps up to a goal.
        QPainterPath steps;
        steps.moveTo(c.left(), c.bottom());
        steps.lineTo(m.x() - s * .35, c.bottom()); steps.lineTo(m.x() - s * .35, m.y());
        steps.lineTo(m.x() + s * .3, m.y()); steps.lineTo(m.x() + s * .3, c.top() + s * .3);
        steps.lineTo(c.right(), c.top() + s * .3);
        p.drawPath(steps);
    } else if (kind == "stopped") {
        p.setPen(Qt::NoPen); p.setBrush(color);
        p.drawRoundedRect(QRectF(c.left() + s * .15, c.top() + s * .15, c.width() - s * .3, c.height() - s * .3), 1.5, 1.5);
    } else if (kind == "rejected") {
        // A curved arrow back to the member.
        QPainterPath back;
        back.moveTo(c.right() - s * .1, c.bottom() - s * .2);
        back.cubicTo(c.right() - s * .15, c.top() + s * .15, c.left() + s * .1, c.top() + s * .1, c.left() + s * .2, m.y());
        p.drawPath(back);
        p.drawLine(QPointF(c.left() + s * .2, m.y()), QPointF(c.left() + s * .5, m.y() - s * .35));
        p.drawLine(QPointF(c.left() + s * .2, m.y()), QPointF(c.left() + s * .5, m.y() + s * .35));
    } else if (kind == "in_review" || kind == "stage_review") {
        // U36: a review stage - a magnifier.
        p.drawEllipse(QRectF(c.left(), c.top(), s * 1.25, s * 1.25));
        p.drawLine(QPointF(c.left() + s * 1.1, c.top() + s * 1.1), QPointF(c.right(), c.bottom()));
    } else if (kind == "in_test" || kind == "stage_test") {
        // U36: a test stage - a flask.
        QPainterPath flask;
        flask.moveTo(m.x() - s * .3, c.top()); flask.lineTo(m.x() - s * .3, c.top() + s * .6);
        flask.lineTo(c.left() + s * .05, c.bottom()); flask.lineTo(c.right() - s * .05, c.bottom());
        flask.lineTo(m.x() + s * .3, c.top() + s * .6); flask.lineTo(m.x() + s * .3, c.top());
        p.drawPath(flask);
        p.drawLine(QPointF(m.x() - s * .45, c.top()), QPointF(m.x() + s * .45, c.top()));
        p.drawLine(QPointF(c.left() + s * .4, c.bottom() - s * .5), QPointF(c.right() - s * .4, c.bottom() - s * .5));
    } else if (kind == "stage_lead") {
        // U36: the lead's final acceptance - a crown.
        QPolygonF crown{QPointF(c.left(), c.top() + s * .4), QPointF(c.left() + s * .5, m.y()), QPointF(m.x(), c.top() + s * .2),
                        QPointF(c.right() - s * .5, m.y()), QPointF(c.right(), c.top() + s * .4), QPointF(c.right() - s * .2, c.bottom() - s * .3),
                        QPointF(c.left() + s * .2, c.bottom() - s * .3)};
        p.drawPolygon(crown);
    } else if (kind == "stage_auto") {
        // U36: accepted by the pipeline itself - a bolt.
        QPolygonF bolt{QPointF(m.x() + s * .2, c.top()), QPointF(c.left() + s * .3, m.y() + s * .15), QPointF(m.x(), m.y() + s * .15),
                       QPointF(m.x() - s * .2, c.bottom()), QPointF(c.right() - s * .3, m.y() - s * .15), QPointF(m.x(), m.y() - s * .15)};
        p.drawPolygon(bolt);
    } else if (kind == "folder_held") {
        // U36: a folder held by a test - a padlock.
        p.drawRoundedRect(QRectF(c.left() + s * .15, m.y() - s * .1, c.width() - s * .3, s * 1.1), 1.5, 1.5);
        QPainterPath shackle;
        shackle.moveTo(m.x() - s * .45, m.y() - s * .1);
        shackle.lineTo(m.x() - s * .45, c.top() + s * .45);
        shackle.arcTo(QRectF(m.x() - s * .45, c.top(), s * .9, s * .9), 180, -180);
        shackle.lineTo(m.x() + s * .45, m.y() - s * .1);
        p.drawPath(shackle);
    } else if (kind == "stage_skip") {
        // U36: a skipped stage - fast forward.
        p.drawPolygon(QPolygonF{QPointF(c.left(), c.top() + s * .2), QPointF(m.x(), m.y()), QPointF(c.left(), c.bottom() - s * .2)});
        p.drawPolygon(QPolygonF{QPointF(m.x(), c.top() + s * .2), QPointF(c.right(), m.y()), QPointF(m.x(), c.bottom() - s * .2)});
    } else if (kind == "offline" || kind == "unconfirmed" || kind == "failed") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .4, -s * .4), m + QPointF(s * .4, s * .4)); p.drawLine(m + QPointF(-s * .4, s * .4), m + QPointF(s * .4, -s * .4));
    } else {
        p.drawEllipse(c);
    }
    p.restore();
}

// A glyph as a pixmap or icon, for buttons and rows that need one.
inline QPixmap glyphPixmap(const QColor &color, const QString &kind, int size = 14, double ratio = 2.0)
{
    QPixmap pix(int(size * ratio), int(size * ratio));
    pix.setDevicePixelRatio(ratio);
    pix.fill(Qt::transparent);
    QPainter p(&pix);
    glyph(p, QRectF(0, 0, size, size), color, kind);
    return pix;
}

inline QIcon glyphIcon(const QColor &color, const QString &kind, int size = 14)
{
    return QIcon(glyphPixmap(color, kind, size));
}

// "just now", "3 min ago", "2 h ago": the time a phase tooltip names.
inline QString agoText(double then, double now)
{
    if (then <= 0 || now <= then + 1) return QCoreApplication::translate("TeamPanel", "just now");
    const int seconds = int(now - then);
    if (seconds < 60) return QCoreApplication::translate("TeamPanel", "%1 s ago").arg(seconds);
    if (seconds < 3600) return QCoreApplication::translate("TeamPanel", "%1 min ago").arg(seconds / 60);
    if (seconds < 86400) return QCoreApplication::translate("TeamPanel", "%1 h ago").arg(seconds / 3600);
    return QCoreApplication::translate("TeamPanel", "%1 d ago").arg(seconds / 86400);
}

// U37 (owner, 2026-10-28): how long a task has worked, in small text right
// after its activity label. The views read one clock and tick on one timer,
// once a minute; a test sets its own clock and ticks by hand.
inline std::function<double()> &workedClock()
{
    static std::function<double()> clock = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    return clock;
}

inline QTimer &workedTicker()
{
    static QTimer *timer = [] {
        auto *t = new QTimer(QCoreApplication::instance());
        t->setObjectName(QStringLiteral("teamWorkedTicker"));
        t->setInterval(60 * 1000);
        t->start();
        return t;
    }();
    return *timer;
}

// "<1m", "12m", "1h 05m".
inline QString workedText(qint64 seconds)
{
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    if (seconds < 60) return tr("<1m");
    const qint64 minutes = seconds / 60;
    if (minutes < 60) return tr("%1m").arg(minutes);
    return tr("%1h %2m").arg(minutes / 60).arg(minutes % 60, 2, 10, QLatin1Char('0'));
}

// The time since the running attempt started, else the sum of the attempts'
// work; empty while the task never worked.
inline QString workedLabel(const Task &task, double now)
{
    const auto &timing = task.timing;
    if (timing.runningSince > 0) return workedText(qint64(qMax(0.0, now - timing.runningSince)));
    return timing.worked > 0 ? workedText(timing.worked) : QString();
}

// The room a running time keeps: the widest text it grows to, so nothing
// after it moves when "9m" becomes "10m" or "1h 00m".
inline int workedRoom(const QFontMetrics &metrics, double runningSince)
{
    return runningSince > 0 ? metrics.horizontalAdvance(QStringLiteral("88h 88m")) : 0;
}

// Owner 2026-10-31 (#746): a task's time against its estimate, in minutes:
// "~45 min" before it works, "32 of ~45 min" while it works and after,
// "no estimate yet" without one; `over` once the time spent passed the
// estimate. An older hgs sends no estimate: the plain working time (U37).
struct TaskTime { QString text; bool over = false; };
inline TaskTime estimateTime(double estimate, bool known, qint64 spentSeconds, bool spent)
{
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    if (!known) return {spent ? workedText(spentSeconds) : QString(), false};
    const qint64 minutes = (spentSeconds + 30) / 60;
    if (estimate > 0) {
        const qint64 wanted = qRound64(estimate);
        if (!spent) return {tr("~%1 min").arg(wanted), false};
        return {tr("%1 of ~%2 min").arg(minutes).arg(wanted), minutes > wanted};
    }
    return {spent ? tr("%1 min, no estimate yet").arg(minutes) : tr("no estimate yet"), false};
}

// The time a task has spent: the running attempt's, else its work, else
// what hgs counted for the open attempt.
inline TaskTime taskTime(const Task &task, double now)
{
    const auto &timing = task.timing;
    qint64 seconds = 0;
    bool spent = true;
    if (timing.runningSince > 0) seconds = qint64(qMax(0.0, now - timing.runningSince));
    else if (timing.worked > 0) seconds = timing.worked;
    else if (task.estimateSpent >= 0) seconds = qint64(task.estimateSpent * 60);
    else spent = false;
    return estimateTime(task.estimateMinutes, task.estimateKnown(), seconds, spent);
}

// The room a running time against its estimate grows to: as many digits as
// the larger of its minutes and the estimate, so "9 of ~45" becoming "10 of
// ~45" moves nothing and a narrow node keeps its activity label.
inline int estimateRoom(const QFontMetrics &metrics, double estimate, bool known, double runningSince)
{
    if (runningSince <= 0) return 0;
    if (!known) return workedRoom(metrics, runningSince);
    const qint64 minutes = qMax<qint64>(qint64(qMax(0.0, workedClock()() - runningSince) / 60) + 1, qRound64(qMax(estimate, 0.0)));
    const QString widest = QString(QString::number(minutes).size(), QLatin1Char('8'));
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    return metrics.horizontalAdvance(estimate > 0 ? tr("%1 of ~%2 min").arg(widest).arg(qRound64(estimate))
                                                  : tr("%1 min, no estimate yet").arg(widest));
}

// The tooltip's line: the estimate and where it comes from.
inline QString estimateTip(const Task &task)
{
    if (!task.estimateKnown()) return {};
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    if (task.estimateMinutes <= 0) return tr("No estimate yet: it fills in after this role's first solved task");
    const QString from = task.estimateSource == "lead" ? tr("the lead's plan")
                         : task.estimateSource == "role" ? tr("this role's solved tasks") : tr("the team's solved tasks");
    return tr("Estimate ~%1 min, from %2").arg(qRound64(task.estimateMinutes)).arg(from);
}

// The tooltip: the total, each attempt's work and waits, the task's waits
// named separately, and what an older team never recorded.
inline QString workedTip(const Task &task, double now)
{
    const auto &timing = task.timing;
    if (!timing.known) return {};
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    const auto clock = [](double at) { return QDateTime::fromSecsSinceEpoch(qint64(at)).toString(QStringLiteral("HH:mm")); };
    const auto waits = [&](qint64 deps, qint64 member, qint64 lead) {
        QStringList parts;
        if (deps > 0) parts << tr("%1 for dependencies").arg(workedText(deps));
        if (member > 0) parts << tr("%1 for a free member").arg(workedText(member));
        if (lead > 0) parts << tr("%1 for the lead").arg(workedText(lead));
        return parts.join(QStringLiteral(" · "));
    };
    QStringList lines;
    if (timing.runningSince > 0)
        lines << tr("Working %1, since %2").arg(workedText(qint64(qMax(0.0, now - timing.runningSince))), clock(timing.runningSince));
    else if (timing.worked > 0)
        lines << (timing.attempts.size() > 1 ? tr("Worked %1 in %2 attempts").arg(workedText(timing.worked)).arg(timing.attempts.size())
                                             : tr("Worked %1").arg(workedText(timing.worked)));
    // Each attempt once there is more than one; the running one says since when.
    if (timing.attempts.size() > 1) {
        for (const auto &span : timing.attempts) {
            const bool running = timing.runningSince > 0 && span.to <= 0 && span.from == timing.runningSince;
            QString line = running ? tr("Attempt %1 (%2): working since %3").arg(span.n).arg(span.member, clock(span.from))
                                   : tr("Attempt %1 (%2, %3): worked %4").arg(span.n).arg(span.member, span.state, workedText(span.worked));
            const QString waited = waits(0, span.memberWait, span.leadWait);
            if (!waited.isEmpty()) line += tr(" · waited %1").arg(waited);
            lines << line;
        }
    }
    const QString waited = waits(timing.deps, timing.member, timing.lead);
    if (!waited.isEmpty()) lines << tr("Waited %1, not counted").arg(waited);
    QStringList unknown;
    for (const auto &kind : timing.unknown)
        unknown << (kind == QLatin1String("deps") ? tr("the wait for dependencies")
                    : kind == QLatin1String("member") ? tr("the wait for a member")
                    : kind == QLatin1String("lead") ? tr("the lead's decision")
                    : tr("the end of a replaced attempt"));
    if (!unknown.isEmpty()) lines << tr("Not recorded: %1").arg(unknown.join(QStringLiteral(", ")));
    return lines.join(QLatin1Char('\n'));
}

// An attempt phase as the desktop draws it (A1, spec 11): the glyph kind, its
// tone, and a tooltip naming the state, the time and the next step. `phase` is
// empty when the task has no attempt.
struct PhaseMark {
    QString phase, kind, tooltip;
    Tone tone = Tone::Neutral;
};

inline PhaseMark phaseMark(const Team &team, const Task &task, double now = 0)
{
    PhaseMark mark;
    mark.phase = lastPhase(task);
    mark.kind = mark.phase;
    if (mark.phase.isEmpty()) return mark;
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    if (now <= 0) now = double(QDateTime::currentSecsSinceEpoch());
    const auto *attempt = task.last();
    const QString role = team.assigneeRole(task, tr("You"));
    const double delivered = attempt ? (attempt->deliveredAt > 0 ? attempt->deliveredAt : attempt->sentAt) : 0;
    if (mark.phase == "sent") {
        mark.tone = Tone::Assigned; mark.tooltip = tr("Sending to %1").arg(role);
    } else if (mark.phase == "delivered") {
        mark.tone = Tone::Assigned; mark.tooltip = tr("Delivered %1, not started yet").arg(agoText(delivered, now));
    } else if (mark.phase == "late") {
        mark.tone = Tone::Warn; mark.tooltip = tr("Delivered %1 and not started: check its terminal").arg(agoText(delivered, now));
    } else if (mark.phase == "working") {
        mark.tone = Tone::Active;
        mark.tooltip = attempt && attempt->startedAt > 0 ? tr("%1 started %2").arg(role, agoText(attempt->startedAt, now))
                                                         : tr("%1 is working on it").arg(role);
    } else if (mark.phase == "asking") {
        mark.tone = Tone::Warn;
        const auto *question = attempt ? attempt->openQuestion() : nullptr;
        mark.tooltip = question ? tr("%1 asks: \"%2\" (Answer)").arg(role, question->text)
                                : tr("%1 asks a question (Answer)").arg(role);
    } else if (mark.phase == "held") {
        mark.tone = Tone::Muted;
        mark.tooltip = task.hold.by.isEmpty() ? tr("Held: waiting for the owner")
                      : task.hold.reason.isEmpty() ? tr("Held by %1: waiting for the owner").arg(task.hold.by)
                                                   : tr("Held by %1: %2").arg(task.hold.by, task.hold.reason);
    } else if (mark.phase == "reported" && task.inStage()) {
        // U36: the report is in a stage: its glyph and round.
        const auto *stage = task.pipelineRun.openRun();
        mark.kind = task.status; mark.tone = Tone::Active;
        mark.tooltip = stage ? tr("%1 round %2 of %3, after %4's report").arg(stageName(stage->kind)).arg(stage->round).arg(task.pipelineRun.rounds()).arg(role)
                             : tr("Report from %1 in its pipeline").arg(role);
    } else if (mark.phase == "reported") {
        mark.tone = Tone::Done;
        mark.tooltip = !attempt || attempt->reportAt <= 0 ? tr("Report from %1").arg(role)
                                                          : tr("Report from %1, %2").arg(role, agoText(attempt->reportAt, now));
    } else if (mark.phase == "no_report") {
        mark.tone = Tone::Warn; mark.tooltip = tr("Turn ended without a report");
    } else if (mark.phase == "stopped") {
        mark.tone = Tone::Danger; mark.tooltip = tr("%1's session stopped").arg(role);
    } else if (mark.phase == "rejected") {
        mark.tone = Tone::Danger; mark.tooltip = tr("Rejected: returned to %1").arg(role);
    } else if (mark.phase == "unconfirmed") {
        mark.tone = Tone::Danger; mark.tooltip = tr("Delivery unconfirmed");
    }
    return mark;
}

// One row of a member's queue (A5): the task's phase icon, its number and
// title, and the phase's tooltip. The current task's row comes first.
inline QPushButton *memberQueueRow(const Team &team, const Task &task, const Colors &c, bool current)
{
    const auto mark = phaseMark(team, task);
    auto *row = new QPushButton;
    row->setObjectName("teamMemberQueueRow");
    row->setProperty("task", task.id);
    row->setProperty("current", current);
    row->setProperty("phase", mark.phase);
    row->setIcon(glyphIcon(toneColor(c, mark.tone == Tone::Neutral ? Tone::Muted : mark.tone), mark.kind, 13));
    row->setIconSize(QSize(13, 13));
    row->setText(QStringLiteral("#%1 %2").arg(task.id).arg(task.title));
    row->setFlat(true);
    row->setCursor(Qt::PointingHandCursor);
    row->setFixedHeight(18);
    row->setToolTip(mark.tooltip);
    if (!mark.tooltip.isEmpty()) row->setAccessibleName(row->text() + QStringLiteral(" - ") + mark.tooltip);
    row->setStyleSheet(QStringLiteral("QPushButton{text-align:left;font-size:11px;color:%1;border:none;padding:0 2px;}"
                                      "QPushButton:hover{color:%2;}").arg(current ? c.text2.name() : c.muted.name(), c.fg.name()));
    return row;
}

// U29: a task added to its phase after the phase started: who added it, when,
// and the owner's request and confirmation.
inline QString addedLateTip(const Task &task)
{
    if (!task.addedLate.known) return {};
    const auto &late = task.addedLate;
    QString tip = QCoreApplication::translate("TeamPanel", "Out of the phase's original scope: added by %1, %2")
                      .arg(late.by.isEmpty() ? QCoreApplication::translate("TeamPanel", "the owner") : late.by,
                           QDateTime::fromSecsSinceEpoch(qint64(late.at)).toLocalTime().toString(QStringLiteral("d MMM HH:mm")));
    if (!late.request.isEmpty()) tip += QLatin1Char('\n') + QCoreApplication::translate("TeamPanel", "Asked: “%1”").arg(late.request);
    if (!late.confirm.isEmpty()) tip += QLatin1Char('\n') + QCoreApplication::translate("TeamPanel", "Confirmed: “%1”").arg(late.confirm);
    return tip;
}

// U29: the "out of the phase's original scope" icon with its tooltip.
inline QLabel *outOfScopeMark(const Task &task, const Colors &c, int size = 14)
{
    auto *mark = new QLabel; mark->setObjectName("teamOutOfScope"); mark->setProperty("task", task.id);
    mark->setPixmap(workspaceIcon("out-of-scope", c.violet).pixmap(size, size));
    mark->setFixedSize(size, size); mark->setToolTip(addedLateTip(task)); mark->setAccessibleName(addedLateTip(task));
    return mark;
}

inline QString kindOfTask(const Task &task, const Status &status)
{
    // A finished task keeps its own mark (accepted, cancelled or rejected): hgs
    // sends no attempt phase for one on purpose (N4).
    if (task.finished()) return task.status;
    // U36: a task in a stage shows the stage (a magnifier or a flask).
    if (task.inStage()) return task.status;
    // A live task shows the attempt's phase; one hgs did not send is derived
    // (Teams::lastPhase).
    const QString phase = lastPhase(task);
    if (!phase.isEmpty()) return phase;
    if (status.tone == Tone::Danger) return QStringLiteral("unconfirmed");
    if (status.tone == Tone::Warn) return QStringLiteral("no_report");
    return task.status;
}

// The summary node of a collapsed phase: a finished phase says so, and both
// kinds stay clickable.
inline QString phaseSummaryText(int done, int total)
{
    if (total > 0 && done == total) return QCoreApplication::translate("TeamPanel", "%1/%2 done · click to open").arg(done).arg(total);
    return QCoreApplication::translate("TeamPanel", "%1/%2 · click to open").arg(done).arg(total);
}

// A pill: role badges and the team badge. Text, border and fill share a color.
class Pill : public QWidget {
public:
    Pill(const QString &text, const QColor &color, const QString &kind = {}, bool filled = true, QWidget *parent = nullptr)
        : QWidget(parent), m_text(text), m_color(color), m_kind(kind), m_filled(filled)
    {
        QFont f = font(); f.setPixelSize(11); f.setWeight(QFont::Medium); setFont(f);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
        setToolTip(text);
    }
    QSize sizeHint() const override { return {fontMetrics().horizontalAdvance(m_text) + 14 + (m_kind.isEmpty() ? 0 : 14), 18}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const QRectF r = QRectF(rect()).adjusted(.5, .5, -.5, -.5);
        p.setPen(QPen(alpha(m_color, m_filled ? .45 : 1), 1)); p.setBrush(m_filled ? alpha(m_color, .12) : Qt::transparent);
        p.drawRoundedRect(r, 9, 9);
        int x = 7;
        if (!m_kind.isEmpty()) { glyph(p, QRectF(6, 4, 10, 10), m_color, m_kind); x = 20; }
        p.setPen(m_color); p.drawText(QRect(x, 0, width() - x - 6, height()), Qt::AlignVCenter | Qt::AlignLeft,
                                      fontMetrics().elidedText(m_text, Qt::ElideRight, width() - x - 6));
    }
private:
    QString m_text; QColor m_color; QString m_kind; bool m_filled;
};

// A status: mark and text, elided; a fixed width keeps neighbours in place.
// `tooltip` names the state, the time and the next step when the text alone
// does not (the attempt phases of the delivery spec).
class StatusMark : public QWidget {
public:
    StatusMark(const QString &text, const QColor &color, const QString &kind, int fixedWidth = 0, Qt::Alignment align = Qt::AlignLeft,
               QWidget *parent = nullptr, const QString &tooltip = {})
        : QWidget(parent), m_text(text), m_color(color), m_kind(kind), m_align(align)
    {
        QFont f = font(); f.setPixelSize(12); setFont(f);
        if (fixedWidth) setFixedWidth(fixedWidth); else setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        setFixedHeight(18);
        const QString tip = tooltip.isEmpty() ? text : text.isEmpty() ? tooltip : tooltip + QLatin1Char('\n') + text;
        setToolTip(tip);
    }
    QSize sizeHint() const override { return {fontMetrics().horizontalAdvance(m_text) + 18, 18}; }
    QString text() const { return m_text; }
    // #706: a member's live state changes its mark in place.
    void setState(const QString &text, const QColor &color, const QString &kind, const QString &tooltip = {})
    {
        if (text == m_text && color == m_color && kind == m_kind) return;
        m_text = text; m_color = color; m_kind = kind;
        setToolTip(tooltip.isEmpty() ? text : text.isEmpty() ? tooltip : tooltip + QLatin1Char('\n') + text);
        updateGeometry(); update();
    }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        const QString text = fontMetrics().elidedText(m_text, Qt::ElideRight, width() - 18);
        const int w = fontMetrics().horizontalAdvance(text) + 17;
        const int x = m_align & Qt::AlignRight ? width() - w : 0;
        glyph(p, QRectF(x, 3, 12, 12), m_color, m_kind);
        p.setPen(m_color); p.drawText(QRect(x + 17, 0, width() - x - 17, height()), Qt::AlignVCenter | Qt::AlignLeft, text);
    }
private:
    QString m_text; QColor m_color; QString m_kind; Qt::Alignment m_align;
};

class Progress : public QWidget {
public:
    Progress(const QList<QColor> &segments, int height, const QColor &empty, QWidget *parent = nullptr)
        : QWidget(parent), m_segments(segments), m_empty(empty) { setFixedHeight(height); }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing); p.setPen(Qt::NoPen);
        if (m_segments.isEmpty()) { p.setBrush(m_empty); p.drawRoundedRect(rect(), 2, 2); return; }
        const double gap = 2, w = (width() - gap * (m_segments.size() - 1)) / m_segments.size();
        for (int i = 0; i < m_segments.size(); ++i) {
            p.setBrush(m_segments[i]); p.drawRoundedRect(QRectF(i * (w + gap), 0, qMax(1.0, w), height()), 1.5, 1.5);
        }
    }
private:
    QList<QColor> m_segments; QColor m_empty;
};

// G6: the two compact bars that replace one bar over every task: the phase in
// work (or the next draft waiting for approval) with its done/total, and the
// phases with finished/planned, where a discarded draft never counts. The
// caption carries the live phase number; both bars share one tooltip.
class PhaseProgressBars : public QWidget {
public:
    PhaseProgressBars(const Colors &colors, const Teams::PhaseProgress &progress, bool compact = false, QWidget *parent = nullptr)
        : QWidget(parent), m_c(colors), m_p(progress), m_compact(compact)
    {
        setObjectName("teamPhaseProgress");
        setProperty("phase", progress.phase);
        setProperty("phaseDone", progress.done);
        setProperty("phaseTotal", progress.total);
        setProperty("phaseDraft", progress.draft);
        setProperty("phasesDone", progress.phasesDone);
        setProperty("phasesTotal", progress.phasesTotal);
        setToolTip(progress.tooltip());
        setFixedHeight(compact ? 16 : 18);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
        setFixedWidth((compact ? 62 : 92) * 2 + (compact ? 8 : 10));
    }
    QSize sizeHint() const override { return {width(), height()}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        if (!m_p.valid()) return;
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const int blockWidth = m_compact ? 62 : 92, gap = m_compact ? 8 : 10;
        const auto block = [&](int x, const QString &caption, const QString &count, int done, int total) {
            QFont f = font(); f.setPixelSize(9); f.setWeight(QFont::DemiBold); p.setFont(f);
            p.setPen(m_c.dim);
            p.drawText(QRect(x, 0, blockWidth, 10), Qt::AlignLeft | Qt::AlignVCenter, caption);
            QFont mono = font(); mono.setPixelSize(9); p.setFont(mono); p.setPen(m_c.text2);
            p.drawText(QRect(x, 0, blockWidth, 10), Qt::AlignRight | Qt::AlignVCenter, count);
            const QRectF track(x, 12, blockWidth, 4);
            p.setPen(Qt::NoPen); p.setBrush(m_c.border); p.drawRoundedRect(track, 2, 2);
            const double fraction = total > 0 ? qBound(0.0, done / double(total), 1.0) : 0.0;
            if (fraction > 0) { p.setBrush(m_c.accent); p.drawRoundedRect(QRectF(x, 12, blockWidth * fraction, 4), 2, 2); }
        };
        const QString phaseCaption = m_p.draft ? tr("Phase %1 · draft").arg(m_p.phase) : tr("Phase %1").arg(m_p.phase);
        block(0, phaseCaption, m_p.phaseText(), m_p.done, m_p.total);
        block(blockWidth + gap, tr("Phases"), m_p.phasesText(), m_p.phasesDone, m_p.phasesTotal);
    }
private:
    Colors m_c; Teams::PhaseProgress m_p; bool m_compact;
};

// U25 (spec 3): one of the lead header's three mini progress bars (the
// prototype's .prog2): the caption at 9.5 px and the count above a 4 px bar.
class MiniProgress : public QWidget {
public:
    MiniProgress(const Colors &colors, const QString &caption, int done, int total, const QColor &fill, int width, QWidget *parent = nullptr)
        : QWidget(parent), m_c(colors), m_caption(caption), m_done(done), m_total(total), m_fill(fill)
    {
        setFixedSize(width, 16);
        setProperty("done", done); setProperty("total", total); setProperty("caption", caption);
        setAccessibleName(QStringLiteral("%1 %2/%3").arg(caption).arg(done).arg(total));
    }
    QString countText() const { return QStringLiteral("%1/%2").arg(m_done).arg(m_total); }
    QSize sizeHint() const override { return size(); }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        QFont mono(QStringLiteral("Cascadia Mono")); mono.setStyleHint(QFont::Monospace); mono.setPixelSize(10); mono.setWeight(QFont::DemiBold);
        const int countWidth = QFontMetrics(mono).horizontalAdvance(countText());
        p.setFont(mono); p.setPen(m_c.text3);
        p.drawText(QRect(width() - countWidth, 0, countWidth, 10), Qt::AlignRight | Qt::AlignVCenter, countText());
        QFont f = font(); f.setPixelSize(10); p.setFont(f); p.setPen(m_c.dim);
        const int room = width() - countWidth - 6;
        p.drawText(QRect(0, 0, room, 10), Qt::AlignLeft | Qt::AlignVCenter, QFontMetrics(f).elidedText(m_caption, Qt::ElideRight, room));
        const QRectF track(0, 12, width(), 4);
        p.setPen(Qt::NoPen); p.setBrush(m_c.border); p.drawRoundedRect(track, 2, 2);
        const double fraction = m_total > 0 ? qBound(0.0, m_done / double(m_total), 1.0) : 0.0;
        if (fraction > 0) { p.setBrush(m_fill); p.drawRoundedRect(QRectF(0, 12, width() * fraction, 4), 2, 2); }
    }
private:
    Colors m_c; QString m_caption; int m_done, m_total; QColor m_fill;
};

// U25: a row that hides its least important items when it is narrower than
// they need, instead of squeezing them (the lead header's team progress). An
// item of priority 0 always shows; higher numbers go first. Its size hint
// counts every item, so hiding never changes it.
class FitRow : public QWidget {
public:
    explicit FitRow(int gap, QWidget *parent = nullptr) : QWidget(parent), m_gap(gap) { setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Fixed); }
    void add(QWidget *item, int priority)
    {
        item->setParent(this); m_items.append({item, priority}); item->show();
        updateGeometry(); place();
    }
    QSize sizeHint() const override { return measure(false); }
    QSize minimumSizeHint() const override { return measure(true); }
protected:
    void resizeEvent(QResizeEvent *event) override { QWidget::resizeEvent(event); place(); }
private:
    struct Item { QPointer<QWidget> widget; int priority = 0; };
    static QSize hintOf(const QWidget *widget) { return widget->sizeHint().expandedTo(widget->minimumSize()).expandedTo(QSize(0, 0)); }
    QSize measure(bool required) const
    {
        int w = 0, h = 0, n = 0;
        for (const auto &item : m_items) {
            if (!item.widget || (required && item.priority)) continue;
            const auto hint = hintOf(item.widget); w += hint.width(); h = qMax(h, hint.height()); ++n;
        }
        return {w + qMax(0, n - 1) * m_gap, qMax(h, 24)};
    }
    void place()
    {
        // Keep items by priority while they fit, then lay the kept ones out in order.
        QList<int> order; for (int i = 0; i < m_items.size(); ++i) if (m_items[i].widget) order << i;
        std::stable_sort(order.begin(), order.end(), [this](int a, int b) { return m_items[a].priority < m_items[b].priority; });
        QSet<int> kept; int used = 0;
        for (int i : order) {
            const int w = hintOf(m_items[i].widget).width() + (kept.isEmpty() ? 0 : m_gap);
            if (m_items[i].priority && used + w > width()) continue;
            kept.insert(i); used += w;
        }
        int x = 0;
        for (int i = 0; i < m_items.size(); ++i) {
            auto *widget = m_items[i].widget.data();
            if (!widget) continue;
            widget->setVisible(kept.contains(i));
            if (!kept.contains(i)) continue;
            const auto hint = hintOf(widget);
            widget->setGeometry(x, (height() - hint.height()) / 2, hint.width(), hint.height());
            x += hint.width() + m_gap;
        }
    }
    int m_gap;
    QList<Item> m_items;
};

// Tasks as a graph or as a list by layer. Painted, so hovering a task lights
// its chain without moving anything. Graph runs top to bottom (the inspector);
// Wide runs left to right with layer columns (the team workspace). While
// planning, a node's handle drags onto another node to add a dependency, and a
// selected edge is removed with Delete or its ×.
// G9: the status chip of a phase header, from status.phase_summary.
inline QString phaseStatusText(const QString &status)
{
    if (status == "done") return QCoreApplication::translate("TeamPanel", "Done");
    if (status == "in_work") return QCoreApplication::translate("TeamPanel", "In work");
    if (status == "approved") return QCoreApplication::translate("TeamPanel", "Approved (not started)");
    if (status == "draft") return QCoreApplication::translate("TeamPanel", "Draft");
    if (status == "paused") return QCoreApplication::translate("TeamPanel", "Paused");
    return status;
}

inline QColor phaseStatusColor(const Colors &c, const QString &status)
{
    if (status == "done") return c.accent;
    if (status == "in_work") return c.folder;
    if (status == "approved") return c.muted;
    if (status == "draft") return c.warn;
    if (status == "paused") return c.dim;
    return c.muted;
}

class TaskView : public QWidget {
public:
    enum Mode { List, Graph, Wide };
    struct Node { int id = 0, layer = 0, phase = 1; QString title, role, kind, statusText, deps, agent, tooltip, subteam; QColor tone, roleTone; bool problem = false, draft = false, error = false, match = true; int attempt = 0;
        // U25 part 5 (spec 5.4): "↤#2" - only the dependencies from other
        // phases (the ones inside it are edges); the problem text on the
        // second line; whether the assignee's agent really works on it. The
        // owner's 2026-10-27 addition: a small activity label ("review",
        // "attempt 2", "held", "sub-team 1/2") right of the role chip and the
        // model tile; a problem text wins over it.
        QString ext, problemText, activity; bool working = false;
        // U29: added to its running phase, outside the original scope.
        bool late = false;
        // U37: the working time after the activity label; a running one
        // ticks from `runningSince` on the shared minute timer.
        QString worked; double runningSince = 0;
        // #746: the estimate the time is measured against (known: hgs sent
        // one, or "none"); over it, the time takes the warning color.
        double estimate = -1; bool estimateKnown = false, timeOver = false;
        // U38: the heat tint's fill and the loss at the second line's end
        // (it takes the working time's place).
        QColor heatFill, heatColor; QString heat; };
    // A phase group as the plan, board and layers show it: the heading is
    // "Phase 2 · QA fixes · 3/5". A finished phase starts collapsed; its tasks
    // are in `tasks` but no node is built, and one summary node stands in.
    // G9: the header also carries the phase's status, progress and health from
    // status.phase_summary/phases_health, and a draft's Approve/Discard.
    struct PhaseBand {
        int n = 0; QString heading, title; bool barrier = false, draft = false, collapsed = false; int done = 0, total = 0; QList<int> tasks;
        QString status;       // done|in_work|approved|draft|paused (empty: older hgs)
        QString healthLine;   // "path 8 · par 2.4 · 1 warning"
        QString healthTip;
        QString blocked;      // F12: why Approve is disabled, empty when ready
        QString blockedTip;
        bool alongside = false;   // F12/N40: Approve runs it beside a running phase
        int changes = 0;      // the lead's pending edits on a draft
        QStringList changeLines;   // those edits as a diff, for the header's tooltip
        bool autoDraft = false, startAhead = false;   // U61: Auto accepts it at its first hand-out
    };
    TaskView(const Colors &colors, Mode mode, QWidget *parent = nullptr) : QWidget(parent), m_c(colors), m_mode(mode)
    {
        setMouseTracking(true);
        setFocusPolicy(Qt::ClickFocus);
        QFont f = font(); f.setPixelSize(12); setFont(f);
        // Spec 1.4: the working dots under a model tile move while animated.
        m_dots.setInterval(AgentWorking::FrameMs);
        QObject::connect(&m_dots, &QTimer::timeout, this, [this] { update(); });
        // U37: a running time ticks on the shared minute timer: the texts
        // change and the view repaints once, no node is rebuilt.
        QObject::connect(&workedTicker(), &QTimer::timeout, this, [this] { tickWorked(); });
    }
    void tickWorked()
    {
        const double now = workedClock()();
        bool changed = false;
        for (auto &n : nodes) {
            if (n.runningSince <= 0) continue;
            const auto time = estimateTime(n.estimate, n.estimateKnown, qint64(qMax(0.0, now - n.runningSince)), true);
            changed = changed || time.text != n.worked || time.over != n.timeOver;
            n.worked = time.text; n.timeOver = time.over;
        }
        if (changed) update();
    }
    // #706: a member's live state changes its nodes' activity labels and
    // working dots in place; the layout stays.
    void refreshLive(const std::function<void(Node &)> &apply)
    {
        bool working = false;
        for (auto &n : nodes) { apply(n); working = working || n.working; }
        if (m_mode == Wide && working && AgentWorking::animated()) m_dots.start(); else m_dots.stop();
        update();
    }
    // Compatibility with the two-mode form.
    TaskView(const Colors &colors, bool graph, QWidget *parent = nullptr) : TaskView(colors, graph ? Graph : List, parent) {}
    std::function<void(int task, const QPoint &global)> clicked;
    // Planning: `task` gains a dependency on `dependency`, or loses it.
    std::function<void(int dependency, int task)> linked, unlinked;
    bool editable = false;
    // U25 part 5: in a running team only these tasks (a draft phase's) take
    // or lose a dependency; `editable` opens the whole plan.
    QSet<int> editableTasks;
    bool canEdit(int task) const { return editable || editableTasks.contains(task); }
    // U25 part 5 (spec 5.4): an edge's orthogonal route in the Wide graph, its
    // corner points from the dependency's right side to just before the
    // task's left side, and where its middle (the remove ×) is.
    struct Route { QPolygonF points; QPointF mid; };
    Route route(int dependency, int task) const { return m_routes.value({dependency, task}); }
    bool routed(int dependency, int task) const { return m_routes.contains({dependency, task}); }
    // The task a click on a node's handle started to connect from (0: none).
    int connecting() const { return m_connectFrom; }
    // #465 N18: every render builds a new view, so the owner's half-made link
    // and selected edge live with the caller: `editChanged` reports them, and
    // keepEdit() puts them back at the next layout() while the task and the
    // edge are still there.
    std::function<void(int connectFrom, QPair<int, int> edge)> editChanged;
    void keepEdit(int connectFrom, QPair<int, int> edge) { m_keepFrom = connectFrom; m_keepEdge = edge; m_keep = true; }
    // #570: the task under a resting pointer stays lit through a poll. The
    // view says when its hover changes; keepHover() lights that task again at
    // the next layout() while it is still in the view.
    std::function<void(int from, int to)> hoverChanged;
    void keepHover(int task) { m_keepHover = task; }
    int hovered() const { return m_hover; }
    QList<Node> nodes;
    // Phase grouping: empty while the plan is phase 1 only. The owner's
    // toggles call phaseToggled; the caller re-renders with the new collapse.
    QList<PhaseBand> bands;
    std::function<void(int phase)> phaseToggled;
    // G9: the draft decisions from the band header, and the F10 reorder. A
    // decision returns true only when it was really sent (the owner confirmed),
    // so a cancelled confirmation leaves the header usable.
    std::function<bool(int phase)> phaseApprove, phaseDiscard;
    std::function<void(int phase)> phaseMoveUp, phaseMoveDown;
    // G9: a decision goes out once; the header control stays off until a new
    // render builds a new view.
    QSet<int> decided;
    // The painted place of a band's header controls, for hit tests and tests.
    QRectF approveRect(int phase) const { return m_approve.value(phase); }
    QRectF discardRect(int phase) const { return m_discard.value(phase); }
    QRectF moveUpRect(int phase) const { return m_moveUp.value(phase); }
    QRectF moveDownRect(int phase) const { return m_moveDown.value(phase); }
    QString bandTooltip(int phase) const { return m_bandTips.value(phase); }
    const PhaseBand *bandOf(int phase) const
    {
        for (const auto &band : bands) if (band.n == phase) return &band;
        return nullptr;
    }
    // The painted place of a phase header or of a collapsed phase's summary
    // node, in the view's own (unzoomed) coordinates.
    QRectF bandRect(int phase) const { return m_bandRects.value(phase); }
    QRectF summaryRect(int phase) const { return m_summaries.value(phase); }
    // G3: where a task landed; a dead-end sits below the tasks others need.
    QRectF nodeRect(int id) const { return m_rects.value(id); }
    QMap<int, QList<int>> edges;   // task -> its dependencies
    QSet<QPair<int, int>> soft;    // (dependency, task) edges that need only In progress
    QSet<int> cycle, emphasis;     // emphasis: Show what blocks
    std::function<QSet<int>(int)> chain;
    double zoom = 1.0;
    // Wide: the visible canvas; the graph centers in it and its layer bands
    // fill its height (design D3, no empty bottom).
    QSize area;
    // G4: one entry per layer inside a phase band, with the small "L2 · 3"
    // label rect and the count; the paint draws its stripe and label from this.
    struct BandLayer { int phase = 0, layer = 0, count = 0; QRectF rect; };
    // Owner, 2026-10-28: a layer is not always one column. A layer with many
    // tasks wraps into columns of at most `rows` tasks: as tall as the
    // tallest other layer, at least WrapRows, and about the square root of
    // twice its size, so 13 tasks beside layers of 1-2 take 3 columns of 5.
    // `start` is the first column of the layer: a later layer moves right by
    // the extra columns of the earlier ones.
    struct LayerSpread { int start = 0, span = 1, rows = 1, count = 0; };
    static constexpr int WrapRows = 6;
    static QMap<int, LayerSpread> layerColumns(const QMap<int, int> &counts)
    {
        int tallest = 0, second = 0;
        for (int count : counts) {
            if (count > tallest) { second = tallest; tallest = count; }
            else second = qMax(second, count);
        }
        const int cap = qMax(WrapRows, qMax(second, int(std::ceil(std::sqrt(2.0 * tallest)))));
        QMap<int, LayerSpread> spreads;
        int extra = 0;
        for (auto it = counts.cbegin(); it != counts.cend(); ++it) {
            LayerSpread spread;
            spread.count = it.value();
            spread.span = qMax(1, (it.value() + cap - 1) / cap);
            spread.rows = qMax(1, (it.value() + spread.span - 1) / spread.span);
            spread.start = it.key() + extra;
            extra += spread.span - 1;
            spreads.insert(it.key(), spread);
        }
        return spreads;
    }
    // The columns a group of tasks takes on screen, from each task's layer.
    static int wideColumns(const QMap<int, int> &layerOfTask)
    {
        QMap<int, int> counts;
        for (int layer : layerOfTask) ++counts[layer];
        int columns = 1;
        for (const auto &spread : layerColumns(counts)) columns = qMax(columns, spread.start + spread.span);
        return columns;
    }
    QList<BandLayer> bandLayers() const { return m_bandLayers; }
    // Wide: the graph's own size at zoom 1.
    QSizeF natural() const
    {
        if (banded() && m_mode == Wide) {
            double wide = BandMin;
            int tall = 8;
            for (const auto &band : bands) {
                int rows = 0;
                for (const auto &spread : layerColumns(layerCounts(band.tasks))) rows = qMax(rows, spread.rows);
                wide = qMax(wide, double(bandWidth(band)));
                tall += BandHead + (band.collapsed ? 0 : bandBody(rows)) + BandGap;
            }
            return {wide + 8, double(tall + 2)};
        }
        QList<int> all;
        for (const auto &n : nodes) all.append(n.id);
        int columns = 0, tall = 0;
        for (const auto &spread : layerColumns(layerCounts(all))) { columns = qMax(columns, spread.start + spread.span); tall = qMax(tall, spread.rows); }
        return {double(WideX + (qMax(1, columns) - 1) * WideStep + WideW + 16), double(WideTop + (qMax(1, tall) - 1) * WideRow + WideH + WideUnder)};
    }
    void layout()
    {
        m_rects.clear(); m_bandRects.clear(); m_summaries.clear(); m_bandBackgrounds.clear(); m_listLayers.clear(); m_bandLayers.clear();
        m_approve.clear(); m_discard.clear(); m_moveUp.clear(); m_moveDown.clear();
        m_routes.clear(); m_groupOf.clear(); m_groups.clear(); m_columnOf.clear();
        // G3: inside a layer (and inside a phase band) a task other tasks depend
        // on comes first, in the existing order; a dead-end task — nothing
        // depends on it — sinks to the bottom of its column.
        QSet<int> needed;
        for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) needed.insert(dep);
        if (!needed.isEmpty()) {
            std::stable_partition(nodes.begin(), nodes.end(), [&needed](const Node &n) { return needed.contains(n.id); });
            for (auto &band : bands)
                std::stable_partition(band.tasks.begin(), band.tasks.end(), [&needed](int id) { return needed.contains(id); });
        }
        QMap<int, int> column;
        if (m_mode == Graph) {
            if (banded()) {
                int y = 4, wide = Gutter + NodeW + 4;
                for (const auto &band : bands) {
                    QMap<int, int> used;
                    int layers = 0, bandWide = Gutter + NodeW + 4;
                    for (int id : band.tasks) {
                        const Node *node = nodeOf(id);
                        if (!node) continue;
                        layers = qMax(layers, node->layer + 1);
                        const int x = Gutter + used[node->layer]++ * (NodeW + 6);
                        m_rects.insert(id, QRectF(x, y + BandTop + node->layer * (NodeH + 26), NodeW, NodeH));
                        bandWide = qMax(bandWide, x + NodeW + 4);
                    }
                    m_bandRects.insert(band.n, QRectF(0, y, bandWide, BandHead));
                    m_bandBackgrounds.append(QRectF(0, y, bandWide, 0));
                    if (band.collapsed) { m_summaries.insert(band.n, QRectF(Gutter, y + BandTop, NodeW, NodeH)); m_rects.insert(-band.n, m_summaries.value(band.n)); }
                    wide = qMax(wide, bandWide);
                    y += BandTop + qMax(1, layers) * (NodeH + 26) + BandGap;
                }
                setFixedSize(int((wide + 4) * zoom), int((y + 2) * zoom));
            } else {
                for (const auto &n : nodes) {
                    const int x = Gutter + column[n.layer]++ * (NodeW + 6), y = 8 + n.layer * (NodeH + 26);
                    m_rects.insert(n.id, QRectF(x, y, NodeW, NodeH));
                }
                int layers = 0, wide = 0;
                for (auto it = column.cbegin(); it != column.cend(); ++it) { layers = qMax(layers, it.key() + 1); wide = qMax(wide, it.value()); }
                setFixedSize(int((Gutter + wide * (NodeW + 6) + 4) * zoom), int((8 + layers * (NodeH + 26)) * zoom));
            }
        } else if (m_mode == Wide) {
            const QSizeF size = natural();
            const double viewW = area.isValid() ? area.width() / zoom : 0, viewH = area.isValid() ? area.height() / zoom : 0;
            m_ox = qMax(0.0, (viewW - size.width()) / 2);
            m_oy = banded() ? 0.0 : qMax(0.0, (viewH - size.height()) / 2);
            if (banded()) {
                // G9: phases stack top to bottom; a band's layers run left to
                // right inside it. A collapsed band is one header row.
                const double width = qMax(size.width() - 8, viewW > 0 ? viewW - 16 : 0.0);
                double y = 4;
                for (const auto &band : bands) {
                    const auto spreads = layerColumns(layerCounts(band.tasks));
                    int rows = 0;
                    for (const auto &spread : spreads) rows = qMax(rows, spread.rows);
                    const double body = band.collapsed ? 0 : bandBody(rows);
                    m_bandRects.insert(band.n, QRectF(8, y, width, BandHead - 4));
                    m_bandBackgrounds.append(QRectF(0, y - 4, width + 16, BandHead + body + 6));
                    if (band.collapsed) {
                        // G9: a collapsed band is its header row alone; edges
                        // that end in it attach to the header.
                        m_rects.insert(-band.n, m_bandRects.value(band.n));
                    } else {
                        // Spec 5.4: the band's own layers, its columns centered.
                        placeWide(band.tasks, WideX, y + BandTop + WidePad);
                        // G4: each layer inside the band keeps its stripe and
                        // small label above the band's tasks.
                        for (auto it = spreads.cbegin(); it != spreads.cend(); ++it) {
                            const double left = qMax(0.0, WideX + it->start * WideStep - (WideStep - WideW) / 2.0);
                            const double w = qMin(double(it->span * WideStep), width - left + 8);
                            if (w <= 4) continue;
                            BandLayer layer; layer.phase = band.n; layer.layer = it.key(); layer.count = it->count;
                            layer.rect = QRectF(left, y + BandHead, w, BandTop - BandHead);
                            m_bandLayers.append(layer);
                        }
                    }
                    y += BandHead + body + BandGap;
                }
                m_spreads.clear();
                const double finalW = qMax(size.width(), viewW);
                const double finalH = qMax(qMax(double(y), size.height()), viewH);
                setFixedSize(int(finalW * zoom), int(finalH * zoom));
            } else {
                QList<int> all;
                for (const auto &n : nodes) all.append(n.id);
                placeWide(all, m_ox + WideX, m_oy + WideTop);
                m_spreads = layerColumns(layerCounts(all));
                setFixedSize(int(qMax(size.width(), viewW) * zoom), int(qMax(size.height(), viewH) * zoom));
            }
            routeAll();
        } else if (banded()) {
            int y = 0;
            for (const auto &band : bands) {
                if (y) y += 8;
                m_bandRects.insert(band.n, QRectF(0, y, 10000, BandHead));
                y += BandHead + 2;
                if (band.collapsed) continue;
                QMap<int, int> counts;
                for (int id : band.tasks) if (const Node *node = nodeOf(id); node && node->match) ++counts[node->layer];
                int layer = -1;
                for (int id : band.tasks) {
                    const Node *node = nodeOf(id);
                    if (!node || !node->match) continue;
                    if (node->layer != layer) {
                        layer = node->layer;
                        m_listLayers.append({band.n, layer, counts.value(layer), QRectF(0, y, 10000, 20)});
                        y += 22;
                    }
                    m_rects.insert(id, QRectF(0, y, 10000, 38));
                    y += 40;
                }
            }
            setMinimumHeight(y + 4); setMaximumHeight(y + 4);
        } else {
            int y = 0, layer = -1;
            for (const auto &n : nodes) {
                if (!n.match) continue;
                if (n.layer != layer) { layer = n.layer; y += 24; }
                m_rects.insert(n.id, QRectF(0, y, 10000, 38)); y += 40;
            }
            setMinimumHeight(y + 4); setMaximumHeight(y + 4);
        }
        if (m_keep) {
            m_keep = false;
            m_connectFrom = m_keepFrom && m_rects.contains(m_keepFrom) && canEdit(m_keepFrom) ? m_keepFrom : 0;
            if (m_connectFrom) {
                // The dashed line runs to the pointer when it is over the view.
                const QPoint cursor = mapFromGlobal(QCursor::pos());
                m_dragAt = rect().contains(cursor) ? QPointF(cursor) : out(m_rects[m_connectFrom]) * zoom;
            }
            const bool edge = m_keepEdge.first && edges.value(m_keepEdge.second).contains(m_keepEdge.first) && canEdit(m_keepEdge.second);
            m_edge = edge ? m_keepEdge : qMakePair(0, 0);
        }
        if (m_keepHover) {
            if (m_rects.contains(m_keepHover)) {
                m_hover = m_keepHover;
                for (const auto &node : nodes) if (node.id == m_hover) { setToolTip(node.tooltip); break; }
                setCursor(Qt::PointingHandCursor);
            }
            m_keepHover = 0;
        }
        bool working = false;
        for (const auto &n : nodes) working = working || n.working;
        if (m_mode == Wide && working && AgentWorking::animated()) m_dots.start(); else m_dots.stop();
        update();
    }
    QPair<int, int> selectedEdge() const { return m_edge; }
protected:
    static constexpr int NodeW = 118, NodeH = 46, Gutter = 20;
    // U25 part 5 (spec 5.4): the prototype's node 196x54, column step 236, row
    // step 74, 7 px corners; room above the nodes for a corridor and below
    // them for back edges.
    static constexpr int WideW = 196, WideH = 54, WideStep = 236, WideRow = 74, WideCorner = 7, WideX = 14, WideTop = 34;
    static constexpr int WidePad = 14, WideUnder = 26, WideLane = 6;
    static constexpr int BandHead = 40, BandTop = 58, BandGap = 10, BandMin = 240;
    bool banded() const { return !bands.isEmpty(); }
    const Node *nodeOf(int id) const
    {
        for (const auto &node : nodes) if (node.id == id) return &node;
        return nullptr;
    }
    // Wide: a band is as wide as its own layers, and at least one header.
    int bandWidth(const PhaseBand &band) const
    {
        int columns = 0;
        for (const auto &spread : layerColumns(layerCounts(band.tasks))) columns = qMax(columns, spread.start + spread.span);
        return qMax(BandMin, WideX + (qMax(1, columns) - 1) * WideStep + WideW + 16);
    }
    QMap<int, int> layerCounts(const QList<int> &ids) const
    {
        QMap<int, int> counts;
        for (int id : ids) if (const Node *node = nodeOf(id)) ++counts[node->layer];
        return counts;
    }
    // Wide: an open band's height under its header for `rows` rows.
    static double bandBody(int rows) { return (BandTop - BandHead) + WidePad + (qMax(1, rows) - 1) * WideRow + WideH + WideUnder; }
    // Spec 5.4: one group's tasks (a phase band, or the whole plan) in columns
    // by layer, each column centered on the tallest; returns the lowest bottom.
    double placeWide(const QList<int> &ids, double left, double top)
    {
        QMap<int, QList<int>> columns;
        for (int id : ids) if (const Node *node = nodeOf(id)) columns[node->layer].append(id);
        const auto spreads = layerColumns(layerCounts(ids));
        int tallest = 1;
        for (const auto &spread : spreads) tallest = qMax(tallest, spread.rows);
        const int group = m_groups.size();
        for (auto it = columns.cbegin(); it != columns.cend(); ++it) {
            const LayerSpread spread = spreads.value(it.key());
            for (int i = 0; i < it.value().size(); ++i) {
                // A wrapped layer fills its columns top to bottom; each column
                // centers on the tallest, the last one may be shorter.
                const int column = i / spread.rows, row = i % spread.rows;
                const int inColumn = qMin(spread.rows, int(it.value().size()) - column * spread.rows);
                const double y = top + row * WideRow + (tallest - inColumn) * WideRow / 2.0;
                m_rects.insert(it.value().at(i), QRectF(left + (spread.start + column) * WideStep, y, WideW, WideH));
                m_groupOf.insert(it.value().at(i), group);
                m_columnOf.insert(it.value().at(i), spread.start + column);
            }
        }
        const double bottom = top + (tallest - 1) * WideRow + WideH;
        m_groups.append({top, bottom});
        return bottom;
    }
    // Spec 5.4: orthogonal routes inside each group. Next layer: right into
    // the gap, down or up, into the task. Further: into the gap, along the
    // free corridor (above, below, or between rows) with the least deviation,
    // into the gap before the task, into it. Edges through one gap spread by
    // 6 px; a back edge (a cycle) runs under every node. Edges between groups
    // (phases) are not drawn: the node names them ("↤#2").
    void routeAll()
    {
        if (m_mode != Wide) return;
        const double gap = WideStep - WideW;
        QMap<QPair<int, int>, int> uses;
        const auto lane = [&](int group, double gx) {
            // The lanes stay inside the gap: a crowded gap reuses them.
            const int lanes = 2 * int((gap / 2 - 2) / WideLane) + 1;
            const int k = uses[{group, qRound(gx)}]++ % lanes;
            return gx + (k % 2 ? 1 : -1) * std::ceil(k / 2.0) * WideLane;
        };
        // #465 N19: the lookups are built once per routing, not once per
        // edge and candidate: each task's layer, each group's columns of
        // (top, bottom) by layer, and each group's corridor candidates.
        // Routes follow the columns on screen: the columns of a wrapped layer
        // are crossed like layers, through the corridors between their rows.
        const auto layerOf = [&](int id) { return m_columnOf.value(id, 0); };
        QHash<int, QMap<int, QList<QPair<double, double>>>> columns;
        QHash<int, QList<double>> corridors;
        for (auto it = m_groupOf.cbegin(); it != m_groupOf.cend(); ++it) {
            const QRectF q = m_rects.value(it.key());
            columns[it.value()][layerOf(it.key())].append({q.top(), q.bottom()});
            corridors[it.value()] << q.top() - (WideRow - WideH) / 2.0 << q.bottom() + (WideRow - WideH) / 2.0;
        }
        // The rows a long edge must not cross: the nodes of the columns
        // between its ends, 6 px around each, merged and sorted.
        const auto blocked = [&](int group, int la, int lb) {
            QList<QPair<double, double>> spans;
            const auto &byLayer = columns[group];
            for (auto it = byLayer.lowerBound(la + 1); it != byLayer.cend() && it.key() < lb; ++it)
                for (const auto &[top, bottom] : it.value()) spans.append({top - 6, bottom + 6});
            std::sort(spans.begin(), spans.end());
            QList<QPair<double, double>> merged;
            for (const auto &span : spans) {
                if (!merged.isEmpty() && span.first < merged.last().second) merged.last().second = qMax(merged.last().second, span.second);
                else merged.append(span);
            }
            return merged;
        };
        const auto freeY = [](const QList<QPair<double, double>> &spans, double y) {
            // The first span that ends past y is the only one that can hold it.
            const auto it = std::upper_bound(spans.cbegin(), spans.cend(), y, [](double v, const QPair<double, double> &span) { return v < span.second; });
            return it == spans.cend() || !(y > it->first);
        };
        for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) {
            const int task = it.key();
            if (dep <= 0 || task <= 0 || !m_groupOf.contains(dep) || !m_groupOf.contains(task)) continue;
            const int group = m_groupOf.value(dep);
            if (m_groupOf.value(task) != group) continue;
            const QRectF a = m_rects.value(dep), b = m_rects.value(task);
            const int la = layerOf(dep), lb = layerOf(task);
            const double x1 = a.right(), y1 = a.center().y(), x2 = b.left(), y2 = b.center().y();
            const double bottom = m_groups.at(group).second, top = m_groups.at(group).first;
            Route r;
            if (lb <= la) {
                const double yb = bottom + 12;
                r.points << QPointF(x1, y1) << QPointF(x1 + 14, y1) << QPointF(x1 + 14, yb) << QPointF(x2 - 14, yb) << QPointF(x2 - 14, y2) << QPointF(x2 - 2, y2);
                r.mid = QPointF((x1 + x2) / 2, yb);
            } else if (lb == la + 1) {
                const double gx = lane(group, a.left() + WideW + gap / 2);
                r.points << QPointF(x1, y1) << QPointF(gx, y1) << QPointF(gx, y2) << QPointF(x2 - 2, y2);
                r.mid = QPointF(gx, (y1 + y2) / 2);
            } else {
                const QList<double> candidates = QList<double>{top - 9, bottom + 10} + corridors.value(group);
                const auto spans = blocked(group, la, lb);
                QList<double> free;
                for (double c : candidates) if (freeY(spans, c)) free << c;
                std::stable_sort(free.begin(), free.end(), [&](double u, double v) {
                    return std::abs(u - y1) + std::abs(u - y2) < std::abs(v - y1) + std::abs(v - y2);
                });
                const double cy = free.isEmpty() ? bottom + 10 : free.first();
                if (freeY(spans, y1) && std::abs(y1 - y2) < 1) {
                    r.points << QPointF(x1, y1) << QPointF(x2 - 2, y2);
                    r.mid = QPointF((x1 + x2) / 2, y1);
                } else {
                    const double g1 = lane(group, a.left() + WideW + gap / 2), g2 = lane(group, b.left() - gap / 2);
                    r.points << QPointF(x1, y1) << QPointF(g1, y1) << QPointF(g1, cy) << QPointF(g2, cy) << QPointF(g2, y2) << QPointF(x2 - 2, y2);
                    r.mid = QPointF((g1 + g2) / 2, cy);
                }
            }
            m_routes.insert({dep, task}, r);
        }
    }
    // The route as a path with rounded corners (at most 7 px, at most half
    // of either leg).
    static QPainterPath orthoPath(const QPolygonF &route)
    {
        QPolygonF points;
        for (const QPointF &point : route) if (points.isEmpty() || points.last() != point) points << point;
        QPainterPath path;
        if (points.isEmpty()) return path;
        path.moveTo(points.first());
        for (int i = 1; i + 1 < points.size(); ++i) {
            const QPointF a = points.at(i - 1), b = points.at(i), c = points.at(i + 1);
            const double l1 = QLineF(a, b).length(), l2 = QLineF(b, c).length();
            const double r = qMin(double(WideCorner), qMin(l1 / 2, l2 / 2));
            const QPointF p1 = b - (b - a) / (l1 > 0 ? l1 : 1) * r, p2 = b + (c - b) / (l2 > 0 ? l2 : 1) * r;
            path.lineTo(p1); path.quadTo(b, p2);
        }
        path.lineTo(points.last());
        return path;
    }
    // Where an edge's remove × sits.
    QPointF edgeMid(int dep, int task) const
    {
        if (m_mode == Wide) return m_routes.value({dep, task}).mid;
        return edgePath(dep, task).pointAtPercent(.5);
    }
    // A two-line Wide node (spec 5.4): status icon, #id, the dependencies from
    // other phases, the title; the role chip, the model tile with the working
    // dots and the problem text. No status word: the tooltip has it.
    void paintWideNode(QPainter &p, const Node &n, const QRectF &r, bool lit, bool target)
    {
        const QColor stroke = target ? m_c.accent
                            : n.error ? m_c.danger
                            : n.problem ? (n.tone == m_c.danger ? m_c.danger : m_c.warn)
                            : lit ? m_c.accent : m_c.border;
        QColor fill = m_c.surface;
        if (n.error) fill = alpha(m_c.danger, m_c.dark ? .1 : .08);
        else if (target) fill = alpha(m_c.accent, .08);
        else if (n.heatFill.isValid()) fill = n.heatFill;   // U38: the tint only; stroke and edge stay
        QPainterPath box; box.addRoundedRect(r.adjusted(.5, .5, -.5, -.5), 9, 9);
        p.setPen(QPen(stroke, 1, n.draft ? Qt::DashLine : Qt::SolidLine)); p.setBrush(fill); p.drawPath(box);
        // The 3 px status edge on the left.
        p.save(); p.setClipPath(box); p.fillRect(QRectF(r.left(), r.top(), 3, r.height()), n.tone); p.restore();
        // Show on graph: the blockers carry a warn ring.
        if (!emphasis.isEmpty() && emphasis.contains(n.id)) {
            p.setPen(QPen(m_c.warn, 2)); p.setBrush(Qt::NoBrush); p.drawRoundedRect(r.adjusted(-1.5, -1.5, 1.5, 1.5), 10, 10);
        }
        const double line1 = r.top() + 7, line2 = r.top() + 29, lineH = 18;
        double x = r.left() + 11;
        glyph(p, QRectF(x, line1 + 3, 12, 12), n.tone, n.kind);
        x += 18;
        QFont mono = NewSessionDense::mono(11); mono.setBold(true); p.setFont(mono); p.setPen(m_c.fg);
        const QString id = QStringLiteral("#%1").arg(n.id);
        const int idw = QFontMetrics(mono).horizontalAdvance(id);
        p.drawText(QRectF(x, line1, idw + 2, lineH), Qt::AlignLeft | Qt::AlignVCenter, id);
        x += idw + 6;
        if (n.late) { workspaceIcon("out-of-scope", m_c.violet).paint(&p, QRect(int(x), int(line1 + 3), 12, 12)); x += 17; }
        if (!n.ext.isEmpty()) {
            QFont ext = NewSessionDense::mono(10); p.setFont(ext); p.setPen(m_c.folder);
            const int ew = QFontMetrics(ext).horizontalAdvance(n.ext);
            p.drawText(QRectF(x, line1, ew + 2, lineH), Qt::AlignLeft | Qt::AlignVCenter, n.ext);
            x += ew + 6;
        }
        QFont title = font(); title.setPixelSize(12); title.setWeight(QFont::DemiBold); p.setFont(title); p.setPen(m_c.fg);
        const double room = qMax(0.0, r.right() - 10 - x);
        p.drawText(QRectF(x, line1, room, lineH), Qt::AlignLeft | Qt::AlignVCenter, QFontMetrics(title).elidedText(n.title, Qt::ElideRight, int(room)));
        // The role chip (the prototype's .role: 18 high, 9 px radius).
        QFont small = font(); small.setPixelSize(11); small.setWeight(QFont::DemiBold);
        const int roleW = QFontMetrics(small).horizontalAdvance(n.role) + 12;
        const QRectF chip(r.left() + 11, line2, roleW, lineH);
        p.setPen(QPen(alpha(n.roleTone, .45), 1)); p.setBrush(alpha(n.roleTone, .12)); p.drawRoundedRect(chip.adjusted(.5, .5, -.5, -.5), 9, 9);
        p.setFont(small); p.setPen(n.roleTone); p.drawText(chip, Qt::AlignCenter, n.role);
        double right = chip.right() + 6;
        if (!n.agent.isEmpty()) {
            const QRectF tile(right, line2 + .5, 17, 17);
            AgentWorking::paintTile(p, tile, agentLogo(n.agent, m_c.fg), m_c.dark, n.working, AgentWorking::phase());
            right = tile.right() + 6;
        }
        // U37: the working time after the activity label keeps its room: a
        // long label is elided first.
        QFont metaFont = font(); metaFont.setPixelSize(11);
        const QFontMetrics metaMetrics(metaFont);
        const double timeRoom = n.worked.isEmpty() ? 0.0 : qMax(metaMetrics.horizontalAdvance(n.worked), estimateRoom(metaMetrics, n.estimate, n.estimateKnown, n.runningSince)) + 6;
        // U38: the loss sits at the line's end, bold in its heat color; the
        // texts before it keep to the room left of it.
        double end = r.right() - 8;
        if (!n.heat.isEmpty()) {
            QFont heat = font(); heat.setPixelSize(11); heat.setWeight(n.heatFill.isValid() ? QFont::DemiBold : QFont::Normal);
            p.setFont(heat); p.setPen(n.heatColor);
            const double w = qMin(double(QFontMetrics(heat).horizontalAdvance(n.heat)), qMax(0.0, end - right));
            p.drawText(QRectF(end - w, line2, w, lineH), Qt::AlignRight | Qt::AlignVCenter, QFontMetrics(heat).elidedText(n.heat, Qt::ElideRight, int(w)));
            end -= w + 8;
        }
        if (!n.activity.isEmpty()) {
            // The owner's small activity label: muted text3 at 11 px, right of
            // the role chip and the model tile, and elided before a problem
            // text would lose room to it.
            QFont meta = font(); meta.setPixelSize(11); p.setFont(meta); p.setPen(m_c.text3);
            const double w = qMax(0.0, end - right - timeRoom);
            const QString shown = QFontMetrics(meta).elidedText(n.activity, Qt::ElideRight, int(w));
            p.drawText(QRectF(right, line2, w, lineH), Qt::AlignLeft | Qt::AlignVCenter, shown);
            right += qMin(w, double(QFontMetrics(meta).horizontalAdvance(shown))) + 6;
        }
        if (!n.worked.isEmpty()) {
            // U37: the working time right after it, in the same color and size;
            // a running one keeps the room it grows to.
            QFont meta = font(); meta.setPixelSize(11); p.setFont(meta); p.setPen(n.timeOver ? m_c.warn : m_c.text3);
            const QFontMetrics metrics(meta);
            const double w = qMax(0.0, end - right);
            const QString shown = metrics.elidedText(n.worked, Qt::ElideRight, int(w));
            p.drawText(QRectF(right, line2, w, lineH), Qt::AlignLeft | Qt::AlignVCenter, shown);
            right += qMin(w, double(qMax(metrics.horizontalAdvance(shown), estimateRoom(metrics, n.estimate, n.estimateKnown, n.runningSince)))) + 6;
        }
        if (!n.problemText.isEmpty()) {
            QFont meta = font(); meta.setPixelSize(11); p.setFont(meta); p.setPen(n.tone);
            const double w = qMax(0.0, end - right);
            p.drawText(QRectF(right, line2, w, lineH), Qt::AlignLeft | Qt::AlignVCenter, QFontMetrics(meta).elidedText(n.problemText, Qt::ElideRight, int(w)));
        }
        if (n.error) { p.setPen(m_c.danger); p.setFont(small); p.drawText(QRectF(r.right() - 16, r.top() + 4, 12, 12), Qt::AlignCenter, QStringLiteral("!")); }
    }
    // A collapsed phase's tasks hang their edges on the phase summary node.
    int anchor(int id) const
    {
        if (m_rects.contains(id)) return id;
        for (const auto &band : bands)
            if (band.collapsed && band.tasks.contains(id) && m_bandRects.contains(band.n)) return -band.n;
        return 0;
    }
    // The phase header or summary node under a point, 0 when there is none.
    int phaseAt(const QPointF &position) const
    {
        const QPointF point = graph() ? position / zoom : position;
        for (auto it = m_bandRects.cbegin(); it != m_bandRects.cend(); ++it) if (it.value().contains(point)) return it.key();
        for (auto it = m_summaries.cbegin(); it != m_summaries.cend(); ++it) if (it.value().contains(point)) return it.key();
        return 0;
    }
    // A phase header: chevron, "Phase 2 · QA fixes", the phase status chip,
    // done/total, the health line and, for a draft, Approve and Discard (and
    // the F10 move arrows). G9: without phase_summary (older hgs) it falls back
    // to today's heading with the barrier and draft tags.
    void paintBand(QPainter &p, const PhaseBand &band, const QRectF &r)
    {
        p.setOpacity(1);
        QStringList tips;
        if (!band.healthTip.isEmpty()) tips << band.healthTip;
        if (!band.blockedTip.isEmpty()) tips << band.blockedTip;
        if (!band.changeLines.isEmpty())
            tips << QCoreApplication::translate("TeamPanel", "Changes from the lead:") + QLatin1Char('\n') + band.changeLines.join(QLatin1Char('\n'));
        if (tips.isEmpty()) m_bandTips.remove(band.n);
        else m_bandTips.insert(band.n, tips.join(QLatin1Char('\n')));
        const double line1 = r.top() + 3, lineH = 16;
        const QPointF middle(r.left() + 7, line1 + lineH / 2);
        QPainterPath chevron;
        if (band.collapsed) { chevron.moveTo(middle + QPointF(-2, -4)); chevron.lineTo(middle + QPointF(3, 0)); chevron.lineTo(middle + QPointF(-2, 4)); }
        else { chevron.moveTo(middle + QPointF(-4, -2)); chevron.lineTo(middle + QPointF(0, 3)); chevron.lineTo(middle + QPointF(4, -2)); }
        p.setPen(Qt::NoPen); p.setBrush(m_c.muted); p.drawPath(chevron);
        QFont titleFont = font(); titleFont.setPixelSize(11); titleFont.setWeight(QFont::DemiBold);
        QFont metaFont = font(); metaFont.setPixelSize(9); metaFont.setWeight(QFont::DemiBold);
        const QFontMetrics tf(titleFont), mf(metaFont);
        double right = r.right() - 6;
        const auto pill = [&](const QString &text, const QColor &tone) {
            const double width = QFontMetrics(metaFont).horizontalAdvance(text) + 12;
            const QRectF box(right - width, line1, width, lineH);
            p.setPen(QPen(alpha(tone, .5), 1)); p.setBrush(alpha(tone, .12)); p.drawRoundedRect(box, 7, 7);
            p.setFont(metaFont); p.setPen(tone); p.drawText(box, Qt::AlignCenter, text);
            right -= width + 5;
        };
        const auto control = [&](const QString &text, const QColor &tone, bool enabled, QRectF *store) {
            const double width = mf.horizontalAdvance(text) + 14;
            const QRectF box(right - width, line1, width, lineH);
            p.setPen(QPen(enabled ? alpha(tone, .65) : m_c.border, 1));
            p.setBrush(enabled ? alpha(tone, .12) : QColor(Qt::transparent));
            p.drawRoundedRect(box, 4, 4);
            p.setFont(metaFont); p.setPen(enabled ? tone : m_c.dim);
            p.drawText(box, Qt::AlignCenter, text);
            if (store) *store = box;
            right -= width + 5;
        };
        if (band.draft) {
            control(QCoreApplication::translate("TeamPanel", "Approve"), m_c.accent, band.blocked.isEmpty() && !decided.contains(band.n), &m_approve[band.n]);
            control(QCoreApplication::translate("TeamPanel", "Discard"), m_c.danger, !decided.contains(band.n), &m_discard[band.n]);
            const QRectF up(right - 14, line1, 14, lineH), down(right - 30, line1, 14, lineH);
            m_moveUp.insert(band.n, up); m_moveDown.insert(band.n, down);
            p.setFont(metaFont); p.setPen(m_c.text3);
            p.drawText(up, Qt::AlignCenter, QStringLiteral("↑"));
            p.drawText(down, Qt::AlignCenter, QStringLiteral("↓"));
            right -= 36;
        } else {
            m_approve.remove(band.n); m_discard.remove(band.n); m_moveUp.remove(band.n); m_moveDown.remove(band.n);
        }
        if (band.autoDraft) {
            // U61: a draft Auto accepts at its first hand-out, not one waiting for you.
            pill(band.startAhead ? QCoreApplication::translate("TeamPanel", "Draft (auto), started ahead") : Teams::autoDraftBadge(), m_c.folder);
        } else if (!band.status.isEmpty()) {
            pill(phaseStatusText(band.status), phaseStatusColor(m_c, band.status));
        } else if (band.draft) {
            pill(QCoreApplication::translate("TeamPanel", "draft"), m_c.warn);
        }
        if (!band.status.isEmpty() || band.draft) {
            const QString progress = QStringLiteral("%1/%2").arg(band.done).arg(band.total);
            const double width = mf.horizontalAdvance(progress) + 2;
            p.setFont(metaFont); p.setPen(m_c.muted);
            p.drawText(QRectF(right - width, line1, width, lineH), Qt::AlignRight | Qt::AlignVCenter, progress);
            right -= width + 8;
        }
        const double titleLeft = r.left() + 17;
        const double room = qMax(0.0, right - titleLeft);
        p.setFont(titleFont); p.setPen(m_c.text2);
        p.drawText(QRectF(titleLeft, line1, room, lineH), Qt::AlignLeft | Qt::AlignVCenter, tf.elidedText(band.heading, Qt::ElideRight, int(room)));
        // The second line: the phase health and, for a blocked draft, the F12
        // reason. Details are in the header's tooltip.
        QStringList meta;
        if (!band.healthLine.isEmpty()) meta << band.healthLine;
        if (!band.blocked.isEmpty()) meta << band.blocked;
        if (band.changes > 0) meta << QCoreApplication::translate("TeamPanel", "%1 change(s) from the lead").arg(band.changes);
        if (!meta.isEmpty()) {
            QFont small = font(); small.setPixelSize(9);
            const bool blocked = !band.blocked.isEmpty();
            p.setFont(small); p.setPen(blocked ? m_c.warn : m_c.text3);
            p.drawText(QRectF(titleLeft, r.top() + 21, qMax(0.0, r.width() - 24), 13), Qt::AlignLeft | Qt::AlignVCenter,
                       QFontMetrics(small).elidedText(meta.join(QStringLiteral(" · ")), Qt::ElideRight, int(qMax(0.0, r.width() - 24))));
        }
    }
    // A collapsed phase stands in for its tasks in the graph.
    void paintSummary(QPainter &p, int phase, const QRectF &r)
    {
        const PhaseBand *band = nullptr;
        for (const auto &candidate : bands) if (candidate.n == phase) band = &candidate;
        p.setOpacity(1);
        p.setPen(QPen(m_c.border, 1, Qt::DashLine)); p.setBrush(m_c.card);
        p.drawRoundedRect(r.adjusted(.5, .5, -.5, -.5), 8, 8);
        p.setPen(Qt::NoPen); p.setBrush(m_c.dim); p.drawRoundedRect(QRectF(r.left() + 1, r.top() + 6, 3, r.height() - 12), 1.5, 1.5);
        QFont title = font(); title.setPixelSize(11); title.setWeight(QFont::DemiBold); p.setFont(title); p.setPen(m_c.fg);
        const QString text = !band || band->title.isEmpty() ? QCoreApplication::translate("TeamPanel", "Phase %1").arg(phase)
                                                            : QCoreApplication::translate("TeamPanel", "Phase %1 · %2").arg(phase).arg(band->title);
        p.drawText(r.adjusted(9, 6, -6, -18), Qt::AlignLeft | Qt::AlignTop, QFontMetrics(title).elidedText(text, Qt::ElideRight, int(r.width() - 16)));
        QFont meta = font(); meta.setPixelSize(10); p.setFont(meta); p.setPen(m_c.muted);
        p.drawText(r.adjusted(9, 0, -6, -7), Qt::AlignLeft | Qt::AlignBottom,
                   phaseSummaryText(band ? band->done : 0, band ? band->total : 0));
    }
    // The layer list: a "LAYER n · count" rule and one task row.
    void paintLayerHeader(QPainter &p, int layer, int count, const QRectF &r)
    {
        QFont caps = font(); caps.setPixelSize(10); caps.setWeight(QFont::DemiBold); caps.setLetterSpacing(QFont::AbsoluteSpacing, .6);
        p.setOpacity(1); p.setFont(caps); p.setPen(m_c.dim);
        const QString text = QCoreApplication::translate("TeamPanel", "LAYER %1 · %2").arg(layer + 1).arg(count);
        p.drawText(QRectF(2, r.top(), width(), r.height()), Qt::AlignLeft | Qt::AlignVCenter, text);
        const int lw = QFontMetrics(caps).horizontalAdvance(text) + 10;
        p.setPen(m_c.border); p.drawLine(QPointF(lw, r.center().y()), QPointF(width() - 2, r.center().y()));
    }
    void paintListRow(QPainter &p, const Node &n, bool dim)
    {
        const QRectF r = m_rects[n.id];
        p.setOpacity(dim ? .35 : 1);
        const QRectF box(1, r.top(), width() - 2, r.height());
        if (n.problem || n.error) {
            p.setPen(QPen(n.tone == m_c.warn ? m_c.warn : m_c.danger, 1)); p.setBrush(alpha(n.tone == m_c.warn ? m_c.warn : m_c.danger, .07));
            p.drawRoundedRect(box.adjusted(.5, .5, -.5, -.5), 7, 7);
        } else if (m_hover == n.id) { p.setPen(Qt::NoPen); p.setBrush(alpha(m_c.fg, .05)); p.drawRoundedRect(box, 7, 7); }
        glyph(p, QRectF(8, r.top() + 5, 12, 12), n.tone, n.kind);
        QFont mono = NewSessionDense::mono(11); p.setFont(mono); p.setPen(m_c.dim);
        const QString id = QStringLiteral("#%1").arg(n.id);
        const int idw = QFontMetrics(mono).horizontalAdvance(id) + 6;
        p.drawText(QRectF(26, r.top() + 2, idw, 18), Qt::AlignLeft | Qt::AlignVCenter, id);
        QFont small = font(); small.setPixelSize(10); small.setWeight(QFont::Medium);
        const int roleW = QFontMetrics(small).horizontalAdvance(n.role) + 14;
        const QRectF pill(width() - roleW - 6, r.top() + 3, roleW, 16);
        p.setPen(QPen(alpha(n.roleTone, .45), 1)); p.setBrush(alpha(n.roleTone, .12)); p.drawRoundedRect(pill, 8, 8);
        p.setFont(small); p.setPen(n.roleTone); p.drawText(pill, Qt::AlignCenter, n.role);
        QFont title = font(); title.setPixelSize(12); p.setFont(title); p.setPen(m_c.fg);
        const double titleW = pill.left() - 32 - idw;
        p.drawText(QRectF(26 + idw, r.top() + 2, titleW, 18), Qt::AlignLeft | Qt::AlignVCenter, QFontMetrics(title).elidedText(n.title, Qt::ElideRight, int(titleW)));
        QFont meta = font(); meta.setPixelSize(11); p.setFont(meta); p.setPen(n.tone);
        const int statusW = qMin(int(width() - 150), QFontMetrics(meta).horizontalAdvance(n.statusText) + 2);
        p.drawText(QRectF(26, r.top() + 19, statusW, 16), Qt::AlignLeft | Qt::AlignVCenter, n.statusText);
        double left = 26 + statusW + 8;
        const double timeRoom = n.worked.isEmpty() ? 0.0 : qMax(QFontMetrics(meta).horizontalAdvance(n.worked), estimateRoom(QFontMetrics(meta), n.estimate, n.estimateKnown, n.runningSince)) + 6;
        if (!n.activity.isEmpty()) {
            // The same small activity label as the graph, muted, before the deps;
            // the working time after it keeps its room (U37).
            p.setPen(m_c.text3);
            const double w = qMax(0.0, width() - 112 - left - timeRoom);
            const QString shown = QFontMetrics(meta).elidedText(n.activity, Qt::ElideRight, int(w));
            p.drawText(QRectF(left, r.top() + 19, w, 16), Qt::AlignLeft | Qt::AlignVCenter, shown);
            left += qMin(w, double(QFontMetrics(meta).horizontalAdvance(shown))) + 6;
        }
        if (!n.worked.isEmpty()) {
            // U37: the working time right after it, as on the graph.
            p.setPen(n.timeOver ? m_c.warn : m_c.text3);
            const double w = qMax(0.0, width() - 112 - left);
            p.drawText(QRectF(left, r.top() + 19, w, 16), Qt::AlignLeft | Qt::AlignVCenter,
                       QFontMetrics(meta).elidedText(n.worked, Qt::ElideRight, int(w)));
        }
        if (!n.deps.isEmpty()) {
            p.setFont(NewSessionDense::mono(10)); p.setPen(m_c.dim);
            p.drawText(QRectF(width() - 112, r.top() + 19, 104, 16), Qt::AlignRight | Qt::AlignVCenter, n.deps);
        }
    }
    bool graph() const { return m_mode != List; }
    QPointF out(const QRectF &r) const { return m_mode == Wide ? QPointF(r.right(), r.center().y()) : QPointF(r.center().x(), r.bottom()); }
    QPointF in(const QRectF &r) const { return m_mode == Wide ? QPointF(r.left(), r.center().y()) : QPointF(r.center().x(), r.top()); }
    QRectF handle(const QRectF &r) const
    {
        return m_mode == Wide ? QRectF(r.right() - 9, r.center().y() - 8, 10, 16) : QRectF(r.center().x() - 6, r.bottom() - 4, 12, 8);
    }
    QPainterPath edgePath(int dep, int task) const
    {
        const QRectF a = m_rects[dep], b = m_rects[task];
        QPainterPath path;
        const bool forward = m_mode == Wide ? b.left() > a.right() : b.top() > a.bottom();
        if (forward) {
            const QPointF s = out(a), e = in(b);
            const QPointF bend = m_mode == Wide ? QPointF(18, 0) : QPointF(0, 14);
            path.moveTo(s); path.cubicTo(s + bend, e - bend, e);
        } else {   // a back edge of a cycle: a loop beside the nodes
            const QPointF s(a.right(), a.center().y()), e(b.right(), b.center().y());
            path.moveTo(s); path.cubicTo(s + QPointF(28, 0), e + QPointF(28, 0), e);
        }
        return path;
    }
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        if (graph()) p.fillRect(rect(), m_c.surface);
        const QSet<int> lit = m_hover && chain && !m_dragFrom ? chain(m_hover) : QSet<int>();
        const auto dimmed = [&](const Node &n) {
            if (!lit.isEmpty()) return !lit.contains(n.id);
            if (!emphasis.isEmpty()) return !emphasis.contains(n.id);
            return graph() && !n.match;
        };
        if (graph()) {
            p.scale(zoom, zoom);
            if (banded() && m_mode == Wide) {
                for (int i = 0; i < m_bandBackgrounds.size(); ++i)
                    if (i % 2) p.fillRect(m_bandBackgrounds[i], alpha(m_c.fg, .025));
                // G4: the alternating layer stripes and the small layer label
                // live inside every band, under its phase header.
                QFont caps = font(); caps.setPixelSize(9); caps.setWeight(QFont::DemiBold); caps.setLetterSpacing(QFont::AbsoluteSpacing, .5);
                const double bottom = height() / zoom;
                for (const auto &layer : m_bandLayers) {
                    if (layer.layer % 2)
                        p.fillRect(QRectF(layer.rect.left(), layer.rect.top(), layer.rect.width(), qMax(0.0, bottom - layer.rect.top())), alpha(m_c.fg, .03));
                    p.setFont(caps); p.setPen(m_c.dim);
                    p.drawText(layer.rect.adjusted(2, 0, -2, 0), Qt::AlignLeft | Qt::AlignVCenter,
                               QStringLiteral("L%1 · %2").arg(layer.layer + 1).arg(layer.count));
                }
            }
            QFont small = font(); small.setPixelSize(10); p.setFont(small); p.setPen(m_c.dim);
            if (banded()) {
                // The phase headers name the bands; each band draws its own layer
                // labels and stripes above.
            } else if (m_mode == Wide) {
                for (auto it = m_spreads.cbegin(); it != m_spreads.cend(); ++it) {
                    const int l = it.key();
                    const double x = m_ox + 4 + it->start * WideStep, w = it->span * WideStep;
                    if (l % 2) p.fillRect(QRectF(x, 0, w, height() / zoom), alpha(m_c.fg, .025));
                    QFont caps = small; caps.setWeight(QFont::DemiBold); caps.setLetterSpacing(QFont::AbsoluteSpacing, .6); p.setFont(caps);
                    p.drawText(QRectF(x + 8, 6, w, 18), Qt::AlignLeft | Qt::AlignVCenter,
                               QCoreApplication::translate("TeamPanel", "LAYER %1 · %2").arg(l + 1).arg(it->count));
                }
            } else {
                QSet<int> labelled;
                for (const auto &n : nodes) if (!labelled.contains(n.layer)) {
                    labelled.insert(n.layer); p.drawText(QRectF(0, m_rects[n.id].top(), Gutter, NodeH), Qt::AlignCenter, QStringLiteral("L%1").arg(n.layer + 1));
                }
            }
            for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) {
                if (m_mode == Wide) { paintWideEdge(p, dep, it.key(), lit); continue; }
                const int from = anchor(dep), to = anchor(it.key());
                if (!from || !to || from == to) continue;
                const bool hot = lit.contains(dep) && lit.contains(it.key());
                const bool bad = cycle.contains(dep) && cycle.contains(it.key());
                const bool selected = m_edge == qMakePair(dep, it.key());
                QColor color = selected || bad ? m_c.danger : hot ? m_c.accent : m_c.border;
                if (!hot && !selected && (!lit.isEmpty() || !emphasis.isEmpty()) && !(emphasis.contains(dep) && emphasis.contains(it.key()))) color = alpha(color, .35);
                p.setPen(QPen(color, hot || selected ? 2 : 1.4, selected || soft.contains({dep, it.key()}) ? Qt::DashLine : Qt::SolidLine)); p.setBrush(Qt::NoBrush);
                const auto path = edgePath(from, to);
                p.drawPath(path);
                const QPointF e = path.pointAtPercent(1), before = path.pointAtPercent(.96);
                const QLineF tail(e, before);
                QLineF wing1 = tail; wing1.setLength(5); wing1.setAngle(tail.angle() + 35);
                QLineF wing2 = tail; wing2.setLength(5); wing2.setAngle(tail.angle() - 35);
                p.drawLine(wing1); p.drawLine(wing2);
                if (selected) {   // the × that removes it
                    const QPointF mid = path.pointAtPercent(.5);
                    p.setPen(Qt::NoPen); p.setBrush(m_c.danger); p.drawEllipse(mid, 7, 7);
                    p.setPen(QPen(Qt::white, 1.5)); p.drawLine(mid + QPointF(-3, -3), mid + QPointF(3, 3)); p.drawLine(mid + QPointF(-3, 3), mid + QPointF(3, -3));
                }
            }
            if (const int from = m_dragFrom ? m_dragFrom : m_connectFrom; from && m_rects.contains(from)) {
                p.setPen(QPen(m_c.accent, 1.6, Qt::DashLine)); p.drawLine(out(m_rects[from]), m_dragAt / zoom);
            }
            for (const auto &n : nodes) {
                const QRectF r = m_rects[n.id];
                p.setOpacity(dimmed(n) ? (m_mode == Wide ? .33 : .35) : 1);
                const int from = m_dragFrom ? m_dragFrom : m_connectFrom;
                const bool target = from && m_hover == n.id && n.id != from;
                if (m_mode == Wide) {
                    paintWideNode(p, n, r, lit.contains(n.id), target || (m_connectFrom && n.id != m_connectFrom && canEdit(n.id)));
                    if (canEdit(n.id) && (m_hover == n.id || from == n.id)) {
                        p.setOpacity(1); p.setPen(Qt::NoPen); p.setBrush(m_c.accent); p.drawRoundedRect(handle(r), 5, 5);
                    }
                    continue;
                }
                p.setPen(target ? QPen(m_c.accent, 2)
                         : n.error || n.problem ? QPen(n.error || n.tone == m_c.danger ? m_c.danger : m_c.warn, 1)
                                                : QPen(m_c.border, 1, n.draft ? Qt::DashLine : Qt::SolidLine));
                p.setBrush(n.error ? alpha(m_c.danger, .1) : m_c.card);
                p.drawRoundedRect(r.adjusted(.5, .5, -.5, -.5), 8, 8);
                p.setPen(Qt::NoPen); p.setBrush(n.tone); p.drawRoundedRect(QRectF(r.left() + 1, r.top() + 6, 3, r.height() - 12), 1.5, 1.5);
                QFont mono = NewSessionDense::mono(10); p.setFont(mono); p.setPen(m_c.dim);
                const QString id = QStringLiteral("#%1").arg(n.id);
                p.drawText(QRectF(r.left() + 9, r.top() + 5, 30, 16), Qt::AlignLeft | Qt::AlignVCenter, id);
                int idw = QFontMetrics(mono).horizontalAdvance(id) + 4;
                if (n.late) { workspaceIcon("out-of-scope", m_c.violet).paint(&p, QRect(int(r.left() + 9 + idw), int(r.top() + 7), 12, 12)); idw += 15; }
                QFont title = font(); title.setPixelSize(12); if (m_mode == Wide) title.setWeight(QFont::DemiBold); p.setFont(title); p.setPen(m_c.fg);
                if (m_mode == Wide) {
                    // Two lines of title, then role and status.
                    const QRectF text(r.left() + 9 + idw, r.top() + 5, r.width() - 14 - idw, 32);
                    const QFontMetrics fm(title);
                    QString shown = n.title;
                    if (fm.boundingRect(text.toRect(), Qt::TextWordWrap, shown).height() > 32) {
                        while (shown.size() > 4 && fm.boundingRect(text.toRect(), Qt::TextWordWrap, shown + QStringLiteral("…")).height() > 32) shown.chop(1);
                        shown += QStringLiteral("…");
                    }
                    p.drawText(text, Qt::TextWordWrap | Qt::AlignLeft | Qt::AlignTop, shown);
                    QFont small = font(); small.setPixelSize(10); small.setWeight(QFont::Medium);
                    const int roleW = QFontMetrics(small).horizontalAdvance(n.role) + 14;
                    const QRectF pill(r.left() + 9, r.top() + 38, roleW, 15);
                    p.setPen(QPen(alpha(n.roleTone, .45), 1)); p.setBrush(alpha(n.roleTone, .12)); p.drawRoundedRect(pill, 7, 7);
                    p.setFont(small); p.setPen(n.roleTone); p.drawText(pill, Qt::AlignCenter, n.role);
                    if (!n.agent.isEmpty()) agentLogo(n.agent, m_c.text3).paint(&p, QRect(int(pill.right() + 5), int(pill.top()), 14, 14));
                    glyph(p, QRectF(r.left() + 9, r.top() + 58, 11, 11), n.tone, n.kind);
                    QFont meta = font(); meta.setPixelSize(11); p.setFont(meta); p.setPen(n.tone);
                    p.drawText(QRectF(r.left() + 24, r.top() + 55, r.width() - 30, 16), Qt::AlignLeft | Qt::AlignVCenter,
                               QFontMetrics(meta).elidedText(n.statusText, Qt::ElideRight, int(r.width() - 30)));
                } else {
                    p.drawText(QRectF(r.left() + 9 + idw, r.top() + 5, r.width() - 14 - idw, 16), Qt::AlignLeft | Qt::AlignVCenter,
                               QFontMetrics(title).elidedText(n.title, Qt::ElideRight, int(r.width() - 14 - idw)));
                    p.setPen(Qt::NoPen); p.setBrush(n.roleTone); p.drawEllipse(QRectF(r.left() + 10, r.top() + 29, 7, 7));
                    QFont role = font(); role.setPixelSize(11); p.setFont(role); p.setPen(m_c.text3);
                    const QString who = n.role + (n.attempt > 1 ? QStringLiteral(" ·%1").arg(n.attempt) : QString());
                    p.drawText(QRectF(r.left() + 21, r.top() + 24, r.width() - 42, 16), Qt::AlignLeft | Qt::AlignVCenter,
                               QFontMetrics(role).elidedText(who, Qt::ElideRight, int(r.width() - 42)));
                    glyph(p, QRectF(r.right() - 17, r.top() + 26, 12, 12), n.tone, n.kind);
                }
                if (n.error) { p.setPen(m_c.danger); p.drawText(QRectF(r.right() - 14, r.top() + 2, 12, 12), Qt::AlignCenter, QStringLiteral("!")); }
                if (editable && (m_hover == n.id || m_dragFrom == n.id)) {
                    p.setOpacity(1); p.setPen(Qt::NoPen); p.setBrush(m_c.accent); p.drawRoundedRect(handle(r), 3, 3);
                }
            }
            // A collapsed phase is one summary node; its header stays above.
            // G9: in the Wide graph a collapsed band is its header row only.
            if (m_mode != Wide)
                for (auto it = m_summaries.cbegin(); it != m_summaries.cend(); ++it) paintSummary(p, it.key(), it.value());
            for (const auto &band : bands) paintBand(p, band, m_bandRects.value(band.n));
            return;
        }
        if (banded()) {
            for (const auto &band : bands) {
                paintBand(p, band, m_bandRects.value(band.n));
                if (band.collapsed) continue;
                for (const auto &layer : m_listLayers) {
                    if (layer.phase != band.n) continue;
                    paintLayerHeader(p, layer.n, layer.count, layer.rect);
                }
                for (int id : band.tasks) {
                    const Node *node = nodeOf(id);
                    if (!node || !node->match) continue;
                    paintListRow(p, *node, dimmed(*node));
                }
            }
            return;
        }
        int layer = -1, count = 0;
        QMap<int, int> perLayer;
        for (const auto &n : nodes) if (n.match) ++perLayer[n.layer];
        for (const auto &n : nodes) {
            if (!n.match) continue;
            if (n.layer != layer) { layer = n.layer; count = perLayer[layer]; paintLayerHeader(p, layer, count, QRectF(2, m_rects[n.id].top() - 24, width() - 4, 20)); }
            paintListRow(p, n, dimmed(n));
        }
    }
    // Spec 5.4: an edge of the Wide graph - border, accent on the hovered
    // chain, warn for Show on graph, danger for a cycle or the selected one,
    // dashed when soft; the arrow at the task.
    void paintWideEdge(QPainter &p, int dep, int task, const QSet<int> &lit)
    {
        if (!m_routes.contains({dep, task})) return;
        const Route &r = m_routes[{dep, task}];
        const bool hot = lit.contains(dep) && lit.contains(task);
        const bool bad = cycle.contains(dep) && cycle.contains(task);
        const bool selected = m_edge == qMakePair(dep, task);
        const bool blocking = !emphasis.isEmpty() && emphasis.contains(dep);
        QColor color = m_c.border; double width = 1.5;
        Qt::PenStyle style = soft.contains({dep, task}) ? Qt::DashLine : Qt::SolidLine;
        if (selected) { color = m_c.danger; width = 2.2; style = Qt::DashLine; }
        else if (bad) { color = m_c.danger; width = 2; }
        else if (hot) { color = m_c.accent; width = 2; }
        else if (blocking) { color = m_c.warn; width = 2; }
        const Node *a = nodeOf(dep), *b = nodeOf(task);
        const bool focusDim = (a && !a->match) && (b && !b->match);
        const bool dim = (!lit.isEmpty() && !hot) || (!emphasis.isEmpty() && !blocking && !selected) || focusDim;
        p.setOpacity(dim ? .2 : 1);
        QPen pen(color, width, style); pen.setCapStyle(Qt::FlatCap); pen.setJoinStyle(Qt::RoundJoin);
        if (style == Qt::DashLine) pen.setDashPattern(selected ? QList<qreal>{5 / width, 3 / width} : QList<qreal>{4 / width, 3 / width});
        p.setPen(pen); p.setBrush(Qt::NoBrush);
        p.drawPath(orthoPath(r.points));
        const QPointF e = r.points.last();
        QPen arrow(color, 1.5); arrow.setJoinStyle(Qt::MiterJoin); p.setPen(arrow);
        p.drawPolyline(QPolygonF{e + QPointF(-7, -4.5), e, e + QPointF(-7, 4.5)});
        p.setOpacity(1);
        if (selected) {   // the × that removes it
            p.setPen(Qt::NoPen); p.setBrush(m_c.danger); p.drawEllipse(r.mid, 10, 10);
            p.setPen(QPen(Qt::white, 1.6)); p.drawLine(r.mid + QPointF(-3.5, -3.5), r.mid + QPointF(3.5, 3.5)); p.drawLine(r.mid + QPointF(-3.5, 3.5), r.mid + QPointF(3.5, -3.5));
        }
    }
    int at(const QPointF &position) const
    {
        const QPointF point = graph() ? position / zoom : position;
        for (auto it = m_rects.cbegin(); it != m_rects.cend(); ++it) {
            if (it.key() < 0) continue;   // a collapsed phase's summary is no task
            QRectF r = it.value(); if (!graph()) r.setWidth(width());
            if (r.adjusted(0, 0, 0, editable ? 4 : 0).contains(point)) return it.key();
        }
        return 0;
    }
    // The edge under the point, within 5px, or its × when selected.
    QPair<int, int> edgeAt(const QPointF &position) const
    {
        const QPointF point = position / zoom;
        if (m_mode == Wide) {
            // A 12 px wide hit line along the route, as the prototype's .hit.
            for (auto it = m_routes.cbegin(); it != m_routes.cend(); ++it) {
                if (!canEdit(it.key().second)) continue;
                const auto &points = it.value().points;
                for (int i = 1; i < points.size(); ++i) {
                    const QLineF leg(points.at(i - 1), points.at(i));
                    const QPointF d = leg.p2() - leg.p1();
                    const double len2 = d.x() * d.x() + d.y() * d.y();
                    const double t = len2 > 0 ? qBound(0.0, QPointF::dotProduct(point - leg.p1(), d) / len2, 1.0) : 0;
                    if (QLineF(leg.p1() + d * t, point).length() <= 6) return it.key();
                }
            }
            return {0, 0};
        }
        for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) {
            if (!m_rects.contains(dep) || !m_rects.contains(it.key())) continue;
            const auto path = edgePath(dep, it.key());
            for (int i = 0; i <= 40; ++i) if (QLineF(path.pointAtPercent(i / 40.), point).length() <= 5) return {dep, it.key()};
        }
        return {0, 0};
    }
    void mousePressEvent(QMouseEvent *e) override
    {
        if (e->button() == Qt::LeftButton && phaseAt(e->position())) return;   // toggled on release
        if (!(editable || !editableTasks.isEmpty()) || !graph() || e->button() != Qt::LeftButton) return QWidget::mousePressEvent(e);
        const QPointF point = e->position() / zoom;
        m_pressAt = e->position();
        if (m_connectFrom) return;   // the click that picks the task is taken on release
        if (m_edge.first && QLineF(edgeMid(m_edge.first, m_edge.second), point).length() <= (m_mode == Wide ? 10 : 8)) {
            const auto edge = m_edge; m_edge = {0, 0}; update(); reportEdit();
            if (unlinked) unlinked(edge.first, edge.second);
            return;
        }
        for (auto it = m_rects.cbegin(); it != m_rects.cend(); ++it)
            if (it.key() > 0 && canEdit(it.key()) && handle(it.value()).adjusted(-4, -4, 4, 4).contains(point)) { m_dragFrom = it.key(); m_dragAt = e->position(); m_edge = {0, 0}; update(); reportEdit(); return; }
        if (!at(e->position())) { m_edge = edgeAt(e->position()); update(); reportEdit(); }
    }
    void mouseMoveEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (m_dragFrom || m_connectFrom) { m_dragAt = e->position(); m_hover = id; update(); return; }
        if (phaseAt(e->position())) {
            if (m_hover) { setHover(0); update(); }
            setCursor(Qt::PointingHandCursor);
            setToolTip(bandTooltip(phaseAt(e->position())));
            return;
        }
        if (id != m_hover) {
            setHover(id);
            // The hovered node's phase tooltip names the state, the time and
            // the next step (spec 11).
            QString tip;
            for (const auto &node : nodes) if (node.id == id) { tip = node.tooltip; break; }
            setToolTip(tip);
            setCursor(id ? Qt::PointingHandCursor : Qt::ArrowCursor);
            update();
        }
    }
    void leaveEvent(QEvent *) override
    {
        // #570: a render hides the old view under a resting pointer and Qt
        // sends it a Leave; that is not the pointer leaving, so the hover
        // stays for the view that replaces it.
        if (!isVisible()) return;
        if (!m_dragFrom && !m_connectFrom) { setHover(0); setToolTip({}); update(); }
    }
    void setHover(int id)
    {
        const int from = m_hover;
        m_hover = id;
        if (from != id && hoverChanged) hoverChanged(from, id);
    }
    void mouseReleaseEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (m_dragFrom) {
            const int from = m_dragFrom; m_dragFrom = 0;
            // Spec 5.4: a click on the handle (no drag) starts connecting; the
            // next click on a task links it.
            if (QLineF(m_pressAt, e->position()).length() < 4 && (!id || id == from)) { m_connectFrom = from; m_dragAt = e->position(); update(); reportEdit(); return; }
            update();
            if (id && id != from && canEdit(id) && linked) linked(from, id);
            return;
        }
        if (m_connectFrom) {
            const int from = m_connectFrom; m_connectFrom = 0; update(); reportEdit();
            if (id && id != from && canEdit(id) && linked) linked(from, id);
            return;
        }
        if (e->button() == Qt::LeftButton) {
            const QPointF point = graph() ? e->position() / zoom : e->position();
            for (auto it = m_approve.cbegin(); it != m_approve.cend(); ++it)
                if (it.value().contains(point)) {
                    const auto *band = bandOf(it.key());
                    if (band && band->blocked.isEmpty() && !decided.contains(it.key()) && phaseApprove) {
                        // The callback asks in a dialog: this view may be gone after it.
                        const int phase = it.key();
                        const QPointer<QWidget> alive(this);
                        if (phaseApprove(phase) && alive) { decided.insert(phase); update(); }
                    }
                    return;
                }
            for (auto it = m_discard.cbegin(); it != m_discard.cend(); ++it)
                if (it.value().contains(point)) {
                    if (!decided.contains(it.key()) && phaseDiscard) {
                        const int phase = it.key();
                        const QPointer<QWidget> alive(this);
                        if (phaseDiscard(phase) && alive) { decided.insert(phase); update(); }
                    }
                    return;
                }
            for (auto it = m_moveUp.cbegin(); it != m_moveUp.cend(); ++it)
                if (it.value().contains(point)) { if (phaseMoveUp) phaseMoveUp(it.key()); return; }
            for (auto it = m_moveDown.cbegin(); it != m_moveDown.cend(); ++it)
                if (it.value().contains(point)) { if (phaseMoveDown) phaseMoveDown(it.key()); return; }
            const int phase = phaseAt(e->position());
            if (phase) { if (phaseToggled) phaseToggled(phase); return; }
        }
        if (id && e->button() == Qt::LeftButton && clicked) clicked(id, e->globalPosition().toPoint());
    }
    void keyPressEvent(QKeyEvent *e) override
    {
        if (m_connectFrom && e->key() == Qt::Key_Escape) { m_connectFrom = 0; update(); reportEdit(); return; }
        if (canEdit(m_edge.second) && m_edge.first && (e->key() == Qt::Key_Delete || e->key() == Qt::Key_Backspace)) {
            const auto edge = m_edge; m_edge = {0, 0}; update(); reportEdit();
            if (unlinked) unlinked(edge.first, edge.second);
            return;
        }
        QWidget::keyPressEvent(e);
    }
private:
    void reportEdit() { if (editChanged) editChanged(m_connectFrom, m_edge); }
    Colors m_c;
    Mode m_mode;
    int m_hover = 0, m_dragFrom = 0, m_connectFrom = 0, m_keepHover = 0;
    bool m_keep = false;
    int m_keepFrom = 0;
    QPair<int, int> m_keepEdge{0, 0};
    double m_ox = 0, m_oy = 0;
    QPointF m_dragAt, m_pressAt;
    // U25 part 5: the Wide graph's routes, each task's group (phase band) and
    // each group's node top and bottom; the timer of the working dots.
    QMap<QPair<int, int>, Route> m_routes;
    QMap<int, int> m_groupOf;
    QHash<int, int> m_columnOf;          // Wide: a task's column on screen
    QMap<int, LayerSpread> m_spreads;    // Wide, one group: each layer's columns
    QList<QPair<double, double>> m_groups;
    QTimer m_dots;
    QPair<int, int> m_edge{0, 0};
    QMap<int, QRectF> m_rects;
    // Phase grouping: a header per band, one summary rect per collapsed band
    // (keyed by phase), the alternating wide bands, and the layer rules inside
    // the list's phase groups.
    QMap<int, QRectF> m_bandRects, m_summaries;
    // G9: the bands' header controls and tooltips, rebuilt by layout().
    QMap<int, QRectF> m_approve, m_discard, m_moveUp, m_moveDown;
    QMap<int, QString> m_bandTips;
    QList<QRectF> m_bandBackgrounds;
    QList<BandLayer> m_bandLayers;
    struct ListLayer { int phase = 0, n = 0, count = 0; QRectF rect; };
    QList<ListLayer> m_listLayers;
};

// G9: the tooltip details behind a phase's health line (C7 numbers + warnings).
inline QString phaseHealthTip(const Team &team, int phase)
{
    const auto *health = team.phaseHealthOf(phase);
    if (!health) return QString();
    QStringList details;
    details << QCoreApplication::translate("TeamPanel", "%1 tasks · critical path %2 · parallelism %3")
                   .arg(health->tasks).arg(health->criticalPath)
                   .arg(health->parallelism, 0, 'f', 1);
    for (const auto &warning : health->warnings) details << warning;
    return details.join(QLatin1Char('\n'));
}

// The phase bands a task view draws, with the owner's collapse choices.
// G9: status.phase_summary gives the status chip and the health line;
// phases_health the tooltip details; phases[].waiting the disabled reason.
inline QList<TaskView::PhaseBand> phaseBands(const Team &team, const std::function<bool(const PhaseGroup &)> &open)
{
    QList<TaskView::PhaseBand> bands;
    const auto tr = [](const char *text, const QString &a = {}, const QString &b = {}) {
        return QCoreApplication::translate("TeamPanel", text).arg(a, b);
    };
    for (const auto &group : phaseGroups(team)) {
        TaskView::PhaseBand band;
        band.n = group.n; band.title = group.title; band.heading = phaseHeading(group);
        band.barrier = group.barrier; band.draft = group.draft; band.done = group.done; band.total = group.tasks.size();
        band.tasks = group.tasks; band.collapsed = !open(group);
        if (const auto *record = team.phase(group.n); record && record->autoDraft()) { band.autoDraft = true; band.startAhead = record->startAhead; }
        if (const auto *summary = team.phaseSummaryOf(group.n)) {
            band.status = summary->status;
            band.healthLine = summary->healthLine;
            // With a summary the header owns the numbers, so the heading is
            // just "Phase N · title" (the fallback keeps today's heading).
            band.heading = summary->title.isEmpty() ? tr("Phase %1", QString::number(group.n))
                                                    : tr("Phase %1 · %2", QString::number(group.n), summary->title);
            band.done = summary->done;
            band.total = summary->total;
            band.healthTip = phaseHealthTip(team, group.n);
        }
        // F12/N40: a draft whose predecessor is not ready says why Approve is
        // off; one that can run alongside an approved phase keeps it on.
        const int alongside = band.draft ? alongsidePhase(team, group.n) : 0;
        if (alongside) {
            band.alongside = true;
            // N6: a barrier phase's tasks wait however it is approved.
            band.blockedTip = group.barrier
                ? tr("Approve phase %1 now; its tasks start when phase %2 is finished",
                     QString::number(group.n), QString::number(alongside))
                : tr("Approve runs phase %1 now, alongside phase %2", QString::number(group.n), QString::number(alongside));
        } else if (const auto *record = team.phase(group.n); record && !record->waiting.text.isEmpty()) {
            band.blocked = record->waiting.text;
            QStringList open;
            for (int id : record->waiting.open) open << QStringLiteral("#%1").arg(id);
            QString tip = tr("Phase %1 is not ready: %2", QString::number(record->waiting.phase),
                             open.isEmpty() ? tr("no open tasks listed") : open.join(QStringLiteral(", ")));
            const QString also = approveBlocked(team, group.n);
            if (!also.isEmpty()) tip += QLatin1Char('\n') + also;
            band.blockedTip = tip;
        } else if (band.draft) {
            const QString blocked = approveBlocked(team, group.n);
            if (!blocked.isEmpty()) { band.blocked = approveBlockedShort(team, group.n); band.blockedTip = blocked; }
        }
        if (const auto *record = team.phase(group.n)) {
            band.changes = record->changes.size();
            // The lead's changes as a diff (added +, removed −, changed ~,
            // moved →); the G5 strip that listed them went with G9.
            const auto line = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
            for (const auto &change : record->changes) {
                if (change.kind == "moved") band.changeLines << line("→ moved from phase %1 to phase %2").arg(change.from, change.to);
                else if (change.added()) band.changeLines << line("+ #%1 %2").arg(change.task).arg(change.title);
                else if (change.removed()) band.changeLines << line("− #%1 %2").arg(change.task).arg(change.title);
                else band.changeLines << line("~ #%1 %2: %3 → %4").arg(change.task).arg(change.title, change.from, change.to);
            }
        }
        bands.append(band);
    }
    return bands;
}

// The tasks a grouped view builds nodes for, phase by phase and each phase's
// own layers in order; a collapsed phase builds none.
inline QList<int> phaseOrder(const Team &team, const QList<TaskView::PhaseBand> &bands)
{
    QList<int> order;
    for (const auto &band : bands) {
        if (band.collapsed) continue;
        const auto layers = phaseLayers(team, band.tasks);
        QMap<int, QList<int>> byLayer;
        for (int id : band.tasks) byLayer[layers.value(id)].append(id);
        for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) order += it.value();
    }
    return order;
}

// Replaced content: a click inside it may still be running, so it goes once
// control returns; meanwhile it is hidden and keeps no names to be found by.
inline void retire(QObject *object)
{
    if (!object) return;
    if (auto *widget = qobject_cast<QWidget *>(object)) widget->hide();
    object->setObjectName({});
    for (auto *child : object->findChildren<QObject *>()) child->setObjectName({});
    object->deleteLater();
}

// #737: a dialog is a window: it takes neither its view's palette nor its
// theme, but the system's palette, under the window's style sheet. A team
// dialog names its surface and its text in the theme itself.
inline QString dialogSheet(const Colors &c)
{
    return QStringLiteral("QDialog{background:%1;color:%2;} QLabel{background:transparent;} QCheckBox,QRadioButton{color:%2;}")
        .arg(c.bg.name(), c.fg.name());
}
// The theme a dialog opened over `parent` draws: its window's, which the
// window sets from the workspace theme; without a parent, its own palette's.
inline bool dialogDark(const QWidget *dialog, const QWidget *parent)
{
    return (parent ? parent->window() : dialog)->palette().color(QPalette::Window).lightness() < 128;
}

inline QLabel *label(const QString &text, const QColor &color, int pixels = 12, bool bold = false, const char *name = nullptr)
{
    auto *l = new QLabel(text);
    QFont f = l->font(); f.setPixelSize(pixels); if (bold) f.setWeight(QFont::DemiBold); l->setFont(f);
    QPalette pal = l->palette(); pal.setColor(QPalette::WindowText, color); l->setPalette(pal);
    l->setTextFormat(Qt::PlainText);
    if (name) l->setObjectName(name);
    return l;
}

// U15: what hgs refused, shown next to what was tried.
inline QLabel *refusal(const QString &error, const Colors &c, const char *name)
{
    auto *l = label(QStringLiteral("⚠ ") + error, c.danger, 11, false, name);
    l->setWordWrap(true);
    return l;
}

// QA: a label that keeps its whole text (for tests and tooltips) and paints it
// elided with an ellipsis when the row is narrower than the text.
class ElidingLabel : public QLabel {
public:
    explicit ElidingLabel(const QString &text, QWidget *parent = nullptr) : QLabel(text, parent)
    {
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        setMinimumWidth(20);
    }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setPen(palette().color(QPalette::WindowText));
        p.drawText(rect(), Qt::AlignVCenter | Qt::AlignLeft,
                   fontMetrics().elidedText(text(), Qt::ElideRight, width()));
    }
};

inline QLabel *elided(const QString &text, const QColor &color, int pixels = 12)
{
    auto *l = label(text, color, pixels);
    l->setToolTip(text);
    l->setMinimumWidth(20);
    l->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    return l;
}

// U17: who decided a permission request, as the owner reads it.
inline QString permissionBy(const QString &by)
{
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    if (by == "owner") return tr("you");
    if (by == "lead") return tr("the lead");
    if (by == "rule") return tr("a rule");
    if (by == "timeout") return tr("a timeout");
    return tr("unknown");
}

// U17: the stored answers as one line: "Permissions: 18 allowed · 2 denied ·
// last: allowed by the lead, 3 min ago".
inline QString permissionSummary(const QList<PermissionAnswer> &answers, double now)
{
    if (answers.isEmpty()) return {};
    int allowed = 0, denied = 0;
    for (const auto &answer : answers) { if (answer.denied()) ++denied; else ++allowed; }
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    const auto &last = answers.last();
    const QString decision = last.denied() ? tr("denied by %1").arg(permissionBy(last.by))
                                           : tr("allowed by %1").arg(permissionBy(last.by));
    return tr("Permissions: %1 allowed · %2 denied").arg(allowed).arg(denied)
           + tr(" · last: %1, %2").arg(decision, agoText(last.at, now));
}

// U17: one stored answer as one row: the decision, who made it and how long
// ago; the command and the deny reason each on one elided line, their whole
// text in the line's tooltip.
inline QWidget *permissionAnswerRow(const PermissionAnswer &answer, const Colors &c, double now, int pixels = 10)
{
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    auto *row = new QWidget; row->setObjectName("teamPermissionAnswerRow");
    row->setProperty("decision", answer.decision);
    auto *v = new QVBoxLayout(row); v->setContentsMargins(0, 1, 0, 1); v->setSpacing(0);
    const QString decision = answer.denied() ? tr("Denied by %1").arg(permissionBy(answer.by))
                                             : tr("Allowed by %1").arg(permissionBy(answer.by));
    auto *line = label(QStringLiteral("%1 · %2").arg(decision, agoText(answer.at, now)),
                       answer.denied() ? c.warn : c.accent, pixels, false, "teamPermissionDecision");
    line->setProperty("decision", answer.decision);
    v->addWidget(line);
    const QString command = !answer.command.isEmpty() ? answer.command : answer.tool;
    if (!command.isEmpty()) {
        auto *asked = new ElidingLabel(command);
        asked->setObjectName("teamPermissionCommand");
        asked->setToolTip(command);
        QPalette pal = asked->palette(); pal.setColor(QPalette::WindowText, c.text2); asked->setPalette(pal);
        QFont f = asked->font(); f.setPixelSize(pixels); asked->setFont(f);
        v->addWidget(asked);
    }
    if (!answer.reason.isEmpty()) {
        auto *reason = new ElidingLabel(answer.reason);
        reason->setObjectName("teamPermissionReason");
        reason->setToolTip(answer.reason);
        QPalette pal = reason->palette(); pal.setColor(QPalette::WindowText, c.text3); reason->setPalette(pal);
        QFont f = reason->font(); f.setPixelSize(pixels); reason->setFont(f);
        v->addWidget(reason);
    }
    QStringList tip;
    tip << QStringLiteral("%1 · %2").arg(decision, agoText(answer.at, now));
    if (!command.isEmpty()) tip << command;
    if (!answer.reason.isEmpty()) tip << answer.reason;
    row->setToolTip(tip.join(QLatin1Char('\n')));
    return row;
}

// U17: the answers behind one toggle: the newest `limit` (0: all of them), each
// one row, then "+N more" when the list is longer.
inline QWidget *permissionAnswerList(const QList<PermissionAnswer> &answers, const Colors &c, double now, int limit, const char *name)
{
    auto *list = new QWidget; list->setObjectName(name);
    auto *v = new QVBoxLayout(list); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(1);
    const int shown = limit > 0 ? qMin(limit, int(answers.size())) : int(answers.size());
    for (int i = 0; i < shown; ++i) v->addWidget(permissionAnswerRow(answers.at(answers.size() - 1 - i), c, now));
    if (answers.size() > shown)
        v->addWidget(label(QCoreApplication::translate("TeamPanel", "+%1 more").arg(answers.size() - shown),
                           c.dim, 10, false, "teamPermissionMore"));
    return list;
}

// U17: the one-line toggle both cards open their answers with; the choice is
// kept in QSettings under `key`.
inline QToolButton *permissionToggle(const QList<PermissionAnswer> &answers, const Colors &c, bool open, const char *name)
{
    auto *toggle = new QToolButton;
    toggle->setObjectName(name);
    toggle->setText(permissionSummary(answers, QDateTime::currentSecsSinceEpoch()));
    toggle->setToolTip(QCoreApplication::translate("TeamPanel", "%1 answers · click to %2")
                           .arg(answers.size())
                           .arg(open ? QCoreApplication::translate("TeamPanel", "fold them")
                                     : QCoreApplication::translate("TeamPanel", "expand them")));
    toggle->setAccessibleDescription(open ? QCoreApplication::translate("TeamPanel", "Expanded")
                                          : QCoreApplication::translate("TeamPanel", "Collapsed"));
    toggle->setIcon(workspaceIcon(open ? "chevron-down" : "chevron-right", c.text2));
    toggle->setIconSize(QSize(10, 10));
    toggle->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    toggle->setAutoRaise(true); toggle->setCheckable(true); toggle->setChecked(open);
    toggle->setStyleSheet(QStringLiteral("QToolButton#%1{color:%2;font-size:11px;border:0;padding:1px 4px;background:transparent;}"
                                         "QToolButton#%1:hover{color:%3;}")
                              .arg(QString::fromLatin1(name), c.text2.name(), c.fg.name()));
    return toggle;
}

// A member's queue (A5) under its current task: the open task first, then the
// tasks returned to it, at most five rows and "+N more". Nothing when the only
// entry is the current task, which the card already names.
inline QWidget *memberQueueWidget(const Team &team, const Member &member, const Colors &c, const std::function<void(int)> &open)
{
    if (member.queue.size() < 2) return nullptr;
    auto *box = new QWidget;
    box->setObjectName("teamMemberQueue");
    box->setProperty("member", member.id);
    auto *v = new QVBoxLayout(box);
    v->setContentsMargins(0, 0, 0, 0); v->setSpacing(1);
    const int shown = qMin(5, int(member.queue.size()));
    for (int i = 0; i < shown; ++i) {
        const auto *task = team.task(member.queue[i]);
        if (!task) continue;
        auto *row = memberQueueRow(team, *task, c, i == 0);
        QObject::connect(row, &QPushButton::clicked, row, [open, id = task->id] { if (open) open(id); });
        v->addWidget(row);
    }
    if (member.queue.size() > shown) {
        auto *more = label(QCoreApplication::translate("TeamPanel", "+%1 more").arg(member.queue.size() - shown), c.dim, 11, false, "teamMemberQueueMore");
        v->addWidget(more);
    }
    return box;
}

inline QPushButton *button(const QString &text, const char *name, bool primary = false)
{
    auto *b = new QPushButton(text);
    b->setObjectName(name); b->setAutoDefault(false); b->setCursor(Qt::PointingHandCursor);
    b->setProperty("primary", primary);
    return b;
}

inline QFrame *card(const Colors &c, const QColor &border = {})
{
    auto *f = new QFrame;
    f->setObjectName("teamCard");
    f->setStyleSheet(QStringLiteral("QFrame#teamCard{background:%1;border:1px solid %2;border-radius:9px;}")
                         .arg(c.surface.name(QColor::HexArgb), (border.isValid() ? border : c.border).name(QColor::HexArgb)));
    return f;
}

inline QHBoxLayout *row(int spacing = 6)
{
    auto *l = new QHBoxLayout; l->setContentsMargins(0, 0, 0, 0); l->setSpacing(spacing); return l;
}

inline QString agentIcon(const QString &agent)
{
    return QStringList{"claude", "codex", "kimi", "opencode", "dsh"}.contains(agent) ? agent : QStringLiteral("terminal");
}

// An agent's own logo (LobeHub lobe-icons, MIT, in tray/resources/agent-icons):
// in its colors where it has them, its monochrome parts in `color`; the line
// icon for an agent without one.
inline QIcon agentLogo(const QString &agent, const QColor &color)
{
    const QIcon logo = agentLogoIcon(agent, color);
    return logo.isNull() ? workspaceIcon(agentIcon(agent), color) : logo;
}

inline QString clock(double at) { return QDateTime::fromSecsSinceEpoch(qint64(at)).toString("HH:mm"); }

// 12m, 3h 12m, 3d 4h, 2w 3d (design HANDOFF §14, fmtDur).
inline QString duration(double seconds)
{
    const qint64 s = qMax<qint64>(0, qint64(seconds));
    const qint64 minutes = s / 60, hours = s / 3600, days = s / 86400;
    if (hours < 1) return QCoreApplication::translate("TeamPanel", "%1m").arg(qMax<qint64>(1, minutes));
    if (days < 1) return QCoreApplication::translate("TeamPanel", "%1h %2m").arg(hours).arg(minutes % 60);
    if (days < 7) return QCoreApplication::translate("TeamPanel", "%1d %2h").arg(days).arg(hours % 24);
    return QCoreApplication::translate("TeamPanel", "%1w %2d").arg(days / 7).arg(days % 7);
}

// #693: a label that names an age keeps its start, its format ("%1 in this
// stage") and its kind ("duration": 3h 12m, "age": 3 min) and is set again
// in place on the minute timer (tickAged), so a poll needs no rebuild for it.
inline QString agedText(const QString &format, const QString &kind, double since, double now)
{
    const QString value = kind == QLatin1String("duration") ? duration(now - since)
                                                             : agoText(since, now).remove(QCoreApplication::translate("TeamPanel", " ago"));
    return format.arg(value);
}
inline QLabel *aged(QLabel *label, const QString &format, const QString &kind, double since, double now = -1)
{
    label->setProperty("agedSince", since); label->setProperty("agedFormat", format); label->setProperty("agedKind", kind);
    label->setText(agedText(format, kind, since, now < 0 ? workedClock()() : now));
    return label;
}
// #709 L4: a health or warning text names the minutes hgs counted when it was
// read ("starved: qa idle 5 min ..."). The visible shape leaves those minutes
// out, so the text grows by the whole minutes since its read, in place.
inline QString agedMinutes(const QString &text, double at, double now)
{
    const qint64 add = at > 0 ? qMax<qint64>(0, qint64((now - at) / 60)) : 0;
    if (!add || !text.contains(QLatin1String(" min"))) return text;
    static const QRegularExpression minutes(QStringLiteral("\\b(\\d+) min\\b"));
    QString out;
    qsizetype last = 0;
    for (auto it = minutes.globalMatch(text); it.hasNext();) {
        const auto match = it.next();
        out += text.mid(last, match.capturedStart() - last) + QString::number(match.captured(1).toLongLong() + add) + QStringLiteral(" min");
        last = match.capturedEnd();
    }
    return out + text.mid(last);
}
// A widget that shows such a text: a label's text (null keeps its own) and
// its tooltip, read at `at` (now by default); tickAged sets them again.
inline void agedHealth(QWidget *widget, const QString &text, const QString &tip, double at = -1)
{
    if (at < 0) at = workedClock()();
    widget->setProperty("healthAt", at); widget->setProperty("healthText", text); widget->setProperty("healthTip", tip);
    if (auto *label = qobject_cast<QLabel *>(widget); label && !text.isNull()) label->setText(text);
    widget->setToolTip(tip);
}
inline void tickAged(QWidget *root, double now)
{
    for (auto *label : root->findChildren<QLabel *>()) {
        const QVariant since = label->property("agedSince");
        if (!since.isValid()) continue;
        const QString text = agedText(label->property("agedFormat").toString(), label->property("agedKind").toString(), since.toDouble(), now);
        if (text != label->text()) label->setText(text);
    }
    for (auto *widget : root->findChildren<QWidget *>()) {
        const QVariant at = widget->property("healthAt");
        if (!at.isValid()) continue;
        const QString text = widget->property("healthText").toString(), tip = agedMinutes(widget->property("healthTip").toString(), at.toDouble(), now);
        if (auto *label = qobject_cast<QLabel *>(widget); label && !text.isEmpty()) {
            const QString shown = agedMinutes(text, at.toDouble(), now);
            if (shown != label->text()) label->setText(shown);
        }
        if (tip != widget->toolTip()) widget->setToolTip(tip);
    }
}

// The WhiteWh look of team surfaces: ghost buttons, accent primary ones,
// segmented controls (a widget with property seg) and fields. Set on the
// surface's top widget; children inherit it.
inline QString teamSheet(const Colors &c)
{
    const QString accentText = c.accentText.name(), accentHover = c.accentHover.name(), segmentOn = c.segOn.name();
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    return QStringLiteral(
        "QPushButton{background:transparent;color:%1;border:1px solid %2;border-radius:7px;padding:4px 10px;}"
        "QPushButton:hover{background:%3;}"
        "QPushButton:disabled{color:%4;border-color:%5;}"
        "QPushButton[primary=\"true\"]{background:%6;color:%7;border:1px solid %6;font-weight:600;}"
        "QPushButton[primary=\"true\"]:hover{background:%8;border-color:%8;}"
        "QPushButton[primary=\"true\"]:disabled{background:%9;color:%10;border-color:transparent;}"
        "QPushButton[danger=\"true\"]{color:%11;border-color:transparent;}"
        "QPushButton[ghost=\"true\"]{border-color:transparent;}"
        "QPushButton[chip=\"true\"]{border-radius:11px;padding:2px 9px;color:%12;}"
        "QWidget[seg=\"true\"]{background:%13;border:1px solid %2;border-radius:8px;}"
        "QWidget[seg=\"true\"] QPushButton{border:none;border-radius:6px;padding:4px 12px;color:%12;background:transparent;}"
        "QWidget[seg=\"true\"] QPushButton:checked{background:%14;color:%6;font-weight:600;}"
        "QLineEdit,QComboBox,QPlainTextEdit,QSpinBox{background:%15;color:%1;border:1px solid %2;border-radius:6px;padding:3px 6px;}"
        "QLineEdit:focus,QComboBox:focus,QPlainTextEdit:focus,QSpinBox:focus{border-color:%6;}"
        "QLineEdit:disabled,QComboBox:disabled,QPlainTextEdit:disabled{color:%4;}")
        .arg(c.fg.name(), c.border.name(), a(c.fg, .06), c.dim.name(), a(c.border, .6), c.accent.name(), accentText, accentHover,
             a(c.accent, .35), a(QColor(accentText), .7), c.danger.name(), c.text2.name(), c.tile.name(), segmentOn, c.card.name())
        + workspaceSpinButtons(a(c.fg, .08));
}

// Items that keep their width and wrap onto the next line.
class FlowLayout : public QLayout {
public:
    explicit FlowLayout(int spacing) { setSpacing(spacing); setContentsMargins(0, 0, 0, 0); }
    ~FlowLayout() override { while (auto *item = takeAt(0)) delete item; }
    void addItem(QLayoutItem *item) override { m_items.append(item); }
    int count() const override { return m_items.size(); }
    QLayoutItem *itemAt(int index) const override { return m_items.value(index); }
    QLayoutItem *takeAt(int index) override { return index >= 0 && index < m_items.size() ? m_items.takeAt(index) : nullptr; }
    Qt::Orientations expandingDirections() const override { return {}; }
    bool hasHeightForWidth() const override { return true; }
    int heightForWidth(int width) const override { return arrange(QRect(0, 0, width, 0), false); }
    void setGeometry(const QRect &rect) override { QLayout::setGeometry(rect); arrange(rect, true); }
    QSize sizeHint() const override { return minimumSize(); }
    QSize minimumSize() const override
    {
        QSize size; for (auto *item : m_items) size = size.expandedTo(item->minimumSize());
        return size;
    }
private:
    int arrange(const QRect &rect, bool apply) const
    {
        int x = rect.x(), y = rect.y(), line = 0;
        for (auto *item : m_items) {
            const QSize hint = item->sizeHint();
            if (x + hint.width() > rect.right() + 1 && line > 0) { x = rect.x(); y += line + spacing(); line = 0; }
            if (apply) item->setGeometry(QRect(QPoint(x, y), hint));
            x += hint.width() + spacing(); line = qMax(line, hint.height());
        }
        return y + line - rect.y();
    }
    QList<QLayoutItem *> m_items;
};

// A segmented control: one checked button among `items` (value, text).
inline QWidget *segmented(const QList<std::pair<QString, QString>> &items, const QString &current, const char *name,
                          const std::function<void(const QString &)> &picked)
{
    auto *box = new QWidget; box->setObjectName(name); box->setProperty("seg", true);
    box->setAttribute(Qt::WA_StyledBackground);
    auto *h = new QHBoxLayout(box); h->setContentsMargins(2, 2, 2, 2); h->setSpacing(0);
    for (const auto &[value, text] : items) {
        auto *b = new QPushButton(text); b->setCheckable(true); b->setChecked(value == current); b->setAutoDefault(false);
        b->setAutoExclusive(true); b->setCursor(Qt::PointingHandCursor); b->setProperty("segment", value);
        h->addWidget(b);
        QObject::connect(b, &QPushButton::clicked, box, [picked, value = value] { if (picked) picked(value); });
    }
    box->setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Fixed);
    return box;
}

// The same control with icons: `items` are (value, icon, tooltip); it keeps
// its width however narrow the window gets.
inline QWidget *iconSegmented(const QList<std::tuple<QString, QString, QString>> &items, const QString &current, const char *name,
                              const QColor &color, const std::function<void(const QString &)> &picked)
{
    QList<std::pair<QString, QString>> plain;
    for (const auto &item : items) plain.append({std::get<0>(item), QString()});
    auto *box = segmented(plain, current, name, picked);
    const auto buttons = box->findChildren<QPushButton *>();
    for (int i = 0; i < buttons.size() && i < items.size(); ++i) {
        const auto &[value, icon, tip] = items[i];
        buttons[i]->setIcon(workspaceIcon(icon, color)); buttons[i]->setIconSize(QSize(16, 16));
        buttons[i]->setToolTip(tip); buttons[i]->setAccessibleName(tip);
        buttons[i]->setStyleSheet(QStringLiteral("padding:4px 8px;"));
    }
    return box;
}

// G1: how many sub-teams and sessions a tree action reaches (the team itself
// is not counted; the list is the controller's current teams).
inline QPair<int, int> subtreeImpact(const Team &team, const QList<Team> &all)
{
    int subteams = 0, sessions = 0;
    QSet<QString> seen;
    std::function<void(const Team &)> walk = [&](const Team &current) {
        for (const auto &child : current.children) {
            if (seen.contains(child.team)) continue;
            seen.insert(child.team);
            ++subteams;
            for (const auto &candidate : all) if (candidate.id == child.team) {
                for (const auto &member : candidate.members) if (!member.session.isEmpty()) ++sessions;
                walk(candidate);
                break;
            }
        }
    };
    walk(team);
    return {subteams, sessions};
}

// G1: Cancel or Reassign of a task with a sub-team asks what happens to it;
// empty means the owner cancelled the action.
inline QString subteamChoice(QWidget *parent, const Team &team, int task, const QString &verb)
{
    const auto *found = team.task(task);
    if (!found || !found->subteam.known || found->subteam.team.isEmpty()) return QStringLiteral("keep");
    QMessageBox box(QMessageBox::Question, QCoreApplication::translate("TeamUi", "%1 task #%2?").arg(verb).arg(task),
                    QCoreApplication::translate("TeamUi", "Task #%1 has the sub-team %2. Stop that sub-team too, or keep it running?")
                        .arg(task).arg(found->subteam.team),
                    QMessageBox::NoButton, parent);
    auto *stop = box.addButton(QCoreApplication::translate("TeamUi", "Stop sub-team"), QMessageBox::DestructiveRole);
    auto *keep = box.addButton(QCoreApplication::translate("TeamUi", "Keep it running"), QMessageBox::AcceptRole);
    box.addButton(QMessageBox::Cancel);
    box.exec();
    if (box.clickedButton() == stop) return QStringLiteral("stop");
    if (box.clickedButton() == keep) return QStringLiteral("keep");
    return QString();
}

}
