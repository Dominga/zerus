#pragma once
#include <QFile>
#include <QHash>
#include <QIcon>
#include <QPainter>
#include <QPainterPath>
#include <QPixmap>
#include <QPushButton>
#include "WorkspaceFocus.h"

// WhiteWh fork: an agent's own logo (LobeHub lobe-icons, MIT, in
// tray/resources/agent-icons), in its colors where it has them and its
// monochrome parts in `color`; a null icon for an agent without one.
inline QIcon agentLogoIcon(const QString &agent, const QColor &color)
{
    static const QHash<QString, QString> files{{"claude", "claude"}, {"codex", "codex"}, {"dsh", "deepseek"}, {"deepseek", "deepseek"},
                                               {"kimi", "kimi-color"}, {"opencode", "opencode"}, {"ollama", "ollama"}};
    const QString file = files.value(agent);
    if (file.isEmpty()) return {};
    // Painted once per agent and color; never destroyed, as for workspaceIcon.
    static auto *cache = new QHash<std::pair<QString, QRgb>, QIcon>;
    const std::pair<QString, QRgb> key{file, color.rgba()};
    if (const auto found = cache->constFind(key); found != cache->cend()) return *found;
    QFile svg(QStringLiteral(":/hgs/agent-icons/%1.svg").arg(file));
    QByteArray data = svg.open(QIODevice::ReadOnly) ? svg.readAll() : QByteArray();
    data.replace("currentColor", color.name().toLatin1()).replace("height=\"1em\"", "height=\"48\"").replace("width=\"1em\"", "width=\"48\"");
    QPixmap pixmap;
    const QIcon icon = pixmap.loadFromData(data, "SVG") ? QIcon(pixmap) : QIcon();
    cache->insert(key, icon);
    return icon;
}

// U25: icons drawn from the owner's prototype (Zerus-prototype.html, its `I`
// table): the toothed settings gear, also on the services chip, and the bell
// and crossed-out bell of notifications and mute. Its strokes: 1.6, round.
inline const char *prototypeIconPath(const QString &name)
{
    static const QHash<QString, const char *> paths{
        {"settings", "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM9 12a3 3 0 1 0 6 0a3 3 0 1 0 -6 0"},
        {"bell", "M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0"},
        {"bell-off", "M8.7 4.3A6 6 0 0 1 18 11v4M6 11v5l-2 2h12M10 20a2 2 0 0 0 4 0M3 3l18 18"},
        {"mute", "M8.7 4.3A6 6 0 0 1 18 11v4M6 11v5l-2 2h12M10 20a2 2 0 0 0 4 0M3 3l18 18"},
        // The lead's session header and the composer row (spec 3, 4).
        {"clock", "M4 13a8 8 0 1 0 16 0a8 8 0 1 0 -16 0ZM12 9v4l2.5 2M10 2h4"},
        // U38: the heat tint's legend and its Tokens choice.
        {"flame", "M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.4-.5-2-1-3-1.1-2.1-.2-4.1 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.2.4-2.3 1-3a2.5 2.5 0 0 0 2.5 2.5z"},
        {"coins","M2 8a6 6 0 1 0 12 0a6 6 0 1 0 -12 0ZM18.1 10.4A6 6 0 1 1 10.3 18M7 6h1v4M16.7 13.9l.7.7-2.8 2.8"},
        // U25 part 6: the inspector's Environment rows (the prototype's key,
        // pc, sparkle and bolt).
        {"key", "M4 15a4 4 0 1 0 8 0a4 4 0 1 0 -8 0ZM10.8 12.2L20 3M17 6l2.5 2.5M14.5 8.5L17 11"},
        {"monitor", "M4.5 4h15a1.5 1.5 0 0 1 1.5 1.5v9a1.5 1.5 0 0 1 -1.5 1.5h-15a1.5 1.5 0 0 1 -1.5 -1.5v-9a1.5 1.5 0 0 1 1.5 -1.5ZM8 20h8M12 16v4"},
        {"asterisk", "M12 3v18M3 12h18M5.6 5.6l12.8 12.8M18.4 5.6L5.6 18.4"},
        {"bolt", "M13 3L5 14h6l-1 7 8-11h-6z"},
        {"arrow-left", "M10 6l-6 6 6 6M4 12h16"},
        {"check-circle", "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0ZM8 12.5l2.8 2.8L16.5 9"},
        {"archive", "M4 4h16a1 1 0 0 1 1 1v3a1 1 0 0 1 -1 1h-16a1 1 0 0 1 -1 -1v-3a1 1 0 0 1 1 -1ZM5 9v10h14V9M10 13h4"},
        {"link", "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"},
        {"shield", "M12 3l7 3v5c0 4.5-3 8.3-7 10-4-1.7-7-5.5-7-10V6z"},
        // The Team tab (spec 5): a phase's note, Show on graph, a draft's pencil.
        {"message", "M5 5h14a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H9l-4 3.5V6a1 1 0 0 1 1-1z"},
        {"eye", "M2.5 12s3.5-6.5 9.5-6.5 9.5 6.5 9.5 6.5-3.5 6.5-9.5 6.5S2.5 12 2.5 12zM9 12a3 3 0 1 0 6 0a3 3 0 1 0 -6 0"},
        {"pencil", "M4 20l1-4L16 5l3 3L8 19zM14 7l3 3"},
        {"warning", "M12 4l9 16H3zM12 10v4M12 17v.5"},
        // U60 (#656): the Model servers rail button and page, Up and Down, Logs.
        {"server", "M4 3h16a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1zM4 13h16a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1zM7 7h.01M7 17h.01"},
        {"power", "M12 3v9M17.7 6.3a8 8 0 1 1-11.4 0"},
        {"logs", "M6 3h9l4 4v14H6zM14 3v5h5M9 12h7M9 16h7"},
        // #648: the Tasks card's subagent row (the prototype's users).
        {"users", "M16 20v-1.5a3.5 3.5 0 0 0-3.5-3.5h-6A3.5 3.5 0 0 0 3 18.5V20M6 8a3.5 3.5 0 1 0 7 0a3.5 3.5 0 1 0 -7 0M21 20v-1.5a3.5 3.5 0 0 0-2.6-3.4M15.5 4.6a3.5 3.5 0 0 1 0 6.8"},
        // U50: the rail's Waiting for you page, an inbox tray.
        {"inbox", "M4 13l2.5-7.5A1.5 1.5 0 0 1 7.9 4.5h8.2a1.5 1.5 0 0 1 1.4 1L20 13v5.5a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 18.5zM4 13h4.5l1.5 2.5h4l1.5-2.5H20"}};
    return paths.value(name);
}

// Draws a prototype path into a square pixmap of `size`; false when the SVG
// image plugin is missing, and the caller paints its older line icon.
inline bool paintPrototypeIcon(QPainter &painter, int size, const char *path, const QColor &color)
{
    const QByteArray svg = QStringLiteral("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"%1\" height=\"%1\" viewBox=\"0 0 24 24\">"
                                          "<path d=\"%2\" fill=\"none\" stroke=\"%3\" stroke-opacity=\"%4\" stroke-width=\"1.6\" "
                                          "stroke-linecap=\"round\" stroke-linejoin=\"round\"/></svg>")
                               .arg(size).arg(QLatin1String(path), color.name()).arg(color.alphaF()).toUtf8();
    QPixmap drawn;
    if (!drawn.loadFromData(svg, "SVG")) return false;
    painter.save(); painter.resetTransform(); painter.drawPixmap(0, 0, drawn); painter.restore();
    return true;
}

// One small stroke family, drawn at native DPR instead of platform-dependent glyphs.
inline QIcon workspaceIcon(const QString &name, const QColor &color)
{
    // List rebuilds, details, polls and card delegates on every frame request the
    // same few icons, and building one renders six pixmaps. Paint each name and
    // color once. Never destroyed: pixmaps must not outlive QGuiApplication at exit.
    static auto *cache = new QHash<std::pair<QString, QRgb>, QIcon>;
    const std::pair<QString, QRgb> key{name, color.rgba()};
    if (const auto found = cache->constFind(key); found != cache->cend()) return *found;
    QIcon icon;
    const QIcon symbol = name == "github" ? QIcon(":/hgs/icons/github-mark.svg")
                         : name == "zerus" ? QIcon(":/hgs/icons/hgs-zerus-symbolic.svg") : QIcon();
    for (int size : {16, 20, 24, 32, 40, 48}) {
        QPixmap pm(size, size); pm.fill(Qt::transparent);
        QPainter p(&pm); p.setRenderHint(QPainter::Antialiasing); p.scale(size / 24., size / 24.);
        p.setPen(QPen(color, 1.6, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin)); p.setBrush(Qt::NoBrush);
        if (const char *path = prototypeIconPath(name); path && paintPrototypeIcon(p, size, path, color)) {
        } else if (name == "github" || name == "zerus") {
            p.resetTransform(); p.drawPixmap(0, 0, symbol.pixmap(size, size));
            p.setCompositionMode(QPainter::CompositionMode_SourceIn); p.fillRect(pm.rect(), color);
        } else if (name == "codex") {
            QPainterPath petal;petal.moveTo(12,5);petal.cubicTo(20,2,23,12,16,15);petal.lineTo(10,12);petal.lineTo(10,7);
            for(int i=0;i<6;++i){p.save();p.translate(12,12);p.rotate(i*60);p.translate(-12,-12);p.drawPath(petal);p.restore();}
        } else if (name == "claude") {
            for(int i=0;i<8;++i){p.save();p.translate(12,12);p.rotate(i*45);p.drawLine(QPointF(0,3),QPointF(0,9));p.restore();}
        } else if (name == "kimi") {
            p.drawLine(6,4,6,20);p.drawLine(6,12,18,4);p.drawLine(6,12,18,20);
        } else if (name == "opencode") {
            p.drawLine(8,5,3,12);p.drawLine(3,12,8,19);
            p.drawLine(16,5,21,12);p.drawLine(21,12,16,19);p.drawLine(14,5,10,19);
        } else if (name == "dsh") {
            QPainterPath whale;whale.moveTo(3,10);whale.cubicTo(5,5,15,4,18,10);whale.lineTo(23,7);whale.lineTo(22,13);
            whale.lineTo(18,13);whale.cubicTo(15,22,3,19,3,10);p.drawPath(whale);p.drawPoint(8,10);
        } else if (name == "sessions" || name == "all") {
            p.drawRoundedRect(QRectF(4, 4, 16, 7), 2, 2); p.drawRoundedRect(QRectF(4, 14, 16, 6), 2, 2);
            p.drawPoint(QPointF(8, 7.5)); p.drawLine(QPointF(12, 7.5), QPointF(16, 7.5));
        } else if (name == "calendar") {
            p.drawRoundedRect(QRectF(4, 5, 16, 16), 2, 2); p.drawLine(4, 10, 20, 10);
            p.drawLine(8, 3, 8, 7); p.drawLine(16, 3, 16, 7);
            p.drawPoint(QPointF(8, 14)); p.drawPoint(QPointF(12, 14)); p.drawPoint(QPointF(16, 14));
        } else if (name == "chevron-left" || name == "chevron-right") {
            const bool right = name == "chevron-right";
            QPainterPath path; path.moveTo(right ? 9 : 15, 6); path.lineTo(right ? 15 : 9, 12); path.lineTo(right ? 9 : 15, 18); p.drawPath(path);
        } else if (name == "machines") {
            p.drawRoundedRect(QRectF(3, 4, 18, 12), 2, 2); p.drawLine(12, 16, 12, 20); p.drawLine(8, 20, 16, 20);
        } else if (name == "team") {   // WhiteWh fork: agent teams, a lead above two members
            p.drawEllipse(QPointF(12, 6), 2.5, 2.5); p.drawEllipse(QPointF(6, 18), 2.5, 2.5); p.drawEllipse(QPointF(18, 18), 2.5, 2.5);
            p.drawLine(QPointF(10.8, 8.2), QPointF(7.2, 15.8)); p.drawLine(QPointF(13.2, 8.2), QPointF(16.8, 15.8));
        } else if (name == "view-graph") {   // tasks as a graph: nodes joined by edges
            p.drawRoundedRect(QRectF(3, 4, 7, 5), 1.5, 1.5); p.drawRoundedRect(QRectF(14, 4, 7, 5), 1.5, 1.5);
            p.drawRoundedRect(QRectF(8.5, 15, 7, 5), 1.5, 1.5);
            p.drawLine(QPointF(6.5, 9), QPointF(11, 15)); p.drawLine(QPointF(17.5, 9), QPointF(13, 15));
        } else if (name == "view-board") {   // tasks as a board: columns of cards
            p.drawRoundedRect(QRectF(3, 4, 5, 16), 1.5, 1.5); p.drawRoundedRect(QRectF(9.5, 4, 5, 11), 1.5, 1.5);
            p.drawRoundedRect(QRectF(16, 4, 5, 7), 1.5, 1.5);
        } else if (name == "view-profile") {   // U38: a profile: lanes of spans
            p.drawLine(QPointF(4, 7), QPointF(16, 7)); p.drawLine(QPointF(4, 12), QPointF(20, 12)); p.drawLine(QPointF(4, 17), QPointF(12, 17));
        } else if (name == "view-tests") {   // U62 (#719): a flask
            QPainterPath flask; flask.moveTo(9.5, 4); flask.lineTo(9.5, 10); flask.lineTo(4.5, 19); flask.quadTo(4, 20.5, 6, 20.5);
            flask.lineTo(18, 20.5); flask.quadTo(20, 20.5, 19.5, 19); flask.lineTo(14.5, 10); flask.lineTo(14.5, 4); p.drawPath(flask);
            p.drawLine(QPointF(8, 4), QPointF(16, 4)); p.drawLine(QPointF(7, 15), QPointF(17, 15));
        } else if (name == "view-layers") {   // tasks in layers: stacked sheets
            QPainterPath top; top.moveTo(12, 4); top.lineTo(21, 8.5); top.lineTo(12, 13); top.lineTo(3, 8.5); top.closeSubpath(); p.drawPath(top);
            QPainterPath mid; mid.moveTo(3, 12.5); mid.lineTo(12, 17); mid.lineTo(21, 12.5); p.drawPath(mid);
            QPainterPath low; low.moveTo(3, 16.5); low.lineTo(12, 21); low.lineTo(21, 16.5); p.drawPath(low);
        } else if (name == "terminal") {
            p.drawRoundedRect(QRectF(3, 4, 18, 16), 2, 2);
            p.drawLine(7, 9, 10, 12); p.drawLine(10, 12, 7, 15); p.drawLine(13, 15, 17, 15);
        } else if (name == "folder" || name == "projects" || name == "group") {
            QPainterPath path; path.moveTo(3, 8); path.lineTo(3, 5); path.lineTo(10, 5); path.lineTo(12, 8); path.lineTo(21, 8);
            path.lineTo(21, 19); path.lineTo(3, 19); path.closeSubpath(); p.drawPath(path);
            if (name == "group") { p.drawLine(9, 13, 15, 13); p.drawLine(12, 10, 12, 16); }
        } else if (name == "folder-move") {
            QPainterPath folder; folder.moveTo(3, 8); folder.lineTo(3, 5); folder.lineTo(10, 5);
            folder.lineTo(12, 8); folder.lineTo(21, 8); folder.lineTo(21, 20); folder.lineTo(3, 20); folder.lineTo(3, 17);
            p.drawPath(folder); p.drawLine(2, 13, 13, 13); p.drawLine(9, 9, 13, 13); p.drawLine(9, 17, 13, 13);
        } else if (name == "pin" || name == "unpinned" || name == "pinned") {
            if (name == "unpinned") { p.translate(12, 12); p.rotate(-35); p.translate(-12, -12); }
            if (name == "pinned") p.setBrush(color);
            QPainterPath path; path.moveTo(8, 3); path.lineTo(16, 3); path.lineTo(15, 9); path.lineTo(19, 13);
            path.lineTo(5, 13); path.lineTo(9, 9); path.closeSubpath(); p.drawPath(path); p.drawLine(12, 13, 12, 21);
            if (name == "pinned") p.drawLine(8, 19, 16, 19);
        } else if (name == "git-branch") {
            p.drawLine(QPointF(6, 7.5), QPointF(6, 16.5));
            QPainterPath branch; branch.moveTo(18, 7.5); branch.lineTo(18, 10);
            branch.cubicTo(18, 15, 6, 11, 6, 16.5); p.drawPath(branch);
            p.drawEllipse(QPointF(6, 5), 2.5, 2.5);
            p.drawEllipse(QPointF(6, 19), 2.5, 2.5);
            p.drawEllipse(QPointF(18, 5), 2.5, 2.5);
        } else if (name == "git-diff") {
            QPainterPath page; page.moveTo(13, 3); page.lineTo(5, 3); page.lineTo(5, 21);
            page.lineTo(19, 21); page.lineTo(19, 9); page.closeSubpath(); p.drawPath(page);
            p.drawLine(13, 3, 13, 9); p.drawLine(13, 9, 19, 9);
            p.drawLine(8, 11, 12, 11); p.drawLine(10, 9, 10, 13); p.drawLine(8, 17, 14, 17);
        } else if (name == "git-push" || name == "git-pull") {
            QPainterPath tray; tray.moveTo(4, 15); tray.lineTo(4, 19);
            tray.quadTo(4, 21, 6, 21); tray.lineTo(18, 21); tray.quadTo(20, 21, 20, 19); tray.lineTo(20, 15); p.drawPath(tray);
            const bool push = name == "git-push";
            p.drawLine(12, 3, 12, 16);
            QPainterPath arrow; arrow.moveTo(7, push ? 8 : 11); arrow.lineTo(12, push ? 3 : 16); arrow.lineTo(17, push ? 8 : 11); p.drawPath(arrow);
        } else if (name == "git-synced" || name == "git-unknown") {
            QPainterPath cloud; cloud.moveTo(6, 19); cloud.cubicTo(0, 19, 0, 10, 6, 10);
            cloud.cubicTo(5, 2, 17, 1, 19, 9); cloud.cubicTo(25, 9, 25, 19, 19, 19);
            cloud.closeSubpath(); p.drawPath(cloud);
            if (name == "git-synced") {
                QPainterPath check; check.moveTo(8, 13); check.lineTo(11, 16); check.lineTo(16, 11); p.drawPath(check);
            } else {
                QPainterPath question; question.moveTo(10, 10); question.cubicTo(10, 7, 15, 7, 15, 10);
                question.cubicTo(15, 12, 12, 11, 12, 14); p.drawPath(question); p.drawPoint(QPointF(12, 17));
            }
        } else if (name == "accounts") {
            p.drawEllipse(QPointF(10, 7), 3.5, 3.5);
            QPainterPath path; path.moveTo(3, 20); path.cubicTo(3, 11, 17, 11, 17, 20); p.drawPath(path);
            p.drawEllipse(QPointF(18, 12), 2, 2); p.drawLine(18,14,18,20); p.drawLine(18,18,21,18);
        } else if (name == "settings") {
            p.drawEllipse(QPointF(12, 12), 6, 6); p.drawEllipse(QPointF(12, 12), 2.5, 2.5);
            for (int a = 0; a < 360; a += 45) { p.save(); p.translate(12, 12); p.rotate(a); p.drawLine(0, 7, 0, 9); p.restore(); }
        } else if (name == "context-warning") {
            p.drawEllipse(QRectF(3, 3, 18, 18)); p.drawLine(12, 7, 12, 12); p.drawPoint(QPointF(12, 16));
        } else if (name == "question") {
            // An optional question: the circle of context-warning with git-unknown's mark.
            p.drawEllipse(QRectF(3, 3, 18, 18));
            QPainterPath question; question.moveTo(10, 10); question.cubicTo(10, 7, 15, 7, 15, 10);
            question.cubicTo(15, 12, 12, 11, 12, 14); p.drawPath(question); p.drawPoint(QPointF(12, 17));
        } else if (name == "attention" || name == "mute") {
            QPainterPath path; path.moveTo(6, 10); path.cubicTo(6, 3, 18, 3, 18, 10); path.lineTo(18, 15); path.lineTo(20, 18); path.lineTo(4, 18); path.lineTo(6, 15); path.closeSubpath();
            p.drawPath(path); p.drawArc(QRectF(10, 18, 4, 4), 180*16, 180*16);
            if (name == "mute") p.drawLine(3, 3, 21, 21);
        } else if (name == "working" || name == "active") {
            QPainterPath path; path.moveTo(13, 3); path.lineTo(5, 13); path.lineTo(11, 13); path.lineTo(10, 21); path.lineTo(19, 10); path.lineTo(13, 10); path.closeSubpath(); p.drawPath(path);
        } else if (name == "paused" || name == "pause") {
            p.drawLine(8, 5, 8, 19); p.drawLine(16, 5, 16, 19);
        } else if (name == "play") {
            QPainterPath path; path.moveTo(8, 5); path.lineTo(19, 12); path.lineTo(8, 19); path.closeSubpath(); p.drawPath(path);
        } else if (name == "archived") {
            p.drawRect(QRectF(3, 4, 18, 4)); p.drawRect(QRectF(5, 8, 14, 12)); p.drawLine(10, 12, 14, 12);
        } else if (name == "trash") {
            p.drawLine(4, 6, 20, 6); p.drawLine(9, 3, 15, 3);
            p.drawLine(6, 6, 7, 21); p.drawLine(7, 21, 17, 21); p.drawLine(17, 21, 18, 6);
            p.drawLine(10, 10, 10, 17); p.drawLine(14, 10, 14, 17);
        } else if (name == "stop") {
            p.drawRoundedRect(QRectF(5, 5, 14, 14), 2, 2);
        } else if (name == "read-all") {
            QPainterPath check; check.moveTo(3, 12); check.lineTo(7, 16); check.lineTo(16, 7); p.drawPath(check);
            QPainterPath second; second.moveTo(11, 15); second.lineTo(13, 17); second.lineTo(22, 8); p.drawPath(second);
        } else if (name == "add") { p.drawLine(12, 5, 12, 19); p.drawLine(5, 12, 19, 12); }
        // U29: out of the phase's original scope - a plus outside a dashed frame.
        else if (name == "out-of-scope") {
            QPen dashed = p.pen(); dashed.setDashPattern({2.0, 1.6}); p.save(); p.setPen(dashed);
            p.drawRoundedRect(QRectF(3, 3, 14, 14), 3, 3); p.restore();
            p.drawLine(QPointF(18, 13), QPointF(18, 22)); p.drawLine(QPointF(13.5, 17.5), QPointF(22.5, 17.5));
        }
        else if (name == "refresh") {
            p.drawArc(QRectF(5, 5, 14, 14), 40*16, 290*16); p.drawLine(19, 4, 19, 9); p.drawLine(14, 9, 19, 9);
        } else if (name == "reset") {
            // Refresh turned back: counterclockwise, to the default value.
            p.drawArc(QRectF(5, 5, 14, 14), 140*16, -290*16); p.drawLine(5, 4, 5, 9); p.drawLine(5, 9, 10, 9);
        } else if (name == "clear") {
            // An eraser on its baseline: Clear session.
            p.save(); p.translate(12, 11); p.rotate(-45);
            p.drawRoundedRect(QRectF(-7, -4, 14, 8), 1.5, 1.5); p.drawLine(QPointF(-1, -4), QPointF(-1, 4)); p.restore();
            p.drawLine(10, 21, 20, 21);
        } else if (name == "connect" || name == "disconnect") {
            // Opposing chain links, separated for a terminal detach.
            QPainterPath left, right;
            left.moveTo(10, 7); left.lineTo(7, 7); left.cubicTo(1, 7, 1, 17, 7, 17); left.lineTo(10, 17);
            right.moveTo(14, 7); right.lineTo(17, 7); right.cubicTo(23, 7, 23, 17, 17, 17); right.lineTo(14, 17);
            p.drawPath(left); p.drawPath(right);
            if (name == "connect") p.drawLine(8, 12, 16, 12);
            else { p.drawLine(11, 3, 13, 6); p.drawLine(11, 18, 13, 21); }
        } else if (name == "video") {
            p.drawRoundedRect(QRectF(3,5,11,14),2,2);
            QPainterPath lens;lens.moveTo(14,9);lens.lineTo(21,5);lens.lineTo(21,19);lens.lineTo(14,15);p.drawPath(lens);
        } else if (name == "microphone") {
            p.drawRoundedRect(QRectF(9, 3, 6, 12), 3, 3);
            p.drawArc(QRectF(6, 8, 12, 11), 180 * 16, 180 * 16);
            p.drawLine(12, 19, 12, 22); p.drawLine(8, 22, 16, 22);
        } else if (name == "attachment") {
            QPainterPath path; path.moveTo(9, 16); path.lineTo(16, 9);
            path.cubicTo(19, 6, 15, 2, 12, 5); path.lineTo(5, 12);
            path.cubicTo(-1, 18, 7, 26, 13, 20); path.lineTo(20, 13);
            path.cubicTo(24, 9, 18, 3, 14, 7); path.lineTo(7, 14);
            p.drawPath(path);
        } else if (name == "inspector") {
            p.drawRoundedRect(QRectF(3, 4, 18, 16), 2, 2); p.drawLine(14, 4, 14, 20);
            p.drawLine(17, 8, 18, 8); p.drawLine(17, 12, 18, 12);
        } else if (name == "collapse-sessions" || name == "expand-sessions") {
            // The bar is the session list's edge; the arrow points where it goes.
            const int tip = name == "collapse-sessions" ? 9 : 19, tail = name == "collapse-sessions" ? 19 : 9;
            p.drawLine(4, 4, 4, 20); p.drawLine(tail, 12, tip, 12);
            p.drawLine(14, 7, tip, 12); p.drawLine(14, 17, tip, 12);
        } else if (name == "expand-row") {
            p.drawLine(5, 4, 19, 4); p.drawLine(5, 20, 19, 20);
            p.drawLine(12, 8, 12, 16);
            p.drawLine(8, 11, 12, 7); p.drawLine(12, 7, 16, 11);
            p.drawLine(8, 13, 12, 17); p.drawLine(12, 17, 16, 13);
        } else if (name == "compact-row") {
            p.drawLine(5, 4, 19, 4); p.drawLine(5, 20, 19, 20);
            p.drawLine(12, 5, 12, 10); p.drawLine(8, 7, 12, 11); p.drawLine(12, 11, 16, 7);
            p.drawLine(12, 14, 12, 19); p.drawLine(8, 17, 12, 13); p.drawLine(12, 13, 16, 17);
        } else if (name == "search") {
            p.drawEllipse(QPointF(10.5, 10.5), 6, 6); p.drawLine(QPointF(15, 15), QPointF(20, 20));
        } else if (name == "collapse-panel") {
            // Match the centred 18px workspace glyphs: balanced side margins
            // and a shorter arrow that does not crowd the panel edge.
            p.drawLine(19, 5, 19, 19); p.drawLine(5, 12, 14, 12);
            p.drawLine(10, 8, 14, 12); p.drawLine(10, 16, 14, 12);
        } else if (name == "stop") {
            p.setBrush(color);p.drawRoundedRect(QRectF(6,6,12,12),1,1);
        } else if (name == "close") {
            p.drawLine(6, 6, 18, 18); p.drawLine(6, 18, 18, 6);
        } else if (name == "copy") {
            p.drawRoundedRect(QRectF(8, 8, 12, 13), 2, 2); p.drawLine(4, 16, 4, 3); p.drawLine(4, 3, 16, 3);
        } else if (name == "drafts") {
            QPainterPath page; page.moveTo(11, 21); page.lineTo(4, 21); page.lineTo(4, 3);
            page.lineTo(13, 3); page.lineTo(18, 8); page.lineTo(18, 10); p.drawPath(page);
            p.drawLine(13, 3, 13, 8); p.drawLine(13, 8, 18, 8);
            p.drawLine(7, 11, 12, 11); p.drawLine(7, 15, 9, 15);
            QPainterPath pencil; pencil.moveTo(12, 17); pencil.lineTo(19, 10); pencil.lineTo(22, 13);
            pencil.lineTo(15, 20); pencil.lineTo(11, 21); pencil.closeSubpath(); p.drawPath(pencil);
            p.drawLine(17, 12, 20, 15);
        } else if (name == "external") {
            p.drawLine(12, 4, 20, 4); p.drawLine(20, 4, 20, 12); p.drawLine(20, 4, 10, 14);
            QPainterPath path; path.moveTo(8, 5); path.lineTo(4, 5); path.lineTo(4, 20); path.lineTo(19, 20); path.lineTo(19, 16); p.drawPath(path);
        } else if (name == "send" || name == "chevron-down" || name == "chevrons-down") {
            // Composer actions sit beside semibold text; match its weight.
            p.setPen(QPen(color, 2.2, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
            if (name == "send") { p.drawLine(12, 19, 12, 5); p.drawLine(6, 11, 12, 5); p.drawLine(18, 11, 12, 5); }
            else if (name == "chevrons-down") {
                p.drawLine(6, 6, 12, 12); p.drawLine(12, 12, 18, 6);
                p.drawLine(6, 12, 12, 18); p.drawLine(12, 18, 18, 12);
            }
            else { p.drawLine(6, 9, 12, 15); p.drawLine(12, 15, 18, 9); }
        } else if (name == "more") { p.drawPoint(5, 12); p.drawPoint(12, 12); p.drawPoint(19, 12); }
        // Project tasks (TasksView): a stage's icon is the icon of the action that moves a task there.
        else if (name == "task-inbox") {
            QPainterPath tray; tray.moveTo(4, 13.5); tray.lineTo(6.5, 6); tray.lineTo(17.5, 6); tray.lineTo(20, 13.5);
            tray.lineTo(20, 19); tray.lineTo(4, 19); tray.closeSubpath(); p.drawPath(tray);
            QPainterPath slot; slot.moveTo(4, 13.5); slot.lineTo(8.5, 13.5); slot.lineTo(10, 16); slot.lineTo(14, 16);
            slot.lineTo(15.5, 13.5); slot.lineTo(20, 13.5); p.drawPath(slot);
        } else if (name == "task-list") {
            p.drawLine(4, 6, 20, 6); p.drawLine(4, 11, 20, 11); p.drawLine(4, 16, 14, 16);
        } else if (name == "task-list-add") {
            p.drawLine(4, 6, 16, 6); p.drawLine(4, 11, 16, 11); p.drawLine(4, 16, 11, 16);
            p.drawLine(QPointF(17.5, 14), QPointF(17.5, 21)); p.drawLine(QPointF(14, 17.5), QPointF(21, 17.5));
        } else if (name == "task-clock") {
            p.drawEllipse(QPointF(12, 12), 8.5, 8.5); p.drawLine(QPointF(12, 7.5), QPointF(12, 12)); p.drawLine(12, 12, 15, 14);
        } else if (name == "task-check") {
            p.setPen(QPen(color, 2, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin)); p.drawLine(5, 12, 10, 17); p.drawLine(10, 17, 19, 7);
        } else if (name == "task-funnel") {
            QPainterPath funnel; funnel.moveTo(4, 5); funnel.lineTo(20, 5); funnel.lineTo(14, 12.5); funnel.lineTo(14, 19);
            funnel.lineTo(10, 17); funnel.lineTo(10, 12.5); funnel.closeSubpath(); p.drawPath(funnel);
        } else if (name == "task-edit") {
            QPainterPath pencil; pencil.moveTo(4, 20); pencil.lineTo(8, 20); pencil.lineTo(19, 9); pencil.lineTo(15, 5);
            pencil.lineTo(4, 16); pencil.closeSubpath(); p.drawPath(pencil); p.drawLine(13, 7, 17, 11);
        } else if (name == "task-top") {
            p.drawLine(7, 12, 12, 7); p.drawLine(12, 7, 17, 12); p.drawLine(7, 18, 12, 13); p.drawLine(12, 13, 17, 18);
        }
        p.end(); icon.addPixmap(pm);
    }
    cache->insert(key, icon);
    return icon;
}

inline void configureWorkspaceIconButton(QPushButton *button, const QString &name, const QString &caption, bool dark)
{
    WorkspaceFocus::install();
    button->setText({}); button->setToolTip(caption); button->setAccessibleName(caption);
    button->setFixedSize(34, 34); button->setIconSize(QSize(18, 18));
    // Override inherited padding/minimums so this stays square inside any page.
    button->setStyleSheet("QPushButton { padding:0; min-width:32px; max-width:32px; min-height:32px; max-height:32px; }");
    button->setIcon(workspaceIcon(name, QColor(dark ? "#a1adbb" : "#647386")));
}
