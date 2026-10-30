#pragma once
#include <QAbstractButton>
#include <QApplication>
#include <QCheckBox>
#include <QDialog>
#include <QImage>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QScrollArea>
#include <QPalette>
#include <QRadioButton>
#include <QStyle>

// WhiteWh fork, #737: a dialog draws the workspace theme, not the system's.
// Tests set a light system palette and the dark theme; a dialog that falls
// back to the application's palette draws a light surface, and the theme's
// light labels vanish on it (the owner's New team dialog in whitewh-116).
namespace DialogTheme {

// The system palette of a light (or dark) desktop, whatever the platform's.
inline QPalette system(bool dark)
{
    QPalette palette = QApplication::style()->standardPalette();
    for (const auto group : {QPalette::Active, QPalette::Inactive, QPalette::Disabled}) {
        palette.setColor(group, QPalette::Window, QColor(dark ? "#202020" : "#f0f0f0"));
        palette.setColor(group, QPalette::Base, QColor(dark ? "#1b1b1b" : "#ffffff"));
        palette.setColor(group, QPalette::Button, QColor(dark ? "#2d2d2d" : "#f0f0f0"));
        for (const auto role : {QPalette::WindowText, QPalette::Text, QPalette::ButtonText})
            palette.setColor(group, role, QColor(dark ? "#ffffff" : "#000000"));
    }
    return palette;
}

// A point of `widget` as `image`, its window's picture, shows it (a page's
// own grab would fill its palette, not what its window draws behind it).
inline QColor pixel(const QImage &image, QWidget *widget, const QPoint &point)
{
    const QPoint at = widget->mapTo(widget->window(), point) * image.devicePixelRatio();
    return image.isNull() || !image.rect().contains(at) ? QColor() : image.pixelColor(at);
}

// "" when `dialog` (or a page of a window) draws the theme: a dark (or light)
// surface and every shown label's text readable on it; else what is wrong.
inline QString problem(QWidget *dialog, bool dark)
{
    if (!dialog) return QStringLiteral("no dialog");
    const QString name = dialog->objectName();
    // The surface: the bottom-left corner, under no control.
    const QImage image = dialog->window()->grab().toImage();
    const QColor ground = pixel(image, dialog, QPoint(3, dialog->height() - 4));
    if (!ground.isValid()) return name + QStringLiteral(": nothing drawn");
    if ((ground.lightness() < 128) != dark)
        return QStringLiteral("%1: surface %2 in the %3 theme").arg(name, ground.name(), dark ? "dark" : "light");
    // The fields and the scrolled lists sit on the theme's side too: their
    // right edge, inside the border, and their bottom-left corner.
    QList<QPair<QWidget *, QPoint>> grounds;
    for (auto *edit : dialog->findChildren<QLineEdit *>()) grounds << qMakePair<QWidget *, QPoint>(edit, QPoint(edit->width() - 6, edit->height() / 2));
    for (auto *edit : dialog->findChildren<QPlainTextEdit *>()) grounds << qMakePair<QWidget *, QPoint>(edit, QPoint(edit->width() - 6, edit->height() - 6));
    for (auto *area : dialog->findChildren<QScrollArea *>()) grounds << qMakePair<QWidget *, QPoint>(area, QPoint(3, area->height() - 4));
    for (const auto &[widget, point] : grounds) {
        if (!widget->isVisibleTo(dialog) || widget->window() != dialog->window() || widget->width() < 12 || widget->height() < 10) continue;
        const QColor seen = pixel(image, widget, point);
        if (seen.isValid() && (seen.lightness() < 128) != dark)
            return QStringLiteral("%1: %2 %3 draws %4 in the %5 theme").arg(name, QString::fromLatin1(widget->metaObject()->className()), widget->objectName(),
                                                                         seen.name(), dark ? "dark" : "light");
    }
    // Labels, and the text beside a check box or a radio button.
    QList<QWidget *> texts;
    for (auto *label : dialog->findChildren<QLabel *>()) if (!label->text().trimmed().isEmpty()) texts << label;
    for (auto *box : dialog->findChildren<QAbstractButton *>())
        if ((qobject_cast<QCheckBox *>(box) || qobject_cast<QRadioButton *>(box)) && !box->text().trimmed().isEmpty()) texts << box;
    for (auto *widget : texts) {
        if (!widget->isVisibleTo(dialog) || widget->window() != dialog->window()) continue;
        const QColor text = widget->palette().color(widget->foregroundRole());
        if (qAbs(text.lightness() - ground.lightness()) < 80) {
            const auto *label = qobject_cast<QLabel *>(widget);
            const QString shown = label ? label->text() : static_cast<QAbstractButton *>(widget)->text();
            return QStringLiteral("%1: %2 %3 \"%4\" is %5 on %6").arg(name, QString::fromLatin1(widget->metaObject()->className()), widget->objectName(),
                                                                     shown.left(40), text.name(), ground.name());
        }
    }
    return {};
}

}
