#pragma once
#include <QAbstractButton>
#include <QFrame>
#include <QHBoxLayout>
#include <QIcon>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>
#include <QTimer>
#include <QTransform>
#include <QVBoxLayout>
#include <QWidget>

#include "WorkspaceSkin.h"

// WhiteWh fork, U25 part 7 (spec 7-9): the pieces the Projects, Machines and
// Agent accounts pages share - the prototype's .box card, .btn.pri and
// .btn.dng buttons, the .seg segment, warn and error text, an error field and
// a refresh icon that turns while it works. Colors are the skin's design
// tokens; a page applies sheet() to itself in setTheme.
namespace PageCards {

inline QColor token(bool dark, const char *name) { return WorkspaceSkin::color(WorkspaceSkin::active(), dark, QString::fromLatin1(name)); }

// A card: 12 px radius, the card color and border, 12/14 px padding.
inline QFrame *card(const QString &objectName, QBoxLayout **layout = nullptr, bool vertical = true)
{
    auto *frame = new QFrame; frame->setObjectName(objectName); frame->setProperty("pageCard", true);
    QBoxLayout *inner = vertical ? static_cast<QBoxLayout *>(new QVBoxLayout(frame)) : static_cast<QBoxLayout *>(new QHBoxLayout(frame));
    inner->setContentsMargins(14, 12, 14, 12); inner->setSpacing(10);
    if (layout) *layout = inner;
    return frame;
}

inline void primary(QAbstractButton *button) { button->setProperty("pagePrimary", true); }
inline void danger(QAbstractButton *button) { button->setProperty("pageDanger", true); }
// A segment button (checkable); the first and last of a segment round its ends.
inline void segment(QAbstractButton *button, const char *edge = "")
{
    button->setCheckable(true); button->setProperty("pageSegment", true); button->setProperty("segmentEdge", edge);
    button->setCursor(Qt::PointingHandCursor);
}

// The page's own sheet: cards, buttons, segment, warn/error text and fields.
inline QString sheet(bool dark)
{
    const auto c = [dark](const char *name) { return token(dark, name).name(QColor::HexArgb); };
    return QStringLiteral(
        "QFrame[pageCard=\"true\"]{background:%1;border:1px solid %2;border-radius:12px;}"
        "QFrame[pageCard=\"true\"] QLabel{background:transparent;border:0;}"
        "QPushButton[pagePrimary=\"true\"]{background:%3;border:1px solid %3;color:%4;font-weight:600;border-radius:7px;padding:4px 11px;}"
        "QPushButton[pagePrimary=\"true\"]:hover{background:%5;border-color:%5;}"
        "QPushButton[pagePrimary=\"true\"]:disabled{background:%3;color:%4;border-color:%3;}"
        "QPushButton[pageDanger=\"true\"]{color:%6;}"
        "QPushButton[pageSegment=\"true\"]{background:%7;color:%8;border:1px solid %9;border-radius:0;padding:4px 10px;min-height:17px;font-size:11.5px;}"
        "QPushButton[pageSegment=\"true\"][segmentEdge=\"first\"]{border-top-left-radius:8px;border-bottom-left-radius:8px;}"
        "QPushButton[pageSegment=\"true\"][segmentEdge=\"last\"]{border-top-right-radius:8px;border-bottom-right-radius:8px;}"
        "QPushButton[pageSegment=\"true\"]:checked{background:%10;color:%11;font-weight:600;}"
        "QLabel[pageWarn=\"true\"]{color:%12;}"
        "QLabel[pageError=\"true\"]{color:%6;font-size:11px;}"
        "QLabel[pageMono=\"true\"]{color:%13;font-family:\"Cascadia Mono\",Consolas,monospace;font-size:11px;}"
        "QLineEdit[pageError=\"true\"]{border:1px solid %6;}")
        .arg(c("card"), c("cardBorder"), c("accent"), c("accentText"), c("accentHover"), c("danger"),
             c("surface"), c("muted"), c("border"), c("segOn"), c("accent"), c("warn"))
        .arg(c("dim"));
}

// The refresh icon turns while `on`; it stops on the base icon.
inline void spin(QAbstractButton *button, bool on)
{
    auto *timer = button->findChild<QTimer *>(QStringLiteral("pageSpin"));
    if (!on) {
        if (timer) { timer->stop(); button->setIcon(button->property("pageSpinBase").value<QIcon>()); }
        button->setProperty("spinning", false);
        return;
    }
    if (button->property("spinning").toBool()) return;
    button->setProperty("spinning", true);
    if (!timer) {
        timer = new QTimer(button); timer->setObjectName(QStringLiteral("pageSpin")); timer->setInterval(60);
        QObject::connect(timer, &QTimer::timeout, button, [button] {
            const QIcon base = button->property("pageSpinBase").value<QIcon>();
            const int angle = (button->property("pageSpinAngle").toInt() + 30) % 360;
            button->setProperty("pageSpinAngle", angle);
            const QSize size = button->iconSize();
            QPixmap source = base.pixmap(size, button->devicePixelRatioF());
            QPixmap turned(source.size()); turned.setDevicePixelRatio(source.devicePixelRatio()); turned.fill(Qt::transparent);
            QPainter p(&turned); p.setRenderHint(QPainter::SmoothPixmapTransform);
            const QPointF middle(size.width() / 2.0, size.height() / 2.0);
            p.translate(middle); p.rotate(angle); p.translate(-middle);
            p.drawPixmap(QPointF(0, 0), source);
            p.end();
            button->setIcon(QIcon(turned));
        });
    }
    button->setProperty("pageSpinBase", button->icon());
    timer->start();
}

}   // namespace PageCards
