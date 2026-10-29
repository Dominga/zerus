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

// A styled QSpinBox (one with its own border) must also style its buttons.
// Otherwise Qt's style sheet style leaves their geometry to the base style
// while it sizes the editor for one stacked button: Windows 11 places up and
// down side by side, so the editor covered the up button and a click on it
// started editing instead of stepping. With both buttons defined here the
// geometry, painting and hit test all come from the style sheet on every
// platform. `hover` tints a button under the pointer.
inline QString workspaceSpinButtons(const QString &hover)
{
    return QStringLiteral(
        "QSpinBox::up-button,QSpinBox::down-button{subcontrol-origin:padding;width:16px;border:0;border-radius:3px;background:transparent;}"
        "QSpinBox::up-button{subcontrol-position:top right;}"
        "QSpinBox::down-button{subcontrol-position:bottom right;}"
        "QSpinBox::up-button:hover,QSpinBox::down-button:hover{background:%1;}"
        "QSpinBox::up-arrow{image:url(:/hgs/chevron-up.svg);width:10px;height:10px;}"
        "QSpinBox::down-arrow{image:url(:/hgs/chevron-down.svg);width:10px;height:10px;}").arg(hover);
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
