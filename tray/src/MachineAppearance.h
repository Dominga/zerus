#pragma once
#include "Settings.h"
#include <QColor>
#include <QCoreApplication>
#include <QHash>
#include <QIcon>
#include <QPainter>
#include <QSettings>
#include <cmath>

namespace MachineAppearance {
// Badges read these on every paint, where constructing QSettings cost a visible
// share of each frame. Only the writers below change these keys, so they keep the
// cache current; code that rewrites settings wholesale calls invalidate().
struct Cache { QString organization, application; QHash<QString, QColor> colors; QHash<QString, bool> vivid; };
inline Cache &cache() {
    static Cache value;
    const QString organization = QCoreApplication::organizationName(), application = QCoreApplication::applicationName();
    if (value.organization != organization || value.application != application) value = {organization, application, {}, {}};
    return value;
}
inline void invalidate() { cache() = {}; }
// A machine's color without a chosen one: pure, so usable on any thread.
inline QColor paletteColor(const QString &machine) {
    static const char *palette[]{"#5fbfa5", "#8ea9f4", "#c098e6", "#e7ae66", "#e38f9e", "#75bfcf", "#acc577"};
    quint32 hash = 2166136261u;
    for (const auto &byte : machine.toUtf8()) hash = (hash ^ quint8(byte)) * 16777619u;
    return QColor(palette[((hash >> 16) ^ hash) % 7]);
}
// The GUI thread's cached color, chosen or from the palette.
inline QColor color(const QString &machine) {
    auto &colors = cache().colors;
    if (const auto it = colors.constFind(machine); it != colors.cend()) return *it;
    QColor result(Settings::value("machines/colors/" + machine).toString());
    if (!result.isValid()) result = paletteColor(machine);
    colors.insert(machine, result);
    return result;
}
inline void setColor(const QString &machine, const QColor &value) {
    if (machine.isEmpty() || !value.isValid()) return;
    Settings::set("machines/colors/" + machine, value.name()); cache().colors.insert(machine, QColor(value.name()));
}
inline void resetColor(const QString &machine) { Settings::remove("machines/colors/" + machine); cache().colors.remove(machine); }
inline bool vivid(const QString &machine) {
    auto &vivid = cache().vivid;
    if (const auto it = vivid.constFind(machine); it != vivid.cend()) return *it;
    return *vivid.insert(machine, Settings::value("machines/vivid/" + machine, false).toBool());
}
inline void setVivid(const QString &machine, bool enabled) {
    if (machine.isEmpty()) return;
    Settings::set("machines/vivid/" + machine, enabled); cache().vivid.insert(machine, enabled);
}
inline QColor blend(const QColor &a, const QColor &b, qreal amount) {
    return QColor::fromRgbF(a.redF() * (1-amount) + b.redF() * amount,
        a.greenF() * (1-amount) + b.greenF() * amount, a.blueF() * (1-amount) + b.blueF() * amount);
}
inline double luminance(const QColor &color) {
    const auto linear = [](double c) { return c <= .04045 ? c / 12.92 : std::pow((c + .055) / 1.055, 2.4); };
    return .2126 * linear(color.redF()) + .7152 * linear(color.greenF()) + .0722 * linear(color.blueF());
}
inline QColor textColor(const QColor &value, bool dark, bool bright = false) {
    if (!bright) return blend(value, dark ? QColor("#ffffff") : QColor("#111820"), dark ? .38 : .48);
    // Choose the higher WCAG contrast; vivid yellow and blue need different ink.
    return luminance(value) > .179 ? QColor("#000000") : QColor("#ffffff");
}
inline QColor backgroundColor(const QColor &value, bool dark, bool bright = false) {
    return bright ? value : blend(value, dark ? QColor("#171e25") : QColor("#ffffff"), dark ? .78 : .83);
}
inline QIcon icon(const QString &machine) {
    QIcon result;
    for (int size : {16, 24, 32, 48}) {
        QPixmap pixmap(size, size); pixmap.fill(Qt::transparent);
        QPainter painter(&pixmap); painter.setRenderHint(QPainter::Antialiasing); painter.setPen(Qt::NoPen); painter.setBrush(color(machine));
        painter.drawEllipse(QRectF(size*.22, size*.22, size*.56, size*.56)); painter.end(); result.addPixmap(pixmap);
    }
    return result;
}
}
