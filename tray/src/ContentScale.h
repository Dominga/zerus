#pragma once
#include "Settings.h"
#include <QRegularExpression>
#include <QSettings>
#include <QString>
#include <QStringList>
#include <QtMath>

// Session content (Activity, its composer and Terminal) can be resized
// independently of the surrounding workspace chrome. 1.0 is the native size.
namespace ContentScale {
constexpr double Minimum = 0.75;
constexpr double Maximum = 2.0;
constexpr double Default = 1.0;
inline double clamp(double scale) { return qBound(Minimum, scale, Maximum); }
inline double factor() { return clamp(Settings::value("workspace/contentScale", Default).toDouble()); }
inline int px(double size, double scale) { return qRound(size * scale); }

// Scales every pixel length of generated rich text. Only markup is touched:
// text nodes are escaped and can contain neither tags nor style attributes.
inline QString html(const QString &source, double scale)
{
    if (qFuzzyCompare(scale, 1.0)) return source;
    static const QRegularExpression markup(QStringLiteral("<style>[^<]*</style>|<[^<>]*>"));
    static const QRegularExpression attribute(QStringLiteral("([A-Za-z-]+)=(?:'([^']*)'|\"([^\"]*)\")"));
    static const QRegularExpression length(QStringLiteral("\\b(\\d+(?:\\.\\d+)?)px\\b"));
    static const QRegularExpression integer(QStringLiteral("^\\d+$"));
    static const QStringList lengthAttributes{"cellpadding", "cellspacing", "width", "height"};
    const auto lengths = [scale](const QString &css) {
        QString result; qsizetype last = 0;
        for (auto it = length.globalMatch(css); it.hasNext();) {
            const auto match = it.next();
            result += css.mid(last, match.capturedStart() - last) + QString::number(px(match.captured(1).toDouble(), scale)) + "px";
            last = match.capturedEnd();
        }
        return result + css.mid(last);
    };
    QString result; qsizetype last = 0;
    for (auto it = markup.globalMatch(source); it.hasNext();) {
        const auto match = it.next(); QString tag = match.captured();
        if (tag.startsWith("<style>")) tag = lengths(tag);
        else {
            // Whole attributes are consumed, so names inside values never match.
            QString rewritten; qsizetype position = 0;
            for (auto attributes = attribute.globalMatch(tag); attributes.hasNext();) {
                const auto value = attributes.next(); const int group = value.hasCaptured(2) ? 2 : 3;
                const auto name = value.captured(1).toLower(); QString scaled = value.captured(group);
                if (name == "style") scaled = lengths(scaled);
                else if (lengthAttributes.contains(name) && integer.match(scaled).hasMatch())
                    scaled = QString::number(px(scaled.toInt(), scale));
                rewritten += tag.mid(position, value.capturedStart(group) - position) + scaled;
                position = value.capturedEnd(group);
            }
            tag = rewritten + tag.mid(position);
        }
        result += source.mid(last, match.capturedStart() - last) + tag;
        last = match.capturedEnd();
    }
    return result + source.mid(last);
}
}
