#pragma once

#include "CachedText.h"
#include "GitStatusBadge.h"
#include "SessionList.h"
#include "WorkspaceIcons.h"
#include "WorkspaceSkin.h"
#include <QDateTime>
#include <QJsonObject>
#include <QModelIndex>
#include <QPainter>

// U25 (spec 2.2): the chips on a session card's metadata line, right to the
// folder: services, background commands, then git — changed files, outgoing,
// incoming, and in sync or not checked. An icon and a count without a fill;
// the hovered one takes the hover fill. Each names what it counts, and a click
// opens where it lives (SessionList::chipActivated).
namespace SessionRowChips {
enum Kind { Services, Background, Changes, Outgoing, Incoming, Synced, Unchecked, More };
struct Chip { Kind kind; QString icon, count, tone; QRect rect; };

// 10.5 px in the prototype; Qt sizes text in whole pixels.
inline QFont font(QFont base) { base.setPixelSize(11); base.setWeight(QFont::Normal); return base; }
inline int width(const Chip &chip, const QFont &base) {
    return 4 + 12 + (chip.count.isEmpty() ? 0 : 3 + CachedText::width(font(base), chip.count)) + 4;
}
inline bool isGit(Kind kind) { return kind >= Changes; }

inline QList<Chip> of(const QModelIndex &index) {
    QList<Chip> chips;
    if (const int services = index.data(SessionRoles::Services).toList().size())
        chips.append({Services, QStringLiteral("settings"), QString::number(services), QStringLiteral("muted"), {}});
    if (const int jobs = index.data(SessionRoles::BackgroundJobs).toInt(); jobs > 0)
        chips.append({Background, QStringLiteral("active"), GitStatusBadge::countText(jobs), QStringLiteral("bgChip"), {}});
    const auto git = index.data(SessionRoles::GitStatus).toJsonObject();
    for (const auto &mark : GitStatusBadge::marks(git)) {
        if (mark.icon == "git-diff") chips.append({Changes, mark.icon, mark.count, mark.tone == GitStatusBadge::Conflict ? QStringLiteral("danger") : QStringLiteral("warn"), {}});
        else if (mark.icon == "git-push") chips.append({Outgoing, mark.icon, mark.count, QStringLiteral("muted"), {}});
        else if (mark.icon == "git-pull") chips.append({Incoming, mark.icon, mark.count, QStringLiteral("muted"), {}});
        else if (mark.icon == "git-synced") chips.append({Synced, mark.icon, {}, QStringLiteral("accent"), {}});
        else chips.append({Unchecked, QStringLiteral("question"), {}, QStringLiteral("dim"), {}});
    }
    return chips;
}

// The chips that fit, placed right to left so the last ends at `right`;
// git folds into one More chip before services or background work go.
inline QList<Chip> layout(const QModelIndex &index, const QFont &base, int left, int right, int y) {
    auto chips = of(index);
    const auto total = [&base](const QList<Chip> &list) { int w = 0; for (const auto &c : list) w += width(c, base) + 3; return qMax(0, w - 3); };
    if (total(chips) > right - left) {
        QList<Chip> folded; bool more = false;
        for (const auto &chip : chips) if (!isGit(chip.kind)) folded.append(chip); else if (!more) { folded.append({More, QStringLiteral("more"), {}, QStringLiteral("muted"), {}}); more = true; }
        chips = folded;
    }
    while (!chips.isEmpty() && total(chips) > right - left) chips.removeLast();
    int x = right - total(chips);
    for (auto &chip : chips) { chip.rect = QRect(x, y, width(chip, base), 18); x += chip.rect.width() + 3; }
    return chips;
}

inline QString tooltip(const Chip &chip, const QModelIndex &index) {
    const auto git = index.data(SessionRoles::GitStatus).toJsonObject();
    switch (chip.kind) {
    case Services: {
        QStringList lines;
        for (const auto &value : index.data(SessionRoles::Services).toList()) {
            const auto service = value.toMap();
            const double since = service.value("since").toDouble();
            lines << QObject::tr("%1 · since %2").arg(service.value("name").toString(),
                since > 0 ? QDateTime::fromSecsSinceEpoch(qint64(since)).toString(QStringLiteral("d MMM HH:mm")) : QObject::tr("unknown"));
        }
        lines << QObject::tr("These live with the session and are not Background work.");
        return lines.join(QLatin1Char('\n'));
    }
    case Background: {
        const int jobs = index.data(SessionRoles::BackgroundJobs).toInt();
        const QString command = index.data(SessionRoles::BackgroundCommand).toString();
        return (jobs == 1 ? QObject::tr("1 running command") : QObject::tr("%1 running commands").arg(jobs))
            + (command.isEmpty() ? QString() : QStringLiteral(": ") + command);
    }
    case Changes: {
        const int files = git.value("changed_files").toInt();
        const QString branch = git.value("branch").toString();
        QString text = (files == 1 ? QObject::tr("1 changed file") : QObject::tr("%1 changed files").arg(files))
            + (branch.isEmpty() ? QString() : QObject::tr(" on %1").arg(branch));
        if (const int conflicts = git.value("conflicts").toInt()) text += QLatin1Char('\n') + QObject::tr("%1 files with merge conflicts").arg(conflicts);
        return text;
    }
    case Outgoing: { const int n = git.value("ahead").toInt(); return n == 1 ? QObject::tr("1 outgoing commit") : QObject::tr("%1 outgoing commits").arg(n); }
    case Incoming: { const int n = git.value("behind").toInt(); return n == 1 ? QObject::tr("1 incoming commit") : QObject::tr("%1 incoming commits").arg(n); }
    case Synced: {
        const double at = git.value("remote_checked_at").toDouble();
        return QObject::tr("Clean and in sync with the checked upstream")
            + (at > 0 ? QObject::tr(" · Remote checked: %1").arg(QDateTime::fromSecsSinceEpoch(qint64(at)).toLocalTime().toString(QStringLiteral("HH:mm"))) : QString());
    }
    case Unchecked: return QObject::tr("Not checked yet");
    case More: return GitStatusBadge::tooltip(git);
    }
    return {};
}

inline void paint(QPainter *p, const QList<Chip> &chips, const QFont &base, bool dark, int hovered = -1) {
    const auto &skin = WorkspaceSkin::active();
    p->save(); p->setRenderHint(QPainter::Antialiasing); p->setFont(font(base));
    for (int i = 0; i < chips.size(); ++i) {
        const auto &chip = chips[i];
        // Hover brightens a muted chip; a coloured one keeps its colour.
        const QColor ink = WorkspaceSkin::color(skin, dark, i == hovered && chip.tone == QLatin1String("muted") ? QStringLiteral("fg") : chip.tone);
        if (i == hovered) { p->setPen(Qt::NoPen); p->setBrush(WorkspaceSkin::color(skin, dark, QStringLiteral("hover"))); p->drawRoundedRect(QRectF(chip.rect), 5, 5); }
        workspaceIcon(chip.icon, ink).paint(p, QRect(chip.rect.x() + 4, chip.rect.y() + 3, 12, 12));
        if (!chip.count.isEmpty()) { p->setPen(ink); CachedText::draw(p, chip.rect.adjusted(19, 0, -4, 0), Qt::AlignVCenter | Qt::AlignLeft, chip.count); }
    }
    p->restore();
}
}
