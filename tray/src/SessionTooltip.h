#pragma once

#include <QFont>
#include <QFontMetrics>
#include <QObject>
#include <QStringList>
#include <QTextLayout>
#include <QTextOption>

// Tooltip and accessible text of a Sessions row: each field wrapped to width in
// font, with a bounded number of lines. It lays text out without painting, so
// the row preparation calls it off the GUI thread with a copy of the tooltip
// font taken on the GUI thread (QToolTip::font()).
struct SessionTooltip { QString plain, html; };

inline SessionTooltip sessionTooltip(const QStringList &fields, int width, const QFont &font)
{
    // Tool excerpts can contain minified JSON, base64 or a single very long
    // command. Explicit measured lines also bound unbreakable tokens: a CSS
    // width on its own does not stop Qt rich-text tooltips from expanding.
    const QFontMetrics metrics(font);
    QStringList lines;
    bool truncated = false;
    constexpr int MaxLines = 11, MaxFieldLines = 2, MaxFieldCharacters = 600;
    for (const auto &raw : fields) {
        const QString field = raw.simplified();
        if (field.isEmpty()) continue;
        if (lines.size() >= MaxLines) { truncated = true; break; }
        const QString text = field.left(MaxFieldCharacters);
        QTextLayout layout(text, font);
        QTextOption option; option.setWrapMode(QTextOption::WrapAtWordBoundaryOrAnywhere);
        layout.setTextOption(option); layout.beginLayout();
        int end = 0, fieldLines = 0;
        while (fieldLines < MaxFieldLines && lines.size() < MaxLines) {
            auto line = layout.createLine();
            if (!line.isValid()) break;
            line.setLineWidth(width);
            end = line.textStart() + line.textLength();
            lines.append(text.mid(line.textStart(), line.textLength()).trimmed());
            ++fieldLines;
        }
        layout.endLayout();
        if (end < field.size()) {
            truncated = true;
            if (!lines.isEmpty()) lines.last() = metrics.elidedText(lines.last() + QChar(0x2026), Qt::ElideRight, width);
        }
    }
    if (truncated) lines.append(QObject::tr("Open Activity for details."));
    const QString plain = lines.join('\n');
    return {plain, QString("<qt><table width='%1' cellspacing='0' cellpadding='0'><tr><td>%2</td></tr></table></qt>")
                       .arg(width).arg(plain.toHtmlEscaped().replace('\n', "<br>"))};
}
