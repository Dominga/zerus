#pragma once
#include <QString>
#include <QWidget>
#include "WorkspaceSkin.h"

// QWidget repolishes even an identical stylesheet, invalidating layout and
// repainting descendants. Live snapshots should only style changed values.
inline void setWorkspaceStyle(QWidget *widget, const QString &style)
{
    if (widget->styleSheet() != style) widget->setStyleSheet(style);
}

// Style every subcontrol: leaving page/arrow controls native lets some Linux
// styles paint a black groove even when the scrollbar itself is transparent.
// U25: the prototype's thin bars everywhere: 8 px, no arrow buttons, a
// transparent track and a round thumb of the skin's text3 at 24 % inside a
// 2 px clear margin (45 % on hover).
inline QString workspaceScrollbars(bool dark)
{
    const QColor text3 = WorkspaceSkin::color(WorkspaceSkin::active(), dark, QStringLiteral("text3"));
    const auto thumb = [&text3](double alpha) { QColor c = text3; c.setAlphaF(alpha); return c.name(QColor::HexArgb); };
    return QStringLiteral(R"(
        QScrollBar { background:transparent; border:0; }
        QScrollBar:vertical { width:8px; margin:0; }
        QScrollBar:horizontal { height:8px; margin:0; }
        QScrollBar::handle { background:%1; border:0; border-radius:2px; margin:2px; }
        QScrollBar::handle:vertical { min-height:28px; }
        QScrollBar::handle:horizontal { min-width:28px; }
        QScrollBar::handle:hover { background:%2; }
        QScrollBar::handle:pressed { background:%3; }
        QScrollBar::add-line, QScrollBar::sub-line { background:transparent; border:0; width:0; height:0; }
        QScrollBar::add-page, QScrollBar::sub-page { background:transparent; border:0; }
        QScrollBar::up-arrow, QScrollBar::down-arrow, QScrollBar::left-arrow, QScrollBar::right-arrow { background:transparent; border:0; width:0; height:0; }
        QAbstractScrollArea::corner { background:transparent; border:0; }
    )").arg(thumb(.24), thumb(.45), thumb(.6));
}
